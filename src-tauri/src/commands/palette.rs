use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::services::PaletteService;
pub use crate::services::palette_service::{
    AddColorRequest, AddPaletteRequest, BuiltinPaletteDto, CreatePaletteRequest,
    DeletePaletteResponse, ExportPaletteRequest, GeneratePaletteRequest, PaletteDto,
    RenamePaletteRequest, RemoveColorRequest, ReplacePaletteRequest, ReorderColorRequest,
    UpdateColorRequest,
};

#[tauri::command]
pub fn list_palettes(state: State<'_, Arc<AppState>>) -> Result<Vec<PaletteDto>, String> {
    PaletteService::new(state.inner().clone())
        .list_palettes()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_builtin_palettes() -> Result<Vec<BuiltinPaletteDto>, String> {
    PaletteService::list_builtin_palettes().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_builtin_palette(
    doc_id: u32,
    id: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .import_builtin_palette(&app_handle, doc_id, id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_palette(
    doc_id: u32,
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .import_palette(&app_handle, doc_id, path)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_palette(
    req: AddPaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .add_palette(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn replace_palette(
    req: ReplacePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .replace_palette(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn generate_palette(
    req: GeneratePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let state = Arc::clone(state.inner());
    let service = PaletteService::new(state.clone());
    tauri::async_runtime::spawn_blocking(move || {
        service.generate_palette_blocking(req, Some(&app_handle))
    })
    .await
    .map_err(|e| format!("Palette generation task failed: {}", e))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_palette(
    doc_id: u32,
    palette_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    PaletteService::new(state.inner().clone())
        .remove_palette(&app_handle, doc_id, palette_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn rename_palette(
    req: RenamePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .rename_palette(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_palette(
    req: CreatePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .create_palette(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_palette(
    req: ExportPaletteRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    PaletteService::new(state.inner().clone())
        .export_palette(req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_color_to_palette(
    req: AddColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .add_color_to_palette(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_palette_color(
    req: UpdateColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .update_palette_color(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_palette_color(
    req: RemoveColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .remove_palette_color(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_palette_color(
    req: ReorderColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    PaletteService::new(state.inner().clone())
        .reorder_palette_color(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_palette(
    doc_id: u32,
    palette_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DeletePaletteResponse, String> {
    PaletteService::new(state.inner().clone())
        .delete_palette(&app_handle, doc_id, palette_id)
        .map_err(|e| e.to_string())
}
