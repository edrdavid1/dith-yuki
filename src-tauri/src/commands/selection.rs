use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, State};

use crate::commands::AppState;

/// Cross-window selection state. Updated via selection-changed events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelectionState {
    pub selected_layer_id: Option<u32>,
    pub selected_filter_id: Option<String>,
}

/// Payload emitted with the `selection-changed` Tauri event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionChangedPayload {
    pub selected_layer_id: Option<u32>,
    pub selected_filter_id: Option<String>,
}

/// Update selection state and broadcast to all windows.
#[tauri::command]
pub fn set_selection(
    layer_id: Option<u32>,
    filter_id: Option<String>,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut sel = state.ui.selection.lock().map_err(|e| e.to_string())?;
    sel.selected_layer_id = layer_id;
    sel.selected_filter_id = filter_id.clone();
    drop(sel);

    let _ = app_handle.emit_to(
        tauri::EventTarget::Any,
        "selection-changed",
        SelectionChangedPayload {
            selected_layer_id: layer_id,
            selected_filter_id: filter_id,
        },
    );

    Ok(())
}

/// Get current selection state (for initial fetch on window mount).
#[tauri::command]
pub fn get_selection(state: State<'_, Arc<AppState>>) -> Result<SelectionState, String> {
    let sel = state.ui.selection.lock().map_err(|e| e.to_string())?;
    Ok(sel.clone())
}
