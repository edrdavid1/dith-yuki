use std::sync::Arc;
use tauri::State;

use crate::commands::AppState;
use crate::services::ViewportService;
pub use crate::viewport::SetViewportResponse;

/// Set the current viewport state.
/// Optional `trace_seq` is only used when `DITHER_TRACE=1` (FE send correlation).
#[tauri::command]
pub fn set_viewport(
    zoom: f64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    trace_seq: Option<u64>,
    state: State<'_, Arc<AppState>>,
) -> Result<SetViewportResponse, String> {
    ViewportService::new(state.inner().clone())
        .set_viewport_traced(zoom, x, y, width, height, trace_seq)
        .map_err(|e| e.to_string())
}
