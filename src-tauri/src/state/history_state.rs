use std::sync::{Arc, Mutex};
use engine_project::document::Document;
use crate::undo::UndoManager;

pub struct HistoryState {
    pub undo_manager: Mutex<UndoManager>,
    pub saved_snapshot: Mutex<Option<Arc<Document>>>,
}

impl HistoryState {
    pub fn new() -> Self {
        Self {
            undo_manager: Mutex::new(UndoManager::new()),
            saved_snapshot: Mutex::new(None),
        }
    }
}

impl Default for HistoryState {
    fn default() -> Self {
        Self::new()
    }
}
