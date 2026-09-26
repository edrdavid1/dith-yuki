use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::commands::{
    emit_document_changed, schedule_dirty_viewport_tiles, AppState, DocumentResponse, QuitGuard,
};
use crate::document_session::{emit_tabs_changed, OpenDocumentsPayload};
pub use crate::services::document_service::{
    AsciiClipboardRequest, BlankBackground, ExportAsciiRequest, ExportImageRequest,
    ExportPatternRequest, ImportPatternRequest, ImportPatternResponse, LoadImageResponse,
    OpenProjectResponse, SaveProjectResponse, ShareProjectCopyOptions,
};
use crate::services::DocumentService;

#[tauri::command]
pub fn allow_app_exit(app: AppHandle, gate: State<'_, Arc<QuitGuard>>) {
    gate.allow_exit.store(true, Ordering::SeqCst);
    // Soft-discard B: intentional exit clears leftover journals, then marker.
    let _ = crate::journal::commands::discard_recovery_journals(None, app.clone());
    crate::journal::commands::write_marker_from_app(&app);
}

#[tauri::command]
pub fn confirm_app_quit(app: AppHandle, gate: State<'_, Arc<QuitGuard>>) {
    gate.allow_exit.store(true, Ordering::SeqCst);
    let _ = crate::journal::commands::discard_recovery_journals(None, app.clone());
    crate::journal::commands::write_marker_from_app(&app);
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
pub fn get_document_snapshot(state: State<'_, Arc<AppState>>) -> Result<DocumentResponse, String> {
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
pub async fn share_project_copy(
    doc_id: u32,
    path: String,
    opts: Option<ShareProjectCopyOptions>,
    state: State<'_, Arc<AppState>>,
) -> Result<SaveProjectResponse, String> {
    DocumentService::new(state.inner().clone())
        .share_project_copy(doc_id, path, opts.unwrap_or_default())
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
pub async fn export_ascii(
    req: ExportAsciiRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    DocumentService::new(state.inner().clone())
        .export_ascii(req)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ascii_clipboard_text(
    req: AsciiClipboardRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<String, String> {
    DocumentService::new(state.inner().clone())
        .ascii_clipboard_text(req)
        .await
        .map_err(|e| e.to_string())
}

/// Preview Image|ASCII switch. `true` = show ASCII raster; `false` = Image (skip Ascii filters).
///
/// Does **not** drop full-document cache entries: Image and ASCII results coexist so
/// toggling republishes from cache instead of recomputing ASCII. Processed/Composite
/// tiles are marked dirty so the viewport swaps to the other mode.
#[tauri::command]
pub fn set_ascii_preview(enabled: bool, state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    use engine_tiles::CacheStage;

    let prev = state.ascii_preview.swap(enabled, Ordering::Relaxed);
    if prev == enabled {
        return Ok(enabled);
    }

    let mut keys = Vec::new();
    for entry in state.tiles.tile_cache.entries.iter() {
        let key = *entry.key();
        if matches!(key.stage, CacheStage::Processed | CacheStage::Composite) {
            keys.push(key);
        }
    }
    for key in keys {
        state.tiles.tile_cache.mark_dirty(key);
    }
    schedule_dirty_viewport_tiles(state.inner());
    Ok(enabled)
}

#[tauri::command]
pub fn get_ascii_preview(state: State<'_, Arc<AppState>>) -> bool {
    state.ascii_preview.load(Ordering::Relaxed)
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
