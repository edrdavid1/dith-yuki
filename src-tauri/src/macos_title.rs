//! Center the native macOS window title in the system title bar.

#[cfg(target_os = "macos")]
use cocoa::base::{id, nil, NO, YES};
#[cfg(target_os = "macos")]
use cocoa::foundation::{NSPoint, NSRect, NSSize, NSString};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};

const FILL_ID: &str = "dy-titlebar-fill";
const LABEL_ID: &str = "dy-centered-title";
const CHROME: (f64, f64, f64) = (153.0 / 255.0, 153.0 / 255.0, 153.0 / 255.0);
const TITLE_ACTIVE: (f64, f64, f64) = (0.0, 0.0, 0.0);
const TITLE_INACTIVE: (f64, f64, f64) = (0.22, 0.22, 0.22);
/// Flexible left+right+top+bottom margins — keeps a fixed-size view centered.
const CENTER_MASK: u64 = 1 | 4 | 8 | 32;

#[cfg(not(target_os = "macos"))]
pub fn apply_overlay_csd(_window: &tauri::WebviewWindow) {}

#[cfg(not(target_os = "macos"))]
pub fn refresh_traffic_lights(_window: &tauri::Window) {}

/// Draw the webview under the traffic lights so File/Edit sit on the same row.
///
/// **Do not** manually `setFrame` the traffic lights on resize. Tauri/tao already
/// re-applies `trafficLightPosition` from `tauri.conf.json` inside the NSView
/// `drawRect` path. A second DidResize layout fought that every frame and made
/// the buttons jump while dragging the window edge.
#[cfg(target_os = "macos")]
pub fn apply_overlay_csd(window: &tauri::WebviewWindow) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as id;
    let _ = window.set_title_bar_style(tauri::TitleBarStyle::Overlay);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        let _: () = msg_send![ns_window, setTitleVisibility: 1]; // NSWindowTitleHidden
        let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: YES];
        let mask: u64 = msg_send![ns_window, styleMask];
        let full_size: u64 = 1 << 15; // NSFullSizeContentViewWindowMask
        if mask & full_size == 0 {
            let _: () = msg_send![ns_window, setStyleMask: mask | full_size];
        }
        // macOS 11+: kill the native separator line above the HTML chrome.
        let _: () = msg_send![ns_window, setTitlebarSeparatorStyle: 0]; // None
        install_zoom_not_fullscreen(ns_window);
        observe_zoom_not_fullscreen(ns_window);
    }));

    // Native chrome can reset the green button action after first paint.
    let delayed = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let win = delayed.clone();
        let _ = delayed.run_on_main_thread(move || {
            if let Ok(ns) = win.ns_window() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
                    install_zoom_not_fullscreen(ns as id);
                }));
            }
        });
    });
}

/// Re-apply zoom wiring after exit fullscreen (no traffic-light frame writes).
#[cfg(target_os = "macos")]
pub fn refresh_traffic_lights(window: &tauri::Window) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        install_zoom_not_fullscreen(ns_window as id);
    }));
}

/// Photoshop-style green button: `zoom:` (fit screen), never Mission Control fullscreen.
#[cfg(target_os = "macos")]
unsafe fn install_zoom_not_fullscreen(ns_window: id) {
    use cocoa::appkit::{NSWindow, NSWindowButton, NSWindowCollectionBehavior};

    let mut behavior = ns_window.collectionBehavior();
    behavior.remove(NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenPrimary);
    behavior.remove(NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary);
    // NSWindowCollectionBehaviorFullScreenNone (10.11+) — not in cocoa 0.26 bitflags.
    const FULL_SCREEN_NONE: u64 = 1 << 9;
    let bits = behavior.bits() | FULL_SCREEN_NONE;
    ns_window.setCollectionBehavior_(NSWindowCollectionBehavior::from_bits_truncate(bits));

    // Strip fullscreen bit from style mask if present.
    let mask: u64 = msg_send![ns_window, styleMask];
    const FULLSCREEN_MASK: u64 = 1 << 14; // NSFullScreenWindowMask
    if mask & FULLSCREEN_MASK != 0 {
        let _: () = msg_send![ns_window, setStyleMask: mask & !FULLSCREEN_MASK];
    }

    // Default green-button click is toggleFullScreen on modern macOS — force zoom:.
    let zoom: id = ns_window.standardWindowButton_(NSWindowButton::NSWindowZoomButton);
    if zoom != nil {
        let _: () = msg_send![zoom, setTarget: ns_window];
        let _: () = msg_send![zoom, setAction: sel!(zoom:)];
    }
}

/// Keep green-button → zoom: after AppKit fullscreen transitions.
#[cfg(target_os = "macos")]
unsafe fn observe_zoom_not_fullscreen(ns_window: id) {
    use block::ConcreteBlock;

    let center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
    let names = [
        "NSWindowDidEndLiveResizeNotification",
        "NSWindowDidExitFullScreenNotification",
        "NSWindowDidEnterFullScreenNotification",
    ];
    for name in names {
        let ns_name: id = ns_string(name);
        let win = ns_window;
        let exit_fs_if_entered = name == "NSWindowDidEnterFullScreenNotification";
        let block = ConcreteBlock::new(move |notification: id| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let obj: id = msg_send![notification, object];
                if obj != win {
                    return;
                }
                if exit_fs_if_entered {
                    let _: () = msg_send![win, toggleFullScreen: nil];
                }
                install_zoom_not_fullscreen(win);
            }));
        });
        let block = block.copy();
        let _: id = msg_send![
            center,
            addObserverForName: ns_name
            object: ns_window
            queue: nil
            usingBlock: &*block
        ];
        std::mem::forget(block);
    }

    let will: id = ns_string("NSWindowWillEnterFullScreenNotification");
    let win = ns_window;
    let block = ConcreteBlock::new(move |notification: id| {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let obj: id = msg_send![notification, object];
            if obj != win {
                return;
            }
            let _: () = msg_send![win, toggleFullScreen: nil];
            install_zoom_not_fullscreen(win);
        }));
    });
    let block = block.copy();
    let _: id = msg_send![
        center,
        addObserverForName: will
        object: ns_window
        queue: nil
        usingBlock: &*block
    ];
    std::mem::forget(block);
}

#[cfg(target_os = "macos")]
pub fn install_centered_title(window: &tauri::WebviewWindow) {
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe {
        install_on_ns_window(ns_window as id);
    }));
}

#[cfg(target_os = "macos")]
unsafe fn install_on_ns_window(ns_window: id) {
    let Some(titlebar) = titlebar_view(ns_window) else {
        return;
    };
    let host = title_host(ns_window, titlebar);

    let _: () = msg_send![ns_window, setTitleVisibility: 1]; // NSWindowTitleHidden
    let _: () = msg_send![ns_window, setTitlebarAppearsTransparent: NO];

    let fill = find_by_id(titlebar, FILL_ID).unwrap_or_else(|| make_fill());
    if find_by_id(titlebar, FILL_ID).is_none() {
        let _: () = msg_send![titlebar, addSubview: fill];
    }

    let label = find_by_id(host, LABEL_ID)
        .or_else(|| find_by_id(titlebar, LABEL_ID))
        .unwrap_or_else(|| make_label());
    if find_by_id(host, LABEL_ID).is_none() {
        let _: () = msg_send![host, addSubview: label];
        observe_window(ns_window, label);
    }

    sync_label(ns_window, label);
    layout_views(titlebar, host, fill, label);
}

#[cfg(target_os = "macos")]
unsafe fn make_fill() -> id {
    let fill: id = msg_send![class!(NSBox), alloc];
    let fill: id = msg_send![
        fill,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(1.0, 1.0))
    ];
    let _: () = msg_send![fill, setIdentifier: ns_string(FILL_ID)];
    let _: () = msg_send![fill, setBoxType: 4]; // NSBoxCustom
    let _: () = msg_send![fill, setBorderType: 0]; // NSNoBorder
    let _: () = msg_send![fill, setTitlePosition: 0]; // NSNoTitle
    let _: () = msg_send![fill, setContentViewMargins: NSSize::new(0.0, 0.0)];
    let _: () = msg_send![fill, setAutoresizingMask: 18];
    let color: id = msg_send![
        class!(NSColor),
        colorWithCalibratedRed: CHROME.0
        green: CHROME.1
        blue: CHROME.2
        alpha: 1.0
    ];
    let _: () = msg_send![fill, setFillColor: color];
    fill
}

#[cfg(target_os = "macos")]
unsafe fn make_label() -> id {
    let label: id = msg_send![class!(NSTextField), alloc];
    let label: id = msg_send![
        label,
        initWithFrame: NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(200.0, 22.0))
    ];
    let _: () = msg_send![label, setIdentifier: ns_string(LABEL_ID)];
    let _: () = msg_send![label, setEditable: NO];
    let _: () = msg_send![label, setSelectable: NO];
    let _: () = msg_send![label, setBezeled: NO];
    let _: () = msg_send![label, setBordered: NO];
    let _: () = msg_send![label, setDrawsBackground: NO];
    let _: () = msg_send![label, setAlignment: 2];
    let _: () = msg_send![label, setTranslatesAutoresizingMaskIntoConstraints: YES];
    let _: () = msg_send![label, setAutoresizingMask: CENTER_MASK];
    let font: id = msg_send![class!(NSFont), systemFontOfSize: 13f64];
    if font != nil {
        let _: () = msg_send![label, setFont: font];
    }
    label
}

#[cfg(target_os = "macos")]
unsafe fn titlebar_view(ns_window: id) -> Option<id> {
    let close: id = msg_send![ns_window, standardWindowButton: 0];
    if close == nil {
        return None;
    }
    let titlebar: id = msg_send![close, superview];
    if titlebar == nil {
        None
    } else {
        Some(titlebar)
    }
}

/// Walk up until the view is as wide as the window, so the title can sit on the
/// true horizontal center (NSTitlebarView pins extra subviews to the trailing edge).
unsafe fn title_host(ns_window: id, titlebar: id) -> id {
    let content: id = msg_send![ns_window, contentView];
    let mut target_w = 0.0;
    if content != nil {
        let cf: NSRect = msg_send![content, frame];
        target_w = cf.size.width;
    }
    let mut view = titlebar;
    for _ in 0..6 {
        let bounds: NSRect = msg_send![view, bounds];
        if target_w > 0.0 && bounds.size.width >= target_w - 1.0 {
            return view;
        }
        let parent: id = msg_send![view, superview];
        if parent == nil {
            break;
        }
        view = parent;
    }
    if content != nil {
        let theme: id = msg_send![content, superview];
        if theme != nil {
            return theme;
        }
    }
    titlebar
}

#[cfg(target_os = "macos")]
unsafe fn find_by_id(parent: id, id_str: &str) -> Option<id> {
    let views: id = msg_send![parent, subviews];
    if views == nil {
        return None;
    }
    let count: usize = msg_send![views, count];
    let want: id = ns_string(id_str);
    for i in 0..count {
        let view: id = msg_send![views, objectAtIndex: i];
        let ident: id = msg_send![view, identifier];
        if ident == nil {
            continue;
        }
        let equal: BOOLISH = msg_send![ident, isEqual: want];
        if equal != 0 {
            return Some(view);
        }
    }
    None
}

#[cfg(target_os = "macos")]
unsafe fn layout_views(titlebar: id, host: id, fill: id, label: id) {
    let bar_bounds: NSRect = msg_send![titlebar, bounds];
    let _: () = msg_send![fill, setFrame: bar_bounds];

    let _: () = msg_send![label, sizeToFit];
    let fitted: NSRect = msg_send![label, frame];
    let host_bounds: NSRect = msg_send![host, bounds];

    let mut title_h = bar_bounds.size.height;
    if title_h < 16.0 {
        title_h = 28.0;
    }
    let mut title_y = host_bounds.size.height - title_h;
    if host == titlebar {
        title_y = 0.0;
        title_h = host_bounds.size.height;
    } else {
        let bar_in_host: NSRect = msg_send![host, convertRect: bar_bounds fromView: titlebar];
        title_y = bar_in_host.origin.y;
        title_h = bar_in_host.size.height.max(16.0);
    }

    let max_w = (host_bounds.size.width - 156.0).max(80.0);
    let w = (fitted.size.width.max(40.0) + 8.0).min(max_w);
    let h = fitted.size.height.max(14.0);
    let x = ((host_bounds.size.width - w) / 2.0).max(0.0);
    let y = title_y + ((title_h - h) / 2.0).max(0.0);
    let rect = NSRect::new(NSPoint::new(x, y), NSSize::new(w, h));
    let _: () = msg_send![label, setFrame: rect];
}

#[cfg(target_os = "macos")]
unsafe fn sync_label(ns_window: id, label: id) {
    let title: id = msg_send![ns_window, title];
    if title != nil {
        let _: () = msg_send![label, setStringValue: title];
    }
    let is_main: BOOLISH = msg_send![ns_window, isMainWindow];
    let (r, g, b) = if is_main != 0 {
        TITLE_ACTIVE
    } else {
        TITLE_INACTIVE
    };
    let color: id = msg_send![
        class!(NSColor),
        colorWithCalibratedRed: r
        green: g
        blue: b
        alpha: 1.0
    ];
    let _: () = msg_send![label, setTextColor: color];
}

type BOOLISH = i8;

#[cfg(target_os = "macos")]
unsafe fn observe_window(ns_window: id, label: id) {
    use block::ConcreteBlock;

    let center: id = msg_send![class!(NSNotificationCenter), defaultCenter];
    let names = [
        "NSWindowDidResizeNotification",
        "NSWindowDidBecomeMainNotification",
        "NSWindowDidResignMainNotification",
        "NSWindowDidUpdateNotification",
    ];
    for name in names {
        let ns_name: id = ns_string(name);
        let win = ns_window;
        let lbl = label;
        let always_layout = name != "NSWindowDidUpdateNotification";
        let block = ConcreteBlock::new(move |notification: id| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let obj: id = msg_send![notification, object];
                if obj != win {
                    return;
                }
                let before: id = msg_send![lbl, stringValue];
                sync_label(win, lbl);
                let after: id = msg_send![lbl, stringValue];
                let unchanged: BOOLISH = if before != nil && after != nil {
                    msg_send![before, isEqual: after]
                } else {
                    0
                };
                if !always_layout && unchanged != 0 {
                    return;
                }
                if let Some(titlebar) = titlebar_view(win) {
                    let host = title_host(win, titlebar);
                    if let (Some(fill), Some(found)) = (
                        find_by_id(titlebar, FILL_ID),
                        find_by_id(host, LABEL_ID)
                            .or_else(|| find_by_id(titlebar, LABEL_ID))
                            .or(Some(lbl)),
                    ) {
                        layout_views(titlebar, host, fill, found);
                    }
                }
            }));
        });
        let block = block.copy();
        let _: id = msg_send![
            center,
            addObserverForName: ns_name
            object: ns_window
            queue: nil
            usingBlock: &*block
        ];
        std::mem::forget(block);
    }
}

#[cfg(target_os = "macos")]
unsafe fn ns_string(s: &str) -> id {
    NSString::alloc(nil).init_str(s)
}
