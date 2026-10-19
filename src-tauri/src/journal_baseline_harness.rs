//! Step 1 journal baseline: write wall time / size / IncompleteRaw / concurrency.
//!
//! ```text
//! cargo test -p dither --release journal_baseline_ \
//!   -- --ignored --nocapture --test-threads=1
//! ```

#![cfg(test)]

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use engine_project::document::Document;
use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey, HALO, TILE_SIZE};

use crate::commands::AppState;
use crate::journal::stats::{self, JournalWriteSource};
use crate::journal::{
    journal_blobs_dir, journal_state_path, set_recovery_dir, write_journal_for_doc_from,
    JOURNAL_MAX_BYTES,
};

struct Case {
    label: &'static str,
    w: u32,
    h: u32,
    layers: u32,
}

fn mpix(w: u32, h: u32) -> f64 {
    (w as f64 * h as f64) / 1_000_000.0
}

fn tiles_xy(w: u32, h: u32) -> (u32, u32) {
    (
        (w + TILE_SIZE - 1) / TILE_SIZE,
        (h + TILE_SIZE - 1) / TILE_SIZE,
    )
}

fn make_doc(doc_id: u32, w: u32, h: u32, n_layers: u32) -> Document {
    let mut doc = Document::new(DocumentId::new(doc_id), w, h);
    for i in 0..n_layers {
        let lid = i + 1;
        let mut layer = Layer::new(LayerId::new(lid), LayerKind::Raster, w, h);
        layer.bump_raw_rev();
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 4,
                pixel_size: 1,
                palette_id: None,
                ..DitherParamsV2::default()
            }),
        ));
        doc.root.push(LayerNode::Leaf(layer));
    }
    doc
}

fn dir_size(path: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

fn fill_raw(state: &AppState, doc_id: u32, layer: u32, w: u32, h: u32) {
    let (tx_n, ty_n) = tiles_xy(w, h);
    let full = TILE_SIZE + 2 * HALO;
    for cy in 0..ty_n {
        for cx in 0..tx_n {
            let coord = TileCoord {
                level: 0,
                x: cx,
                y: cy,
            };
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
                    coord,
                    stage: CacheStage::Raw,
                },
                Arc::new(tile),
            );
        }
    }
}

fn run_case(case: &Case) {
    stats::reset();
    let dir = tempfile::tempdir().expect("tempdir");
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 4096 * 1024 * 1024, true));
    set_recovery_dir(&state, recovery.clone());

    let doc = make_doc(1, case.w, case.h, case.layers);
    let session = state.spawn_session(doc);
    // Force dirty so journal path matches production (not required for direct write).
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }

    for layer in 1..=case.layers {
        fill_raw(&state, 1, layer, case.w, case.h);
    }

    let t0 = Instant::now();
    write_journal_for_doc_from(&state, 1, JournalWriteSource::Direct).expect("journal write");
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;

    // Second write: state-only (same raw_rev) — should be cheap vs first (blob encode).
    let t1 = Instant::now();
    session.document_handle.mutate(|d| d.increment_generation());
    write_journal_for_doc_from(&state, 1, JournalWriteSource::Direct).expect("journal rewrite");
    let rewrite_ms = t1.elapsed().as_secs_f64() * 1000.0;

    let state_path = journal_state_path(&recovery, session.recovery_id);
    let blobs = journal_blobs_dir(&recovery, session.recovery_id);
    let state_bytes = std::fs::metadata(&state_path).map(|m| m.len()).unwrap_or(0);
    let blobs_bytes = dir_size(&blobs);
    let total = state_bytes + blobs_bytes;
    let snap = stats::snapshot();
    let avg_ms = if snap.write_samples > 0 {
        snap.total_write_ms as f64 / snap.write_samples as f64
    } else {
        wall_ms
    };

    println!(
        "JOURNAL_H9 label={} size={}x{} mpix={:.2} layers={} first_ms={:.1} rewrite_ms={:.1} stats_ms={:.1} state_bytes={} blobs_bytes={} total_mib={:.2} concurrent_peak={} incomplete_raw={} cap={}",
        case.label,
        case.w,
        case.h,
        mpix(case.w, case.h),
        case.layers,
        wall_ms,
        rewrite_ms,
        avg_ms,
        state_bytes,
        blobs_bytes,
        total as f64 / (1024.0 * 1024.0),
        snap.concurrent_peak,
        snap.incomplete_raw_skips,
        JOURNAL_MAX_BYTES,
    );
}

#[test]
#[ignore]
fn journal_baseline_sizes_release() {
    let cases = [
        Case {
            label: "4mp_1L",
            w: 2000,
            h: 2000,
            layers: 1,
        },
        Case {
            label: "12mp_1L",
            w: 4000,
            h: 3000,
            layers: 1,
        },
        Case {
            label: "25mp_1L",
            w: 5984,
            h: 4172,
            layers: 1,
        },
        Case {
            label: "4mp_2L",
            w: 2000,
            h: 2000,
            layers: 2,
        },
    ];
    for case in &cases {
        run_case(case);
    }
}

#[test]
#[ignore]
fn journal_baseline_incomplete_raw_skip() {
    stats::reset();
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 256 * 1024 * 1024, true));
    set_recovery_dir(&state, recovery);
    let _ = state.spawn_session(make_doc(1, 512, 512, 1));
    // No Raw tiles → IncompleteRaw → soft skip.
    write_journal_for_doc_from(&state, 1, JournalWriteSource::Direct).unwrap();
    let snap = stats::snapshot();
    assert_eq!(snap.incomplete_raw_skips, 1, "expected IncompleteRaw skip");
    println!(
        "JOURNAL_BASELINE incomplete_raw_skip ok skips={}",
        snap.incomplete_raw_skips
    );
}

#[test]
#[ignore]
fn journal_baseline_concurrent_peak() {
    stats::reset();
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 1024 * 1024 * 1024, true));
    set_recovery_dir(&state, recovery);
    let _ = state.spawn_session(make_doc(1, 1024, 1024, 1));
    fill_raw(&state, 1, 1, 1024, 1024);

    let s1 = Arc::clone(&state);
    let s2 = Arc::clone(&state);
    let h1 = thread::spawn(move || {
        write_journal_for_doc_from(&s1, 1, JournalWriteSource::Debounce).unwrap();
    });
    // Overlap intentionally — current code has no single-flight.
    thread::sleep(Duration::from_millis(5));
    let h2 = thread::spawn(move || {
        write_journal_for_doc_from(&s2, 1, JournalWriteSource::Heartbeat).unwrap();
    });
    h1.join().unwrap();
    h2.join().unwrap();

    let snap = stats::snapshot();
    println!(
        "JOURNAL_BASELINE concurrent debounce={} heartbeat={} peak={} (expect peak>=2 without single-flight)",
        snap.debounce, snap.heartbeat, snap.concurrent_peak
    );
    assert!(
        snap.concurrent_peak >= 2,
        "expected overlapping writes; peak={}",
        snap.concurrent_peak
    );
}
