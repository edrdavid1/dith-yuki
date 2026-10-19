//! Pre-merge invariants (author checklist items 2, 4, 5, 6).
//!
//! Heavy items (25 MPix restore timing, kill -9 loop) live in
//! `journal_premerge_harness.rs` as `#[ignore]` release tests.

#![cfg(test)]

use std::sync::Arc;
use std::time::Duration;

use engine_project::document::Document;
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::pixels::assemble_layer_rgba8;
use engine_project::serialize::{
    assemble_layer_png_fast, blob_sha256_hex, decode_journal_state, open_project_from_bytes,
    read_png_file, save_project_to_bytes,
};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};
use sha2::{Digest, Sha256};

use super::assemble::assemble_dyproj_from_state_bytes;
use super::meta::{journal_blobs_dir, journal_meta_path, journal_state_path};
use super::{set_recovery_dir, write_journal_for_doc};
use crate::commands::AppState;
use crate::tile_pipeline::compute_processed_tile;

fn fill_raw(state: &AppState, doc_id: u32, layer: u32, w: u32, h: u32) {
    let tx_n = (w + TILE_SIZE - 1) / TILE_SIZE;
    let ty_n = (h + TILE_SIZE - 1) / TILE_SIZE;
    let full = TILE_SIZE + 2 * HALO;
    for cy in 0..ty_n {
        for cx in 0..tx_n {
            let mut tile = PixelTile::new();
            for y in 0..full {
                for x in 0..full {
                    let gx = cx as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
                    let gy = cy as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
                    let t = ((gx + gy) as f32 * 0.11).sin() * 0.5 + 0.5;
                    tile.set(x, y, 0, t);
                    tile.set(x, y, 1, t * 0.9);
                    tile.set(x, y, 2, 1.0 - t);
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

fn hash_tile(tile: &PixelTile) -> String {
    let mut h = Sha256::new();
    for y in 0..(TILE_SIZE + 2 * HALO) {
        for x in 0..(TILE_SIZE + 2 * HALO) {
            for c in 0..4u32 {
                h.update(tile.at(x, y, c).to_le_bytes());
            }
        }
    }
    format!("{:x}", h.finalize())
}

fn processed_hashes(state: &AppState, doc_id: u32, layer: u32, w: u32, h: u32) -> Vec<String> {
    let tx_n = (w + TILE_SIZE - 1) / TILE_SIZE;
    let ty_n = (h + TILE_SIZE - 1) / TILE_SIZE;
    let mut out = Vec::new();
    for cy in 0..ty_n {
        for cx in 0..tx_n {
            let key = TileKey {
                doc: doc_id,
                layer,
                coord: TileCoord {
                    level: 0,
                    x: cx,
                    y: cy,
                },
                stage: CacheStage::Processed,
            };
            let tile = compute_processed_tile(key, state).expect("processed");
            out.push(hash_tile(&tile));
        }
    }
    out
}

fn raw_rgba(state: &AppState, doc_id: u32) -> Vec<u8> {
    let session = state.require_session(doc_id).unwrap();
    let snap = session.document_handle.snapshot();
    let LayerNode::Leaf(layer) = &snap.root[0] else {
        panic!("expected leaf");
    };
    assemble_layer_rgba8(
        &state.tiles.tile_cache,
        layer,
        snap.width,
        snap.height,
        doc_id,
    )
    .unwrap()
}

fn raw_png_fast(state: &AppState, doc_id: u32) -> Vec<u8> {
    let session = state.require_session(doc_id).unwrap();
    let snap = session.document_handle.snapshot();
    let LayerNode::Leaf(layer) = &snap.root[0] else {
        panic!("expected leaf");
    };
    assemble_layer_png_fast(
        &state.tiles.tile_cache,
        layer,
        snap.width,
        snap.height,
        doc_id,
    )
    .unwrap()
}

#[test]
fn clean_document_writes_neither_state_nor_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery.clone());

    let mut doc = Document::new(DocumentId::new(1), 64, 64);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    fill_raw(&state, 1, 1, 64, 64);
    // Match post-Save / post-open: saved_snapshot == live → not dirty.
    crate::undo::mark_clean_doc(&state, 1);
    assert!(!crate::undo::is_dirty_doc(&state, 1));

    write_journal_for_doc(&state, 1).unwrap();
    assert!(
        !journal_state_path(&recovery, session.recovery_id).exists(),
        "clean doc must not write .state"
    );
    assert!(
        !journal_blobs_dir(&recovery, session.recovery_id).exists()
            || std::fs::read_dir(journal_blobs_dir(&recovery, session.recovery_id))
                .map(|d| d.count() == 0)
                .unwrap_or(true),
        "clean doc must not write blobs"
    );
    assert!(!journal_meta_path(&recovery, session.recovery_id).exists());
}

#[test]
fn first_dirty_write_creates_blobs() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery.clone());

    let mut doc = Document::new(DocumentId::new(1), 64, 64);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    fill_raw(&state, 1, 1, 64, 64);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None; // dirty
    }
    assert!(crate::undo::is_dirty_doc(&state, 1));

    write_journal_for_doc(&state, 1).unwrap();
    let blob = journal_blobs_dir(&recovery, session.recovery_id).join("layer-1-1.png");
    assert!(blob.is_file(), "first dirty write must create layer blob");
    assert!(journal_state_path(&recovery, session.recovery_id).is_file());
}

#[test]
fn delete_layer_write_undo_write_reuses_blob() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery.clone());

    let mut doc = Document::new(DocumentId::new(1), 64, 64);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    fill_raw(&state, 1, 1, 64, 64);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }

    write_journal_for_doc(&state, 1).unwrap();
    let blob = journal_blobs_dir(&recovery, session.recovery_id).join("layer-1-1.png");
    let mtime1 = std::fs::metadata(&blob).unwrap().modified().unwrap();
    let sha1 = blob_sha256_hex(&std::fs::read(&blob).unwrap());

    // Delete layer (keep node for undo), write state without the layer.
    let removed = {
        let snap = session.document_handle.snapshot();
        snap.root[0].clone()
    };
    session.document_handle.mutate(|d| {
        d.root.clear();
        d.increment_generation();
    });
    write_journal_for_doc(&state, 1).unwrap();

    // Undo: restore same layer (same raw_rev).
    session.document_handle.mutate(|d| {
        d.root.push(removed.clone());
        d.increment_generation();
    });
    std::thread::sleep(Duration::from_millis(40));
    write_journal_for_doc(&state, 1).unwrap();

    let mtime2 = std::fs::metadata(&blob).unwrap().modified().unwrap();
    let sha2 = blob_sha256_hex(&std::fs::read(&blob).unwrap());
    assert_eq!(mtime1, mtime2, "blob must not be re-encoded after undo");
    assert_eq!(sha1, sha2);

    let state_bytes = std::fs::read(journal_state_path(&recovery, session.recovery_id)).unwrap();
    let decoded = decode_journal_state(&state_bytes).unwrap();
    let names: Vec<_> = decoded.blobs.iter().map(|b| b.filename()).collect();
    assert!(
        names.iter().any(|n| *n == "layer-1-1.png"),
        "state must reference restored blob, got {names:?}"
    );
}

#[test]
fn equivalence_import_raw_bytes_and_processed_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 256 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery.clone());

    let w = 256u32;
    let h = 256u32;
    let mut doc = Document::new(DocumentId::new(1), w, h);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    fill_raw(&state, 1, 1, w, h);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }

    // Journal stores fast-PNG Raw; "before crash" baseline must use that encoding.
    let snap = session.document_handle.snapshot();
    let LayerNode::Leaf(layer) = &snap.root[0] else {
        panic!("leaf");
    };
    let before_png = assemble_layer_png_fast(
        &state.tiles.tile_cache,
        layer,
        snap.width,
        snap.height,
        1,
    )
    .unwrap();

    write_journal_for_doc(&state, 1).unwrap();
    let blob = journal_blobs_dir(&recovery, session.recovery_id).join("layer-1-1.png");
    assert_eq!(
        before_png,
        std::fs::read(&blob).unwrap(),
        "journal blob must equal import Raw PNG"
    );

    let state_bytes = std::fs::read(journal_state_path(&recovery, session.recovery_id)).unwrap();
    let blobs = journal_blobs_dir(&recovery, session.recovery_id);
    let zip = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, "0.0.0-test").unwrap();

    // Baseline Processed: open the journal zip once (PNG-quantized Raw, like recover).
    let staging_a = TileCache::new(256 * 1024 * 1024);
    let opened_a = open_project_from_bytes(&zip, &staging_a, DocumentId::new(8)).unwrap();
    let state_a = Arc::new(AppState::empty_process(None, 256 * 1024 * 1024, false));
    let live_gen = 1u64;
    for entry in staging_a.entries.iter() {
        let key = *entry.key();
        let tile = entry.value().tile.clone();
        let _ = state_a.tiles.tile_cache.insert_fresh_gen(key, tile, live_gen);
    }
    let mut doc_a = opened_a.document;
    doc_a.increment_generation();
    doc_a.generations.set_document_gen(live_gen);
    state_a.spawn_session(doc_a);
    let before_proc = processed_hashes(&state_a, 8, 1, w, h);

    // Recover again into a fresh runtime id and compare.
    let staging_b = TileCache::new(256 * 1024 * 1024);
    let opened_b = open_project_from_bytes(&zip, &staging_b, DocumentId::new(9)).unwrap();
    let state_b = Arc::new(AppState::empty_process(None, 256 * 1024 * 1024, false));
    for entry in staging_b.entries.iter() {
        let key = *entry.key();
        let tile = entry.value().tile.clone();
        let _ = state_b.tiles.tile_cache.insert_fresh_gen(key, tile, live_gen);
    }
    let mut doc_b = opened_b.document;
    doc_b.increment_generation();
    doc_b.generations.set_document_gen(live_gen);
    state_b.spawn_session(doc_b);

    // Pixel equality (RGBA8), not PNG container bytes (fast vs default zlib differs).
    let before_rgba = {
        // Decode journal PNG → same pixels open_project installs.
        let img = image::load_from_memory(&before_png).unwrap().to_rgba8();
        img.into_raw()
    };
    assert_eq!(
        before_rgba,
        raw_rgba(&state_b, 9),
        "Raw pixels must match byte-for-byte after recover"
    );
    assert_eq!(
        before_png,
        raw_png_fast(&state_b, 9),
        "re-encoded fast PNG from recovered Raw must match journal blob"
    );
    assert_eq!(
        before_proc,
        processed_hashes(&state_b, 9, 1, w, h),
        "Processed tile hashes must match after recover"
    );
}

#[test]
fn equivalence_from_dyproj_raw_bytes_and_processed_hashes() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 256 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery.clone());

    let w = 128u32;
    let h = 128u32;
    let mut doc = Document::new(DocumentId::new(1), w, h);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    fill_raw(&state, 1, 1, w, h);

    let saved = save_project_to_bytes(
        session.document_handle.snapshot().as_ref(),
        &state.tiles.tile_cache,
        "0.0.0-test",
        read_png_file,
    )
    .unwrap();

    // Open dyproj into new runtime id, then journal + recover.
    let staging = TileCache::new(256 * 1024 * 1024);
    let opened = open_project_from_bytes(&saved.zip_bytes, &staging, DocumentId::new(2)).unwrap();
    let state_b = Arc::new(AppState::empty_process(None, 256 * 1024 * 1024, false));
    set_recovery_dir(&state_b, recovery.clone());
    let live_gen = 1u64;
    for entry in staging.entries.iter() {
        let key = *entry.key();
        let tile = entry.value().tile.clone();
        let _ = state_b
            .tiles
            .tile_cache
            .insert_fresh_gen(key, tile, live_gen);
    }
    let mut doc2 = opened.document;
    doc2.increment_generation();
    doc2.generations.set_document_gen(live_gen);
    let session_b = state_b.spawn_session(doc2);
    if let Ok(mut g) = session_b.history.saved_snapshot.lock() {
        *g = None;
    }

    let before_rgba = raw_rgba(&state_b, 2);
    let before_proc = processed_hashes(&state_b, 2, 1, w, h);

    write_journal_for_doc(&state_b, 2).unwrap();
    let state_bytes =
        std::fs::read(journal_state_path(&recovery, session_b.recovery_id)).unwrap();
    let blobs = journal_blobs_dir(&recovery, session_b.recovery_id);
    let zip = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, "0.0.0-test").unwrap();

    let staging2 = TileCache::new(256 * 1024 * 1024);
    let opened2 = open_project_from_bytes(&zip, &staging2, DocumentId::new(3)).unwrap();
    let state_c = Arc::new(AppState::empty_process(None, 256 * 1024 * 1024, false));
    for entry in staging2.entries.iter() {
        let key = *entry.key();
        let tile = entry.value().tile.clone();
        let _ = state_c.tiles.tile_cache.insert_fresh_gen(key, tile, live_gen);
    }
    let mut doc3 = opened2.document;
    doc3.increment_generation();
    doc3.generations.set_document_gen(live_gen);
    state_c.spawn_session(doc3);

    assert_eq!(before_rgba, raw_rgba(&state_c, 3), "Raw pixels from .dyproj path");
    assert_eq!(before_proc, processed_hashes(&state_c, 3, 1, w, h));
}

#[test]
fn atomic_write_private_sets_unix_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.state");
    super::fsutil::atomic_write_private(&path, b"abc").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"abc");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "journal files must be 0600");
        let blobs = dir.path().join("blobs");
        super::fsutil::ensure_dir_private(&blobs).unwrap();
        let dmode = std::fs::metadata(&blobs).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700, "journal dirs must be 0700");
    }
}

#[test]
fn blob_atomic_replace_is_durable_rename() {
    // Same contract as engine_io::atomic_write: temp → fsync → rename.
    let dir = tempfile::tempdir().unwrap();
    let blobs = dir.path().join("blobs");
    super::fsutil::ensure_dir_private(&blobs).unwrap();
    super::fsutil::write_blob_atomic(&blobs, "layer-1-1.png", b"first").unwrap();
    assert_eq!(std::fs::read(blobs.join("layer-1-1.png")).unwrap(), b"first");
    // Idempotent: second write with same name does not truncate existing.
    super::fsutil::write_blob_atomic(&blobs, "layer-1-1.png", b"second-longer").unwrap();
    assert_eq!(std::fs::read(blobs.join("layer-1-1.png")).unwrap(), b"first");
}

#[test]
fn legacy_dyproj_journal_claim_reads_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    std::fs::create_dir_all(&recovery).unwrap();
    let id = uuid::Uuid::new_v4();
    let legacy = crate::journal::journal_blob_path(&recovery, id);
    let payload = b"PK\x03\x04-legacy-dyproj-bytes";
    std::fs::write(&legacy, payload).unwrap();
    let meta = super::meta::JournalMeta {
        recovery_id: id,
        runtime_doc_id: 1,
        display_name: "legacy.dyproj".into(),
        project_path: Some("/tmp/legacy.dyproj".into()),
        source_path: None,
        original_mtime_ms: None,
        written_at_ms: 1,
        app_version: "0.0.0-test".into(),
        content_kind: super::meta::JournalContentKind::FullDyproj,
        discarded: false,
    };
    super::meta::write_meta(&recovery, &meta).unwrap();

    // Claim path used by recover_journal for FullDyproj.
    let claimed = {
        let mut c = legacy.clone();
        let name = legacy
            .file_name()
            .map(|n| {
                let mut s = n.to_os_string();
                s.push(".claiming");
                s
            })
            .unwrap();
        c.set_file_name(name);
        c
    };
    std::fs::rename(&legacy, &claimed).unwrap();
    let bytes = std::fs::read(&claimed).unwrap();
    assert_eq!(bytes, payload);
    // Restore (simulate failed open) — old journal remains recoverable.
    std::fs::rename(&claimed, &legacy).unwrap();
    assert!(legacy.is_file());
    assert_eq!(std::fs::read(&legacy).unwrap(), payload);
}

#[test]
fn unknown_newer_state_version_rejected_without_overwrite() {
    use engine_project::serialize::{
        build_journal_state, encode_journal_state, decode_journal_state, JOURNAL_STATE_VERSION,
    };

    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery.clone());
    let mut doc = Document::new(DocumentId::new(1), 64, 64);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    fill_raw(&state, 1, 1, 64, 64);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }
    write_journal_for_doc(&state, 1).unwrap();
    let state_path = journal_state_path(&recovery, session.recovery_id);
    let good = std::fs::read(&state_path).unwrap();

    // Simulate a future writer leaving a newer .state next to valid blobs.
    let mut newer = build_journal_state(
        session.document_handle.snapshot().as_ref(),
        "99.0.0",
        &[(LayerId::new(1), 1, "0".repeat(64))],
        &[],
    );
    newer.version = JOURNAL_STATE_VERSION + 1;
    let newer_bytes = encode_journal_state(&newer).unwrap();
    std::fs::write(&state_path, &newer_bytes).unwrap();
    assert!(decode_journal_state(&newer_bytes).is_err());
    let err = assemble_dyproj_from_state_bytes(&newer_bytes, &journal_blobs_dir(&recovery, session.recovery_id), "x")
        .unwrap_err();
    assert!(
        err.to_lowercase().contains("version") || err.to_lowercase().contains("journal"),
        "{err}"
    );
    // Do not auto-migrate: leave bytes as-is (caller must not overwrite with v1).
    assert_eq!(std::fs::read(&state_path).unwrap(), newer_bytes);
    // Previous good bytes are distinct; recovery must fail closed, not rewrite.
    assert_ne!(good, newer_bytes);
}
