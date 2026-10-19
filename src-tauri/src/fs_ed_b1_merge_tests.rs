//! Pre-merge B1 hardening: bitwise vs sequential strip-once golden, and
//! ed-progress row-granularity on large grids.
//!
//! Sequential strip-once-per-ty matches pre-B1 *results* (pre-B1 recomputed the
//! same strip N times per row; output is identical when the strip is
//! deterministic).

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
    compute_processed_tile, ed_strip_busy_park_deps, schedule_ed_prefix_closure,
    wake_ed_frontier_after_insert,
};

const LAYER: u32 = 1;

fn ed_modes() -> Vec<DitherModeV2> {
    vec![
        DitherModeV2::FloydSteinberg,
        DitherModeV2::Atkinson,
        DitherModeV2::JarvisJudiceNinke,
        DitherModeV2::Stucki,
        DitherModeV2::Burkes,
        DitherModeV2::Fan93,
        DitherModeV2::Sierra,
        DitherModeV2::SierraTwoRow,
        DitherModeV2::SierraLite,
        DitherModeV2::ShiauFan,
        DitherModeV2::StevensonArce,
        DitherModeV2::Ostromoukhov,
        DitherModeV2::ZhouFang,
    ]
}

fn fill_raw(state: &AppState, w: u32, h: u32, layer: u32) {
    let tx_n = (w + TILE_SIZE - 1) / TILE_SIZE;
    let ty_n = (h + TILE_SIZE - 1) / TILE_SIZE;
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
                    let t = ((gx.wrapping_mul(3) + gy.wrapping_mul(5)) as f32 * 0.013).sin() * 0.5
                        + 0.5;
                    tile.set(x, y, 0, t);
                    tile.set(x, y, 1, (t * 0.7 + 0.15).clamp(0.0, 1.0));
                    tile.set(x, y, 2, (1.0 - t * 0.9).clamp(0.0, 1.0));
                    tile.set(x, y, 3, 1.0);
                }
            }
            state.tiles.tile_cache.insert_fresh(
                TileKey {
                    doc: 1,
                    layer,
                    coord,
                    stage: CacheStage::Raw,
                },
                Arc::new(tile),
            );
        }
    }
}

fn make_state_with_filters(w: u32, h: u32, filters: Vec<FilterInstance>) -> Arc<AppState> {
    let mut doc = Document::new(DocumentId::new(1), w, h);
    let mut layer = Layer::new(LayerId::new(LAYER), LayerKind::Raster, w, h);
    layer.filters = filters;
    doc.root.push(LayerNode::Leaf(layer));
    let state = AppState::empty_process(None, 1024 * 1024 * 1024, true);
    state.spawn_session(doc);
    let state = Arc::new(state);
    fill_raw(&state, w, h, LAYER);
    state
}

fn dither_filter(mode: DitherModeV2, pixel_size: u8) -> FilterInstance {
    FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode,
            levels: 4,
            pixel_size,
            palette_id: None,
            ..DitherParamsV2::default()
        }),
    )
}

fn levels_filter() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Levels,
        FilterParams::Levels {
            input_black: 0.05,
            input_white: 0.95,
            gamma: 1.1,
            output_black: 0.0,
            output_white: 1.0,
            channel_r: true,
            channel_g: true,
            channel_b: true,
        },
    )
}

fn adjust_filter() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Adjust,
        FilterParams::Adjust {
            contrast: 0.1,
            brightness: -0.02,
            saturation: 0.0,
            blur: 0.0,
            sharpness: 0.0,
            noise: 0.0,
        },
    )
}

fn invalidate(state: &AppState) {
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

fn pkey(x: u32, y: u32) -> TileKey {
    TileKey {
        doc: 1,
        layer: LAYER,
        coord: TileCoord { level: 0, x, y },
        stage: CacheStage::Processed,
    }
}

fn grid(w: u32, h: u32) -> (u32, u32) {
    (
        (w + TILE_SIZE - 1) / TILE_SIZE,
        (h + TILE_SIZE - 1) / TILE_SIZE,
    )
}

fn count_fresh(state: &AppState, tx_n: u32, ty_n: u32) -> u64 {
    let mut n = 0u64;
    for y in 0..ty_n {
        for x in 0..tx_n {
            if engine_tiles::tile_fresh(&state.tiles.tile_cache, pkey(x, y)) {
                n += 1;
            }
        }
    }
    n
}

/// Sequential strip-once golden (one `compute` per ty via min_x owner).
fn compute_sequential_golden(state: &AppState, tx_n: u32, ty_n: u32) {
    invalidate(state);
    for y in 0..ty_n {
        compute_processed_tile(pkey(0, y), state).expect("sequential strip owner");
        for x in 0..tx_n {
            assert!(
                engine_tiles::tile_fresh(&state.tiles.tile_cache, pkey(x, y)),
                "sequential row {y} missing tx={x}"
            );
        }
    }
}

fn snapshot_processed(state: &AppState, tx_n: u32, ty_n: u32) -> Vec<Box<[f32]>> {
    let mut out = Vec::with_capacity((tx_n * ty_n) as usize);
    for y in 0..ty_n {
        for x in 0..tx_n {
            let tile = state
                .tiles
                .tile_cache
                .get_entry(pkey(x, y))
                .expect("processed");
            out.push(tile.data.clone());
        }
    }
    out
}

fn drain_workers(state: &Arc<AppState>, tx_n: u32, ty_n: u32, n_workers: usize) {
    invalidate(state);
    schedule_ed_prefix_closure(
        state,
        1,
        LAYER,
        0,
        tx_n - 1,
        ty_n - 1,
        Priority::Immediate,
        0,
        0,
    );
    let total = (tx_n * ty_n) as u64;
    let stop = Arc::new(AtomicBool::new(false));
    let t0 = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..n_workers {
            let state = Arc::clone(state);
            let stop = Arc::clone(&stop);
            scope.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if count_fresh(&state, tx_n, ty_n) >= total {
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
                            match compute_processed_tile(task.key, &state) {
                                Ok(_) => wake_ed_frontier_after_insert(&state, task.key),
                                Err(engine_project::error::EngineError::EdStripBusy) => {
                                    let deps = ed_strip_busy_park_deps(&state, task.key);
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
                            if count_fresh(&state, tx_n, ty_n) >= total {
                                stop.store(true, Ordering::Relaxed);
                                break;
                            }
                            std::thread::yield_now();
                        }
                    }
                    if t0.elapsed() > Duration::from_secs(60) {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }
            });
        }
    });
    assert_eq!(count_fresh(state, tx_n, ty_n), total, "drain incomplete");
    assert!(!state.tiles.ed_strip_gates.any_owned());
}

fn assert_bitwise_eq(a: &[Box<[f32]>], b: &[Box<[f32]>], label: &str) {
    assert_eq!(a.len(), b.len(), "{label}: tile count");
    for (i, (ta, tb)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(ta.len(), tb.len(), "{label}: tile {i} len");
        for (j, (pa, pb)) in ta.iter().zip(tb.iter()).enumerate() {
            assert_eq!(
                pa.to_bits(),
                pb.to_bits(),
                "{label}: mismatch tile={i} idx={j} a={pa} b={pb}"
            );
        }
    }
}

#[test]
fn b1_bitwise_vs_sequential_non_multiple_and_stacks_and_kernels() {
    let cases: Vec<(u32, u32, Vec<FilterInstance>, String)> = vec![
        (
            300,
            200,
            vec![dither_filter(DitherModeV2::FloydSteinberg, 1)],
            "300x200 FS".into(),
        ),
        (
            520,
            400,
            vec![dither_filter(DitherModeV2::FloydSteinberg, 2)],
            "520x400 FS pixel_size=2".into(),
        ),
        (
            384,
            384,
            vec![
                levels_filter(),
                dither_filter(DitherModeV2::Atkinson, 1),
                adjust_filter(),
            ],
            "stack Levels→Atkinson→Adjust".into(),
        ),
    ];

    for (w, h, filters, label) in cases {
        let (tx_n, ty_n) = grid(w, h);
        let golden_state = make_state_with_filters(w, h, filters.clone());
        compute_sequential_golden(&golden_state, tx_n, ty_n);
        let golden = snapshot_processed(&golden_state, tx_n, ty_n);

        let concurrent = make_state_with_filters(w, h, filters);
        drain_workers(&concurrent, tx_n, ty_n, 8);
        let got = snapshot_processed(&concurrent, tx_n, ty_n);
        assert_bitwise_eq(&golden, &got, &label);
    }

    for mode in ed_modes() {
        let label = format!("kernel {mode:?}");
        let w = 512u32;
        let h = 320u32;
        let (tx_n, ty_n) = grid(w, h);
        let filters = vec![dither_filter(mode, 1)];
        let golden_state = make_state_with_filters(w, h, filters.clone());
        compute_sequential_golden(&golden_state, tx_n, ty_n);
        let golden = snapshot_processed(&golden_state, tx_n, ty_n);
        let concurrent = make_state_with_filters(w, h, filters);
        drain_workers(&concurrent, tx_n, ty_n, 4);
        let got = snapshot_processed(&concurrent, tx_n, ty_n);
        assert_bitwise_eq(&golden, &got, &label);
    }
}

/// ed-progress style counter: `done` = fresh L0 Processed. Per-row strip publish
/// must keep `done` monotonic and never jump past `total`.
#[test]
fn b1_ed_progress_row_granularity_large_doc() {
    let w = 2048u32;
    let h = 1536u32; // 8×6 = 48 tiles
    let (tx_n, ty_n) = grid(w, h);
    let state = make_state_with_filters(w, h, vec![dither_filter(DitherModeV2::FloydSteinberg, 1)]);
    invalidate(&state);
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

    let total = (tx_n * ty_n) as u64;
    let samples = Arc::new(std::sync::Mutex::new(Vec::<u64>::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let done_max = Arc::new(AtomicU64::new(0));

    // Progress sampler
    {
        let state = Arc::clone(&state);
        let samples = Arc::clone(&samples);
        let stop = Arc::clone(&stop);
        let done_max = Arc::clone(&done_max);
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let d = count_fresh(&state, tx_n, ty_n);
                done_max.fetch_max(d, Ordering::Relaxed);
                if let Ok(mut v) = samples.lock() {
                    if v.last().copied() != Some(d) {
                        v.push(d);
                    }
                }
                if d >= total {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
    }

    drain_workers(&state, tx_n, ty_n, 8);
    let final_done = count_fresh(&state, tx_n, ty_n);
    assert_eq!(final_done, total);
    if let Ok(mut v) = samples.lock() {
        if v.last().copied() != Some(final_done) {
            v.push(final_done);
        }
    }
    stop.store(true, Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(5));

    let series = samples.lock().unwrap().clone();
    assert!(!series.is_empty());
    assert_eq!(*series.last().unwrap(), total);
    for w in series.windows(2) {
        assert!(w[1] >= w[0], "ed-progress must be monotonic: {series:?}");
        let jump = w[1] - w[0];
        // Row publish can jump by a full row (tx_n), never more than remaining.
        assert!(
            jump <= tx_n as u64,
            "progress jump {jump} exceeds row width {tx_n}; series={series:?}"
        );
    }
    assert!(done_max.load(Ordering::Relaxed) <= total);
    assert_eq!(final_done, total);
}
