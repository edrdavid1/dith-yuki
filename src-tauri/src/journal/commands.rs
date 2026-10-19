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
use crate::journal::stats::JournalWriteSource;
use crate::journal::{delete_journal, flush_doc_sync, journal_state_path, recovery_subdir};
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
            .filter(|m| {
                matches!(
                    m.content_kind,
                    JournalContentKind::FullDyproj | JournalContentKind::StateV1
                )
            })
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
    let state_path = journal_state_path(&recovery, id);
    let meta_path = crate::journal::meta::journal_meta_path(&recovery, id);
    let meta = read_meta(&meta_path).ok();
    let is_state_v1 = state_path.is_file()
        && meta
            .as_ref()
            .map(|m| m.content_kind == JournalContentKind::StateV1)
            .unwrap_or(true);
    if !blob.exists() && !is_state_v1 {
        return Err(format!(
            "Recovery journal is missing for «{}».",
            meta.as_ref()
                .map(|m| m.display_name.as_str())
                .unwrap_or(recovery_id.as_str())
        ));
    }

    let display = meta
        .as_ref()
        .map(|m| m.display_name.clone())
        .unwrap_or_else(|| recovery_id.clone());

    // Claim → assemble/read → open. Delete claimed files only after a successful open
    // so a corrupt journal stays on disk until the user discards it (H9 §4.6).
    let claimed = tauri::async_runtime::spawn_blocking({
        let blob = blob.clone();
        let state_path = state_path.clone();
        let recovery = recovery.clone();
        let display = display.clone();
        move || claim_journal_zip(id, &recovery, &state_path, &blob, is_state_v1, &display)
    })
    .await
    .map_err(|e| format!("Recover read error: {e}"))??;

    let runtime_id = state.alloc_doc_id();
    let staging = TileCache::new(state.tiles.tile_cache.budget_bytes_count());
    let mut claimed = claimed;
    let zip_bytes = std::mem::take(&mut claimed.zip);
    let opened = tauri::async_runtime::spawn_blocking(move || {
        open_project_from_bytes(&zip_bytes, &staging, DocumentId::new(runtime_id))
            .map(|r| (r, staging))
            .map_err(|e| format!("Could not open recovered project «{display}»: {e}"))
    })
    .await
    .map_err(|e| format!("Recover open error: {e}"))?;

    let opened = match opened {
        Ok(v) => v,
        Err(e) => {
            // Restore claim so Discard / retry still works.
            let _ = tauri::async_runtime::spawn_blocking(move || claimed.restore()).await;
            return Err(e);
        }
    };

    // Open succeeded — drop claimed journal files now.
    let _ = tauri::async_runtime::spawn_blocking({
        let meta_path = meta_path.clone();
        move || {
            claimed.finalize();
            let _ = fs::remove_file(&meta_path);
        }
    })
    .await;

    if crate::tile_gen_trace::enabled()
        || std::env::var("DITHER_TRACE").ok().as_deref() == Some("1")
    {
        log::info!(
            target: "journal",
            "journal_recover id={recovery_id} kind={}",
            if is_state_v1 { "state_v1" } else { "legacy" }
        );
    }

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

struct ClaimedJournal {
    zip: Vec<u8>,
    state_claiming: Option<PathBuf>,
    blobs_claiming: Option<PathBuf>,
    legacy_claiming: Option<PathBuf>,
    state_restored: PathBuf,
    blobs_restored: PathBuf,
    legacy_restored: PathBuf,
}

impl std::fmt::Debug for ClaimedJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaimedJournal")
            .field("zip_len", &self.zip.len())
            .field("state_claiming", &self.state_claiming)
            .field("blobs_claiming", &self.blobs_claiming)
            .field("legacy_claiming", &self.legacy_claiming)
            .finish()
    }
}

impl ClaimedJournal {
    fn restore(self) {
        use std::fs;
        if let Some(ref c) = self.state_claiming {
            if c.exists() {
                let _ = fs::rename(c, &self.state_restored);
            }
        }
        if let Some(ref c) = self.blobs_claiming {
            if c.exists() {
                let _ = fs::rename(c, &self.blobs_restored);
            }
        }
        if let Some(ref c) = self.legacy_claiming {
            if c.exists() {
                let _ = fs::rename(c, &self.legacy_restored);
            }
        }
    }

    fn finalize(self) {
        use std::fs;
        if let Some(c) = self.state_claiming {
            let _ = fs::remove_file(c);
        }
        if let Some(c) = self.blobs_claiming {
            let _ = fs::remove_dir_all(c);
        }
        if let Some(c) = self.legacy_claiming {
            let _ = fs::remove_file(c);
        }
    }
}

fn claim_journal_zip(
    id: Uuid,
    recovery: &std::path::Path,
    state_path: &std::path::Path,
    blob: &std::path::Path,
    is_state_v1: bool,
    display: &str,
) -> Result<ClaimedJournal, String> {
    use std::fs;

    if is_state_v1 {
        let state_claiming = crate::journal::cleanup::state_claiming_path(recovery, id);
        let blobs_dir = crate::journal::meta::journal_blobs_dir(recovery, id);
        let blobs_claiming = crate::journal::cleanup::blobs_claiming_path(recovery, id);

        fs::rename(state_path, &state_claiming).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("Recovery journal for «{display}» is missing (state file).")
            } else {
                format!("Recovery journal for «{display}» is already being recovered.")
            }
        })?;

        let blobs_claimed = if blobs_dir.is_dir() {
            fs::rename(&blobs_dir, &blobs_claiming).map_err(|e| {
                // Undo state claim so the journal stays recoverable.
                let _ = fs::rename(&state_claiming, state_path);
                format!("Could not claim blob directory for «{display}»: {e}")
            })?;
            Some(blobs_claiming.clone())
        } else {
            None
        };

        let assemble_dir = blobs_claimed.as_deref().unwrap_or(blobs_dir.as_path());

        let state_bytes = fs::read(&state_claiming).map_err(|e| {
            restore_state_v1_claim(
                &state_claiming,
                state_path,
                blobs_claimed.as_ref(),
                &blobs_dir,
            );
            format!("Could not read recovery journal for «{display}»: {e}")
        })?;

        let zip = match crate::journal::assemble::assemble_dyproj_from_state_bytes(
            &state_bytes,
            assemble_dir,
            env!("CARGO_PKG_VERSION"),
        ) {
            Ok(z) => z,
            Err(e) => {
                restore_state_v1_claim(
                    &state_claiming,
                    state_path,
                    blobs_claimed.as_ref(),
                    &blobs_dir,
                );
                return Err(format!(
                    "Recovery journal for «{display}» is incomplete or corrupt: {e}"
                ));
            }
        };

        Ok(ClaimedJournal {
            zip,
            state_claiming: Some(state_claiming),
            blobs_claiming: blobs_claimed,
            legacy_claiming: None,
            state_restored: state_path.to_path_buf(),
            blobs_restored: blobs_dir,
            legacy_restored: blob.to_path_buf(),
        })
    } else {
        let claimed = {
            let mut c = blob.to_path_buf();
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
        fs::rename(blob, &claimed).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!("Recovery journal for «{display}» is missing.")
            } else {
                format!("Recovery journal for «{display}» is already being recovered.")
            }
        })?;
        let bytes = fs::read(&claimed).map_err(|e| {
            let _ = fs::rename(&claimed, blob);
            format!("Could not read recovery journal for «{display}»: {e}")
        })?;
        Ok(ClaimedJournal {
            zip: bytes,
            state_claiming: None,
            blobs_claiming: None,
            legacy_claiming: Some(claimed),
            state_restored: state_path.to_path_buf(),
            blobs_restored: crate::journal::meta::journal_blobs_dir(recovery, id),
            legacy_restored: blob.to_path_buf(),
        })
    }
}

fn restore_state_v1_claim(
    state_claiming: &std::path::Path,
    state_path: &std::path::Path,
    blobs_claimed: Option<&PathBuf>,
    blobs_dir: &std::path::Path,
) {
    use std::fs;
    let _ = fs::rename(state_claiming, state_path);
    if let Some(c) = blobs_claimed {
        let _ = fs::rename(c, blobs_dir);
    }
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

pub(crate) fn prepare_soft_discard_sync(
    state: &AppState,
    doc_id: u32,
) -> Result<SoftDiscardDto, String> {
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

    let reuse_existing = can_reuse_journal_for_soft_discard(state, doc_id, recovery_id, revision);
    if !reuse_existing {
        flush_doc_sync(state, doc_id, JournalWriteSource::SoftDiscard)?;
    }

    mark_journal_discarded(state, recovery_id)?;
    // If reuse claimed a journal but meta was missing, mark_journal_discarded is a no-op —
    // rewrite once so Restore still has something to open.
    if reuse_existing && !journal_meta_is_discarded(state, recovery_id) {
        flush_doc_sync(state, doc_id, JournalWriteSource::SoftDiscard)?;
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
    read_meta(&meta_path).map(|m| m.discarded).unwrap_or(false)
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
    let state_path = journal_state_path(dir, recovery_id);
    let legacy = journal_blob_path(dir, recovery_id);
    let meta_path = crate::journal::meta::journal_meta_path(dir, recovery_id);
    if !state_path.is_file() && !legacy.is_file() {
        return false;
    }
    match read_meta(&meta_path) {
        Ok(meta) => matches!(
            meta.content_kind,
            JournalContentKind::StateV1 | JournalContentKind::FullDyproj
        ),
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
        if let Err(e) = flush_doc_sync(state, doc_id, JournalWriteSource::Signals) {
            log::warn!("flush_all_dirty_journals doc {doc_id}: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::meta::JournalContentKind;
    use crate::journal::{set_recovery_dir, write_journal_for_doc};
    use engine_project::document::Document;
    use engine_project::layer::{Layer, LayerNode};
    use engine_project::types::{DocumentId, LayerId, LayerKind};
    use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey, HALO, TILE_SIZE};
    use std::sync::Arc;
    use uuid::Uuid;

    #[test]
    fn claim_corrupt_state_v1_restores_files() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
        set_recovery_dir(&state, recovery.clone());

        let mut doc = Document::new(DocumentId::new(1), 64, 64);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        layer.bump_raw_rev();
        doc.root.push(LayerNode::Leaf(layer));
        let session = state.spawn_session(doc);
        let full = TILE_SIZE + 2 * HALO;
        let mut tile = PixelTile::new();
        for y in 0..full {
            for x in 0..full {
                tile.set(x, y, 0, 0.5);
                tile.set(x, y, 1, 0.5);
                tile.set(x, y, 2, 0.5);
                tile.set(x, y, 3, 1.0);
            }
        }
        state.tiles.tile_cache.insert_fresh(
            TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord {
                    level: 0,
                    x: 0,
                    y: 0,
                },
                stage: CacheStage::Raw,
            },
            Arc::new(tile),
        );
        write_journal_for_doc(&state, 1).unwrap();

        let blobs = crate::journal::meta::journal_blobs_dir(&recovery, session.recovery_id);
        std::fs::remove_file(blobs.join("layer-1-1.png")).unwrap();
        let state_path = journal_state_path(&recovery, session.recovery_id);
        let blob = journal_blob_path(&recovery, session.recovery_id);
        let err = claim_journal_zip(
            session.recovery_id,
            &recovery,
            &state_path,
            &blob,
            true,
            "test.png",
        )
        .unwrap_err();
        assert!(err.contains("incomplete or corrupt"), "{err}");
        assert!(
            state_path.is_file(),
            "state must be restored after failed claim"
        );
        assert!(
            blobs.is_dir(),
            "blobs dir must be restored after failed claim"
        );
        assert!(
            !crate::journal::cleanup::state_claiming_path(&recovery, session.recovery_id).exists()
        );
    }

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
