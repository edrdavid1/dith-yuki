//! Theme backdrop for the OS window and the webview.
//!
//! One theme: `--bg-window` / `--color-gray` (`#CDCDCD`). No light/dark switch.
//!
//! The resize bars are the WKWebView view fill, not the NSWindow and not the
//! HTML. `underPageBackgroundColor` only colors overscroll past the page.
//! wry 0.55 applies a real webview color on macOS only when its `transparent`
//! cargo feature is on, and that path sets the private `drawsBackground` key
//! to false. We set that same key here. The window stays opaque (`transparent`
//! is not enabled); the gray NSWindow shows through the unpainted gap.

use tauri::utils::config::Color;
use tauri::webview::PageLoadEvent;
use tauri::WebviewWindow;

/// Opaque `#CDCDCD`. Windows ignores the alpha channel on the window layer.
pub const THEME_BG: Color = Color(0xCD, 0xCD, 0xCD, 0xFF);

/// Re-apply the theme color to the window and the webview.
pub fn apply(window: &WebviewWindow) {
    if let Err(err) = window.set_background_color(Some(THEME_BG)) {
        log::warn!(
            "[window-bg] {}: set_background_color: {err}",
            window.label()
        );
    }
    #[cfg(target_os = "macos")]
    paint_webview(window);
}

/// Dev mode navigates to the Vite server after the window exists. Re-apply once
/// that load finishes so WebKit cannot restore the white view fill.
pub fn reapply_after_load(window: &WebviewWindow, event: PageLoadEvent) {
    if event == PageLoadEvent::Finished {
        apply(window);
    }
}

#[cfg(target_os = "macos")]
fn paint_webview(window: &WebviewWindow) {
    let label = window.label().to_string();
    let err_label = label.clone();
    if let Err(err) = window.with_webview(move |webview| unsafe {
        use cocoa::base::{id, nil, BOOL, NO, YES};
        use cocoa::foundation::NSString;
        use objc::{class, msg_send, sel, sel_impl};

        let view = webview.inner() as id;
        if view.is_null() {
            log::warn!("[window-bg] {label}: WKWebView handle is null");
            return;
        }

        let bg: id = msg_send![
            class!(NSColor),
            colorWithRed: (0xCD as f64) / 255.0
            green: (0xCD as f64) / 255.0
            blue: (0xCD as f64) / 255.0
            alpha: 1.0f64
        ];

        // Private KVC. Stops the default white fill in the live-resize gap.
        let key = NSString::alloc(nil).init_str("drawsBackground");
        let no: id = msg_send![class!(NSNumber), numberWithBool: NO];
        let _: () = msg_send![view, setValue: no forKey: key];

        let setter = sel!(setUnderPageBackgroundColor:);
        let responds: BOOL = msg_send![view, respondsToSelector: setter];
        if responds != NO {
            let _: () = msg_send![view, setUnderPageBackgroundColor: bg];
        }

        let _: () = msg_send![view, setWantsLayer: YES];
        let layer: id = msg_send![view, layer];
        if !layer.is_null() {
            let cg: id = msg_send![bg, CGColor];
            if !cg.is_null() {
                let _: () = msg_send![layer, setBackgroundColor: cg];
            }
        }
    }) {
        log::warn!("[window-bg] {err_label}: webview backdrop: {err}");
    }
}
