//! Pre-merge heavy checks: 25 MPix restore timing + real SIGKILL crash loop.
//!
//! ```text
//! cargo test -p dither --release journal_premerge_ \
//!   -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Kill-loop child mode (invoked by the parent test via `current_exe`):
//! `JOURNAL_KILL_CHILD=1 JOURNAL_KILL_DIR=<path>`.

#![cfg(test)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use engine_project::document::Document;
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::{
    decode_journal_state, open_project_from_bytes, JOURNAL_STATE_VERSION,
};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};

use crate::commands::AppState;
use crate::journal::assemble::assemble_dyproj_from_state_bytes;
use crate::journal::cleanup::startup_cleanup;
use crate::journal::{
    journal_blobs_dir, journal_state_path, set_recovery_dir, write_journal_for_doc,
};

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

fn assert_journal_valid(recovery: &Path, recovery_id: uuid::Uuid) {
    startup_cleanup(recovery);
    let state_path = journal_state_path(recovery, recovery_id);
    if !state_path.is_file() {
        panic!("expected .state after crash (seeded write must survive)");
    }
    let bytes = std::fs::read(&state_path).expect("read state");
    let decoded = decode_journal_state(&bytes).expect("state must decode (old or new)");
    assert!(
        decoded.version > 0 && decoded.version <= JOURNAL_STATE_VERSION,
        "version {}",
        decoded.version
    );
    let blobs = journal_blobs_dir(recovery, recovery_id);
    assemble_dyproj_from_state_bytes(&bytes, &blobs, "0.0.0-test")
        .expect("assemble must succeed for durable state");
}

/// Child process: mutate + write journal until killed (same recovery_id as parent seed).
fn kill_child_main(recovery: PathBuf, recovery_id: uuid::Uuid) {
    let state = Arc::new(AppState::empty_process(None, 512 * 1024 * 1024, false));
    set_recovery_dir(&state, recovery);
    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 512, 512);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session_with_recovery(doc, recovery_id);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }
    fill_raw(&state, 1, 1, 512, 512);
    let _ = write_journal_for_doc(&state, 1);

    let mut n = 0u64;
    loop {
        n += 1;
        session.document_handle.mutate(|d| {
            d.increment_generation();
            // Occasionally bump raw_rev so blob writes are in flight when killed.
            if n % 7 == 0 {
                if let Some(LayerNode::Leaf(layer)) = d.root.first_mut() {
                    layer.bump_raw_rev();
                }
            }
        });
        if n % 7 == 0 {
            fill_raw(&state, 1, 1, 512, 512);
        }
        let _ = write_journal_for_doc(&state, 1);
        // Yield so the parent can land SIGKILL mid-write / mid-rename.
        thread::sleep(Duration::from_micros(200));
    }
}

#[test]
#[ignore]
fn journal_premerge_restore_time_25mp() {
    let w = 5984u32;
    let h = 4172u32;
    let dir = tempfile::tempdir().unwrap();
    let recovery = dir.path().join("recovery");
    let state = Arc::new(AppState::empty_process(None, 4096 * 1024 * 1024, true));
    set_recovery_dir(&state, recovery.clone());

    let mut doc = Document::new(DocumentId::new(1), w, h);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.bump_raw_rev();
    doc.root.push(LayerNode::Leaf(layer));
    let session = state.spawn_session(doc);
    if let Ok(mut g) = session.history.saved_snapshot.lock() {
        *g = None;
    }
    fill_raw(&state, 1, 1, w, h);
    write_journal_for_doc(&state, 1).expect("seed journal");

    let state_bytes =
        std::fs::read(journal_state_path(&recovery, session.recovery_id)).unwrap();
    let blobs = journal_blobs_dir(&recovery, session.recovery_id);

    // "Click Recover → start recalculation" ≈ assemble zip + open_project_from_bytes
    // (tiles land in cache; Processed recompute begins after this returns).
    let t0 = Instant::now();
    let zip = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, env!("CARGO_PKG_VERSION"))
        .expect("assemble");
    let assemble_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let staging = TileCache::new(4096 * 1024 * 1024);
    let t1 = Instant::now();
    let opened =
        open_project_from_bytes(&zip, &staging, DocumentId::new(99)).expect("open recovered");
    let open_ms = t1.elapsed().as_secs_f64() * 1000.0;
    let total_ms = t0.elapsed().as_secs_f64() * 1000.0;

    println!(
        "JOURNAL_RESTORE_25MP assemble_ms={assemble_ms:.1} open_ms={open_ms:.1} total_ms={total_ms:.1} width={} height={} raw_tiles={}",
        opened.document.width,
        opened.document.height,
        staging.entries.len()
    );
    assert!(
        total_ms <= 2000.0,
        "restore until recalculation starts must be ≤2s, got {total_ms:.1} ms"
    );
}

#[test]
#[ignore]
fn journal_premerge_kill9_crash_loop() {
    // Child entry: same test binary, env-gated.
    if std::env::var_os("JOURNAL_KILL_CHILD").is_some() {
        let dir = PathBuf::from(std::env::var("JOURNAL_KILL_DIR").expect("JOURNAL_KILL_DIR"));
        let rid = uuid::Uuid::parse_str(
            &std::env::var("JOURNAL_KILL_RID").expect("JOURNAL_KILL_RID"),
        )
        .expect("rid");
        kill_child_main(dir, rid);
        return;
    }

    let iterations: u32 = std::env::var("JOURNAL_KILL_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let exe = std::env::current_exe().expect("current_exe");
    // Simple LCG — avoid pulling `rand` into the app crate.
    let mut seed = 0x4a9u64;

    for i in 0..iterations {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        std::fs::create_dir_all(&recovery).unwrap();

        // Seed a valid journal in the parent so "old state" always exists.
        let state = Arc::new(AppState::empty_process(None, 512 * 1024 * 1024, false));
        set_recovery_dir(&state, recovery.clone());
        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 512, 512);
        layer.bump_raw_rev();
        doc.root.push(LayerNode::Leaf(layer));
        let session = state.spawn_session(doc);
        if let Ok(mut g) = session.history.saved_snapshot.lock() {
            *g = None;
        }
        fill_raw(&state, 1, 1, 512, 512);
        write_journal_for_doc(&state, 1).unwrap();
        let recovery_id = session.recovery_id;
        // Drop parent session locks before child uses the same dir.
        drop(session);
        drop(state);

        let mut child = Command::new(&exe)
            .env("JOURNAL_KILL_CHILD", "1")
            .env("JOURNAL_KILL_DIR", &recovery)
            .env("JOURNAL_KILL_RID", recovery_id.to_string())
            .env("RUST_BACKTRACE", "0")
            .args([
                "--exact",
                "journal_premerge_kill9_crash_loop",
                "--ignored",
                "--nocapture",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn kill child");

        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1);
        let delay_ms = 1 + (seed % 80);
        thread::sleep(Duration::from_millis(delay_ms));
        #[cfg(unix)]
        {
            let pid = child.id();
            let _ = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .status();
        }
        #[cfg(not(unix))]
        {
            let _ = child.kill();
        }
        let _ = child.wait();

        assert_journal_valid(&recovery, recovery_id);
        if i % 25 == 0 || i + 1 == iterations {
            println!("JOURNAL_KILL9 iter={}/{iterations} ok delay_ms={delay_ms}", i + 1);
        }
    }
    println!("JOURNAL_KILL9 all {iterations} iterations recovered old-or-new valid state");
}
