//! Security-scoped bookmarks for files the user picked.
//!
//! Direct (non-sandbox) builds still record them when macOS allows it. Store
//! builds need them to open the same path after relaunch. Not a recent-files
//! list: ICC profiles and palettes live here too.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::platform::{self, BookmarkGuard};

struct LiveGrant {
    path: String,
    _guard: BookmarkGuard,
}

fn live_grants() -> &'static Mutex<Vec<LiveGrant>> {
    static LIVE: OnceLock<Mutex<Vec<LiveGrant>>> = OnceLock::new();
    LIVE.get_or_init(|| Mutex::new(Vec::new()))
}

const MAX_BOOKMARKS: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BookmarkEntry {
    path: String,
    bookmark: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct BookmarkFile {
    entries: Vec<BookmarkEntry>,
}

fn store_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to resolve app data directory")
        .join("file_bookmarks.json")
}

fn load(file: &Path) -> BookmarkFile {
    let Ok(text) = std::fs::read_to_string(file) else {
        return BookmarkFile::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

fn save(file: &Path, data: &BookmarkFile) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    std::fs::write(file, text).map_err(|e| e.to_string())
}

fn already_live(path: &str) -> bool {
    live_grants()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|g| g.path == path)
}

fn retain_grant(path: &str, bookmark: &str) {
    if already_live(path) {
        return;
    }
    let guard = platform::begin_bookmark_access(Some(bookmark));
    if !guard.is_active() {
        return;
    }
    live_grants()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(LiveGrant {
            path: path.to_string(),
            _guard: guard,
        });
}

/// Start access for every bookmark saved last session. Call once at startup,
/// before recent files or filter paths are read.
pub fn activate_stored(app: &tauri::AppHandle) {
    for entry in load(&store_path(app)).entries {
        retain_grant(&entry.path, &entry.bookmark);
    }
}

pub fn remember(app: &tauri::AppHandle, path: &str) {
    let Some(bookmark) = platform::create_security_bookmark(path) else {
        return;
    };
    let file = store_path(app);
    let mut data = load(&file);
    data.entries.retain(|e| e.path != path);
    data.entries.insert(
        0,
        BookmarkEntry {
            path: path.to_string(),
            bookmark: bookmark.clone(),
        },
    );
    data.entries.truncate(MAX_BOOKMARKS);
    if let Err(e) = save(&file, &data) {
        log::warn!("failed to store file bookmark for {path}: {e}");
    }
    retain_grant(path, &bookmark);
}

pub fn bookmark_for(app: &tauri::AppHandle, path: &str) -> Option<String> {
    load(&store_path(app))
        .entries
        .into_iter()
        .find(|e| e.path == path)
        .map(|e| e.bookmark)
}

/// Recent-list prune: a bookmarked path stays even when `exists()` is false
/// inside the sandbox, until the bookmark itself no longer resolves.
pub fn path_reachable(app: &tauri::AppHandle, path: &str) -> bool {
    if already_live(path) || Path::new(path).exists() {
        return true;
    }
    bookmark_for(app, path)
        .as_deref()
        .is_some_and(platform::bookmark_still_valid)
}

#[tauri::command]
pub fn remember_user_file(app: tauri::AppHandle, path: String) {
    remember(&app, &path);
}
