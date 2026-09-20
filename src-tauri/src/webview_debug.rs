//! Release-build diagnosis helpers (Windows-first).
//!
//! - DevTools stay available in non-dev builds (`Cargo` feature `devtools`).
//! - Optional WebView2 remote debugging via `DITHER_REMOTE_DEVTOOLS=1`
//!   → Chrome on another machine can open `http://<windows-ip>:9222`.

use tauri::webview::WebviewWindowBuilder;
use tauri::{Manager, Runtime};

/// Apply diagnosis options shared by every window (main, panels, flex popouts).
pub fn apply<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    let builder = builder.devtools(true);

    // WebView2 only; harmless no-op elsewhere if args are ignored.
    if std::env::var_os("DITHER_REMOTE_DEVTOOLS").is_some() {
        eprintln!(
            "[webview-debug] DITHER_REMOTE_DEVTOOLS set — remote debugging on port 9222"
        );
        log::info!("webview-debug: remote debugging enabled on port 9222");
        builder.additional_browser_args("--remote-debugging-port=9222")
    } else {
        builder
    }
}
