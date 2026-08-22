use std::sync::Arc;
use tauri::State;

use crate::commands::AppState;
use crate::services::ViewportService;
pub use crate::viewport::SetViewportResponse;

/// Set the current viewport state.
#[tauri::command]
pub fn set_viewport(
    zoom: f64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    state: State<'_, Arc<AppState>>,
) -> Result<SetViewportResponse, String> {
    ViewportService::new(state.inner().clone())
        .set_viewport(zoom, x, y, width, height)
        .map_err(|e| e.to_string())
}
