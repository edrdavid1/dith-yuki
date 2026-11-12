//! FlexLayout shell persistence commands (W7/W10).
//!
//! SoT is `load_shell_layout` / `save_shell_layout` (single `layout.json`).
//! Legacy per-side and `flexlayout_state` commands were removed in W10.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::State;

use crate::commands::AppState;
use crate::shell_layout_persistence::{LayoutEnvelope, LAYOUT_VERSION};

/// Load the v1 layout.json envelope (migrates legacy side files when needed).
#[tauri::command]
pub fn load_shell_layout(state: State<Arc<AppState>>) -> Result<String, String> {
    let p = state
        .shell_layout
        .lock()
        .map_err(|e| format!("Failed to lock shell layout: {e}"))?;
    let env = p
        .load()
        .map_err(|e| format!("Failed to load shell layout: {e}"))?;
    serde_json::to_string(&env).map_err(|e| format!("serialize envelope: {e}"))
}

/// Save the full left/center/right envelope atomically.
#[tauri::command]
pub fn save_shell_layout(state: State<Arc<AppState>>, json: String) -> Result<(), String> {
    let mut env: LayoutEnvelope = serde_json::from_str(&json)
        .map_err(|e| format!("Invalid shell layout JSON: {e}"))?;
    if env.version == 0 {
        env.version = LAYOUT_VERSION;
    }
    let p = state
        .shell_layout
        .lock()
        .map_err(|e| format!("Failed to lock shell layout: {e}"))?;
    p.save(&env)
        .map_err(|e| format!("Failed to save shell layout: {e}"))
}

/// FE ack after handling `flush-layout` (W7 close flush).
#[tauri::command]
pub fn layout_flushed(state: State<Arc<AppState>>) -> Result<(), String> {
    state.layout_flush_ack.store(true, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn shell_commands_are_the_only_layout_ipc() {
        // W10: deprecated save_layout / load_layout_* must not reappear as commands.
        // Strip the test module so the banned-name list here cannot false-positive.
        let src = include_str!("flexlayout.rs");
        let prod = src.split("#[cfg(test)]").next().unwrap_or(src);
        for banned in [
            "pub fn save_layout(",
            "pub fn load_layout(",
            "pub fn reset_layout_to_default(",
            "pub fn load_layout_left(",
            "pub fn save_layout_left(",
            "pub fn load_layout_right(",
            "pub fn save_layout_right(",
            "pub fn load_layout_center(",
            "pub fn save_layout_center(",
        ] {
            assert!(
                !prod.contains(banned),
                "deprecated layout command must stay removed: {banned}"
            );
        }
        assert!(prod.contains("pub fn load_shell_layout("));
        assert!(prod.contains("pub fn save_shell_layout("));
        assert!(prod.contains("pub fn layout_flushed("));
    }
}
