//! W5: flex-popout OS window lifecycle tied to the main window.
//!
//! Reloading or destroying `main` leaves orphaned `flex-popout-*` shells
//! (portals die with the main JS tree). Close them on main close/reload/crash
//! and scrub any leftovers at startup.

use tauri::{AppHandle, Manager};

pub const FLEX_POPOUT_PREFIX: &str = "flex-popout-";

pub fn is_flex_popout_label(label: &str) -> bool {
    label.starts_with(FLEX_POPOUT_PREFIX)
}

/// Close every `flex-popout-*` WebviewWindow. Best-effort; logs each label.
pub fn close_all_flex_popouts(app: &AppHandle) {
    let targets: Vec<_> = app
        .webview_windows()
        .into_iter()
        .filter(|(label, _)| is_flex_popout_label(label))
        .collect();

    if targets.is_empty() {
        return;
    }

    eprintln!(
        "[flex-dock] close_all_flex_popouts count={} labels={:?}",
        targets.len(),
        targets.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>()
    );

    for (label, win) in targets {
        if let Err(err) = win.close() {
            eprintln!("[flex-dock] close_all_flex_popouts failed label={label}: {err}");
        }
    }
}

/// Startup scrub: drop any flex-popout that survived without a live main window.
pub fn close_orphan_flex_popouts_at_startup(app: &AppHandle) {
    let has_main = app.get_webview_window("main").is_some();
    if has_main {
        // Main exists — only scrub if we somehow still have stale shells from a
        // previous main navigation in this process (normally none at cold start).
        close_all_flex_popouts(app);
        return;
    }
    close_all_flex_popouts(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flex_popout_prefix_match() {
        assert!(is_flex_popout_label("flex-popout-1"));
        assert!(is_flex_popout_label("flex-popout-99"));
        assert!(!is_flex_popout_label("main"));
        assert!(!is_flex_popout_label("flex-popout"));
        assert!(!is_flex_popout_label("panel-layers"));
    }
}
