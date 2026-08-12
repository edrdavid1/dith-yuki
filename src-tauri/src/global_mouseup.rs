//! Session-scoped left-button release detection during OS `startDragging`.
//!
//! macOS: NSEvent local + global leftMouseUp monitors on the AppKit main thread.
//! Windows: GetAsyncKeyState polling.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Whether this build can provide a reliable mouseup end-signal.
pub fn mouseup_backend_available() -> bool {
    cfg!(any(target_os = "macos", target_os = "windows"))
}

/// Opaque handle that removes monitors when cancelled.
pub struct MouseUpHook {
    cancel: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    cleanup: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>,
}

impl MouseUpHook {
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        #[cfg(target_os = "macos")]
        if let Ok(mut guard) = self.cleanup.lock() {
            if let Some(cb) = guard.take() {
                cb();
            }
        }
    }
}

impl Drop for MouseUpHook {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Install a mouseup hook. On macOS, `install_on_main_thread` runs the given
/// closure on the AppKit main thread (use `AppHandle::run_on_main_thread`).
pub fn install_left_mouseup_hook<FInstall, FUp>(
    install_on_main_thread: FInstall,
    on_mouseup: FUp,
) -> MouseUpHook
where
    FInstall: FnOnce(Box<dyn FnOnce() + Send>),
    FUp: FnOnce() + Send + 'static,
{
    let cancel = Arc::new(AtomicBool::new(false));

    #[cfg(target_os = "macos")]
    {
        let cleanup_slot: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>> =
            Arc::new(Mutex::new(None));
        let cleanup_for_install = cleanup_slot.clone();
        let cancel_for_install = cancel.clone();
        let on_mouseup: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>> =
            Arc::new(Mutex::new(Some(Box::new(on_mouseup))));

        install_on_main_thread(Box::new(move || {
            install_macos_monitors(cancel_for_install, on_mouseup, cleanup_for_install);
        }));

        return MouseUpHook {
            cancel,
            cleanup: cleanup_slot,
        };
    }

    #[cfg(target_os = "windows")]
    {
        let cancel_flag = cancel.clone();
        thread::spawn(move || {
            let mut saw_down = left_button_pressed_win();
            for _ in 0..50 {
                if cancel_flag.load(Ordering::SeqCst) {
                    return;
                }
                if left_button_pressed_win() {
                    saw_down = true;
                    break;
                }
                thread::sleep(Duration::from_millis(8));
            }
            if !saw_down {
                return;
            }
            loop {
                if cancel_flag.load(Ordering::SeqCst) {
                    return;
                }
                if !left_button_pressed_win() {
                    if !cancel_flag.swap(true, Ordering::SeqCst) {
                        on_mouseup();
                    }
                    return;
                }
                thread::sleep(Duration::from_millis(16));
            }
        });
        return MouseUpHook { cancel };
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (install_on_main_thread, on_mouseup);
        MouseUpHook { cancel }
    }
}

/// Background tick helper (position polling) until cancel is set.
pub fn spawn_tick_loop<T>(cancel: Arc<AtomicBool>, on_tick: T)
where
    T: Fn() + Send + 'static,
{
    thread::spawn(move || {
        while !cancel.load(Ordering::SeqCst) {
            on_tick();
            thread::sleep(Duration::from_millis(16));
        }
    });
}

/// Backup end-signal: poll HID button state. NSEvent monitors are primary on
/// macOS, but mouseUp can be swallowed after `performWindowDragWithEvent`.
pub fn spawn_hid_mouseup_backup<F>(cancel: Arc<AtomicBool>, on_mouseup: F)
where
    F: FnOnce() + Send + 'static,
{
    #[cfg(target_os = "macos")]
    {
        thread::spawn(move || {
            // Wait until HID reports down at least once (or timeout).
            let mut saw_down = false;
            for _ in 0..60 {
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                if hid_left_pressed() {
                    saw_down = true;
                    break;
                }
                thread::sleep(Duration::from_millis(16));
            }
            if !saw_down {
                // Never confirmed via HID — rely on NSEvent monitors only.
                return;
            }
            let mut up_streak = 0u8;
            loop {
                if cancel.load(Ordering::SeqCst) {
                    return;
                }
                if hid_left_pressed() {
                    up_streak = 0;
                } else {
                    up_streak = up_streak.saturating_add(1);
                    if up_streak >= 3 {
                        if !cancel.swap(true, Ordering::SeqCst) {
                            on_mouseup();
                        }
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(16));
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (cancel, on_mouseup);
    }
}

#[cfg(target_os = "macos")]
fn hid_left_pressed() -> bool {
    extern "C" {
        fn CGEventSourceButtonState(state_id: i32, button: u32) -> u8;
    }
    const HID_SYSTEM_STATE: i32 = 1;
    unsafe { CGEventSourceButtonState(HID_SYSTEM_STATE, 0) != 0 }
}

#[cfg(target_os = "windows")]
fn left_button_pressed_win() -> bool {
    extern "system" {
        fn GetAsyncKeyState(vkey: i32) -> i16;
    }
    const VK_LBUTTON: i32 = 0x01;
    unsafe { GetAsyncKeyState(VK_LBUTTON) < 0 }
}

#[cfg(target_os = "macos")]
fn install_macos_monitors(
    cancel: Arc<AtomicBool>,
    on_mouseup: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>,
    cleanup_slot: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>,
) {
    use block::ConcreteBlock;
    use cocoa::base::{id, nil};
    use objc::{class, msg_send, sel, sel_impl};

    const NS_LEFT_MOUSE_UP: usize = 1 << 2;
    let fired = Arc::new(AtomicBool::new(false));

    let make_fire = |cancel: Arc<AtomicBool>,
                     fired: Arc<AtomicBool>,
                     on_mouseup: Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>| {
        move || {
            if cancel.load(Ordering::SeqCst) {
                return;
            }
            if fired.swap(true, Ordering::SeqCst) {
                return;
            }
            cancel.store(true, Ordering::SeqCst);
            if let Ok(mut guard) = on_mouseup.lock() {
                if let Some(cb) = guard.take() {
                    cb();
                }
            }
        }
    };

    let fire_local = make_fire(cancel.clone(), fired.clone(), on_mouseup.clone());
    let fire_global = make_fire(cancel.clone(), fired.clone(), on_mouseup.clone());

    // Only one of local/global should invoke the callback; `fired` guards that.
    // Store in Mutex so each block can take at most once (blocks are FnMut).
    let local_slot = Arc::new(Mutex::new(Some(fire_local)));
    let global_slot = Arc::new(Mutex::new(Some(fire_global)));

    let local_slot_b = local_slot.clone();
    let local_block = ConcreteBlock::new(move |event: id| -> id {
        if let Ok(mut g) = local_slot_b.lock() {
            if let Some(cb) = g.take() {
                cb();
            }
        }
        event
    });
    let local_block = local_block.copy();

    let global_slot_b = global_slot.clone();
    let global_block = ConcreteBlock::new(move |_event: id| {
        if let Ok(mut g) = global_slot_b.lock() {
            if let Some(cb) = g.take() {
                cb();
            }
        }
    });
    let global_block = global_block.copy();

    let local_monitor: id = unsafe {
        msg_send![
            class!(NSEvent),
            addLocalMonitorForEventsMatchingMask: NS_LEFT_MOUSE_UP
            handler: &*local_block
        ]
    };
    let global_monitor: id = unsafe {
        msg_send![
            class!(NSEvent),
            addGlobalMonitorForEventsMatchingMask: NS_LEFT_MOUSE_UP
            handler: &*global_block
        ]
    };

    eprintln!(
        "[dock-affinity] NSEvent monitors installed (local={}, global={})",
        local_monitor != nil,
        global_monitor != nil
    );

    // AppKit retains the handler blocks for the monitor lifetime. Forget our
    // copies so cleanup stays `Send` (RcBlock is !Send).
    std::mem::forget(local_block);
    std::mem::forget(global_block);

    // `id` is !Send — store as usize for the cleanup closure.
    let local_ptr = local_monitor as usize;
    let global_ptr = global_monitor as usize;

    let cleanup = Box::new(move || {
        unsafe {
            let local_monitor = local_ptr as id;
            let global_monitor = global_ptr as id;
            if local_monitor != nil {
                let _: () = msg_send![class!(NSEvent), removeMonitor: local_monitor];
            }
            if global_monitor != nil {
                let _: () = msg_send![class!(NSEvent), removeMonitor: global_monitor];
            }
        }
        let _ = local_slot.lock().unwrap().take();
        let _ = global_slot.lock().unwrap().take();
        eprintln!("[dock-affinity] NSEvent monitors removed");
    }) as Box<dyn FnOnce() + Send>;

    *cleanup_slot.lock().unwrap() = Some(cleanup);
}
