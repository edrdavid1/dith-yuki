//! Tauri IPC for crash-recovery journals.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use uuid::Uuid;

use crate::commands::{emit_document_changed, schedule_dirty_viewport_tiles, AppState};
use crate::document_session::emit_tabs_changed;
use crate::journal::meta::{
    journal_blob_path, list_metas, read_meta, JournalContentKind, JournalMeta,
};
use crate::journal::{delete_journal, recovery_subdir};
use crate::services::document_service::OpenProjectResponse;

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryScanDto {
    /// True when clean_exit.marker was missing (crash / kill / first launch).
    pub previous_unclean: bool,
    pub journals: Vec<JournalMeta>,
}

fn app_data(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))
}

#[tauri::command]
pub fn scan_recovery_journals(app: AppHandle) -> Result<RecoveryScanDto, String> {
    let data = app_data(&app)?;
    let previous_unclean = crate::journal::clean_exit::previous_run_lacks_clean_marker(&data);
    let recovery = recovery_subdir(&data);
    let journals = list_metas(&recovery)
        .into_iter()
        .filter(|m| m.content_kind == JournalContentKind::FullDyproj)
        .collect();
    Ok(RecoveryScanDto {
        previous_unclean,
        journals,
    })
}

#[tauri::command]
pub async fn recover_journal(
    recovery_id: String,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<OpenProjectResponse, String> {
    use engine_project::serialize::open_project_from_bytes;
    use engine_project::types::DocumentId;
    use engine_tiles::TileCache;
    use std::fs;

    let id = Uuid::parse_str(&recovery_id).map_err(|e| e.to_string())?;
    let data = app_data(&app)?;
    let recovery = recovery_subdir(&data);
    let blob = journal_blob_path(&recovery, id);
    if !blob.exists() {
        return Err(format!("Journal blob missing for {recovery_id}"));
    }
    let meta = read_meta(&crate::journal::meta::journal_meta_path(&recovery, id)).ok();

    let zip_bytes = tauri::async_runtime::spawn_blocking({
        let blob = blob.clone();
        move || fs::read(&blob).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Recover read error: {e}"))??;

    let runtime_id = state.alloc_doc_id();
    let staging = TileCache::new(state.tiles.tile_cache.budget_bytes_count());
    let opened = tauri::async_runtime::spawn_blocking(move || {
        open_project_from_bytes(&zip_bytes, &staging, DocumentId::new(runtime_id))
            .map(|r| (r, staging))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Recover open error: {e}"))??;

    let (opened, staging) = opened;
    let live_gen = 1u64;
    for entry in staging.entries.iter() {
        let key = *entry.key();
        let tile = entry.value().tile.clone();
        let _ = state
            .tiles
            .tile_cache
            .insert_fresh_gen(key, tile, live_gen);
    }

    let width = opened.document.width;
    let height = opened.document.height;
    let mut new_doc = opened.document;
    new_doc.increment_generation();
    new_doc.generations.set_document_gen(live_gen);
    let session = state.spawn_session(new_doc);
    state.evict_inactive_for_pressure_if_needed();
    let doc_id = runtime_id;
    crate::undo::clear_history(&state, Some(&app), doc_id)?;
    // Recovered work is unsaved relative to the user's original path.
    if let Ok(mut guard) = session.history.saved_snapshot.lock() {
        *guard = None;
    }
    if let Some(ref m) = meta {
        if let Some(ref p) = m.project_path {
            if let Ok(mut guard) = session.project_path.lock() {
                *guard = Some(PathBuf::from(p));
            }
        } else if let Some(ref p) = m.source_path {
            if let Ok(mut guard) = session.source_path.lock() {
                *guard = Some(PathBuf::from(p));
            }
        }
    }
    crate::undo::emit_dirty_doc(Some(&app), &state, doc_id);
    schedule_dirty_viewport_tiles(&state);
    emit_document_changed(&app, "project_opened", None, Some(doc_id));
    emit_tabs_changed(Some(&app), &state);

    delete_journal(&recovery, id);

    let path = meta
        .and_then(|m| m.project_path.or(m.source_path))
        .unwrap_or_default();

    Ok(OpenProjectResponse {
        doc_id: runtime_id,
        width,
        height,
        path,
    })
}

#[tauri::command]
pub fn discard_recovery_journals(
    recovery_ids: Option<Vec<String>>,
    app: AppHandle,
) -> Result<(), String> {
    let data = app_data(&app)?;
    let recovery = recovery_subdir(&data);
    let metas = list_metas(&recovery);
    let filter: Option<std::collections::HashSet<Uuid>> = recovery_ids.map(|ids| {
        ids.into_iter()
            .filter_map(|s| Uuid::parse_str(&s).ok())
            .collect()
    });
    for meta in metas {
        if let Some(ref set) = filter {
            if !set.contains(&meta.recovery_id) {
                continue;
            }
        }
        delete_journal(&recovery, meta.recovery_id);
    }
    Ok(())
}

/// Write clean-exit marker (call after QuitGuard allows exit).
pub fn write_marker_from_app(app: &AppHandle) {
    if let Ok(data) = app_data(app) {
        if let Err(e) = crate::journal::clean_exit::write_clean_exit_marker(&data) {
            log::warn!("clean_exit marker write failed: {e}");
        }
    }
}
