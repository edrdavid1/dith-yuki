use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::services::UndoService;
pub use crate::undo::{DirtyDto, UndoStateDto};

#[tauri::command]
pub fn undo(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UndoStateDto, String> {
    UndoService::new(state.inner().clone())
        .undo(&app_handle, doc_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn redo(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UndoStateDto, String> {
    UndoService::new(state.inner().clone())
        .redo(&app_handle, doc_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn is_document_dirty(
    doc_id: Option<u32>,
    state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    UndoService::new(state.inner().clone())
        .is_dirty(doc_id)
        .map_err(|e| e.to_string())
}
