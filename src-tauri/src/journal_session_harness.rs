//! Typical-session journal write-source mix (Step 1 supplement).
//!
//! Simulates continuous editing: many `schedule_dirty` pulses (debounce
//! coalescing), plus soft-discard and signal flush. Heartbeat is invoked
//! directly (30 s wall clock is not waited in CI).
//!
//! ```text
//! cargo test -p dither --release journal_session_typical_mix \
//!   -- --ignored --nocapture --test-threads=1
//! ```

#![cfg(test)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_project::document::Document;
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey, HALO, TILE_SIZE};

use crate::commands::AppState;
use crate::journal::stats::{self, JournalWriteSource};
use crate::journal::{
    set_recovery_dir, write_journal_for_doc_from, JOURNAL_MAX_BYTES,
};
use crate::undo::is_dirty_doc;

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
                    let t = ((gx + gy) as f32 * 0.07).sin() * 0.5 + 0.5;
                    tile.set(x, y, 0, t);
                    tile.set(x, y, 1, t * 0.85);
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

/// Model a 60 s editing session as discrete write events (no 3 s sleeps).
///
/// Continuous slider work → debounce coalesces to ~1 write / 3 s ⇒ ~20 debounce
/// writes/min. Heartbeat fires if still dirty and last flush ≥ 30 s ⇒ ~2/min.
/// Soft discard and signals are rare (0–1 per session).
#[test]
#[ignore]
fn journal_session_typical_mix() {
    stats::reset();
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::empty_process(None, 512 * 1024 * 1024, true));
    set_recovery_dir(&state, dir.path().join("recovery"));

    let w = 1024u32;
    let h = 1024u32;
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(Layer::new(
        LayerId::new(1),
        LayerKind::Raster,
        w,
        h,
    )));
    let session = state.spawn_session(doc);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }
    fill_raw(&state, 1, 1, w, h);
    let _ = is_dirty_doc(&state, 1);

    // 60 s of continuous edits → ~20 debounce writes (coalesced).
    let debounce_n = 20u32;
    for _ in 0..debounce_n {
        write_journal_for_doc_from(&state, 1, JournalWriteSource::Debounce).unwrap();
    }
    // Heartbeat twice in a minute if the user keeps the doc dirty.
    write_journal_for_doc_from(&state, 1, JournalWriteSource::Heartbeat).unwrap();
    write_journal_for_doc_from(&state, 1, JournalWriteSource::Heartbeat).unwrap();
    // Soft discard once (tab close Don't Save) — may reuse matching revision
    // in production; here we force a sourced write.
    write_journal_for_doc_from(&state, 1, JournalWriteSource::SoftDiscard).unwrap();
    // Signal flush once (quit / SIGTERM).
    write_journal_for_doc_from(&state, 1, JournalWriteSource::Signals).unwrap();

    let snap = stats::snapshot();
    let total = snap.debounce + snap.heartbeat + snap.soft_discard + snap.signals;
    println!(
        "JOURNAL_SESSION_MIX debounce={} ({:.0}%) heartbeat={} ({:.0}%) soft_discard={} ({:.0}%) signals={} ({:.0}%) total={} concurrent_peak={} zip_cap={}",
        snap.debounce,
        100.0 * snap.debounce as f64 / total as f64,
        snap.heartbeat,
        100.0 * snap.heartbeat as f64 / total as f64,
        snap.soft_discard,
        100.0 * snap.soft_discard as f64 / total as f64,
        snap.signals,
        100.0 * snap.signals as f64 / total as f64,
        total,
        snap.concurrent_peak,
        JOURNAL_MAX_BYTES,
    );
}

/// Bytes/minute under continuous editing at 25 MPix (full dyproj journal).
#[test]
#[ignore]
fn journal_session_bytes_per_minute_25mp() {
    stats::reset();
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(AppState::empty_process(None, 4096 * 1024 * 1024, true));
    set_recovery_dir(&state, dir.path().join("recovery"));

    let w = 5984u32;
    let h = 4172u32;
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(Layer::new(
        LayerId::new(1),
        LayerKind::Raster,
        w,
        h,
    )));
    let session = state.spawn_session(doc);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }
    fill_raw(&state, 1, 1, w, h);

    let t0 = Instant::now();
    write_journal_for_doc_from(&state, 1, JournalWriteSource::Debounce).unwrap();
    let one_write = t0.elapsed();

    let blob = crate::journal::journal_blob_path(&dir.path().join("recovery"), session.recovery_id);
    let bytes = std::fs::metadata(&blob).map(|m| m.len()).unwrap_or(0);
    // Continuous editing → debounce every 3 s ⇒ 20 writes/min (upper bound when
    // every quiet window completes; heartbeat adds ~2 if debounce is skipped).
    let writes_per_min = 20.0_f64;
    let bytes_per_min = bytes as f64 * writes_per_min;
    println!(
        "JOURNAL_BYTES_PER_MIN size={}x{} one_write_ms={:.1} zip_bytes={} zip_mib={:.2} writes_per_min={:.0} bytes_per_min={} mib_per_min={:.1}",
        w,
        h,
        one_write.as_secs_f64() * 1000.0,
        bytes,
        bytes as f64 / (1024.0 * 1024.0),
        writes_per_min,
        bytes_per_min as u64,
        bytes_per_min / (1024.0 * 1024.0),
    );
    let _ = Duration::from_secs(0);
}
