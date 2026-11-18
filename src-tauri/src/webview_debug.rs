//! Diagnosis helpers.
//!
//! DevTools / Inspect are on for debug builds (`tauri dev`). A direct release
//! stays off unless `DITHER_DEVTOOLS=1`. Store channels never honor that
//! variable, and they never open a remote debugging port.
//! Optional WebView2 remote debugging (direct only): `DITHER_REMOTE_DEVTOOLS=1`
//! → Chrome on another machine can open `http://<windows-ip>:9222`.

use tauri::webview::WebviewWindowBuilder;
use tauri::{Manager, Runtime};

fn store_channel() -> bool {
    cfg!(any(feature = "channel-mas", feature = "channel-msstore"))
}

fn wants_devtools() -> bool {
    if store_channel() {
        cfg!(debug_assertions)
    } else {
        cfg!(debug_assertions) || std::env::var_os("DITHER_DEVTOOLS").is_some()
    }
}

/// Apply options shared by every window (main, panels, flex popouts).
pub fn apply<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> WebviewWindowBuilder<'a, R, M> {
    // WryWebView implements `acceptsFirstMouse:` itself and returns this flag.
    // A WKWebView swizzle never runs for that subclass, so the first click in
    // an inactive Layers or Effect popout was swallowed as window activation.
    let builder = builder.accept_first_mouse(true);

    let builder = if wants_devtools() {
        builder.devtools(true)
    } else {
        builder
    };

    // WebView2 only; harmless no-op elsewhere if args are ignored.
    // Store binaries must not expose a remote inspector.
    if !store_channel() && std::env::var_os("DITHER_REMOTE_DEVTOOLS").is_some() {
        eprintln!("[webview-debug] DITHER_REMOTE_DEVTOOLS set — remote debugging on port 9222");
        log::info!("webview-debug: remote debugging enabled on port 9222");
        builder.additional_browser_args("--remote-debugging-port=9222")
    } else {
        builder
    }
}
