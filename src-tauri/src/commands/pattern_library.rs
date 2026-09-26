use std::sync::Arc;
use tauri::{AppHandle, State};

use crate::commands::AppState;
use crate::pattern_library::{
    self, PatternLibraryEntry,
};
use crate::services::document_service::{
    DocumentService, ExportPatternRequest, ImportPatternRequest, ImportPatternResponse,
};

#[tauri::command]
pub fn list_pattern_library(app: AppHandle) -> Result<Vec<PatternLibraryEntry>, String> {
    let data = pattern_library::app_data_dir(&app)?;
    pattern_library::list_library(&data)
}

#[tauri::command]
pub fn save_pattern_to_library(
    req: ExportPatternRequest,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PatternLibraryEntry, String> {
    let data = pattern_library::app_data_dir(&app)?;
    let name = req
        .name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Pattern".into());
    let (id, path) = pattern_library::allocate_save_path(&data, &name)?;
    let mut req = req;
    req.path = path.to_string_lossy().into_owned();
    req.name = Some(name.clone());
    DocumentService::new(state.inner().clone())
        .export_pattern(req)
        .map_err(|e| e.to_string())?;
    let entries = pattern_library::list_library(&data)?;
    entries
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| "Saved pattern missing from library".into())
}

#[tauri::command]
pub fn apply_pattern_from_library(
    doc_id: u32,
    pattern_id: String,
    target_layer_id: u32,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ImportPatternResponse, String> {
    let data = pattern_library::app_data_dir(&app)?;
    let path = pattern_library::resolve_library_file(&data, &pattern_id)?;
    DocumentService::new(state.inner().clone())
        .import_pattern(
            ImportPatternRequest {
                doc_id,
                path: path.to_string_lossy().into_owned(),
                target_layer_id,
            },
            &app,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_pattern_from_library(pattern_id: String, app: AppHandle) -> Result<(), String> {
    let data = pattern_library::app_data_dir(&app)?;
    pattern_library::delete_from_library(&data, &pattern_id)
}

#[tauri::command]
pub fn rename_pattern_in_library(
    pattern_id: String,
    name: String,
    app: AppHandle,
) -> Result<PatternLibraryEntry, String> {
    let data = pattern_library::app_data_dir(&app)?;
    pattern_library::rename_in_library(&data, &pattern_id, &name)
}

#[tauri::command]
pub fn import_pattern_to_library(path: String, app: AppHandle) -> Result<PatternLibraryEntry, String> {
    let data = pattern_library::app_data_dir(&app)?;
    pattern_library::import_file_to_library(&data, std::path::Path::new(&path))
}

#[tauri::command]
pub fn export_pattern_from_library(
    pattern_id: String,
    path: String,
    app: AppHandle,
) -> Result<(), String> {
    let data = pattern_library::app_data_dir(&app)?;
    pattern_library::export_from_library(&data, &pattern_id, std::path::Path::new(&path))
}
