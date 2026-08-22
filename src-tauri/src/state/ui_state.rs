use std::sync::Mutex;
use crate::commands::selection::SelectionState;
use crate::panel_manager::PanelManager;
use crate::viewport::ViewportState;

pub struct UiState {
    pub viewport: Mutex<ViewportState>,
    pub panel_manager: Mutex<PanelManager>,
    pub selection: Mutex<SelectionState>,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            viewport: Mutex::new(ViewportState::default()),
            panel_manager: Mutex::new(PanelManager::new()),
            selection: Mutex::new(SelectionState::default()),
        }
    }
}

impl Default for UiState {
    fn default() -> Self {
        Self::new()
    }
}
