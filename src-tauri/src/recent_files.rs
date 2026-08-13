//! Disk persistence for the Recent Files list.
//!
//! Same idiom as `panel_persistence.rs`: JSON in `{app_data_dir}/recent_files.json`.
//! Missing or corrupt files are treated as an empty list. Write failures are logged
//! and never fail the caller (open/save already succeeded).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::Manager;

pub const MAX_RECENT: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecentFileKind {
    Image,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentFileEntry {
    pub path: String,
    pub kind: RecentFileKind,
    pub display_name: String,
    pub opened_at: String,
}

/// `{app_data_dir}/recent_files.json`
pub fn recent_files_path(app_handle: &tauri::AppHandle) -> PathBuf {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data directory");
    data_dir.join("recent_files.json")
}

/// Load the list from `file`. Missing or corrupt JSON → empty vec (not an error).
pub fn load_recent_files(file: &Path) -> Vec<RecentFileEntry> {
    let contents = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "Failed to read recent files at {}: {}",
                    file.display(),
                    e
                );
            }
            return Vec::new();
        }
    };

    match serde_json::from_str::<Vec<RecentFileEntry>>(&contents) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!(
                "Failed to parse recent files at {}: {}; treating as empty",
                file.display(),
                e
            );
            Vec::new()
        }
    }
}

/// Insert `path` at the front (dedup by exact path string), cap at `MAX_RECENT`, write.
pub fn record_recent_file(
    file: &Path,
    path: &str,
    kind: RecentFileKind,
) -> Result<(), String> {
    let mut entries = load_recent_files(file);
    entries.retain(|e| e.path != path);
    entries.insert(
        0,
        RecentFileEntry {
            path: path.to_string(),
            kind,
            display_name: display_name_for(path),
            opened_at: now_rfc3339(),
        },
    );
    entries.truncate(MAX_RECENT);
    save_recent_files(file, &entries)
}

/// Log-and-ignore wrapper so open/save commands never fail on recent-file I/O.
pub fn try_record_recent_file(file: &Path, path: &str, kind: RecentFileKind) {
    if let Err(e) = record_recent_file(file, path, kind) {
        log::warn!("Failed to record recent file {}: {}", path, e);
    }
}

pub fn record_from_app(app: &tauri::AppHandle, path: &str, kind: RecentFileKind) {
    let file = recent_files_path(app);
    try_record_recent_file(&file, path, kind);
}

/// Prune dead paths on read. Rewrite errors are logged; the IPC call never fails.
#[tauri::command]
pub fn get_recent_files(app_handle: tauri::AppHandle) -> Vec<RecentFileEntry> {
    let path = recent_files_path(&app_handle);
    load_prune_and_maybe_rewrite(&path)
}

/// Drop entries whose `path` no longer exists on disk.
pub fn prune_missing(entries: Vec<RecentFileEntry>) -> (Vec<RecentFileEntry>, bool) {
    let before = entries.len();
    let kept: Vec<RecentFileEntry> = entries
        .into_iter()
        .filter(|e| Path::new(&e.path).exists())
        .collect();
    let dropped_any = kept.len() != before;
    (kept, dropped_any)
}

/// Load, prune dead paths, rewrite if anything was dropped. Rewrite errors are logged;
/// the filtered in-memory list is still returned.
pub fn load_prune_and_maybe_rewrite(file: &Path) -> Vec<RecentFileEntry> {
    let loaded = load_recent_files(file);
    let (kept, dropped) = prune_missing(loaded);
    if dropped {
        if let Err(e) = save_recent_files(file, &kept) {
            log::warn!(
                "Failed to rewrite recent files after prune at {}: {}",
                file.display(),
                e
            );
        }
    }
    kept
}

fn save_recent_files(file: &Path, entries: &[RecentFileEntry]) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            format!("Failed to create recent-files directory {}: {e}", parent.display())
        })?;
    }
    let json = serde_json::to_string_pretty(entries).map_err(|e| e.to_string())?;
    std::fs::write(file, json)
        .map_err(|e| format!("Failed to write recent files at {}: {e}", file.display()))
}

fn display_name_for(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| path.to_string())
}

fn now_rfc3339() -> String {
    let dur = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    format_rfc3339_utc(dur)
}

/// UTC RFC 3339 / ISO-8601 with millisecond precision (`1970-01-01T00:00:00.000Z`).
fn format_rfc3339_utc(dur: Duration) -> String {
    let secs = dur.as_secs();
    let millis = dur.subsec_millis();
    let (year, month, day, hour, min, sec) = civil_from_unix(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}.{millis:03}Z")
}

/// Howard Hinnant `civil_from_days` from Unix seconds → (Y, M, D, h, m, s).
fn civil_from_unix(secs: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = (secs % 86400) as u32;
    let hour = rem / 3600;
    let min = (rem % 3600) / 60;
    let sec = rem % 60;

    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m as u32, d as u32, hour, min, sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_json() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent_files.json");
        (dir, path)
    }

    #[test]
    fn epoch_formats_as_rfc3339() {
        assert_eq!(
            format_rfc3339_utc(Duration::ZERO),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            format_rfc3339_utc(Duration::from_secs(86400) + Duration::from_millis(1)),
            "1970-01-02T00:00:00.001Z"
        );
    }

    #[test]
    fn missing_file_loads_empty() {
        let (_dir, path) = temp_json();
        assert!(load_recent_files(&path).is_empty());
    }

    #[test]
    fn corrupt_json_loads_empty() {
        let (_dir, path) = temp_json();
        std::fs::write(&path, "{ this is not valid json }").unwrap();
        assert!(load_recent_files(&path).is_empty());
    }

    #[test]
    fn record_dedups_and_moves_to_front() {
        let (_dir, json) = temp_json();
        let a = "/tmp/a.png";
        let b = "/tmp/b.png";

        record_recent_file(&json, a, RecentFileKind::Image).unwrap();
        record_recent_file(&json, b, RecentFileKind::Image).unwrap();
        std::thread::sleep(Duration::from_millis(2));
        record_recent_file(&json, a, RecentFileKind::Image).unwrap();

        let entries = load_recent_files(&json);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, a);
        assert_eq!(entries[0].kind, RecentFileKind::Image);
        assert_eq!(entries[0].display_name, "a.png");
        assert_eq!(entries[1].path, b);
        assert!(entries[0].opened_at >= entries[1].opened_at);
        assert!(entries[0].opened_at.ends_with('Z'));
    }

    #[test]
    fn record_caps_at_max_recent() {
        let (_dir, json) = temp_json();
        for i in 0..(MAX_RECENT + 1) {
            record_recent_file(&json, &format!("/tmp/file-{i}.png"), RecentFileKind::Image)
                .unwrap();
        }
        let entries = load_recent_files(&json);
        assert_eq!(entries.len(), MAX_RECENT);
        assert_eq!(entries[0].display_name, format!("file-{}.png", MAX_RECENT));
        assert_eq!(entries[MAX_RECENT - 1].display_name, "file-1.png");
        assert!(entries.iter().all(|e| e.display_name != "file-0.png"));
    }

    #[test]
    fn prune_drops_missing_path_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let json = dir.path().join("recent_files.json");
        let existing = dir.path().join("exists.png");
        std::fs::write(&existing, b"x").unwrap();
        let missing = dir.path().join("gone.png");

        record_recent_file(
            &json,
            missing.to_str().unwrap(),
            RecentFileKind::Image,
        )
        .unwrap();
        record_recent_file(
            &json,
            existing.to_str().unwrap(),
            RecentFileKind::Project,
        )
        .unwrap();

        let kept = load_prune_and_maybe_rewrite(&json);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].path, existing.to_string_lossy());
        assert_eq!(kept[0].kind, RecentFileKind::Project);

        let reloaded = load_recent_files(&json);
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].path, existing.to_string_lossy());
    }

    #[test]
    fn display_name_falls_back_to_full_path() {
        assert_eq!(display_name_for("/tmp/photo.png"), "photo.png");
        assert_eq!(display_name_for("/"), "/");
    }
}
