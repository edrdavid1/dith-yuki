use std::sync::Arc;
use crate::commands::AppState;
use crate::panel_manager::{DockSide, PanelInfo, SavedBounds, SerializedPanelState, UndockResult};
use crate::services::AppError;

pub struct PanelService {
    state: Arc<AppState>,
}

impl PanelService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub fn get_panels_state(&self) -> Result<SerializedPanelState, AppError> {
        let pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        Ok(pm.serialize())
    }

    pub fn undock(
        &self,
        panel_id: &str,
    ) -> Result<(UndockResult, Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let result = pm.undock(panel_id).map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        Ok((result, snapshot, left, right))
    }

    pub fn dock(
        &self,
        panel_id: &str,
    ) -> Result<(Option<String>, Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let side = pm.remembered_dock_side(panel_id);
        let old_label = pm
            .dock(panel_id, side, usize::MAX)
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        Ok((old_label, snapshot, left, right))
    }

    pub fn hide(
        &self,
        panel_id: &str,
    ) -> Result<(bool, Option<String>, Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let changed = pm.hide(panel_id).map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        let window_label = snapshot
            .iter()
            .find(|p| p.id == panel_id)
            .and_then(|p| p.window_label.clone());
        Ok((changed, window_label, snapshot, left, right))
    }

    pub fn show(
        &self,
        panel_id: &str,
    ) -> Result<(bool, Option<String>, Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let changed = pm.show(panel_id).map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        let window_label = snapshot
            .iter()
            .find(|p| p.id == panel_id)
            .and_then(|p| p.window_label.clone());
        Ok((changed, window_label, snapshot, left, right))
    }

    pub fn reorder_side(
        &self,
        side: DockSide,
        order: Vec<String>,
    ) -> Result<(Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        pm.reorder_side(side, order)
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        Ok((snapshot, left, right))
    }

    pub fn move_to_side(
        &self,
        panel_id: &str,
        side: DockSide,
        index: usize,
    ) -> Result<(Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        pm.move_to_side(panel_id, side, index)
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        Ok((snapshot, left, right))
    }

    pub fn move_all_to_side(
        &self,
        side: DockSide,
    ) -> Result<(Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        pm.move_all_to_side(side)
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        Ok((snapshot, left, right))
    }

    pub fn swap_sidebars(&self) -> Result<(Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        pm.swap_sides();
        let (snapshot, left, right) = pm.get_state_with_orders();
        Ok((snapshot, left, right))
    }

    pub fn save_bounds(&self, panel_id: &str, bounds: SavedBounds) -> Result<(), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        pm.update_bounds(panel_id, bounds)
            .map_err(|e| AppError::Generic(e.to_string()))?;
        Ok(())
    }

    pub fn dock_at(
        &self,
        panel_id: &str,
        side: DockSide,
        index: usize,
    ) -> Result<(Option<String>, Vec<PanelInfo>, Vec<String>, Vec<String>), AppError> {
        let mut pm = self
            .state
            .ui
            .panel_manager
            .lock()
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let old_label = pm
            .dock(panel_id, side, index)
            .map_err(|e| AppError::Generic(e.to_string()))?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        Ok((old_label, snapshot, left, right))
    }
}
