//! macOS Dock attention while the main window is still hidden at launch.
//!
//! The window starts `visible: false`. Fast cold starts finish before the
//! reveal delay and never touch the Dock. Slow starts request continuous
//! Dock bouncing so the user can tell the app is opening, not frozen.
//!
//! `stop` only cancels attention (and re-activates if we had to yield focus
//! for bouncing). Showing the window is the caller's job.

#[cfg(target_os = "macos")]
mod native {
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
    use std::time::Duration;

    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSRequestUserAttentionType};
    use tauri::{AppHandle, Manager};

    /// Match the old splash gate: under this, launch feels instant — no bounce.
    const REVEAL_DELAY: Duration = Duration::from_millis(400);
    /// Never leave the Dock bouncing if JS never calls finishBoot.
    const MAX_BOUNCE: Duration = Duration::from_secs(15);

    static ARMED: AtomicBool = AtomicBool::new(false);
    static BOUNCING: AtomicBool = AtomicBool::new(false);
    static YIELDED_FOCUS: AtomicBool = AtomicBool::new(false);
    static ATTENTION_ID: AtomicIsize = AtomicIsize::new(-1);

    fn on_main(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
        if MainThreadMarker::new().is_some() {
            f();
        } else {
            let _ = app.run_on_main_thread(f);
        }
    }

    fn cancel_attention_on_main() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let ns_app = NSApplication::sharedApplication(mtm);
        let id = ATTENTION_ID.swap(-1, Ordering::SeqCst);
        if id >= 0 {
            ns_app.cancelUserAttentionRequest(id);
        }
    }

    fn begin_bounce_on_main(app: &AppHandle) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if !ARMED.load(Ordering::SeqCst) || BOUNCING.load(Ordering::SeqCst) {
            return;
        }

        let ns_app = NSApplication::sharedApplication(mtm);

        // Attention requests are ignored while the app is active. Yield focus
        // briefly so the Dock can bounce; stop() restores it once the main
        // window is visible.
        if ns_app.isActive() {
            ns_app.deactivate();
            YIELDED_FOCUS.store(true, Ordering::SeqCst);
        }

        cancel_attention_on_main();

        // Critical = bounce until cancelled or the app becomes active again.
        let req_id = ns_app.requestUserAttention(NSRequestUserAttentionType::CriticalRequest);
        ATTENTION_ID.store(req_id as isize, Ordering::SeqCst);
        BOUNCING.store(true, Ordering::SeqCst);

        if let Some(win) = app.get_webview_window("main") {
            let _ = win.request_user_attention(Some(tauri::UserAttentionType::Critical));
        }

        log::info!("macos_dock_bounce: bouncing (attention id={req_id})");
    }

    /// Arm a delayed Dock bounce for a slow cold start.
    pub fn start(app_handle: &AppHandle) {
        if ARMED.swap(true, Ordering::SeqCst) {
            return;
        }

        let delayed = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(REVEAL_DELAY);
            if !ARMED.load(Ordering::SeqCst) {
                return;
            }
            on_main(&delayed, {
                let h = delayed.clone();
                move || begin_bounce_on_main(&h)
            });
        });

        let timeout = app_handle.clone();
        std::thread::spawn(move || {
            std::thread::sleep(MAX_BOUNCE);
            if ARMED.load(Ordering::SeqCst) || BOUNCING.load(Ordering::SeqCst) {
                log::warn!("macos_dock_bounce: safety timeout — cancelling attention");
                stop(&timeout);
            }
        });
    }

    /// Cancel Dock attention. Does not show the window.
    ///
    /// If we yielded activation so the bounce could run, and the main window
    /// is already visible, restore frontmost status.
    pub fn stop(app_handle: &AppHandle) {
        let was_armed = ARMED.swap(false, Ordering::SeqCst);
        let was_bouncing = BOUNCING.swap(false, Ordering::SeqCst);
        let yielded = YIELDED_FOCUS.load(Ordering::SeqCst);

        if !was_armed && !was_bouncing && !yielded {
            return;
        }

        let h = app_handle.clone();
        on_main(app_handle, move || {
            if was_armed || was_bouncing {
                cancel_attention_on_main();
                if let Some(win) = h.get_webview_window("main") {
                    let _ = win.request_user_attention(None);
                }
            }

            let visible = h
                .get_webview_window("main")
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false);

            // Keep YIELDED_FOCUS until the window is actually shown so a
            // premature Focused/stop does not lose the restore-frontmost duty.
            if yielded && visible {
                YIELDED_FOCUS.store(false, Ordering::SeqCst);
                if let Some(mtm) = MainThreadMarker::new() {
                    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
                }
            }

            if was_bouncing {
                log::info!("macos_dock_bounce: stopped");
            }
        });
    }
}

#[cfg(target_os = "macos")]
pub use native::{start, stop};

#[cfg(not(target_os = "macos"))]
pub fn start(_app_handle: &tauri::AppHandle) {}

#[cfg(not(target_os = "macos"))]
pub fn stop(_app_handle: &tauri::AppHandle) {}
