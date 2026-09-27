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
use crate::journal::roster::{self, SessionRoster};
use crate::journal::{delete_journal, recovery_subdir, write_journal_for_doc};
use crate::services::document_service::OpenProjectResponse;

#[derive(Debug, Clone, Serialize)]
pub struct SoftDiscardDto {
    pub recovery_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecoveryScanDto {
    /// True when clean_exit.marker was missing (crash / kill / first launch).
    pub previous_unclean: bool,
    pub journals: Vec<JournalMeta>,
    /// Last open-tab set (may be present even when journals are empty).
    pub roster: Option<SessionRoster>,
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
    let journals = dedupe_journals_by_source(
        list_metas(&recovery)
            .into_iter()
            .filter(|m| m.content_kind == JournalContentKind::FullDyproj)
            .collect(),
    );
    let roster = if previous_unclean {
        roster::read_roster(&data)
    } else {
        None
    };
    Ok(RecoveryScanDto {
        previous_unclean,
        journals,
        roster,
    })
}

/// Soft-discard can leave an older journal for the same path while a newer
/// session also has one — Recover must open each source once.
fn dedupe_journals_by_source(mut journals: Vec<JournalMeta>) -> Vec<JournalMeta> {
    // Prefer live (non-discarded) journals, then newest write.
    journals.sort_by(|a, b| {
        (!b.discarded)
            .cmp(&(!a.discarded))
            .then_with(|| b.written_at_ms.cmp(&a.written_at_ms))
    });
    let mut seen = std::collections::HashSet::<String>::new();
    let mut out = Vec::with_capacity(journals.len());
    for meta in journals {
        let key = journal_source_key(&meta);
        if !seen.insert(key) {
            continue;
        }
        out.push(meta);
    }
    out.sort_by_key(|m| m.runtime_doc_id);
    out
}

fn journal_source_key(meta: &JournalMeta) -> String {
    if let Some(ref p) = meta.project_path {
        if !p.is_empty() {
            return format!("project:{p}");
        }
    }
    if let Some(ref p) = meta.source_path {
        if !p.is_empty() {
            return format!("source:{p}");
        }
    }
    format!("id:{}", meta.recovery_id)
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
    let meta_path = crate::journal::meta::journal_meta_path(&recovery, id);
    if !blob.exists() {
        return Err(format!("Journal blob missing for {recovery_id}"));
    }
    let meta = read_meta(&meta_path).ok();

    // Claim the journal before opening so a concurrent Recover cannot open a
    // second identical tab from the same blob.
    let zip_bytes = tauri::async_runtime::spawn_blocking({
        let blob = blob.clone();
        let meta_path = meta_path.clone();
        move || {
            let claimed = {
                let mut c = blob.clone();
                let name = blob
                    .file_name()
                    .map(|n| {
                        let mut s = n.to_os_string();
                        s.push(".claiming");
                        s
                    })
                    .unwrap_or_else(|| "journal.claiming".into());
                c.set_file_name(name);
                c
            };
            fs::rename(&blob, &claimed).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    format!("Journal blob missing for {recovery_id}")
                } else {
                    // Another recover already claimed this journal.
                    format!("Journal already claimed for {recovery_id}: {e}")
                }
            })?;
            let bytes = fs::read(&claimed).map_err(|e| e.to_string())?;
            let _ = fs::remove_file(&claimed);
            let _ = fs::remove_file(&meta_path);
            Ok::<Vec<u8>, String>(bytes)
        }
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
        let _ = state.tiles.tile_cache.insert_fresh_gen(key, tile, live_gen);
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

    // Also drop any older soft-discard journals for the same source path.
    if let Some(ref m) = meta {
        purge_sibling_journals_for_source(&recovery, m);
    }

    let path = meta
        .clone()
        .and_then(|m| m.project_path.or(m.source_path))
        .unwrap_or_default();

    Ok(OpenProjectResponse {
        doc_id: runtime_id,
        width,
        height,
        path,
    })
}

fn purge_sibling_journals_for_source(recovery_dir: &std::path::Path, kept: &JournalMeta) {
    let key = journal_source_key(kept);
    if key.starts_with("id:") {
        return;
    }
    for meta in list_metas(recovery_dir) {
        if meta.recovery_id == kept.recovery_id {
            continue;
        }
        if journal_source_key(&meta) == key {
            delete_journal(recovery_dir, meta.recovery_id);
        }
    }
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
        roster::clear_roster(&data);
        if let Err(e) = crate::journal::clean_exit::write_clean_exit_marker(&data) {
            log::warn!("clean_exit marker write failed: {e}");
        }
    }
}

/// Flush journal now and mark it discarded (soft Don't Save on tab ×).
///
/// Skips a full re-serialize when the debounce/heartbeat journal already matches
/// the live document revision — Don't Save should feel instant in that common case.
/// Heavy serialize runs on a blocking pool so the UI IPC loop stays responsive.
#[tauri::command]
pub async fn prepare_soft_discard(
    doc_id: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<SoftDiscardDto, String> {
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || prepare_soft_discard_sync(&state, doc_id))
        .await
        .map_err(|e| format!("prepare_soft_discard join: {e}"))?
}

pub(crate) fn prepare_soft_discard_sync(state: &AppState, doc_id: u32) -> Result<SoftDiscardDto, String> {
    let session = state.require_session(doc_id)?;
    let recovery_id = session.recovery_id;
    let snapshot = session.document_handle.snapshot();
    let revision = snapshot.revision;
    let display_name = {
        let project = session.project_path.lock().ok().and_then(|g| g.clone());
        let source = session.source_path.lock().ok().and_then(|g| g.clone());
        project
            .as_ref()
            .or(source.as_ref())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| format!("Untitled {}", session.id.0))
    };

    let reuse_existing =
        can_reuse_journal_for_soft_discard(state, doc_id, recovery_id, revision);
    if !reuse_existing {
        write_journal_for_doc(state, doc_id)?;
    }

    mark_journal_discarded(state, recovery_id)?;
    // If reuse claimed a journal but meta was missing, mark_journal_discarded is a no-op —
    // rewrite once so Restore still has something to open.
    if reuse_existing && !journal_meta_is_discarded(state, recovery_id) {
        write_journal_for_doc(state, doc_id)?;
        mark_journal_discarded(state, recovery_id)?;
    }

    Ok(SoftDiscardDto {
        recovery_id: recovery_id.to_string(),
        display_name,
    })
}

fn mark_journal_discarded(state: &AppState, recovery_id: Uuid) -> Result<(), String> {
    let Ok(guard) = state.journal.lock() else {
        return Ok(());
    };
    let Some(ref dir) = guard.recovery_dir else {
        return Ok(());
    };
    let meta_path = crate::journal::meta::journal_meta_path(dir, recovery_id);
    if let Ok(mut meta) = read_meta(&meta_path) {
        meta.discarded = true;
        crate::journal::meta::write_meta(dir, &meta)?;
    }
    Ok(())
}

fn journal_meta_is_discarded(state: &AppState, recovery_id: Uuid) -> bool {
    let Ok(guard) = state.journal.lock() else {
        return false;
    };
    let Some(ref dir) = guard.recovery_dir else {
        return false;
    };
    let meta_path = crate::journal::meta::journal_meta_path(dir, recovery_id);
    read_meta(&meta_path)
        .map(|m| m.discarded)
        .unwrap_or(false)
}

fn can_reuse_journal_for_soft_discard(
    state: &AppState,
    doc_id: u32,
    recovery_id: Uuid,
    revision: u64,
) -> bool {
    let Ok(guard) = state.journal.lock() else {
        return false;
    };
    if !guard.flush_matches_revision(doc_id, revision) {
        return false;
    }
    let Some(dir) = guard.recovery_dir.as_ref() else {
        return false;
    };
    let blob = journal_blob_path(dir, recovery_id);
    let meta_path = crate::journal::meta::journal_meta_path(dir, recovery_id);
    if !blob.is_file() {
        return false;
    }
    match read_meta(&meta_path) {
        Ok(meta) => meta.content_kind == JournalContentKind::FullDyproj,
        Err(_) => false,
    }
}

/// Sync flush of every dirty open document (signal handlers / last-chance).
pub fn flush_all_dirty_journals(state: &AppState) {
    let ids: Vec<u32> = match state.sessions.lock() {
        Ok(map) => map.keys().copied().collect(),
        Err(_) => return,
    };
    for doc_id in ids {
        if !crate::undo::is_dirty_doc(state, doc_id) {
            continue;
        }
        if let Err(e) = write_journal_for_doc(state, doc_id) {
            log::warn!("flush_all_dirty_journals doc {doc_id}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::meta::JournalContentKind;
    use uuid::Uuid;

    fn meta(
        recovery_id: Uuid,
        path: &str,
        written_at_ms: u64,
        discarded: bool,
        runtime_doc_id: u32,
    ) -> JournalMeta {
        JournalMeta {
            recovery_id,
            runtime_doc_id,
            display_name: "x.png".into(),
            project_path: None,
            source_path: Some(path.into()),
            original_mtime_ms: None,
            written_at_ms,
            app_version: "0.0.0".into(),
            content_kind: JournalContentKind::FullDyproj,
            discarded,
        }
    }

    #[test]
    fn dedupe_journals_keeps_one_per_source_path() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let c = Uuid::new_v4();
        let out = dedupe_journals_by_source(vec![
            meta(a, "/tmp/one.png", 10, true, 1),
            meta(b, "/tmp/one.png", 20, false, 2),
            meta(c, "/tmp/two.png", 15, false, 3),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].source_path.as_deref(), Some("/tmp/one.png"));
        assert_eq!(out[0].recovery_id, b, "prefer non-discarded for same path");
        assert_eq!(out[1].source_path.as_deref(), Some("/tmp/two.png"));
    }
}
