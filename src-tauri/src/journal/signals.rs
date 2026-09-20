//! Best-effort journal flush on terminate signals.
//!
//! Unix: SIGTERM / SIGINT / SIGHUP via signal-hook.
//! Windows: Ctrl+C / console close via ctrlc.
//! Does **not** catch SIGKILL / Task Manager End Task.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::commands::AppState;

pub fn install_signal_flush(app: AppHandle) {
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        use signal_hook::iterator::Signals;

        let app = app.clone();
        let _ = std::thread::Builder::new()
            .name("journal-signals".into())
            .spawn(move || {
                let mut signals = match Signals::new([SIGTERM, SIGINT, SIGHUP]) {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("journal signal-hook init failed: {e}");
                        return;
                    }
                };
                for sig in signals.forever() {
                    log::info!("journal: caught signal {sig}, flushing dirty journals");
                    flush_and_exit(&app);
                }
            });
    }

    #[cfg(windows)]
    {
        let app_ctrlc = app.clone();
        if let Err(e) = ctrlc::set_handler(move || {
            log::info!("journal: ctrlc handler, flushing dirty journals");
            flush_and_exit(&app_ctrlc);
        }) {
            log::warn!("journal ctrlc init failed: {e}");
        }
    }
}

fn flush_and_exit(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>();
    crate::journal::commands::flush_all_dirty_journals(state.inner());
    crate::journal::commands::write_marker_from_app(app);
    std::process::exit(0);
}
