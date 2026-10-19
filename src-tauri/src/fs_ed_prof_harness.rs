//! Step A: CPU Floyd–Steinberg L0 pass harness @ 5984×4172 (release).
//!
//! ```text
//! DITHER_FS_PROF=1 cargo test -p dither --release fs_ed_l0_5984x4172 \
//!   -- --ignored --nocapture --test-threads=1
//! ```
//!
//! For Instruments/`sample` (no tracing): omit `DITHER_FS_PROF` / `DITHER_TRACE`.

#![cfg(test)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine_project::document::Document;
use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::invalidation::InvalidationEvent;
use engine_tiles::{CacheStage, PixelTile, Priority, TileCoord, TileKey, HALO, TILE_SIZE};

use crate::commands::AppState;
use crate::tile_pipeline::{
    compute_processed_tile, schedule_ed_prefix_closure, wake_ed_frontier_after_insert,
};

const DOC_W: u32 = 5984;
const DOC_H: u32 = 4172;
const LAYER: u32 = 1;

fn tiles_x() -> u32 {
    (DOC_W + TILE_SIZE - 1) / TILE_SIZE
}
fn tiles_y() -> u32 {
    (DOC_H + TILE_SIZE - 1) / TILE_SIZE
}

fn n_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn make_state() -> Arc<AppState> {
    let mut doc = Document::new(DocumentId::new(1), DOC_W, DOC_H);
    let mut layer = Layer::new(LayerId::new(LAYER), LayerKind::Raster, DOC_W, DOC_H);
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

    let state = AppState::empty_process(None, 2048 * 1024 * 1024, true);
    state.spawn_session(doc);
    Arc::new(state)
}

fn fill_all_raw(state: &AppState) {
    let full = TILE_SIZE + 2 * HALO;
    let tx_n = tiles_x();
    let ty_n = tiles_y();
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
                    doc: 1,
                    layer: LAYER,
                    coord,
                    stage: CacheStage::Raw,
                },
                Arc::new(tile),
            );
        }
    }
}

fn count_fresh_processed_grid(state: &AppState, tx_n: u32, ty_n: u32) -> usize {
    let mut n = 0usize;
    for y in 0..ty_n {
        for x in 0..tx_n {
            let key = TileKey {
                doc: 1,
                layer: LAYER,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Processed,
            };
            if engine_tiles::tile_fresh(&state.tiles.tile_cache, key) {
                n += 1;
            }
        }
    }
    n
}

fn count_fresh_processed(state: &AppState) -> usize {
    count_fresh_processed_grid(state, tiles_x(), tiles_y())
}

fn invalidate_processed(state: &AppState) {
    state.tiles.error_residuals.clear();
    state.tiles.clear_strip_gates();
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        InvalidationEvent::LayerFilterChanged {
            doc: 1,
            layer: LAYER,
        },
    );
}

fn drain_l0_processed_grid(
    state: &Arc<AppState>,
    tx_n: u32,
    ty_n: u32,
    timeout: Duration,
) -> (Duration, u64) {
    let total = (tx_n * ty_n) as usize;
    let t0 = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let processed_calls = Arc::new(AtomicU64::new(0));
    let workers = n_workers();

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let state = Arc::clone(state);
            let stop = Arc::clone(&stop);
            let processed_calls = Arc::clone(&processed_calls);
            scope.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if count_fresh_processed_grid(&state, tx_n, ty_n) >= total {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    match state.tiles.scheduler.dequeue() {
                        Some(task) => {
                            if task.key.stage != CacheStage::Processed {
                                continue;
                            }
                            if !engine_tiles::ed_ready(&state.tiles.tile_cache, task.key, true) {
                                state
                                    .tiles
                                    .ed_frontier
                                    .block(task, &state.tiles.tile_cache);
                                continue;
                            }
                            processed_calls.fetch_add(1, Ordering::Relaxed);
                            match compute_processed_tile(task.key, &state) {
                                Ok(_) => {
                                    wake_ed_frontier_after_insert(&state, task.key);
                                }
                                Err(engine_project::error::EngineError::EdStripBusy) => {
                                    let deps = crate::tile_pipeline::ed_strip_busy_park_deps(
                                        &state, task.key,
                                    );
                                    if deps.is_empty() {
                                        state.tiles.scheduler.enqueue_or_bump(task);
                                    } else {
                                        let _ = state.tiles.ed_frontier.block_on(
                                            task,
                                            deps,
                                            &state.tiles.tile_cache,
                                        );
                                    }
                                }
                                Err(_) => {}
                            }
                        }
                        None => {
                            if count_fresh_processed_grid(&state, tx_n, ty_n) >= total {
                                stop.store(true, Ordering::Relaxed);
                                break;
                            }
                            if state.tiles.ed_frontier.blocked_count() == 0 {
                                std::thread::yield_now();
                            } else {
                                std::thread::sleep(Duration::from_micros(50));
                            }
                        }
                    }
                    if t0.elapsed() > timeout {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
        }
    });

    (t0.elapsed(), processed_calls.load(Ordering::Relaxed))
}

fn drain_l0_processed(state: &Arc<AppState>, timeout: Duration) -> (Duration, u64) {
    drain_l0_processed_grid(state, tiles_x(), tiles_y(), timeout)
}

#[test]
#[ignore = "diagnostic: DITHER_FS_PROF=1 cargo test -p dither --release fs_ed_l0_5984x4172 -- --ignored --nocapture --test-threads=1"]
fn fs_ed_l0_5984x4172() {
    // Prefer env; also force so OnceLock freeze from other tests cannot mute counters.
    engine_project::filters::fs_ed_prof::init_from_env();
    if std::env::var("DITHER_FS_PROF").ok().as_deref() == Some("1")
        || std::env::var("DITHER_TRACE").ok().as_deref() == Some("1")
    {
        engine_project::filters::fs_ed_prof::force_enable(true);
    }
    engine_project::filters::fs_ed_prof::reset();

    let tx_n = tiles_x();
    let ty_n = tiles_y();
    let total = (tx_n * ty_n) as usize;
    let workers = n_workers();
    println!(
        "=== fs_ed L0 harness ===\n  doc={DOC_W}x{DOC_H} tiles={tx_n}x{ty_n}={total} workers={workers}\n"
    );

    let loops: u32 = std::env::var("DITHER_FS_PROF_LOOPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);

    let state = make_state();
    fill_all_raw(&state);

    for loop_i in 0..loops {
        // Cold L0 ED: schedule full prefix to bottom-right, drain Processed.
        invalidate_processed(&state);
        if loop_i == 0 {
            engine_project::filters::fs_ed_prof::reset();
        }
        let t_sched = Instant::now();
        let scheduled = schedule_ed_prefix_closure(
            &state,
            1,
            LAYER,
            0,
            tx_n - 1,
            ty_n - 1,
            Priority::Immediate,
            0,
            0,
        );
        let sched_ms = t_sched.elapsed().as_secs_f64() * 1e3;
        println!("loop={loop_i} scheduled_tasks={scheduled} sched_ms={sched_ms:.1}");

        let (wall, processed_calls) = drain_l0_processed(&state, Duration::from_secs(300));
        let fresh = count_fresh_processed(&state);
        let wall_s = wall.as_secs_f64();
        let mpix = (DOC_W as f64 * DOC_H as f64) / 1e6;
        let mpix_s = mpix / wall_s;

        println!(
            "loop={loop_i} wall_s={wall_s:.3} fresh={fresh}/{total} processed_calls={processed_calls} \
             doc_MPix={mpix:.2} wall_MPix/s={mpix_s:.2}"
        );
        assert_eq!(fresh, total, "L0 Processed incomplete");
    }

    let snap = engine_project::filters::fs_ed_prof::Snapshot::capture();
    print!("{}", snap.report_lines());
    if snap.strip_runs > 0 && snap.unique_ty_strips > 0 {
        let runs_per_line = snap.strip_runs as f64 / snap.unique_ty_strips as f64;
        println!(
            "strip_runs/line={runs_per_line:.2} tile_tasks/line={:.2} recalc_extra/line={:.2}",
            snap.tile_tasks as f64 / snap.unique_ty_strips as f64,
            snap.strip_recalc_extra as f64 / snap.unique_ty_strips as f64,
        );
        assert!(
            (runs_per_line - 1.0).abs() < 1e-9,
            "B1 target strip_runs/line=1.0, got {runs_per_line}"
        );
        assert_eq!(snap.strip_recalc_extra, 0);
    }
}

/// Step1 / B1 baseline: cold L0 ED wall for three doc sizes (release).
///
/// ```text
/// DITHER_FS_PROF=1 cargo test -p dither --release ed_l0_pass_baseline_three_sizes \
///   -- --ignored --nocapture --test-threads=1
/// ```
#[test]
#[ignore = "diagnostic: DITHER_FS_PROF=1 cargo test -p dither --release ed_l0_pass_baseline_three_sizes -- --ignored --nocapture --test-threads=1"]
fn ed_l0_pass_baseline_three_sizes() {
    engine_project::filters::fs_ed_prof::force_enable(true);

    let sizes: &[(u32, u32)] = &[(748, 1024), (1920, 1080), (4000, 3000)];
    println!("=== ed_l0_pass_baseline_three_sizes (B1) workers={} ===\n", n_workers());

    for &(w, h) in sizes {
        let tx_n = (w + TILE_SIZE - 1) / TILE_SIZE;
        let ty_n = (h + TILE_SIZE - 1) / TILE_SIZE;
        let total = (tx_n * ty_n) as usize;

        let mut doc = Document::new(DocumentId::new(1), w, h);
        let mut layer = Layer::new(LayerId::new(LAYER), LayerKind::Raster, w, h);
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
        let state = AppState::empty_process(None, 2048 * 1024 * 1024, true);
        state.spawn_session(doc);
        let state = Arc::new(state);

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
                        doc: 1,
                        layer: LAYER,
                        coord,
                        stage: CacheStage::Raw,
                    },
                    Arc::new(tile),
                );
            }
        }

        invalidate_processed(&state);
        engine_project::filters::fs_ed_prof::reset();
        schedule_ed_prefix_closure(
            &state,
            1,
            LAYER,
            0,
            tx_n - 1,
            ty_n - 1,
            Priority::Immediate,
            0,
            0,
        );
        let (wall, processed_calls) =
            drain_l0_processed_grid(&state, tx_n, ty_n, Duration::from_secs(300));
        let fresh = count_fresh_processed_grid(&state, tx_n, ty_n);
        let snap = engine_project::filters::fs_ed_prof::Snapshot::capture();
        let mpix = (w as f64 * h as f64) / 1e6;
        let wall_s = wall.as_secs_f64();
        let runs_per_line = if snap.unique_ty_strips > 0 {
            snap.strip_runs as f64 / snap.unique_ty_strips as f64
        } else {
            f64::NAN
        };
        println!(
            "{w}x{h}: wall_ms={:.1} MPix/s={:.2} fresh={fresh}/{total} \
             processed_calls={processed_calls} strip_runs={} unique_ty={} \
             strip_runs/line={runs_per_line:.2} recalc_extra={}",
            wall_s * 1e3,
            mpix / wall_s,
            snap.strip_runs,
            snap.unique_ty_strips,
            snap.strip_recalc_extra,
        );
        assert_eq!(fresh, total);
        assert_eq!(snap.strip_runs, snap.unique_ty_strips);
        assert_eq!(snap.strip_recalc_extra, 0);
    }
}
