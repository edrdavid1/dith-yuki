//! Preview-latency diagnostic (AGENT_TASK_preview_latency).
//!
//! Not a correctness suite. Run:
//! `cargo test -p dither --release preview_latency_diag -- --ignored --nocapture --test-threads=1`

#![cfg(test)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use engine_project::document::DocumentHandle;
use engine_project::filter::{DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::Document;
use engine_tiles::{
    CacheStage, InvalidationEvent, PixelTile, Scheduler, TileCache, TileCoord, TileKey, HALO,
    TILE_SIZE,
};

use crate::commands::AppState;
use crate::tile_pipeline::{compute_composite_tile, compute_processed_tile};
use crate::viewport::{compute_pyramid_level, compute_visible_tiles, ViewportState};
use crate::worker::WorkerWake;

const DOC: u32 = 3072;
const LAYER: u32 = 1;
const VP_W: f64 = 1920.0;
const VP_H: f64 = 1080.0;

fn dither_params(mode: DitherModeV2) -> DitherParamsV2 {
    DitherParamsV2 {
        mode,
        levels: 4,
        pixel_size: 1,
        palette_id: None,
        ..DitherParamsV2::default()
    }
}

fn make_state(mode: DitherModeV2, gpu: Option<Arc<engine_gpu::GpuContext>>) -> Arc<AppState> {
    let mut doc = Document::new(DocumentId::new(1), DOC, DOC);
    let mut layer = Layer::new(LayerId::new(LAYER), LayerKind::Raster, DOC, DOC);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(dither_params(mode)),
    ));
    doc.root.push(LayerNode::Leaf(layer));

    Arc::new(AppState {
        document_handle: DocumentHandle::new(doc),
        tile_cache: TileCache::new(1024 * 1024 * 1024),
        scheduler: Scheduler::new(),
        viewport: Mutex::new(ViewportState::default()),
        worker_wake: WorkerWake::new(),
        palette_cache: engine_color::palette_cache::PaletteKdCache::new(),
        palette_lut_cache: engine_color::palette_lut::PaletteLutCache::new(),
        threshold_cache: engine_color::threshold_map::ThresholdMapCache::new(),
        error_residuals: engine_project::filters::ErrorResidualsStore::new(),
        block_representatives: engine_tiles::BlockRepresentativeCache::new(),
        diffusion_skip_counter: crate::diffusion_waiters::DiffusionSkipCounter::new(),
        pending_diffusion_waiters: crate::diffusion_waiters::PendingDiffusionWaiters::new(),
        gpu,
        panel_manager: Mutex::new(crate::panel_manager::PanelManager::new()),
        selection: Mutex::new(crate::commands::SelectionState::default()),
        dock_affinity: Mutex::new(crate::dock_affinity::DockAffinityController::new(true)),
        float_drag_mouseup_cancel: Arc::new(AtomicBool::new(true)),
        float_drag_mouseup_hook: Mutex::new(None),
        project_path: Mutex::new(None),
        undo_manager: Mutex::new(crate::undo::UndoManager::new()),
        saved_snapshot: Mutex::new(None),
        preview_pass_inflight: std::sync::atomic::AtomicUsize::new(0),
        pending_preview_refresh: Mutex::new(None),
        ed_prefix_lock: Mutex::new(()),
    })
}

fn fill_raw_tiles(state: &AppState, coords: &[TileCoord]) {
    for coord in coords {
        let mut tile = PixelTile::new();
        let full = TILE_SIZE + 2 * HALO;
        for y in 0..full {
            for x in 0..full {
                let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
                let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
                let r = (gx.max(0) as f32) / DOC as f32;
                let g = (gy.max(0) as f32) / DOC as f32;
                tile.set(x, y, 0, r);
                tile.set(x, y, 1, g);
                tile.set(x, y, 2, 0.5);
                tile.set(x, y, 3, 1.0);
            }
        }
        state.tile_cache.insert_fresh(
            TileKey {
                layer: LAYER,
                coord: *coord,
                stage: CacheStage::Raw,
            },
            Arc::new(tile),
        );
    }
}

fn l0_grid() -> Vec<TileCoord> {
    let cols = (DOC + TILE_SIZE - 1) / TILE_SIZE;
    let rows = (DOC + TILE_SIZE - 1) / TILE_SIZE;
    let mut out = Vec::with_capacity((cols * rows) as usize);
    for y in 0..rows {
        for x in 0..cols {
            out.push(TileCoord { level: 0, x, y });
        }
    }
    out
}

fn prefix_to(coord: TileCoord) -> Vec<TileCoord> {
    let mut out = Vec::new();
    for y in 0..=coord.y {
        for x in 0..=coord.x {
            out.push(TileCoord {
                level: coord.level,
                x,
                y,
            });
        }
    }
    out
}

fn count_fresh(state: &AppState, layer: u32, stage: CacheStage, coords: &[TileCoord]) -> usize {
    coords
        .iter()
        .filter(|coord| {
            let key = TileKey {
                layer,
                coord: **coord,
                stage,
            };
            match state.tile_cache.entries.get(&key) {
                Some(e) => !e.dirty.load(Ordering::Acquire),
                None => false,
            }
        })
        .count()
}

fn simulate_update_filter(state: &AppState) {
    state.error_residuals.clear();
    state.block_representatives.clear_dithered();
    state.document_handle.mutate(|doc| {
        doc.increment_generation();
    });
    {
        let snapshot = state.document_handle.snapshot();
        snapshot.generations.increment_layer_gen(LAYER);
    }
    engine_tiles::invalidation::invalidate(
        &state.tile_cache,
        InvalidationEvent::LayerFilterChanged { layer: LAYER },
    );
    crate::commands::schedule_dirty_viewport_tiles(state);
}

struct DrainStats {
    wall: Duration,
    first_ok: Option<Duration>,
    processed_calls: u64,
    composite_ok: u64,
    composite_retry: u64,
}

fn drain_until_visible(
    state: &Arc<AppState>,
    visible: &[TileCoord],
    n_workers: usize,
    timeout: Duration,
) -> DrainStats {
    let t0 = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let first_ns = Arc::new(AtomicU64::new(0));
    let processed_calls = Arc::new(AtomicU64::new(0));
    let composite_ok = Arc::new(AtomicU64::new(0));
    let composite_retry = Arc::new(AtomicU64::new(0));
    let visible: Vec<TileCoord> = visible.to_vec();

    std::thread::scope(|scope| {
        for _ in 0..n_workers {
            let state = Arc::clone(state);
            let stop = Arc::clone(&stop);
            let first_ns = Arc::clone(&first_ns);
            let processed_calls = Arc::clone(&processed_calls);
            let composite_ok = Arc::clone(&composite_ok);
            let composite_retry = Arc::clone(&composite_retry);
            let visible = visible.clone();
            scope.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if count_fresh(&state, 0, CacheStage::Composite, &visible) == visible.len() {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    match state.scheduler.dequeue() {
                        Some(task) => match task.key.stage {
                            CacheStage::Processed => {
                                processed_calls.fetch_add(1, Ordering::Relaxed);
                                let _ = compute_processed_tile(task.key, &state);
                            }
                            CacheStage::Composite => match compute_composite_tile(task.key, &state) {
                                Ok(_) => {
                                    composite_ok.fetch_add(1, Ordering::Relaxed);
                                    if visible.contains(&task.key.coord) {
                                        let _ = first_ns.compare_exchange(
                                            0,
                                            t0.elapsed().as_nanos() as u64,
                                            Ordering::Relaxed,
                                            Ordering::Relaxed,
                                        );
                                    }
                                }
                                Err(_) => {
                                    composite_retry.fetch_add(1, Ordering::Relaxed);
                                }
                            },
                            CacheStage::Raw => {}
                        },
                        None => {
                            if count_fresh(&state, 0, CacheStage::Composite, &visible)
                                == visible.len()
                            {
                                stop.store(true, Ordering::Relaxed);
                                break;
                            }
                            std::thread::yield_now();
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

    let first = first_ns.load(Ordering::Relaxed);
    DrainStats {
        wall: t0.elapsed(),
        first_ok: if first == 0 {
            None
        } else {
            Some(Duration::from_nanos(first))
        },
        processed_calls: processed_calls.load(Ordering::Relaxed),
        composite_ok: composite_ok.load(Ordering::Relaxed),
        composite_retry: composite_retry.load(Ordering::Relaxed),
    }
}

fn set_viewport(state: &AppState, zoom: f64, x: f64, y: f64) -> Vec<TileCoord> {
    let max_level = crate::viewport::compute_max_level(DOC, DOC);
    let level = compute_pyramid_level(zoom, max_level);
    let visible = compute_visible_tiles(zoom, x, y, VP_W, VP_H, level, DOC, DOC);
    let mut vp = state.viewport.lock().unwrap();
    vp.zoom = zoom;
    vp.x = x;
    vp.y = y;
    vp.width = VP_W;
    vp.height = VP_H;
    vp.level = level;
    vp.visible_tiles = visible.clone();
    vp.prefetch_tiles = Vec::new();
    visible
}

fn n_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}

fn fmt_ms(d: Duration) -> String {
    format!("{:.1}ms", d.as_secs_f64() * 1000.0)
}

fn run_viewport_scenario(
    label: &str,
    mode: DitherModeV2,
    zoom: f64,
    x: f64,
    y: f64,
    raw_coords: &[TileCoord],
    gpu: Option<Arc<engine_gpu::GpuContext>>,
) {
    let workers = n_workers();
    let state = make_state(mode, gpu);
    fill_raw_tiles(&state, raw_coords);
    let visible = set_viewport(&state, zoom, x, y);

    // Warm: one full compute so the slider path is invalidate+recompute, not cold fill.
    simulate_update_filter(&state);
    let warm = drain_until_visible(&state, &visible, workers, Duration::from_secs(120));
    assert_eq!(
        count_fresh(&state, 0, CacheStage::Composite, &visible),
        visible.len(),
        "{label} warm did not finish (wall={})",
        fmt_ms(warm.wall)
    );

    simulate_update_filter(&state);
    let dirty_processed = {
        let mut n = 0u64;
        for e in state.tile_cache.entries.iter() {
            if e.key().stage == CacheStage::Processed && e.dirty.load(Ordering::Acquire) {
                n += 1;
            }
        }
        n
    };
    let queued = visible.len();
    let stats = drain_until_visible(&state, &visible, workers, Duration::from_secs(120));
    let fresh = count_fresh(&state, 0, CacheStage::Composite, &visible);

    println!(
        "SCENARIO {label}\n  workers={workers} visible={} (level={}) dirty_processed_after_invalidate={dirty_processed} scheduled_composites={queued}\n  wall={}  first_visible_ok={}  processed_calls={}  composite_ok={}  composite_retry={}  fresh={}/{}\n",
        visible.len(),
        state.viewport.lock().unwrap().level,
        fmt_ms(stats.wall),
        stats
            .first_ok
            .map(fmt_ms)
            .unwrap_or_else(|| "n/a".into()),
        stats.processed_calls,
        stats.composite_ok,
        stats.composite_retry,
        fresh,
        visible.len()
    );
}

/// Same body as `run_viewport_scenario` but returns the numbers instead of
/// printing, so the A/B can aggregate.
fn measure_scenario(
    mode: DitherModeV2,
    zoom: f64,
    x: f64,
    y: f64,
    raw_coords: &[TileCoord],
) -> (Duration, Option<Duration>) {
    let workers = n_workers();
    let state = make_state(mode, None);
    fill_raw_tiles(&state, raw_coords);
    let visible = set_viewport(&state, zoom, x, y);

    simulate_update_filter(&state);
    drain_until_visible(&state, &visible, workers, Duration::from_secs(120));

    simulate_update_filter(&state);
    let stats = drain_until_visible(&state, &visible, workers, Duration::from_secs(120));
    assert_eq!(
        count_fresh(&state, 0, CacheStage::Composite, &visible),
        visible.len(),
        "scenario did not finish"
    );
    (stats.wall, stats.first_ok)
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

/// A/B the anti-diagonal ED prefill against the depth-first walk. Both arms run
/// interleaved in one process so machine drift hits them equally — measuring the
/// arms in separate runs made an untouched Bayer path look 40% faster.
#[test]
#[ignore = "diagnostic: cargo test -p dither --release ed_prefix_ab -- --ignored --nocapture --test-threads=1"]
fn ed_prefix_ab() {
    const REPEATS: usize = 7;

    let origin = compute_visible_tiles(1.0, 0.0, 0.0, VP_W, VP_H, 0, DOC, DOC);
    let far = compute_visible_tiles(1.0, 2048.0, 2048.0, VP_W, VP_H, 0, DOC, DOC);
    let far_prefix = {
        let max = far.iter().fold(TileCoord { level: 0, x: 0, y: 0 }, |a, c| TileCoord {
            level: 0,
            x: a.x.max(c.x),
            y: a.y.max(c.y),
        });
        prefix_to(max)
    };
    let all_l0 = l0_grid();

    let cases: Vec<(&str, DitherModeV2, f64, f64, f64, &[TileCoord])> = vec![
        ("FS far-corner 100%", DitherModeV2::FloydSteinberg, 1.0, 2048.0, 2048.0, &far_prefix),
        ("FS origin 100%", DitherModeV2::FloydSteinberg, 1.0, 0.0, 0.0, &origin),
        ("FS fit 25%", DitherModeV2::FloydSteinberg, 0.25, 0.0, 0.0, &all_l0),
        ("Bayer fit 25% (control)", DitherModeV2::Bayer8x8, 0.25, 0.0, 0.0, &all_l0),
    ];

    println!(
        "\n=== ED prefix A/B (workers={}, {REPEATS} repeats, interleaved) ===\n",
        n_workers()
    );

    for (label, mode, zoom, x, y, coords) in cases {
        let mut serial = Vec::new();
        let mut diagonal = Vec::new();
        let mut serial_first = Vec::new();
        let mut diagonal_first = Vec::new();

        for _ in 0..REPEATS {
            std::env::set_var("DITHER_ED_SERIAL_PREFIX", "1");
            let (w, f) = measure_scenario(mode.clone(), zoom, x, y, coords);
            serial.push(w.as_secs_f64() * 1000.0);
            serial_first.push(f.map(|d| d.as_secs_f64() * 1000.0).unwrap_or(f64::NAN));

            std::env::remove_var("DITHER_ED_SERIAL_PREFIX");
            let (w, f) = measure_scenario(mode.clone(), zoom, x, y, coords);
            diagonal.push(w.as_secs_f64() * 1000.0);
            diagonal_first.push(f.map(|d| d.as_secs_f64() * 1000.0).unwrap_or(f64::NAN));
        }

        let (ms, md) = (median(serial), median(diagonal));
        println!(
            "{label}\n  wall   depth-first={ms:7.1}ms  diagonal={md:7.1}ms  ratio={:.2}x\n  first  depth-first={:7.1}ms  diagonal={:7.1}ms\n",
            ms / md,
            median(serial_first),
            median(diagonal_first),
        );
    }
}

fn single_tile_apply(mode: DitherModeV2, gpu: Option<Arc<engine_gpu::GpuContext>>) -> Duration {
    let state = make_state(mode, gpu);
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    fill_raw_tiles(&state, &[coord]);
    let key = TileKey {
        layer: LAYER,
        coord,
        stage: CacheStage::Processed,
    };
    let t0 = Instant::now();
    compute_processed_tile(key, &state).expect("single tile");
    t0.elapsed()
}

#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_3k() {
    let workers = n_workers();
    println!(
        "\n=== preview latency diag ===\nplatform=macos workers={workers} doc={DOC}x{DOC} vp={VP_W}x{VP_H} TILE={TILE_SIZE}\n"
    );

    let origin = compute_visible_tiles(1.0, 0.0, 0.0, VP_W, VP_H, 0, DOC, DOC);
    let far = compute_visible_tiles(1.0, 2048.0, 2048.0, VP_W, VP_H, 0, DOC, DOC);
    let fit_level = compute_pyramid_level(0.25, crate::viewport::compute_max_level(DOC, DOC));
    let fit = compute_visible_tiles(0.25, 0.0, 0.0, VP_W, VP_H, fit_level, DOC, DOC);
    println!(
        "tile counts: origin100%={} far-corner100%={} fit25%={} (level {fit_level}) l0_grid={}\n",
        origin.len(),
        far.len(),
        fit.len(),
        l0_grid().len()
    );

    let bayer = single_tile_apply(DitherModeV2::Bayer8x8, None);
    let fs = single_tile_apply(DitherModeV2::FloydSteinberg, None);
    println!(
        "single-tile CPU: Bayer8x8={}  FloydSteinberg={}\n",
        fmt_ms(bayer),
        fmt_ms(fs)
    );

    // Wavefront: one worker, farthest origin-viewport tile.
    let farthest = *origin.last().unwrap();
    {
        let state = make_state(DitherModeV2::Bayer8x8, None);
        fill_raw_tiles(&state, &origin);
        let t0 = Instant::now();
        compute_processed_tile(
            TileKey {
                layer: LAYER,
                coord: farthest,
                stage: CacheStage::Processed,
            },
            &state,
        )
        .unwrap();
        let n = count_fresh(&state, LAYER, CacheStage::Processed, &origin);
        println!(
            "wavefront Bayer farthest {:?}: wall={} fresh_processed_in_origin_set={n} (expect 1)\n",
            farthest,
            fmt_ms(t0.elapsed())
        );
    }
    {
        let state = make_state(DitherModeV2::FloydSteinberg, None);
        fill_raw_tiles(&state, &prefix_to(farthest));
        let t0 = Instant::now();
        compute_processed_tile(
            TileKey {
                layer: LAYER,
                coord: farthest,
                stage: CacheStage::Processed,
            },
            &state,
        )
        .unwrap();
        let prefix = prefix_to(farthest);
        let n = count_fresh(&state, LAYER, CacheStage::Processed, &prefix);
        println!(
            "wavefront FS farthest {:?}: wall={} fresh_processed_in_prefix={n}/{} (one worker recurse)\n",
            farthest,
            fmt_ms(t0.elapsed()),
            prefix.len()
        );
    }

    run_viewport_scenario(
        "Bayer CPU origin 100%",
        DitherModeV2::Bayer8x8,
        1.0,
        0.0,
        0.0,
        &origin,
        None,
    );
    run_viewport_scenario(
        "FS CPU origin 100%",
        DitherModeV2::FloydSteinberg,
        1.0,
        0.0,
        0.0,
        &origin,
        None,
    );

    let far_prefix = {
        let max = far.iter().fold(TileCoord { level: 0, x: 0, y: 0 }, |a, c| {
            TileCoord {
                level: 0,
                x: a.x.max(c.x),
                y: a.y.max(c.y),
            }
        });
        prefix_to(max)
    };
    run_viewport_scenario(
        "Bayer CPU far-corner 100%",
        DitherModeV2::Bayer8x8,
        1.0,
        2048.0,
        2048.0,
        &far,
        None,
    );
    run_viewport_scenario(
        "FS CPU far-corner 100%",
        DitherModeV2::FloydSteinberg,
        1.0,
        2048.0,
        2048.0,
        &far_prefix,
        None,
    );

    let all_l0 = l0_grid();
    run_viewport_scenario(
        "Bayer CPU fit 25% (L2 display / L0 filters)",
        DitherModeV2::Bayer8x8,
        0.25,
        0.0,
        0.0,
        &all_l0,
        None,
    );
    run_viewport_scenario(
        "FS CPU fit 25% (L2 display / L0 filters)",
        DitherModeV2::FloydSteinberg,
        0.25,
        0.0,
        0.0,
        &all_l0,
        None,
    );

    // GPU: eligible Bayer only. Env must be set because try_ordered_bayer_gpu gates on it.
    match engine_gpu::GpuContext::try_new_blocking() {
        Some(ctx) => {
            std::env::set_var("DITHER_GPU", "1");
            let gpu = Arc::new(ctx);
            let gpu_tile = single_tile_apply(DitherModeV2::Bayer8x8, Some(Arc::clone(&gpu)));
            println!("single-tile GPU Bayer8x8={}\n", fmt_ms(gpu_tile));
            run_viewport_scenario(
                "Bayer GPU origin 100% (DITHER_GPU=1)",
                DitherModeV2::Bayer8x8,
                1.0,
                0.0,
                0.0,
                &origin,
                Some(gpu),
            );
            std::env::remove_var("DITHER_GPU");
        }
        None => {
            println!("GPU: no adapter — skip DITHER_GPU viewport timing\n");
        }
    }
}
