//! H9 §6 failure-injection and invariant tests (Step 7).

#![cfg(test)]

use std::sync::Arc;

use engine_project::document::Document;
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::open_project_from_bytes;
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};

use super::meta::{journal_blobs_dir, journal_meta_path, journal_state_path, read_meta};
use crate::commands::AppState;
use crate::journal::assemble::assemble_dyproj_from_state_bytes;
use crate::journal::cleanup::startup_cleanup;
use crate::journal::failpoint::{self, FailPoint};
use crate::journal::{set_recovery_dir, write_journal_for_doc};

fn fill_raw(state: &AppState, doc_id: u32, layer: u32) {
    let full = TILE_SIZE + 2 * HALO;
    let mut tile = PixelTile::new();
    for y in 0..full {
        for x in 0..full {
            tile.set(x, y, 0, 0.3);
            tile.set(x, y, 1, 0.4);
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
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        },
        Arc::new(tile),
    );
}

fn setup_dirty_doc() -> (
    tempfile::TempDir,
    Arc<AppState>,
    uuid::Uuid,
    std::path::PathBuf,
) {
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery.clone());
    let mut doc = Document::new(DocumentId::new(1), 64, 64);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }
    fill_raw(&state, 1, 1);
    (dir, state, session.recovery_id, recovery)
}

fn assert_recoverable(recovery: &std::path::Path, recovery_id: uuid::Uuid) {
    startup_cleanup(recovery);
    let state_path = journal_state_path(recovery, recovery_id);
    let blobs = journal_blobs_dir(recovery, recovery_id);
    assert!(
        state_path.is_file(),
        "expected durable .state after crash cleanup"
    );
    let state_bytes = std::fs::read(&state_path).unwrap();
    let zip = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, "0.0.0-test")
        .expect("assembled zip must open after crash");
    let staging = TileCache::new(64 * 1024 * 1024);
    let opened = open_project_from_bytes(&zip, &staging, DocumentId::new(99)).unwrap();
    assert_eq!(opened.document.width, 64);
    assert_eq!(opened.document.layer_raw_rev(LayerId::new(1)), Some(1));
}

#[test]
fn crash_before_blob_rename_leaves_previous_or_empty_recoverable() {
    let (_dir, state, id, recovery) = setup_dirty_doc();
    // First successful write establishes a baseline.
    write_journal_for_doc(&state, 1).unwrap();
    assert_recoverable(&recovery, id);

    // Bump generation so a second write is attempted; crash before blob rename
    // should not be needed (blob already exists) — force a new raw_rev instead.
    state
        .require_session(1)
        .unwrap()
        .document_handle
        .mutate(|d| {
            if let Some(LayerNode::Leaf(layer)) = d.root.first_mut() {
                layer.bump_raw_rev();
            }
            d.increment_generation();
        });
    fill_raw(&state, 1, 1);

    failpoint::arm(FailPoint::BeforeBlobRename);
    let err = write_journal_for_doc(&state, 1).unwrap_err();
    assert!(err.contains("failpoint"), "{err}");
    failpoint::clear();

    // Previous state (raw_rev=1) must still assemble; tmp cleaned on startup.
    startup_cleanup(&recovery);
    let state_bytes = std::fs::read(journal_state_path(&recovery, id)).unwrap();
    let blobs = journal_blobs_dir(&recovery, id);
    let zip = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, "0.0.0-test").unwrap();
    let staging = TileCache::new(64 * 1024 * 1024);
    let opened = open_project_from_bytes(&zip, &staging, DocumentId::new(7)).unwrap();
    assert_eq!(opened.document.layer_raw_rev(LayerId::new(1)), Some(1));
    // No leftover tmp for the aborted rev=2 blob.
    let tmp_left = std::fs::read_dir(&blobs)
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().contains(".tmp-"));
    assert!(!tmp_left);
}

#[test]
fn crash_after_blob_before_state_keeps_old_state() {
    let (_dir, state, id, recovery) = setup_dirty_doc();
    write_journal_for_doc(&state, 1).unwrap();
    let state_before = std::fs::read(journal_state_path(&recovery, id)).unwrap();

    state
        .require_session(1)
        .unwrap()
        .document_handle
        .mutate(|d| {
            if let Some(LayerNode::Leaf(layer)) = d.root.first_mut() {
                layer.bump_raw_rev();
            }
            d.increment_generation();
        });
    fill_raw(&state, 1, 1);

    failpoint::arm(FailPoint::BeforeStateWrite);
    assert!(write_journal_for_doc(&state, 1)
        .unwrap_err()
        .contains("failpoint"));
    failpoint::clear();

    let state_after = std::fs::read(journal_state_path(&recovery, id)).unwrap();
    assert_eq!(
        state_before, state_after,
        "state must stay previous revision"
    );
    // New blob may exist on disk but must not be required by old state.
    assert_recoverable(&recovery, id);
}

#[test]
fn crash_after_state_before_meta_still_recoverable() {
    let (_dir, state, id, recovery) = setup_dirty_doc();
    write_journal_for_doc(&state, 1).unwrap();

    state
        .require_session(1)
        .unwrap()
        .document_handle
        .mutate(|d| {
            d.increment_generation();
        });

    failpoint::arm(FailPoint::BeforeMetaWrite);
    assert!(write_journal_for_doc(&state, 1)
        .unwrap_err()
        .contains("failpoint"));
    failpoint::clear();

    // New state is durable; meta may be stale — assemble uses .state + blobs.
    assert_recoverable(&recovery, id);
    let meta = read_meta(&journal_meta_path(&recovery, id)).unwrap();
    assert_eq!(meta.recovery_id, id);
}

#[test]
fn recovery_equivalence_document_and_raw_rev() {
    let (_dir, state, id, recovery) = setup_dirty_doc();
    write_journal_for_doc(&state, 1).unwrap();
    let snap = state.require_session(1).unwrap().document_handle.snapshot();
    let w = snap.width;
    let h = snap.height;
    let rev = snap.layer_raw_rev(LayerId::new(1));

    let state_bytes = std::fs::read(journal_state_path(&recovery, id)).unwrap();
    let blobs = journal_blobs_dir(&recovery, id);
    let zip = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, "0.0.0-test").unwrap();
    let staging = TileCache::new(64 * 1024 * 1024);
    let opened = open_project_from_bytes(&zip, &staging, DocumentId::new(42)).unwrap();
    assert_eq!(opened.document.width, w);
    assert_eq!(opened.document.height, h);
    assert_eq!(opened.document.layer_raw_rev(LayerId::new(1)), rev);
}

#[test]
fn second_write_same_raw_rev_does_not_rewrite_blob() {
    let (_dir, state, id, recovery) = setup_dirty_doc();
    write_journal_for_doc(&state, 1).unwrap();
    let blob = journal_blobs_dir(&recovery, id).join("layer-1-1.png");
    let m1 = std::fs::metadata(&blob).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(40));
    state
        .require_session(1)
        .unwrap()
        .document_handle
        .mutate(|d| {
            d.increment_generation();
        });
    write_journal_for_doc(&state, 1).unwrap();
    let m2 = std::fs::metadata(&blob).unwrap().modified().unwrap();
    assert_eq!(m1, m2);
}
