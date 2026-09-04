use crate::commands::AppState;
use std::sync::Arc;
use tauri::State;

/// Track O: launch auto-check is release-only (`cfg!(debug_assertions)` skip).
#[tauri::command]
pub fn is_release_build() -> bool {
    !cfg!(debug_assertions)
}

/// A8: atlas occupancy. No GPU → zeros. Not a product UI surface.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuVramStatusDto {
    pub available: bool,
    pub live_slots: u32,
    pub max_slots: u32,
    pub free_slots: u32,
    pub peak_live: u32,
    pub pressure_evicts: u64,
    pub budget_bytes: u64,
    pub occupancy: f64,
    pub peak_occupancy: f64,
}

#[tauri::command]
pub fn get_gpu_vram_status(state: State<'_, Arc<AppState>>) -> GpuVramStatusDto {
    let Some(cache) = state.gpu_resident.as_ref() else {
        return GpuVramStatusDto {
            available: false,
            live_slots: 0,
            max_slots: 0,
            free_slots: 0,
            peak_live: 0,
            pressure_evicts: 0,
            budget_bytes: 0,
            occupancy: 0.0,
            peak_occupancy: 0.0,
        };
    };
    let s = cache.vram_stats();
    GpuVramStatusDto {
        available: true,
        live_slots: s.live_slots,
        max_slots: s.max_slots,
        free_slots: s.free_slots,
        peak_live: s.peak_live,
        pressure_evicts: s.pressure_evicts,
        budget_bytes: s.budget_bytes,
        occupancy: s.occupancy(),
        peak_occupancy: s.peak_occupancy(),
    }
}
