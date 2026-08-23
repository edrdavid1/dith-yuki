use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::services::FilterService;
pub use crate::services::filter_service::{
    AddFilterRequest, FilterIdResponse, RemoveFilterRequest, ReorderFilterRequest,
    UpdateFilterRequest,
};

#[tauri::command]
pub fn add_filter(
    req: AddFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<FilterIdResponse, String> {
    FilterService::new(state.inner().clone())
        .add_filter(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn remove_filter(
    req: RemoveFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    FilterService::new(state.inner().clone())
        .remove_filter(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reorder_filter(
    req: ReorderFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    FilterService::new(state.inner().clone())
        .reorder_filter(&app_handle, req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_filter(
    req: UpdateFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    FilterService::new(state.inner().clone())
        .update_filter(&app_handle, req)
        .map_err(|e| e.to_string())
}
