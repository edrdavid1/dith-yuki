use std::sync::Arc;
use tauri::AppHandle;

use crate::commands::AppState;
use crate::services::AppError;
use crate::undo::{apply_redo, apply_undo, is_dirty_doc, UndoStateDto};

pub struct UndoService {
    state: Arc<AppState>,
}

impl UndoService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub fn undo(&self, app_handle: &AppHandle, doc_id: u32) -> Result<UndoStateDto, AppError> {
        apply_undo(&self.state, app_handle, doc_id).map_err(AppError::Generic)
    }

    pub fn redo(&self, app_handle: &AppHandle, doc_id: u32) -> Result<UndoStateDto, AppError> {
        apply_redo(&self.state, app_handle, doc_id).map_err(AppError::Generic)
    }

    pub fn is_dirty(&self, doc_id: Option<u32>) -> Result<bool, AppError> {
        let id = match doc_id.or_else(|| self.state.active_id()) {
            Some(id) => id,
            None => return Ok(false),
        };
        Ok(is_dirty_doc(&self.state, id))
    }
}
