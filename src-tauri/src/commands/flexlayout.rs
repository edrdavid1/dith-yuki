//! FlexLayout layout persistence commands.
//!
//! Provides IPC commands for saving/loading/resetting the FlexLayout Model JSON.
//! These commands are called from the React frontend when the layout changes or
//! at startup/shutdown to persist layout state to disk.

use tauri::{State, Manager};
use std::sync::Arc;

use crate::commands::AppState;
use crate::flexlayout_persistence::{FlexLayoutPersistence, LayoutPersistenceError};

#[tauri::command]
pub fn save_layout(
    state: State<Arc<AppState>>,
    json: String,
) -> Result<(), String> {
    let persistence = state
        .flexlayout_persistence
        .lock()
        .map_err(|e| format!("Failed to lock persistence: {}", e))?;

    persistence.save(&json).map_err(|e| match e {
        LayoutPersistenceError::InvalidJson(msg) => format!("Invalid layout JSON: {}", msg),
        LayoutPersistenceError::WriteError(msg) => {
            format!("Failed to save layout: {}", msg)
        }
        LayoutPersistenceError::DirectoryError(msg) => {
            format!("Failed to create layout directory: {}", msg)
        }
        _ => "Failed to save layout".to_string(),
    })
}

/// Load layout JSON from disk.
///
/// Called from React on app startup to restore the persisted layout.
/// If the layout file is missing or corrupt, returns the default v3 layout.
/// If v2 layout file is detected (old system), returns default v3 layout
/// and shows migration toast to the user.
///
/// # Returns
/// - `Ok(json)` with layout JSON string (either persisted or default)
/// - `Err(String)` only on unrecoverable disk I/O errors
#[tauri::command]
pub fn load_layout(state: State<Arc<AppState>>) -> Result<String, String> {
    let persistence = state
        .flexlayout_persistence
        .lock()
        .map_err(|e| format!("Failed to lock persistence: {}", e))?;

    persistence.load().map_err(|e| match e {
        LayoutPersistenceError::ReadError(msg) => {
            format!("Failed to read layout file: {}", msg)
        }
        _ => "Failed to load layout".to_string(),
    })
}

/// Reset layout to default v3.
///
/// Called when user explicitly requests "Reset Layout to Default" action.
/// Does not modify the persisted file — the React component is responsible
/// for calling `save_layout()` after this if it wants to persist the reset.
///
/// # Returns
/// - `Ok(json)` with default v3 layout JSON
#[tauri::command]
pub fn reset_layout_to_default(state: State<Arc<AppState>>) -> Result<String, String> {
    // Verify we can lock persistence (good practice, though default doesn't require it)
    let _persistence = state
        .flexlayout_persistence
        .lock()
        .map_err(|e| format!("Failed to lock persistence: {}", e))?;

    Ok(FlexLayoutPersistence::default_layout_json())
}

// ─── B4a: per-side commands ───────────────────────────────────────────────────

#[tauri::command]
pub fn load_layout_left(state: State<Arc<AppState>>) -> Result<String, String> {
    let p = state.flexlayout_left.lock()
        .map_err(|e| format!("Failed to lock left persistence: {}", e))?;
    p.load().map_err(|e| format!("Failed to load left layout: {:?}", e))
}

#[tauri::command]
pub fn save_layout_left(
    state: State<Arc<AppState>>,
    json: String,
) -> Result<(), String> {
    let p = state.flexlayout_left.lock()
        .map_err(|e| format!("Failed to lock left persistence: {}", e))?;
    p.save(&json).map_err(|e| format!("Failed to save left layout: {:?}", e))
}

#[tauri::command]
pub fn load_layout_right(state: State<Arc<AppState>>) -> Result<String, String> {
    let p = state.flexlayout_right.lock()
        .map_err(|e| format!("Failed to lock right persistence: {}", e))?;
    p.load().map_err(|e| format!("Failed to load right layout: {:?}", e))
}

#[tauri::command]
pub fn save_layout_right(
    state: State<Arc<AppState>>,
    json: String,
) -> Result<(), String> {
    let p = state.flexlayout_right.lock()
        .map_err(|e| format!("Failed to lock right persistence: {}", e))?;
    p.save(&json).map_err(|e| format!("Failed to save right layout: {:?}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_json_matches_static_version() {
        let default1 = FlexLayoutPersistence::default_layout_json();
        let default2 = FlexLayoutPersistence::default_layout_json();
        assert_eq!(default1, default2);
    }

    #[test]
    fn default_layout_is_valid_json() {
        let json = FlexLayoutPersistence::default_layout_json();
        serde_json::from_str::<serde_json::Value>(&json)
            .expect("default layout must be valid JSON");
    }
}
