//! Assemble and atomically write a recovery journal for one session.

use std::path::Path;

use engine_project::serialize::{read_png_file, save_project_to_bytes, ProjectError};
use uuid::Uuid;

use crate::commands::AppState;
use crate::document_session::DocumentSession;

use super::meta::{
    file_mtime_ms, journal_blob_path, now_ms, write_meta, JournalContentKind, JournalMeta,
    JOURNAL_MAX_BYTES,
};

pub fn delete_journal(recovery_dir: &Path, recovery_id: Uuid) {
    let blob = journal_blob_path(recovery_dir, recovery_id);
    let meta = super::meta::journal_meta_path(recovery_dir, recovery_id);
    let _ = std::fs::remove_file(&blob);
    let _ = std::fs::remove_file(&meta);
}

pub fn delete_journal_for_session(state: &AppState, session: &DocumentSession) {
    let Ok(guard) = state.journal.lock() else {
        return;
    };
    let Some(dir) = guard.recovery_dir.as_ref() else {
        return;
    };
    delete_journal(dir, session.recovery_id);
}

/// Serialize the live document into the recovery journal (or skip if too large).
pub fn write_journal_for_doc(state: &AppState, doc_id: u32) -> Result<(), String> {
    let session = state.require_session(doc_id)?;
    let recovery_dir = {
        let guard = state.journal.lock().map_err(|e| e.to_string())?;
        guard
            .recovery_dir
            .clone()
            .ok_or_else(|| "journal recovery_dir not configured".to_string())?
    };

    std::fs::create_dir_all(&recovery_dir).map_err(|e| e.to_string())?;

    let snapshot = session.document_handle.snapshot();
    let project_path = session
        .project_path
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let source_path = session
        .source_path
        .lock()
        .ok()
        .and_then(|g| g.clone());

    let display_name = project_path
        .as_ref()
        .or(source_path.as_ref())
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("Untitled {}", session.id.0));

    let original_mtime_ms = project_path
        .as_ref()
        .and_then(|p| file_mtime_ms(p));

    let zip_result = save_project_to_bytes(
        snapshot.as_ref(),
        &state.tiles.tile_cache,
        env!("CARGO_PKG_VERSION"),
        read_png_file,
    );

    let (content_kind, zip_bytes) = match zip_result {
        Ok(result) => {
            if result.zip_bytes.len() as u64 > JOURNAL_MAX_BYTES {
                log::warn!(
                    "journal skip: doc {} zip {} bytes exceeds cap {}",
                    doc_id,
                    result.zip_bytes.len(),
                    JOURNAL_MAX_BYTES
                );
                (JournalContentKind::SkippedTooLarge, None)
            } else {
                (JournalContentKind::FullDyproj, Some(result.zip_bytes))
            }
        }
        Err(ProjectError::IncompleteRaw { doc_id, layer_id }) => {
            log::warn!(
                "journal skip: IncompleteRaw doc={doc_id} layer={layer_id} (tiles not in cache yet)"
            );
            return Ok(());
        }
        Err(e) => return Err(e.to_string()),
    };

    if let Some(bytes) = zip_bytes {
        let blob = journal_blob_path(&recovery_dir, session.recovery_id);
        engine_io::atomic_write(&blob, &bytes).map_err(|e| e.to_string())?;
    } else {
        // Cap skip: remove stale full blob if any.
        let blob = journal_blob_path(&recovery_dir, session.recovery_id);
        let _ = std::fs::remove_file(&blob);
    }

    let meta = JournalMeta {
        recovery_id: session.recovery_id,
        runtime_doc_id: doc_id,
        display_name,
        project_path: project_path.map(|p| p.to_string_lossy().into_owned()),
        source_path: source_path.map(|p| p.to_string_lossy().into_owned()),
        original_mtime_ms,
        written_at_ms: now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        content_kind,
        discarded: false,
    };
    write_meta(&recovery_dir, &meta)?;

    if let Ok(mut guard) = state.journal.lock() {
        guard.note_flushed(doc_id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AppState;
    use engine_project::types::DocumentId;
    use engine_project::Document;
    use std::sync::Arc;

    #[test]
    fn write_and_delete_journal_for_blank_doc() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        let state = Arc::new(AppState::empty_process(None, 16 * 1024 * 1024, false));
        crate::journal::set_recovery_dir(&state, recovery.clone());

        let session = state.spawn_session(Document::new(DocumentId::new(1), 64, 64));
        // Mark dirty so journal path is meaningful (blank empty root may still serialize).
        crate::undo::mark_clean_doc(&state, 1);
        // Force dirty: clear saved mark
        if let Ok(mut g) = session.history.saved_snapshot.lock() {
            *g = None;
        }

        write_journal_for_doc(&state, 1).unwrap();
        let blob = journal_blob_path(&recovery, session.recovery_id);
        let meta_path = super::super::meta::journal_meta_path(&recovery, session.recovery_id);
        assert!(meta_path.exists(), "meta should exist");
        // Blank doc may be tiny full journal or skip — meta is enough.
        let meta = super::super::meta::read_meta(&meta_path).unwrap();
        assert_eq!(meta.recovery_id, session.recovery_id);

        delete_journal_for_session(&state, &session);
        assert!(!meta_path.exists());
        assert!(!blob.exists());
    }
}
