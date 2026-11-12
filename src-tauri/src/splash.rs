//! Startup splash: visible feedback while main loads — not a timed brand screen.
//!
//! Main stays hidden until the UI is ready (`finishBoot` shows it, or the 15s
//! watchdog). Splash is destroyed after main is shown so there is no windowless gap.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager};

/// If the frontend never responds, force-reveal main.
const BOOT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct SplashState {
    revealed: AtomicBool,
}

impl SplashState {
    pub fn new() -> Self {
        Self {
            revealed: AtomicBool::new(false),
        }
    }
}

fn reveal_on_main(app: &AppHandle) {
    let state = app.state::<SplashState>();
    if state.revealed.swap(true, Ordering::SeqCst) {
        // Already revealed (JS showed main) — still ensure splash is gone.
        if let Some(splash) = app.get_webview_window("splash") {
            let _ = splash.destroy();
        }
        return;
    }

    // Show main first, then close splash: no windowless gap.
    if let Some(main) = app.get_webview_window("main") {
        if let Err(e) = main.show() {
            eprintln!("[splash] main.show failed: {e}");
        }
        if let Err(e) = main.set_focus() {
            eprintln!("[splash] main.set_focus failed: {e}");
        }
        let _ = main.eval(
            "(function(){var a=document.activeElement;if(a&&a.closest&&a.closest('[role=\"menubar\"]'))a.blur();})()",
        );
    } else {
        eprintln!("[splash] main window not found");
    }

    if let Some(splash) = app.get_webview_window("splash") {
        if let Err(e) = splash.destroy() {
            eprintln!("[splash] splash.destroy failed: {e}");
        }
    }
}

/// Idempotent. Safe to call from IPC or the watchdog thread.
pub fn reveal_main(app: &AppHandle) {
    let app_for_main = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        reveal_on_main(&app_for_main);
    }) {
        eprintln!("[splash] run_on_main_thread failed: {e}");
        // Last resort: try inline (watchdog / tests).
        reveal_on_main(app);
    }
}

pub fn set_stage(app: &AppHandle, text: &str) {
    if let Some(splash) = app.get_webview_window("splash") {
        let arg = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
        let _ = splash.eval(&format!("window.setStage && window.setStage({arg})"));
    }
}

pub fn start_watchdog(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(BOOT_TIMEOUT);
        if !app.state::<SplashState>().revealed.load(Ordering::SeqCst) {
            eprintln!("[splash] boot timeout, forcing reveal");
            reveal_main(&app);
        }
    });
}

#[tauri::command]
pub fn boot_finished(app: AppHandle) {
    // JS usually already showed main; this tears down splash (and covers watchdog).
    reveal_main(&app);
}

#[tauri::command]
pub fn boot_stage(app: AppHandle, text: String) {
    set_stage(&app, &text);
}
