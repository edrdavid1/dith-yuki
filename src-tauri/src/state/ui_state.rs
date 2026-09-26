use crate::commands::selection::SelectionState;
use crate::viewport::ViewportState;
use std::sync::Mutex;

pub struct UiState {
    pub viewport: Mutex<ViewportState>,
    pub selection: Mutex<SelectionState>,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            viewport: Mutex::new(ViewportState::default()),
            selection: Mutex::new(SelectionState::default()),
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}
