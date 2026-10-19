//! Write recovery journal (H9 v2): blobs → `.state` → `.meta.json`.
//!
//! Does **not** call `save_project_to_bytes` / `build_processed_composite_rgba8`.
//! Order: missing layer/threshold blobs first, then state (refs only on-disk
//! blobs), then meta. Single-flight is enforced by [`super::runtime`].

use std::collections::HashSet;
use std::path::Path;
use std::time::Instant;

use engine_project::layer::LayerNode;
use engine_project::serialize::{
    assemble_layer_png_fast, blob_sha256_hex, build_journal_state, collect_custom_png_embeds,
    encode_journal_state, layer_blob_filename, read_png_file, threshold_blob_filename, ProjectError,
};
use engine_project::types::{LayerId, LayerKind};
use uuid::Uuid;

use crate::commands::AppState;
use crate::document_session::DocumentSession;

use super::fsutil::{atomic_write_private, ensure_dir_private, write_blob_atomic};
use super::meta::{
    file_mtime_ms, journal_blob_path, journal_blobs_dir, journal_meta_path, journal_state_path,
    now_ms, JournalContentKind, JournalMeta, JOURNAL_MAX_BYTES,
};
use super::stats::{self, JournalWriteSource};

pub fn delete_journal(recovery_dir: &Path, recovery_id: Uuid) {
    let _ = std::fs::remove_file(journal_blob_path(recovery_dir, recovery_id));
    let _ = std::fs::remove_file(journal_state_path(recovery_dir, recovery_id));
    let _ = std::fs::remove_file(journal_meta_path(recovery_dir, recovery_id));
    let blobs = journal_blobs_dir(recovery_dir, recovery_id);
    let _ = std::fs::remove_dir_all(&blobs);
    // Orphaned mid-recover claims (crash between claim and finalize).
    let _ = std::fs::remove_file(super::cleanup::state_claiming_path(recovery_dir, recovery_id));
    let _ = std::fs::remove_dir_all(super::cleanup::blobs_claiming_path(recovery_dir, recovery_id));
    let mut legacy_claim = journal_blob_path(recovery_dir, recovery_id);
    let name = legacy_claim
        .file_name()
        .map(|n| {
            let mut s = n.to_os_string();
            s.push(".claiming");
            s
        })
        .unwrap_or_else(|| "journal.claiming".into());
    legacy_claim.set_file_name(name);
    let _ = std::fs::remove_file(&legacy_claim);
}

pub fn delete_journal_for_session(state: &AppState, session: &DocumentSession) {
    let Ok(guard) = state.journal.lock() else {
        return;
    };
    let Some(dir) = guard.recovery_dir.as_ref() else {
        return;
    };
    delete_journal(dir, session.recovery_id);
    // Drop remembered blob digests for this recovery id.
    // (runtime clears via note_deleted if present)
}

/// Serialize the live document into the recovery journal (or skip if too large).
pub fn write_journal_for_doc(state: &AppState, doc_id: u32) -> Result<(), String> {
    write_journal_for_doc_from(state, doc_id, JournalWriteSource::Direct)
}

/// Same as [`write_journal_for_doc`], with a call-site source for counters.
pub fn write_journal_for_doc_from(
    state: &AppState,
    doc_id: u32,
    source: JournalWriteSource,
) -> Result<(), String> {
    stats::note_source(source);
    if !super::enabled() {
        stats::note_kill_switch_noop();
        return Ok(());
    }
    let _inflight = stats::InFlightGuard::enter();
    let t0 = Instant::now();

    let result = write_journal_state_v1(state, doc_id, source);
    stats::note_write_duration(t0.elapsed());
    if crate::tile_gen_trace::enabled() || journal_trace_enabled() {
        let snap = stats::snapshot();
        log::info!(
            target: "journal",
            "journal_write source={source:?} doc={doc_id} ms={} ok={} stats={}",
            t0.elapsed().as_millis(),
            result.is_ok(),
            serde_json::to_string(&snap).unwrap_or_default()
        );
    }
    result
}

fn journal_trace_enabled() -> bool {
    std::env::var("DITHER_TRACE").ok().as_deref() == Some("1")
}

fn write_journal_state_v1(
    state: &AppState,
    doc_id: u32,
    source: JournalWriteSource,
) -> Result<(), String> {
    let session = state.require_session(doc_id)?;
    let recovery_dir = {
        let guard = state.journal.lock().map_err(|e| e.to_string())?;
        guard
            .recovery_dir
            .clone()
            .ok_or_else(|| "journal recovery_dir not configured".to_string())?
    };
    ensure_dir_private(&recovery_dir).map_err(|e| e.to_string())?;

    let snapshot = session.document_handle.snapshot();
    let recovery_id = session.recovery_id;
    let revision = snapshot.revision;

    // --- Blobs: write missing (layer_id, raw_rev) and threshold hashes ---
    let blobs_dir = journal_blobs_dir(&recovery_dir, recovery_id);
    ensure_dir_private(&blobs_dir).map_err(|e| e.to_string())?;

    let mut layer_digests: Vec<(LayerId, u64, String)> = Vec::new();
    let mut pending_raw = false;
    let mut blob_bytes_total: u64 = dir_size_approx(&blobs_dir);

    // Filenames referenced by the live document (for GC under the 512 MiB cap).
    let mut keep_blobs: HashSet<String> = HashSet::new();
    collect_raster_layers(&snapshot.root, &mut |layer| {
        if layer.kind == LayerKind::Raster && layer.raw_rev > 0 {
            keep_blobs.insert(layer_blob_filename(layer.id.0, layer.raw_rev));
        }
        Ok(())
    })?;
    let mut read_png_keep = |path: &str| read_png_file(path);
    if let Ok((embeds, _)) = collect_custom_png_embeds(snapshot.as_ref(), &mut read_png_keep) {
        for (basename, _) in &embeds {
            let content_hash = basename.trim_end_matches(".png");
            keep_blobs.insert(threshold_blob_filename(content_hash));
        }
    }
    if blob_bytes_total > JOURNAL_MAX_BYTES {
        let freed = super::cleanup::gc_unreferenced_blobs(&blobs_dir, &keep_blobs);
        blob_bytes_total = dir_size_approx(&blobs_dir);
        if journal_trace_enabled() {
            log::info!(
                target: "journal",
                "journal_blob_gc freed={freed} total={blob_bytes_total}"
            );
        }
    }

    collect_raster_layers(&snapshot.root, &mut |layer| {
        if layer.kind != LayerKind::Raster || layer.raw_rev == 0 {
            return Ok(());
        }
        let filename = layer_blob_filename(layer.id.0, layer.raw_rev);
        let dest = blobs_dir.join(&filename);
        if dest.is_file() {
            // Prefer recorded digest; else hash existing file.
            let sha = if let Ok(guard) = state.journal.lock() {
                guard
                    .blob_sha(recovery_id, layer.id.0, layer.raw_rev)
                    .unwrap_or_else(|| {
                        std::fs::read(&dest)
                            .map(|b| blob_sha256_hex(&b))
                            .unwrap_or_default()
                    })
            } else {
                std::fs::read(&dest)
                    .map(|b| blob_sha256_hex(&b))
                    .unwrap_or_default()
            };
            if !sha.is_empty() {
                layer_digests.push((layer.id, layer.raw_rev, sha));
            }
            return Ok(());
        }

        // Encode from TileCache Raw (may IncompleteRaw).
        let png = match assemble_layer_png_fast(
            &state.tiles.tile_cache,
            layer,
            snapshot.width,
            snapshot.height,
            snapshot.id.0,
        ) {
            Ok(p) => p,
            Err(ProjectError::IncompleteRaw { .. }) => {
                pending_raw = true;
                stats::note_incomplete_raw_skip();
                if journal_trace_enabled() {
                    log::info!(
                        target: "journal",
                        "journal_blob_pending doc={doc_id} layer={} raw_rev={}",
                        layer.id.0,
                        layer.raw_rev
                    );
                }
                return Ok(());
            }
            Err(e) => return Err(e.to_string()),
        };
        let sha = blob_sha256_hex(&png);
        let next_total = blob_bytes_total.saturating_add(png.len() as u64);
        if next_total > JOURNAL_MAX_BYTES {
            let freed = super::cleanup::gc_unreferenced_blobs(&blobs_dir, &keep_blobs);
            blob_bytes_total = dir_size_approx(&blobs_dir);
            let next_total = blob_bytes_total.saturating_add(png.len() as u64);
            if next_total > JOURNAL_MAX_BYTES {
                return Err(format!(
                    "journal blobs exceed cap {JOURNAL_MAX_BYTES} after gc freed={freed} (would be {next_total})"
                ));
            }
        }
        blob_bytes_total = blob_bytes_total.saturating_add(png.len() as u64);
        let t_blob = Instant::now();
        write_blob_atomic(&blobs_dir, &filename, &png).map_err(|e| e.to_string())?;
        if journal_trace_enabled() {
            log::info!(
                target: "journal",
                "journal_blob_write layer={} bytes={} ms={}",
                layer.id.0,
                png.len(),
                t_blob.elapsed().as_millis()
            );
        }
        if let Ok(mut guard) = state.journal.lock() {
            guard.remember_blob(recovery_id, layer.id.0, layer.raw_rev, sha.clone());
        }
        layer_digests.push((layer.id, layer.raw_rev, sha));
        Ok(())
    })?;

    // Threshold maps (content-addressed) — copy once.
    let mut threshold_digests: Vec<(String, String)> = Vec::new();
    let mut read_png = |path: &str| read_png_file(path);
    match collect_custom_png_embeds(snapshot.as_ref(), &mut read_png) {
        Ok((embeds, _)) => {
            for (basename, bytes) in embeds {
                let content_hash = basename.trim_end_matches(".png").to_string();
                let filename = threshold_blob_filename(&content_hash);
                let dest = blobs_dir.join(&filename);
                let sha = blob_sha256_hex(&bytes);
                if !dest.is_file() {
                    let next_total = blob_bytes_total.saturating_add(bytes.len() as u64);
                    if next_total > JOURNAL_MAX_BYTES {
                        let freed =
                            super::cleanup::gc_unreferenced_blobs(&blobs_dir, &keep_blobs);
                        blob_bytes_total = dir_size_approx(&blobs_dir);
                        let next_total = blob_bytes_total.saturating_add(bytes.len() as u64);
                        if next_total > JOURNAL_MAX_BYTES {
                            return Err(format!(
                                "journal blobs exceed cap {JOURNAL_MAX_BYTES} after gc freed={freed} (would be {next_total})"
                            ));
                        }
                    }
                    blob_bytes_total = blob_bytes_total.saturating_add(bytes.len() as u64);
                    write_blob_atomic(&blobs_dir, &filename, &bytes).map_err(|e| e.to_string())?;
                }
                threshold_digests.push((content_hash, sha));
            }
        }
        Err(e) => {
            log::warn!("journal threshold collect failed for doc {doc_id}: {e}");
        }
    }

    // Do not write state that references missing layer blobs.
    if pending_raw {
        // Keep any previous valid state; retry on next schedule.
        return Ok(());
    }

    let state_file = build_journal_state(
        snapshot.as_ref(),
        env!("CARGO_PKG_VERSION"),
        &layer_digests,
        &threshold_digests,
    );
    let state_bytes = encode_journal_state(&state_file).map_err(|e| e.to_string())?;
    let state_hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        state_bytes.hash(&mut h);
        h.finish()
    };

    // Skip state rewrite when unchanged (blobs already durable).
    let skip_state = state
        .journal
        .lock()
        .ok()
        .map(|g| g.state_hash_matches(doc_id, state_hash))
        .unwrap_or(false);
    if skip_state {
        stats::note_skipped();
        if journal_trace_enabled() {
            log::info!(target: "journal", "journal_skipped reason=unchanged_state doc={doc_id}");
        }
        // Still refresh meta written_at / flush bookkeeping for soft-discard reuse.
    } else {
        let t_state = Instant::now();
        let state_path = journal_state_path(&recovery_dir, recovery_id);
        atomic_write_private(&state_path, &state_bytes).map_err(|e| e.to_string())?;
        if journal_trace_enabled() {
            log::info!(
                target: "journal",
                "journal_state_write bytes={} ms={} source={source:?}",
                state_bytes.len(),
                t_state.elapsed().as_millis()
            );
        }
    }

    let project_path = session.project_path.lock().ok().and_then(|g| g.clone());
    let source_path = session.source_path.lock().ok().and_then(|g| g.clone());
    let display_name = project_path
        .as_ref()
        .or(source_path.as_ref())
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| format!("Untitled {}", session.id.0));
    let original_mtime_ms = project_path.as_ref().and_then(|p| file_mtime_ms(p));

    let meta = JournalMeta {
        recovery_id,
        runtime_doc_id: doc_id,
        display_name,
        project_path: project_path.map(|p| p.to_string_lossy().into_owned()),
        source_path: source_path.map(|p| p.to_string_lossy().into_owned()),
        original_mtime_ms,
        written_at_ms: now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        content_kind: JournalContentKind::StateV1,
        discarded: false,
    };
    let meta_path = journal_meta_path(&recovery_dir, recovery_id);
    let json = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
    atomic_write_private(&meta_path, &json).map_err(|e| e.to_string())?;

    if let Ok(mut guard) = state.journal.lock() {
        guard.note_flushed(doc_id, revision);
        guard.note_state_hash(doc_id, state_hash);
    }
    Ok(())
}

fn dir_size_approx(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut n = 0u64;
    for e in entries.flatten() {
        if let Ok(m) = e.metadata() {
            if m.is_file() {
                n = n.saturating_add(m.len());
            }
        }
    }
    n
}

fn collect_raster_layers(
    nodes: &[LayerNode],
    f: &mut dyn FnMut(&engine_project::layer::Layer) -> Result<(), String>,
) -> Result<(), String> {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => f(layer)?,
            LayerNode::Group(g) => collect_raster_layers(&g.children, f)?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AppState;
    use engine_project::types::DocumentId;
    use engine_project::Document;
    use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey, HALO, TILE_SIZE};
    use std::sync::Arc;

    fn fill_tiny_raw(state: &AppState, doc_id: u32, layer: u32, w: u32, h: u32) {
        let tx_n = (w + TILE_SIZE - 1) / TILE_SIZE;
        let ty_n = (h + TILE_SIZE - 1) / TILE_SIZE;
        let full = TILE_SIZE + 2 * HALO;
        for cy in 0..ty_n {
            for cx in 0..tx_n {
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
                        doc: doc_id,
                        layer,
                        coord: TileCoord {
                            level: 0,
                            x: cx,
                            y: cy,
                        },
                        stage: CacheStage::Raw,
                    },
                    Arc::new(tile),
                );
            }
        }
    }

    #[test]
    fn write_state_v1_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
        crate::journal::set_recovery_dir(&state, recovery.clone());

        let mut doc = Document::new(DocumentId::new(1), 64, 64);
        let mut layer =
            engine_project::layer::Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        layer.bump_raw_rev();
        doc.root
            .push(engine_project::layer::LayerNode::Leaf(layer));
        let session = state.spawn_session(doc);
        fill_tiny_raw(&state, 1, 1, 64, 64);
        if let Ok(mut g) = session.history.saved_snapshot.lock() {
            *g = None;
        }

        write_journal_for_doc(&state, 1).unwrap();
        let state_path = journal_state_path(&recovery, session.recovery_id);
        let meta_path = journal_meta_path(&recovery, session.recovery_id);
        let blobs = journal_blobs_dir(&recovery, session.recovery_id);
        assert!(state_path.is_file(), "state must exist");
        assert!(meta_path.is_file(), "meta must exist");
        assert!(blobs.join("layer-1-1.png").is_file(), "layer blob");
        // No full dyproj / no composite path.
        assert!(!journal_blob_path(&recovery, session.recovery_id).exists());

        let meta = super::super::meta::read_meta(&meta_path).unwrap();
        assert_eq!(meta.content_kind, JournalContentKind::StateV1);

        // Second write: blob not re-encoded (mtime stable).
        let mtime1 = std::fs::metadata(blobs.join("layer-1-1.png"))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        session.document_handle.mutate(|d| d.increment_generation());
        write_journal_for_doc(&state, 1).unwrap();
        let mtime2 = std::fs::metadata(blobs.join("layer-1-1.png"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(mtime1, mtime2, "same raw_rev must not rewrite blob");

        delete_journal_for_session(&state, &session);
        assert!(!state_path.exists());
        assert!(!meta_path.exists());
        assert!(!blobs.exists());
    }

    #[test]
    fn soft_discard_reuses_matching_revision_without_rewrite() {
        use crate::journal::commands::prepare_soft_discard_sync;
        use crate::journal::meta::{journal_meta_path, read_meta};

        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
        crate::journal::set_recovery_dir(&state, recovery.clone());

        let mut doc = Document::new(DocumentId::new(1), 64, 64);
        let mut layer =
            engine_project::layer::Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        layer.bump_raw_rev();
        doc.root
            .push(engine_project::layer::LayerNode::Leaf(layer));
        let session = state.spawn_session(doc);
        fill_tiny_raw(&state, 1, 1, 64, 64);
        session.document_handle.mutate(|d| d.increment_generation());

        write_journal_for_doc(&state, 1).unwrap();
        let state_path = journal_state_path(&recovery, session.recovery_id);
        let meta_path = journal_meta_path(&recovery, session.recovery_id);
        let mtime_before = std::fs::metadata(&state_path).unwrap().modified().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        prepare_soft_discard_sync(&state, 1).unwrap();

        let mtime_after = std::fs::metadata(&state_path).unwrap().modified().unwrap();
        assert_eq!(
            mtime_before, mtime_after,
            "matching revision must not rewrite state"
        );
        let meta = read_meta(&meta_path).unwrap();
        assert!(meta.discarded);
    }
}
