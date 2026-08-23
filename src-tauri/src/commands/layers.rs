use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::services::LayerService;
pub use crate::services::layer_service::{
    AddLayerRequest, LayerIdResponse, LayerNodeDto, LayerPropsPatchDto, ReorderLayerRequest,
    SetLayerPropsRequest,
};

#[tauri::command]
pub fn get_layer_tree(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<LayerNodeDto>, String> {
    LayerService::new(state.inner().clone())
        .get_layer_tree()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_layer(
    req: AddLayerRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<LayerIdResponse, String> {
    LayerService::new(state.inner().clone())
        .add_layer(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_layer(
    doc_id: u32,
    layer_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    LayerService::new(state.inner().clone())
        .remove_layer(&app_handle, doc_id, layer_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_layer_props(
    req: SetLayerPropsRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    LayerService::new(state.inner().clone())
        .set_layer_props(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_layer(
    req: ReorderLayerRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    LayerService::new(state.inner().clone())
        .reorder_layer(&app_handle, req)
        .map_err(|e| e.to_string())
}
