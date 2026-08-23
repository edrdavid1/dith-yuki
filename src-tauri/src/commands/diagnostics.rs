use std::sync::Arc;
use tauri::State;
use crate::commands::{schedule_dirty_viewport_tiles, AppState};

/// Track O: launch auto-check is release-only (`cfg!(debug_assertions)` skip).
#[tauri::command]
pub fn is_release_build() -> bool {
    !cfg!(debug_assertions)
}

/// Industrial-gate T10: Preferences GPU preview opt-in status.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuPreviewStatus {
    /// Effective gate (`gpu_preview_enabled`).
    pub enabled: bool,
    /// Adapter + resident executor present.
    pub available: bool,
    /// `DITHER_GPU_PREVIEW` env is set (overrides Preferences for soak/CI).
    pub env_forced: bool,
}

#[tauri::command]
pub fn get_gpu_preview_status(state: State<'_, Arc<AppState>>) -> GpuPreviewStatus {
    GpuPreviewStatus {
        enabled: engine_gpu::gpu_preview_enabled(),
        available: state.gpu.is_some() && state.gpu_executor.is_some(),
        env_forced: std::env::var("DITHER_GPU_PREVIEW").is_ok(),
    }
}

/// Preferences: set Path B GPU preview authorship (UI override). Env still wins when set.
#[tauri::command]
pub fn set_gpu_preview_enabled(
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<GpuPreviewStatus, String> {
    engine_gpu::set_gpu_preview_ui_override(Some(enabled));
    // Re-author visible Composite under the new gate (CPU or GPU).
    if let Ok(session) = state.active_session() {
        let doc = session.document_handle.snapshot().id.0;
        let viewport = state.ui.viewport.lock().unwrap().clone();
        for coord in &viewport.visible_tiles {
            state.tiles.tile_cache.mark_dirty(engine_tiles::TileKey {
                doc,
                layer: 0,
                coord: *coord,
                stage: engine_tiles::CacheStage::Composite,
            });
        }
        schedule_dirty_viewport_tiles(&state);
    }
    Ok(get_gpu_preview_status(state))
}
