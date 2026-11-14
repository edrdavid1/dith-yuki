//! Diagnosis helpers.
//!
//! DevTools / Inspect are on for debug builds (`tauri dev`). Release stays off
//! so the product does not look like a browser, unless `DITHER_DEVTOOLS=1`.
//! Optional WebView2 remote debugging: `DITHER_REMOTE_DEVTOOLS=1`
//! → Chrome on another machine can open `http://<windows-ip>:9222`.

use tauri::webview::WebviewWindowBuilder;
use tauri::{Manager, Runtime};

fn wants_devtools() -> bool {
    cfg!(debug_assertions) || std::env::var_os("DITHER_DEVTOOLS").is_some()
}

/// Apply diagnosis options shared by every window (main, panels, flex popouts).
pub fn apply<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    let builder = if wants_devtools() {
        builder.devtools(true)
    } else {
        builder
    };

    // WebView2 only; harmless no-op elsewhere if args are ignored.
    if std::env::var_os("DITHER_REMOTE_DEVTOOLS").is_some() {
        eprintln!("[webview-debug] DITHER_REMOTE_DEVTOOLS set — remote debugging on port 9222");
        log::info!("webview-debug: remote debugging enabled on port 9222");
        builder.additional_browser_args("--remote-debugging-port=9222")
    } else {
        builder
    }
}
