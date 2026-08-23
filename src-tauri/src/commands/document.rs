use std::sync::Arc;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, State};

use crate::commands::{AppState, QuitGuard, DocumentResponse, emit_document_changed, schedule_dirty_viewport_tiles};
use crate::document_session::{emit_tabs_changed, OpenDocumentsPayload};
use crate::services::{DocumentService, AppError};
pub use crate::services::document_service::{
    BlankBackground, DocumentResponse as DocResponse, ExportImageRequest, ExportPatternRequest,
    ImportPatternRequest, ImportPatternResponse, LoadImageResponse, OpenProjectResponse,
    SaveProjectResponse, MAX_DOCUMENT_DIMENSION, IMAGE_IMPORT_EXTENSIONS,
    validate_document_dimensions, place_image_at_origin, blank_rgba_f32, f32_to_u8,
    encode_rgba_to_png,
};

#[tauri::command]
pub fn allow_app_exit(gate: State<'_, Arc<QuitGuard>>) {
    gate.allow_exit.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn confirm_app_quit(app: AppHandle, gate: State<'_, Arc<QuitGuard>>) {
    gate.allow_exit.store(true, Ordering::SeqCst);
    app.exit(0);
}

#[tauri::command]
pub fn new_document(
    width: u32,
    height: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    DocumentService::new(state.inner().clone())
        .new_document(width, height, &app_handle)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_document_snapshot(
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    DocumentService::new(state.inner().clone())
        .get_document_snapshot()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn load_image(
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<LoadImageResponse, String> {
    let recent_path = path.clone();
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let service = DocumentService::new(state);
        service.load_image_blocking(&path, &app_handle)
    })
    .await
    .map_err(|e| format!("Load error: {}", e))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_document(
    width: u32,
    height: u32,
    background: BlankBackground,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<LoadImageResponse, String> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let service = DocumentService::new(state);
        service.create_document_blocking(width, height, background, &app_handle)
    })
    .await
    .map_err(|e| format!("Create error: {}", e))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_image_layer(
    doc_id: u32,
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<crate::commands::layers::LayerIdResponse, String> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let service = DocumentService::new(state);
        service.import_image_layer_blocking(doc_id, &path, &app_handle)
    })
    .await
    .map_err(|e| format!("Load error: {}", e))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_project(
    doc_id: u32,
    path: Option<String>,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<SaveProjectResponse, String> {
    DocumentService::new(state.inner().clone())
        .save_project(doc_id, path, app_handle)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_project_as(
    doc_id: u32,
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<SaveProjectResponse, String> {
    DocumentService::new(state.inner().clone())
        .save_project_as(doc_id, path, app_handle)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_project(
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<OpenProjectResponse, String> {
    DocumentService::new(state.inner().clone())
        .open_project(path, app_handle)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn export_pattern(
    req: ExportPatternRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    DocumentService::new(state.inner().clone())
        .export_pattern(req)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn import_pattern(
    req: ImportPatternRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ImportPatternResponse, String> {
    DocumentService::new(state.inner().clone())
        .import_pattern(req, &app_handle)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_image(
    req: ExportImageRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    DocumentService::new(state.inner().clone())
        .export_image(req)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_open_documents(state: State<'_, Arc<AppState>>) -> OpenDocumentsPayload {
    state.tab_list()
}

#[tauri::command]
pub fn set_active_document(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    DocumentService::new(state.inner().clone())
        .set_active_document(doc_id, &app_handle)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn close_document(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<OpenDocumentsPayload, String> {
    state.close_session(doc_id)?;
    emit_document_changed(&app_handle, "document_closed", None, Some(doc_id));
    emit_tabs_changed(Some(&app_handle), &state);
    Ok(state.tab_list())
}
