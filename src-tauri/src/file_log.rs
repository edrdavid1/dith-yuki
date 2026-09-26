//! File + stderr logging for diagnosing Windows (and other) production builds.
//!
//! Log path: `{app_data_dir}/logs/app.log`
//! - Windows: `%APPDATA%\com.dither.app\logs\app.log`
//! - macOS: `~/Library/Application Support/com.dither.app/logs/app.log`
//! - Linux: `$XDG_DATA_HOME/com.dither.app/logs/app.log` (or `~/.local/share/...`)

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const APP_DATA_DIR_NAME: &str = "com.dither.app";

struct TeeLogger {
    file: Mutex<Option<File>>,
}

impl log::Log for TeeLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "[{}][{}] {}\n",
            chrono_like_now(),
            record.level(),
            record.args()
        );
        eprint!("{line}");
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.write_all(line.as_bytes());
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

/// Lightweight timestamp without pulling in `chrono`.
fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

fn default_log_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        return base.join(APP_DATA_DIR_NAME).join("logs");
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join("Library")
            .join("Application Support")
            .join(APP_DATA_DIR_NAME)
            .join("logs")
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("."));
                home.join(".local").join("share")
            });
        base.join(APP_DATA_DIR_NAME).join("logs")
    }
}

/// Open `{dir}/app.log`, creating parent dirs. Returns `None` if open fails.
fn open_log_file(dir: &PathBuf) -> Option<File> {
    if let Err(e) = fs::create_dir_all(dir) {
        eprintln!("[file-log] failed to create {}: {e}", dir.display());
        return None;
    }
    let path = dir.join("app.log");
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(f) => {
            eprintln!("[file-log] writing to {}", path.display());
            Some(f)
        }
        Err(e) => {
            eprintln!("[file-log] failed to open {}: {e}", path.display());
            None
        }
    }
}

/// Install a process-wide logger (stderr + file). Safe to call once at startup,
/// before the Tauri window exists — catches GPU / protocol failures that never
/// reach the frontend.
pub fn init() {
    let file = open_log_file(&default_log_dir());
    let logger = TeeLogger {
        file: Mutex::new(file),
    };
    let _ = log::set_boxed_logger(Box::new(logger));
    log::set_max_level(log::LevelFilter::Info);

    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<Any>".into()
        };
        log::error!("panic at {location}: {payload}");
        eprintln!("panic at {location}: {payload}");
    }));

    log::info!("file-log initialized");
}
