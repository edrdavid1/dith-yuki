//! Preview-latency diagnostic (AGENT_TASK_preview_latency).
//!
//! Not a correctness suite. Run:
//! `cargo test -p dither --release preview_latency_diag -- --ignored --nocapture --test-threads=1`

#![cfg(test)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use engine_gpu::{
    GpuCompositeFrameJob, GpuCompositeLayerOp, GpuCompositeTileWork, GpuFrameJob, GpuTileWork,
};
use engine_project::document::DocumentHandle;
use engine_project::filter::{DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams};
use engine_project::filters::gpu_graph::compile_layer_graph;
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{BlendMode, DocumentId, LayerId, LayerKind};
use engine_project::Document;
use engine_tiles::{
    CacheStage, InvalidationEvent, PixelTile, Scheduler, TileCache, TileCoord, TileKey, HALO,
    TILE_SIZE,
};

use engine_project::filters::apply::apply_filter_to_tile;

use crate::commands::AppState;
use crate::services::document_service::{encode_rgba_to_png, f32_to_u8};
use crate::tile_pipeline::{compute_composite_tile, compute_processed_tile};
use crate::viewport::{
    compute_prefetch_ring, compute_pyramid_level, compute_visible_tiles, ViewportState,
};
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

    let state = AppState::empty_process(gpu, 1024 * 1024 * 1024, true);
    state.spawn_session(doc);
    Arc::new(state)
}

/// Typical user stack for T8: Adjust → Floyd–Steinberg → Bayer4 (ED forces CpuCheckpoint).
fn make_realistic_stack_state(gpu: Option<Arc<engine_gpu::GpuContext>>) -> Arc<AppState> {
    let mut doc = Document::new(DocumentId::new(1), DOC, DOC);
    let mut layer = Layer::new(LayerId::new(LAYER), LayerKind::Raster, DOC, DOC);
    layer.filters.push(FilterInstance::new(
        FilterKind::Adjust,
        FilterParams::Adjust {
            contrast: 0.15,
            brightness: 0.0,
            saturation: -0.1,
            blur: 0.0,
            sharpness: 0.0,
            noise: 0.0,
        },
    ));
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(dither_params(DitherModeV2::FloydSteinberg)),
    ));
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(dither_params(DitherModeV2::Bayer4x4)),
    ));
    doc.root.push(LayerNode::Leaf(layer));

    let state = AppState::empty_process(gpu, 1024 * 1024 * 1024, true);
    state.spawn_session(doc);
    Arc::new(state)
}

fn fill_raw_tiles(state: &AppState, coords: &[TileCoord]) {
    fill_raw_tiles_for_layer(state, LAYER, coords);
}

fn fill_raw_tiles_for_layer(state: &AppState, layer: u32, coords: &[TileCoord]) {
    for coord in coords {
        let mut tile = PixelTile::new();
        let full = TILE_SIZE + 2 * HALO;
        for y in 0..full {
            for x in 0..full {
                let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
                let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
                let r = (gx.max(0) as f32) / DOC as f32;
                let g = (gy.max(0) as f32) / DOC as f32;
                // Tint by layer id so multi-layer composites are non-trivial.
                let tint = (layer as f32 * 0.17).fract();
                tile.set(x, y, 0, (r + tint).min(1.0));
                tile.set(x, y, 1, g);
                tile.set(x, y, 2, 0.5);
                tile.set(x, y, 3, 1.0);
            }
        }
        state.tiles.tile_cache.insert_fresh(
            TileKey {
                doc: 1,
                layer,
                coord: *coord,
                stage: CacheStage::Raw,
            },
            Arc::new(tile),
        );
    }
}

/// Three flat raster layers for T7.5 composite timing (Normal / Multiply / Screen).
fn make_multilayer_state(gpu: Option<Arc<engine_gpu::GpuContext>>) -> Arc<AppState> {
    let mut doc = Document::new(DocumentId::new(1), DOC, DOC);
    for (id, mode, opacity) in [
        (1u32, BlendMode::Normal, 1.0f32),
        (2, BlendMode::Multiply, 1.0),
        (3, BlendMode::Screen, 0.85),
    ] {
        let mut layer = Layer::new(LayerId::new(id), LayerKind::Raster, DOC, DOC);
        layer.blend_mode = mode;
        layer.opacity = opacity;
        doc.root.push(LayerNode::Leaf(layer));
    }
    let state = AppState::empty_process(gpu, 1024 * 1024 * 1024, true);
    state.spawn_session(doc);
    Arc::new(state)
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
                doc: 1,
                layer,
                coord: **coord,
                stage,
            };
            match state.tiles.tile_cache.entries.get(&key) {
                Some(e) => !e.dirty.load(Ordering::Acquire),
                None => false,
            }
        })
        .count()
}

fn simulate_invalidate_only(state: &AppState) {
    state.tiles.error_residuals.clear();
    state.tiles.block_representatives.clear_dithered();
    state.must_active().document_handle.mutate(|doc| {
        doc.increment_generation();
    });
    {
        let snapshot = state.must_active().document_handle.snapshot();
        snapshot.generations.increment_layer_gen(LAYER);
    }
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        InvalidationEvent::LayerFilterChanged { doc: 1, layer: LAYER },
    );
}

fn simulate_update_filter(state: &AppState) {
    simulate_invalidate_only(state);
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
                    match state.tiles.scheduler.dequeue() {
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
    let mut vp = state.ui.viewport.lock().unwrap();
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
        for e in state.tiles.tile_cache.entries.iter() {
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
        state.ui.viewport.lock().unwrap().level,
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
    measure_scenario_gpu(mode, zoom, x, y, raw_coords, None)
}

fn measure_scenario_gpu(
    mode: DitherModeV2,
    zoom: f64,
    x: f64,
    y: f64,
    raw_coords: &[TileCoord],
    gpu: Option<Arc<engine_gpu::GpuContext>>,
) -> (Duration, Option<Duration>) {
    let workers = n_workers();
    let state = make_state(mode, gpu);
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

fn measure_realistic_stack_scenario(
    _zoom: f64,
    _x: f64,
    _y: f64,
    raw_coords: &[TileCoord],
) -> (Duration, Option<Duration>) {
    // Direct sequential compute (no scheduler): ED wavefront needs a filled
    // dependency prefix, and the diag worker drain + EdFrontier park path is
    // too brittle for a timing harness.
    let state = make_realistic_stack_state(None);
    let max = raw_coords.iter().fold(
        TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        |a, c| TileCoord {
            level: 0,
            x: a.x.max(c.x),
            y: a.y.max(c.y),
        },
    );
    let prefix = prefix_to(max);
    fill_raw_tiles(&state, &prefix);

    // Warm
    for coord in &prefix {
        let key = TileKey {
            doc: 1,
            layer: LAYER,
            coord: *coord,
            stage: CacheStage::Processed,
        };
        compute_processed_tile(key, &state).expect("warm processed");
    }
    for coord in raw_coords {
        let key = TileKey {
            doc: 1,
            layer: 0,
            coord: *coord,
            stage: CacheStage::Composite,
        };
        compute_composite_tile(key, &state).expect("warm composite");
    }

    simulate_invalidate_only(&state);
    // Re-fill Raw after invalidate wiped freshness; keep pixels.
    fill_raw_tiles(&state, &prefix);

    let t0 = Instant::now();
    let mut first_ok = None;
    for coord in &prefix {
        let key = TileKey {
            doc: 1,
            layer: LAYER,
            coord: *coord,
            stage: CacheStage::Processed,
        };
        compute_processed_tile(key, &state).expect("processed");
    }
    for coord in raw_coords {
        let key = TileKey {
            doc: 1,
            layer: 0,
            coord: *coord,
            stage: CacheStage::Composite,
        };
        compute_composite_tile(key, &state).expect("composite");
        if first_ok.is_none() {
            first_ok = Some(t0.elapsed());
        }
    }
    (t0.elapsed(), first_ok)
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn percentile(mut v: Vec<f64>, p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((v.len() as f64 * p).ceil() as usize)
        .saturating_sub(1)
        .min(v.len() - 1);
    v[idx]
}

fn p95(v: Vec<f64>) -> f64 {
    percentile(v, 0.95)
}

fn p99(v: Vec<f64>) -> f64 {
    percentile(v, 0.99)
}

fn stddev(samples: &[f64], mean: f64) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let var = samples
        .iter()
        .map(|x| {
            let d = x - mean;
            d * d
        })
        .sum::<f64>()
        / (samples.len() - 1) as f64;
    var.sqrt()
}

struct SampleStats {
    n: usize,
    median: f64,
    mean: f64,
    sigma: f64,
    p95: f64,
    p99: f64,
}

fn sample_stats(samples: &[f64]) -> SampleStats {
    let n = samples.len();
    let mean = if n == 0 {
        f64::NAN
    } else {
        samples.iter().sum::<f64>() / n as f64
    };
    SampleStats {
        n,
        median: median(samples.to_vec()),
        mean,
        sigma: stddev(samples, mean),
        p95: p95(samples.to_vec()),
        p99: p99(samples.to_vec()),
    }
}

fn fmt_stats(s: &SampleStats) -> String {
    format!(
        "median: {:.2} ms (σ={:.2}), p95: {:.2} ms, p99: {:.2} ms, n={}",
        s.median, s.sigma, s.p95, s.p99, s.n
    )
}

/// Rough CI overlap: |median_a − median_b| < 1.96 * sqrt(σ_a²/n + σ_b²/n) → not proven faster.
fn not_proven_faster(winner: &SampleStats, baseline: &SampleStats) -> bool {
    if winner.n == 0 || baseline.n == 0 {
        return true;
    }
    let se = ((winner.sigma.powi(2) / winner.n as f64)
        + (baseline.sigma.powi(2) / baseline.n as f64))
        .sqrt();
    (baseline.median - winner.median) < 1.96 * se
}

fn verdict_faster(candidate: &SampleStats, baseline: &SampleStats, label: &str) -> String {
    if candidate.median >= baseline.median {
        format!("{label}: not faster (median ≥ baseline)")
    } else if not_proven_faster(candidate, baseline) {
        format!("{label}: not proven faster (CI overlap)")
    } else {
        format!("{label}: faster (significant)")
    }
}

fn build_resident_frame_job(
    state: &AppState,
    graph: &std::sync::Arc<engine_gpu::ComputeGraph>,
) -> Option<GpuFrameJob> {
    let snapshot = state.must_active().document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let doc = snapshot.id.0;
    let viewport = state.ui.viewport.lock().unwrap().clone();

    let mut tiles = Vec::new();
    for coord in &viewport.visible_tiles {
        if coord.level != 0 {
            continue;
        }
        let raw_key = TileKey {
            doc,
            layer: LAYER,
            coord: *coord,
            stage: CacheStage::Raw,
        };
        let raw = state.tiles.tile_cache.get_entry(raw_key)?;
        let processed_key = TileKey {
            stage: CacheStage::Processed,
            ..raw_key
        };
        tiles.push(GpuTileWork {
            key: processed_key,
            coord: *coord,
            generation: doc_gen,
            pixels: raw,
        });
    }
    if tiles.is_empty() {
        return None;
    }
    Some(GpuFrameJob {
        doc_gen,
        graph: std::sync::Arc::clone(graph),
        tiles,
        speculative: false,
    })
}

/// GPU-resident Path B: one frame job for all visible L0 tiles (Bayer-only graph).
/// Returns (p95 wall ms, tile count, v1 single-tile ms if measured).
fn run_resident_viewport_benchmark(
    origin: &[TileCoord],
    gpu: &Arc<engine_gpu::GpuContext>,
    repeats: usize,
) -> (f64, usize, f64) {
    std::env::set_var("DITHER_GPU_RESIDENT", "1");
    std::env::set_var("DITHER_GPU_RESIDENT_DIAG", "1");
    let state = make_state(DitherModeV2::Bayer4x4, Some(Arc::clone(gpu)));
    fill_raw_tiles(&state, origin);
    set_viewport(&state, 1.0, 0.0, 0.0);
    let l0_count = origin.len();

    let layer = state
        .must_active()
        .document_handle
        .snapshot()
        .root
        .iter()
        .find_map(|n| match n {
            LayerNode::Leaf(l) if l.id.0 == LAYER => Some(l.clone()),
            _ => None,
        })
        .expect("layer");
    let graph = std::sync::Arc::new(
        compile_layer_graph(&layer.filters).expect("resident graph must compile"),
    );
    assert!(graph.is_gpu_only());

    let executor = state.gpu_executor.as_ref().unwrap().lock().unwrap();

    // Warm: promote + first dispatch (excluded from steady-state p95).
    let warm_job = build_resident_frame_job(&state, &graph).expect("warm job");
    let warm_t0 = Instant::now();
    executor.submit_frame_blocking(warm_job).expect("gpu submit");
    let cold_promote_ms = warm_t0.elapsed().as_secs_f64() * 1000.0;

    // Steady-state: same generation, resident slots already warm — re-dispatch only.
    // (Do not invalidate: that forces re-promote and used to make G2 unreadable.)
    let mut samples = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        let job = build_resident_frame_job(&state, &graph).expect("frame job");
        assert_eq!(job.tiles.len(), l0_count);
        let t0 = Instant::now();
        executor.submit_frame_blocking(job).expect("gpu submit");
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let resident_p95 = p95(samples.clone());
    let max_ms = samples.iter().copied().fold(0.0_f64, f64::max);

    std::env::remove_var("DITHER_GPU_RESIDENT");
    std::env::remove_var("DITHER_GPU_RESIDENT_DIAG");

    println!(
        "SCENARIO Bayer GPU-resident origin 100% (DITHER_GPU_RESIDENT=1)\n  visible_l0={l0_count}  cold_promote+1st={cold_promote_ms:.3}ms  steady p95({repeats})={resident_p95:.3}ms  max={max_ms:.3}ms  ms/tile={:.3}\n",
        resident_p95 / l0_count.max(1) as f64
    );
    (resident_p95, l0_count, resident_p95)
}

fn measure_resident_viewport_p95(
    mode: DitherModeV2,
    zoom: f64,
    x: f64,
    y: f64,
    raw_coords: &[TileCoord],
    gpu: &Arc<engine_gpu::GpuContext>,
    repeats: usize,
) -> f64 {
    let _ = (mode, zoom, x, y);
    run_resident_viewport_benchmark(raw_coords, gpu, repeats).0
}

fn build_resident_composite_job(
    state: &AppState,
    layer_ids: &[u32],
    modes: &[(u32, f32)],
) -> Option<GpuCompositeFrameJob> {
    let snapshot = state.must_active().document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let doc = snapshot.id.0;
    let viewport = state.ui.viewport.lock().unwrap().clone();

    let mut tiles = Vec::new();
    for coord in &viewport.visible_tiles {
        if coord.level != 0 {
            continue;
        }
        let mut ops = Vec::new();
        for (i, &layer_id) in layer_ids.iter().enumerate() {
            let processed_key = TileKey {
                doc,
                layer: layer_id,
                coord: *coord,
                stage: CacheStage::Processed,
            };
            let pixels = state
                .tiles
                .tile_cache
                .get_entry(processed_key)
                .or_else(|| {
                    state.tiles.tile_cache.get_entry(TileKey {
                        stage: CacheStage::Raw,
                        ..processed_key
                    })
                })?;
            let (blend_mode, opacity) = modes[i];
            ops.push(GpuCompositeLayerOp {
                processed_key,
                blend_mode,
                opacity,
                pixels: Some(pixels),
            });
        }
        tiles.push(GpuCompositeTileWork {
            coord: *coord,
            composite_key: TileKey {
                doc,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            },
            generation: doc_gen,
            layers: ops,
        });
    }
    if tiles.is_empty() {
        return None;
    }
    Some(GpuCompositeFrameJob {
        doc_gen,
        tiles,
        speculative: false,
    })
}

/// T7.5: multi-layer resident composite p95 (3 layers, no filters — blend only).
fn run_resident_composite_benchmark(
    origin: &[TileCoord],
    gpu: &Arc<engine_gpu::GpuContext>,
    repeats: usize,
) -> f64 {
    std::env::set_var("DITHER_GPU_RESIDENT", "1");
    let state = make_multilayer_state(Some(Arc::clone(gpu)));
    for layer in [1u32, 2, 3] {
        fill_raw_tiles_for_layer(&state, layer, origin);
    }
    set_viewport(&state, 1.0, 0.0, 0.0);
    let l0_count = origin.len();

    let layer_ids = [1u32, 2, 3];
    let modes = [
        (BlendMode::Normal as u32, 1.0f32),
        (BlendMode::Multiply as u32, 1.0),
        (BlendMode::Screen as u32, 0.85),
    ];

    let executor = state.gpu_executor.as_ref().unwrap().lock().unwrap();
    if let Some(job) = build_resident_composite_job(&state, &layer_ids, &modes) {
        executor.submit_composite_blocking(job).expect("gpu submit");
    }

    let mut samples = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        let job = build_resident_composite_job(&state, &layer_ids, &modes).expect("composite job");
        assert_eq!(job.tiles.len(), l0_count);
        let t0 = Instant::now();
        executor.submit_composite_blocking(job).expect("gpu submit");
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let composite_p95 = p95(samples.clone());
    let max_ms = samples.iter().copied().fold(0.0_f64, f64::max);

    // CPU reference: composite_tile wall for same viewport (one run).
    let cpu_t0 = Instant::now();
    for coord in origin {
        let key = TileKey {
            doc: 1,
            layer: 0,
            coord: *coord,
            stage: CacheStage::Composite,
        };
        let _ = compute_composite_tile(key, &state);
    }
    let cpu_ms = cpu_t0.elapsed().as_secs_f64() * 1000.0;

    std::env::remove_var("DITHER_GPU_RESIDENT");

    println!(
        "SCENARIO multi-layer GPU-resident composite (3 layers, origin 100%)\n  visible_l0={l0_count}  GPU p95({repeats})={composite_p95:.3}ms  max={max_ms:.3}ms  CPU composite once={cpu_ms:.1}ms  ms/tile GPU={:.3}\n",
        composite_p95 / l0_count.max(1) as f64
    );
    composite_p95
}

/// T6 gate: Bayer GPU-resident viewport ~40 tiles vs CPU pool (release, `--ignored`).
#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag_gpu_resident -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_gpu_resident() {
    let origin = compute_visible_tiles(1.0, 0.0, 0.0, VP_W, VP_H, 0, DOC, DOC);
    println!(
        "\n=== T6 GPU-resident viewport diag ===\norigin100% L0 tiles={}\n",
        origin.len()
    );
    run_gpu_viewport_timing(&origin);
}

/// T7.5: multi-layer composite resident vs CPU composite.
#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag_gpu_composite -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_gpu_composite() {
    let origin = compute_visible_tiles(1.0, 0.0, 0.0, VP_W, VP_H, 0, DOC, DOC);
    println!(
        "\n=== T7.5 multi-layer GPU-resident composite diag ===\norigin100% L0 tiles={}\n",
        origin.len()
    );
    match engine_gpu::GpuContext::try_new_blocking() {
        Some(ctx) => {
            let gpu = Arc::new(ctx);
            let _ = run_resident_composite_benchmark(&origin, &gpu, 7);
        }
        None => println!("GPU: no adapter — skip composite timing\n"),
    }
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
        doc: 1,
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
                doc: 1,
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
        let prefix = prefix_to(farthest);
        fill_raw_tiles(&state, &prefix);
        let t0 = Instant::now();
        for coord in &prefix {
            compute_processed_tile(
                TileKey {
                    doc: 1,
                    layer: LAYER,
                    coord: *coord,
                    stage: CacheStage::Processed,
                },
                &state,
            )
            .expect("FS prefix tile");
        }
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

    // Path B + v1 GPU timing (before ED scenarios — FS drain can timeout in this harness).
    run_gpu_viewport_timing(&origin);

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
}

fn run_gpu_viewport_timing(origin: &[TileCoord]) {
    match engine_gpu::GpuContext::try_new_blocking() {
        Some(ctx) => {
            let gpu = Arc::new(ctx);

            std::env::set_var("DITHER_GPU", "1");
            let gpu_tile = single_tile_apply(DitherModeV2::Bayer8x8, Some(Arc::clone(&gpu)));
            println!("single-tile GPU v1 Bayer8x8={}\n", fmt_ms(gpu_tile));
            std::env::remove_var("DITHER_GPU");

            const RESIDENT_REPEATS: usize = 7;
            let resident_p95 = measure_resident_viewport_p95(
                DitherModeV2::Bayer4x4,
                1.0,
                0.0,
                0.0,
                origin,
                &gpu,
                RESIDENT_REPEATS,
            );
            let resident_n = origin.len();

            let cpu_p95 = {
                let mut cpu_samples = Vec::with_capacity(RESIDENT_REPEATS);
                for _ in 0..RESIDENT_REPEATS {
                    let (wall, _) = measure_scenario(
                        DitherModeV2::Bayer8x8,
                        1.0,
                        0.0,
                        0.0,
                        origin,
                    );
                    cpu_samples.push(wall.as_secs_f64() * 1000.0);
                }
                p95(cpu_samples)
            };

            let gate_pass = resident_p95 < cpu_p95;
            println!(
                "=== T6 Path B summary (origin ~{resident_n} L0 tiles, {RESIDENT_REPEATS} repeats) ===\n  CPU worker pool p95 (Bayer8)     = {cpu_p95:7.1}ms\n  GPU-resident frame p95 (Bayer4)  = {resident_p95:7.3}ms\n  gate (resident p95 < CPU p95)    = {gate_pass}\n"
            );

            let composite_p95 =
                run_resident_composite_benchmark(origin, &gpu, RESIDENT_REPEATS);
            println!(
                "=== T7.5 multi-layer composite (same origin, 3 layers) ===\n  GPU-resident composite p95 = {composite_p95:7.3}ms\n"
            );
        }
        None => {
            println!("GPU: no adapter — skip DITHER_GPU_RESIDENT timing\n");
        }
    }
}

/// T8: Adjust → ED → Bayer vs Bayer-only — documents checkpoint tax for GpuPreviewGate.
#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag_realistic_stack -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_realistic_stack() {
    const REPEATS: usize = 5;
    // Small L0 footprint: full-origin 40-tile ED wavefront is too heavy for a diag loop.
    let tiles: Vec<TileCoord> = (0..2)
        .flat_map(|y| (0..3).map(move |x| TileCoord { level: 0, x, y }))
        .collect();
    println!(
        "\n=== T8 realistic stack diag (Adjust → FS → Bayer4) ===\nL0 tiles={} (2×3)\n",
        tiles.len()
    );

    let mut realistic = Vec::with_capacity(REPEATS);
    let mut bayer_only = Vec::with_capacity(REPEATS);
    for _ in 0..REPEATS {
        let (w, _) = measure_realistic_stack_scenario(1.0, 0.0, 0.0, &tiles);
        realistic.push(w.as_secs_f64() * 1000.0);
        let (w, _) = measure_scenario(DitherModeV2::Bayer4x4, 1.0, 0.0, 0.0, &tiles);
        bayer_only.push(w.as_secs_f64() * 1000.0);
    }
    let r_p95 = p95(realistic.clone());
    let b_p95 = p95(bayer_only.clone());
    let graph = compile_layer_graph(
        &make_realistic_stack_state(None)
            .must_active()
            .document_handle
            .snapshot()
            .root
            .iter()
            .find_map(|n| match n {
                LayerNode::Leaf(l) => Some(l.filters.clone()),
                _ => None,
            })
            .unwrap(),
    )
    .unwrap();
    let has_ed_checkpoint = graph.nodes.iter().any(|n| {
        matches!(
            n,
            engine_gpu::GraphNode::CpuCheckpoint(engine_gpu::CpuCheckpointKind::ErrorDiffusion)
        )
    });

    println!(
        "=== T8 summary (CPU worker pool, {REPEATS} repeats, {} tiles) ===\n  Bayer4-only p95              = {b_p95:7.1}ms\n  Adjust→FS→Bayer4 p95         = {r_p95:7.1}ms\n  ratio (realistic/Bayer)      = {:.2}x\n  graph has ED CpuCheckpoint   = {has_ed_checkpoint}\n  note: GPU-resident cannot absorb ED; Path B keeps CpuCheckpoint permanently (ED_DECISION.md)\n",
        tiles.len(),
        r_p95 / b_p95.max(1e-9),
    );
    assert!(has_ed_checkpoint, "realistic stack must compile with ED checkpoint");
}

// ─── Industrial gate (gpu-industrial-gate T1–T4) ───────────────────────────

const INDUSTRIAL_N: usize = 20;

fn print_industrial_row(name: &str, s: &SampleStats) {
    println!("  {name}: {}", fmt_stats(s));
}

/// One cold resident frame: fresh AppState + first submit (promote + dispatch).
fn measure_resident_cold_ms(
    origin: &[TileCoord],
    gpu: &Arc<engine_gpu::GpuContext>,
) -> f64 {
    std::env::set_var("DITHER_GPU_RESIDENT", "1");
    std::env::set_var("DITHER_GPU_RESIDENT_DIAG", "1");
    let state = make_state(DitherModeV2::Bayer4x4, Some(Arc::clone(gpu)));
    fill_raw_tiles(&state, origin);
    set_viewport(&state, 1.0, 0.0, 0.0);
    let layer = state
        .must_active()
        .document_handle
        .snapshot()
        .root
        .iter()
        .find_map(|n| match n {
            LayerNode::Leaf(l) if l.id.0 == LAYER => Some(l.clone()),
            _ => None,
        })
        .expect("layer");
    let graph = std::sync::Arc::new(compile_layer_graph(&layer.filters).expect("graph"));
    let executor = state.gpu_executor.as_ref().unwrap().lock().unwrap();
    let job = build_resident_frame_job(&state, &graph).expect("job");
    let t0 = Instant::now();
    executor.submit_frame_blocking(job).expect("submit");
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    std::env::remove_var("DITHER_GPU_RESIDENT");
    std::env::remove_var("DITHER_GPU_RESIDENT_DIAG");
    ms
}

/// Steady-state resident: warm once, then `n` re-dispatch samples (same generation).
fn measure_resident_steady_samples(
    zoom: f64,
    x: f64,
    y: f64,
    raw_coords: &[TileCoord],
    gpu: &Arc<engine_gpu::GpuContext>,
    n: usize,
) -> Vec<f64> {
    std::env::set_var("DITHER_GPU_RESIDENT", "1");
    std::env::set_var("DITHER_GPU_RESIDENT_DIAG", "1");
    let state = make_state(DitherModeV2::Bayer4x4, Some(Arc::clone(gpu)));
    fill_raw_tiles(&state, raw_coords);
    set_viewport(&state, zoom, x, y);
    let layer = state
        .must_active()
        .document_handle
        .snapshot()
        .root
        .iter()
        .find_map(|n| match n {
            LayerNode::Leaf(l) if l.id.0 == LAYER => Some(l.clone()),
            _ => None,
        })
        .expect("layer");
    let graph = std::sync::Arc::new(compile_layer_graph(&layer.filters).expect("graph"));
    let executor = state.gpu_executor.as_ref().unwrap().lock().unwrap();
    let warm = build_resident_frame_job(&state, &graph).expect("warm job (raw+viewport mismatch?)");
    executor
        .submit_frame_blocking(warm)
        .expect("warm submit");
    let mut samples = Vec::with_capacity(n);
    for _ in 0..n {
        let job = build_resident_frame_job(&state, &graph).expect("job");
        let t0 = Instant::now();
        executor.submit_frame_blocking(job).expect("submit");
        samples.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    drop(executor);
    std::env::remove_var("DITHER_GPU_RESIDENT");
    std::env::remove_var("DITHER_GPU_RESIDENT_DIAG");
    samples
}

fn make_crt_halftone_state(gpu: Option<Arc<engine_gpu::GpuContext>>) -> Arc<AppState> {
    let mut doc = Document::new(DocumentId::new(1), DOC, DOC);
    let mut layer = Layer::new(LayerId::new(LAYER), LayerKind::Raster, DOC, DOC);
    layer.filters.push(FilterInstance::new(
        FilterKind::Crt,
        FilterParams::Crt {
            period: 3,
            strength: 0.5,
            mask_strength: 0.35,
        },
    ));
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(dither_params(DitherModeV2::CmykHalftone)),
    ));
    doc.root.push(LayerNode::Leaf(layer));
    let state = AppState::empty_process(gpu, 1024 * 1024 * 1024, true);
    state.spawn_session(doc);
    Arc::new(state)
}

fn make_palette_fs_state(gpu: Option<Arc<engine_gpu::GpuContext>>) -> Arc<AppState> {
    use engine_color::palette::LinearColor;
    use engine_project::types::PaletteId;
    let mut doc = Document::new(DocumentId::new(1), DOC, DOC);
    let pid = doc.add_palette(
        "industrial-gate".into(),
        vec![
            LinearColor {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
            LinearColor {
                r: 0.8,
                g: 0.2,
                b: 0.2,
            },
            LinearColor {
                r: 0.2,
                g: 0.6,
                b: 0.9,
            },
        ],
    );
    assert_eq!(pid, PaletteId::new(1));
    let mut layer = Layer::new(LayerId::new(LAYER), LayerKind::Raster, DOC, DOC);
    // Nearest palette quantize then FS — both force CpuCheckpoint on GPU graph.
    layer.filters.push(FilterInstance::new(
        FilterKind::PaletteQuantize,
        FilterParams::PaletteQuantize {
            palette_id: pid,
            diffusion: None,
        },
    ));
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(dither_params(DitherModeV2::FloydSteinberg)),
    ));
    doc.root.push(LayerNode::Leaf(layer));
    let state = AppState::empty_process(gpu, 1024 * 1024 * 1024, true);
    state.spawn_session(doc);
    Arc::new(state)
}

fn measure_filter_stack_cpu_ms(state_factory: impl Fn() -> Arc<AppState>, tiles: &[TileCoord]) -> f64 {
    let state = state_factory();
    let max = tiles.iter().fold(
        TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        |a, c| TileCoord {
            level: 0,
            x: a.x.max(c.x),
            y: a.y.max(c.y),
        },
    );
    let prefix = prefix_to(max);
    fill_raw_tiles(&state, &prefix);
    for coord in &prefix {
        let _ = compute_processed_tile(
            TileKey {
                doc: 1,
                layer: LAYER,
                coord: *coord,
                stage: CacheStage::Processed,
            },
            &state,
        );
    }
    for coord in tiles {
        let _ = compute_composite_tile(
            TileKey {
                doc: 1,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            },
            &state,
        );
    }
    simulate_invalidate_only(&state);
    fill_raw_tiles(&state, &prefix);
    let t0 = Instant::now();
    for coord in &prefix {
        compute_processed_tile(
            TileKey {
                doc: 1,
                layer: LAYER,
                coord: *coord,
                stage: CacheStage::Processed,
            },
            &state,
        )
        .expect("processed");
    }
    for coord in tiles {
        compute_composite_tile(
            TileKey {
                doc: 1,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            },
            &state,
        )
        .expect("composite");
    }
    t0.elapsed().as_secs_f64() * 1000.0
}

/// Industrial gate evidence harness: T1+T2 one thermal session, then T3 cold, T4 presets.
#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag_industrial_gate -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_industrial_gate() {
    let origin = compute_visible_tiles(1.0, 0.0, 0.0, VP_W, VP_H, 0, DOC, DOC);
    let far = compute_visible_tiles(1.0, 2048.0, 2048.0, VP_W, VP_H, 0, DOC, DOC);
    println!(
        "\n=== Industrial gate (n={INDUSTRIAL_N}, one continuous session) ===\nmachine workers={} origin_L0={} far_L0={}\n",
        n_workers(),
        origin.len(),
        far.len()
    );

    let Some(ctx) = engine_gpu::GpuContext::try_new_blocking() else {
        println!("GPU: no adapter — abort industrial gate");
        return;
    };
    let gpu = Arc::new(ctx);

    // ── T1+T2: back-to-back CPU → resident → v1 in one session ──
    // Not interleaved: two GpuExecutors on one GpuContext deadlock on Drop/shutdown.
    for (label, zoom, x, y, coords) in [
        ("Bayer origin 100%", 1.0, 0.0, 0.0, origin.as_slice()),
        ("Bayer far-corner 100%", 1.0, 2048.0, 2048.0, far.as_slice()),
    ] {
        println!("--- {label} (back-to-back CPU → resident → v1, same session) ---");
        println!("  … CPU ×{INDUSTRIAL_N}");
        let mut cpu = Vec::with_capacity(INDUSTRIAL_N);
        for i in 0..INDUSTRIAL_N {
            let (w, _) = measure_scenario(DitherModeV2::Bayer8x8, zoom, x, y, coords);
            cpu.push(w.as_secs_f64() * 1000.0);
            if i % 5 == 4 {
                println!("    CPU {}/{}", i + 1, INDUSTRIAL_N);
            }
        }

        println!("  … GPU-resident steady ×{INDUSTRIAL_N}");
        let resident = measure_resident_steady_samples(zoom, x, y, coords, &gpu, INDUSTRIAL_N);

        println!("  … v1 per-tile GPU ×{INDUSTRIAL_N}");
        let mut v1 = Vec::with_capacity(INDUSTRIAL_N);
        std::env::set_var("DITHER_GPU", "1");
        for i in 0..INDUSTRIAL_N {
            let (w, _) = measure_scenario_gpu(
                DitherModeV2::Bayer8x8,
                zoom,
                x,
                y,
                coords,
                Some(Arc::clone(&gpu)),
            );
            v1.push(w.as_secs_f64() * 1000.0);
            if i % 5 == 4 {
                println!("    v1 {}/{}", i + 1, INDUSTRIAL_N);
            }
        }
        std::env::remove_var("DITHER_GPU");

        let cpu_s = sample_stats(&cpu);
        let res_s = sample_stats(&resident);
        let v1_s = sample_stats(&v1);
        print_industrial_row("CPU worker pool", &cpu_s);
        print_industrial_row("GPU-resident steady", &res_s);
        print_industrial_row("v1 per-tile GPU", &v1_s);
        println!("  {}", verdict_faster(&res_s, &cpu_s, "resident vs CPU"));
        println!("  {}", verdict_faster(&res_s, &v1_s, "resident vs v1"));
        println!();
    }

    // ── T3: cold path (fresh promote each run) ──
    println!("--- cold path (resident vs CPU, fresh state each run) ---");
    let mut cold_gpu = Vec::with_capacity(INDUSTRIAL_N);
    let mut cold_cpu = Vec::with_capacity(INDUSTRIAL_N);
    for i in 0..INDUSTRIAL_N {
        cold_gpu.push(measure_resident_cold_ms(&origin, &gpu));
        // CPU cold: no warm drain — first schedule after fill
        let workers = n_workers();
        let state = make_state(DitherModeV2::Bayer8x8, None);
        fill_raw_tiles(&state, &origin);
        let visible = set_viewport(&state, 1.0, 0.0, 0.0);
        simulate_update_filter(&state);
        let t0 = Instant::now();
        let _stats = drain_until_visible(&state, &visible, workers, Duration::from_secs(120));
        assert_eq!(
            count_fresh(&state, 0, CacheStage::Composite, &visible),
            visible.len()
        );
        cold_cpu.push(t0.elapsed().as_secs_f64() * 1000.0);
        if i % 5 == 4 {
            println!("  cold {}/{}", i + 1, INDUSTRIAL_N);
        }
    }
    let cold_gpu_s = sample_stats(&cold_gpu);
    let cold_cpu_s = sample_stats(&cold_cpu);
    print_industrial_row("cold path GPU-resident", &cold_gpu_s);
    print_industrial_row("cold path CPU", &cold_cpu_s);
    println!("  {}", verdict_faster(&cold_gpu_s, &cold_cpu_s, "cold resident vs CPU"));
    println!();

    // ── T7.5 composite n=20 (same session) ──
    println!("--- multi-layer composite (3 layers, origin) ---");
    let mut comp_gpu = Vec::with_capacity(INDUSTRIAL_N);
    {
        std::env::set_var("DITHER_GPU_RESIDENT", "1");
        let state = make_multilayer_state(Some(Arc::clone(&gpu)));
        for layer in [1u32, 2, 3] {
            fill_raw_tiles_for_layer(&state, layer, &origin);
        }
        set_viewport(&state, 1.0, 0.0, 0.0);
        let layer_ids = [1u32, 2, 3];
        let modes = [
            (BlendMode::Normal as u32, 1.0f32),
            (BlendMode::Multiply as u32, 1.0),
            (BlendMode::Screen as u32, 0.85),
        ];
        let executor = state.gpu_executor.as_ref().unwrap().lock().unwrap();
        if let Some(job) = build_resident_composite_job(&state, &layer_ids, &modes) {
            executor.submit_composite_blocking(job).expect("warm composite");
        }
        for _ in 0..INDUSTRIAL_N {
            let job = build_resident_composite_job(&state, &layer_ids, &modes).expect("job");
            let t0 = Instant::now();
            executor.submit_composite_blocking(job).expect("composite");
            comp_gpu.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        std::env::remove_var("DITHER_GPU_RESIDENT");
    }
    let mut comp_cpu = Vec::with_capacity(INDUSTRIAL_N);
    for _ in 0..INDUSTRIAL_N {
        let state = make_multilayer_state(None);
        for layer in [1u32, 2, 3] {
            fill_raw_tiles_for_layer(&state, layer, &origin);
        }
        // promote Processed = Raw for empty filter stacks
        for layer in [1u32, 2, 3] {
            for coord in &origin {
                let raw = state
                    .tiles.tile_cache
                    .get_entry(TileKey {
                        doc: 1,
                        layer,
                        coord: *coord,
                        stage: CacheStage::Raw,
                    })
                    .expect("raw");
                state.tiles.tile_cache.insert_fresh(
                    TileKey {
                        doc: 1,
                        layer,
                        coord: *coord,
                        stage: CacheStage::Processed,
                    },
                    raw,
                );
            }
        }
        let t0 = Instant::now();
        for coord in &origin {
            compute_composite_tile(
                TileKey {
                    doc: 1,
                    layer: 0,
                    coord: *coord,
                    stage: CacheStage::Composite,
                },
                &state,
            )
            .expect("cpu composite");
        }
        comp_cpu.push(t0.elapsed().as_secs_f64() * 1000.0);
    }
    let cg = sample_stats(&comp_gpu);
    let cc = sample_stats(&comp_cpu);
    print_industrial_row("GPU fused composite", &cg);
    print_industrial_row("CPU composite", &cc);
    println!("  {}", verdict_faster(&cg, &cc, "composite GPU vs CPU"));
    println!();

    // ── T4: realistic presets (best-guess, not data-derived) ──
    let preset_tiles: Vec<TileCoord> = (0..2)
        .flat_map(|y| (0..3).map(move |x| TileCoord { level: 0, x, y }))
        .collect();
    println!(
        "--- realistic presets (best-guess, not data-derived; {} L0 tiles) ---",
        preset_tiles.len()
    );

    // A: Adjust→FS→Bayer — GPU-resident ineligible (ED checkpoint)
    let mut a_cpu = Vec::with_capacity(INDUSTRIAL_N);
    for _ in 0..INDUSTRIAL_N {
        let (w, _) = measure_realistic_stack_scenario(1.0, 0.0, 0.0, &preset_tiles);
        a_cpu.push(w.as_secs_f64() * 1000.0);
    }
    let a_s = sample_stats(&a_cpu);
    print_industrial_row("Preset A Adjust→FS→Bayer CPU", &a_s);
    println!(
        "  Preset A GPU-resident: ineligible (ED CpuCheckpoint) — effective path = CPU; no separate GPU win"
    );

    // B: Palette Strict → FS
    let mut b_cpu = Vec::with_capacity(INDUSTRIAL_N);
    for _ in 0..INDUSTRIAL_N {
        b_cpu.push(measure_filter_stack_cpu_ms(
            || make_palette_fs_state(None),
            &preset_tiles,
        ));
    }
    let b_s = sample_stats(&b_cpu);
    print_industrial_row("Preset B PaletteStrict→FS CPU", &b_s);
    let b_graph = compile_layer_graph(
        &make_palette_fs_state(None)
            .must_active()
            .document_handle
            .snapshot()
            .root
            .iter()
            .find_map(|n| match n {
                LayerNode::Leaf(l) => Some(l.filters.clone()),
                _ => None,
            })
            .unwrap(),
    );
    println!(
        "  Preset B GPU-only eligible: {} — checkpoint stacks stay CPU for preview",
        b_graph
            .as_ref()
            .map(|g| g.is_gpu_only())
            .unwrap_or(false)
    );

    // C: CRT → Halftone (no ED) — resident eligible
    let mut c_cpu = Vec::with_capacity(INDUSTRIAL_N);
    for _ in 0..INDUSTRIAL_N {
        c_cpu.push(measure_filter_stack_cpu_ms(
            || make_crt_halftone_state(None),
            &preset_tiles,
        ));
    }
    let c_cpu_s = sample_stats(&c_cpu);
    print_industrial_row("Preset C CRT→Halftone CPU", &c_cpu_s);

    let mut c_gpu = Vec::with_capacity(INDUSTRIAL_N);
    std::env::set_var("DITHER_GPU_RESIDENT", "1");
    std::env::set_var("DITHER_GPU_RESIDENT_DIAG", "1");
    let c_state = make_crt_halftone_state(Some(Arc::clone(&gpu)));
    fill_raw_tiles(&c_state, &preset_tiles);
    set_viewport(&c_state, 1.0, 0.0, 0.0);
    // Restrict visible to preset tiles for a fair small-footprint compare
    {
        let mut vp = c_state.ui.viewport.lock().unwrap();
        vp.visible_tiles = preset_tiles.clone();
    }
    let c_layer = c_state
        .must_active()
        .document_handle
        .snapshot()
        .root
        .iter()
        .find_map(|n| match n {
            LayerNode::Leaf(l) if l.id.0 == LAYER => Some(l.clone()),
            _ => None,
        })
        .expect("layer");
    let c_graph = std::sync::Arc::new(compile_layer_graph(&c_layer.filters).expect("crt+ht graph"));
    if c_graph.is_gpu_only() {
        let executor = c_state.gpu_executor.as_ref().unwrap().lock().unwrap();
        let warm = build_resident_frame_job(&c_state, &c_graph).expect("warm");
        executor.submit_frame_blocking(warm).expect("warm");
        for _ in 0..INDUSTRIAL_N {
            let job = build_resident_frame_job(&c_state, &c_graph).expect("job");
            let t0 = Instant::now();
            executor.submit_frame_blocking(job).expect("submit");
            c_gpu.push(t0.elapsed().as_secs_f64() * 1000.0);
        }
        let c_gpu_s = sample_stats(&c_gpu);
        print_industrial_row("Preset C CRT→Halftone GPU-resident", &c_gpu_s);
        println!(
            "  {}",
            verdict_faster(&c_gpu_s, &c_cpu_s, "Preset C resident vs CPU")
        );
    } else {
        println!("  Preset C graph not gpu_only — skip resident arm");
    }
    std::env::remove_var("DITHER_GPU_RESIDENT");
    std::env::remove_var("DITHER_GPU_RESIDENT_DIAG");

    println!("\n=== Industrial gate harness complete — paste rows into EVIDENCE.md ===\n");
}

fn a3_n() -> usize {
    std::env::var("DITHER_A3_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(INDUSTRIAL_N)
}

fn env_set(key: &str, val: Option<&str>) {
    match val {
        Some(v) => std::env::set_var(key, v),
        None => std::env::remove_var(key),
    }
}

fn set_viewport_with_prefetch(state: &AppState, zoom: f64, x: f64, y: f64) -> Vec<TileCoord> {
    let max_level = crate::viewport::compute_max_level(DOC, DOC);
    let level = compute_pyramid_level(zoom, max_level);
    let visible = compute_visible_tiles(zoom, x, y, VP_W, VP_H, level, DOC, DOC);
    let prefetch = compute_prefetch_ring(&visible, level, DOC, DOC);
    *state.ui.viewport.lock().unwrap() = ViewportState {
        zoom,
        x,
        y,
        width: VP_W,
        height: VP_H,
        level,
        visible_tiles: visible.clone(),
        prefetch_tiles: prefetch,
    };
    visible
}

fn fill_all_l0_raw(state: &AppState) {
    fill_raw_tiles(state, &l0_grid());
}

#[allow(dead_code)]
fn wait_gpu_slots(state: &AppState, coords: &[TileCoord], timeout: Duration) {
    let Some(cache) = state.gpu_resident.as_ref() else {
        return;
    };
    let Ok(session) = state.active_session() else {
        return;
    };
    let snapshot = session.document_handle.snapshot();
    let gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let doc = snapshot.id.0;
    let t0 = Instant::now();
    while t0.elapsed() < timeout {
        let warm = coords
            .iter()
            .filter(|coord| {
                cache
                    .get_slot(
                        &TileKey {
                            doc,
                            layer: 0,
                            coord: **coord,
                            stage: CacheStage::Composite,
                        },
                        gen,
                    )
                    .is_some()
            })
            .count();
        if !coords.is_empty() && warm == coords.len() {
            return;
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

#[derive(Clone, Copy)]
enum A3Mode {
    Cpu,
    OptIn,
    Auto,
}

fn measure_open_then_pan(mode: A3Mode, gpu: Option<Arc<engine_gpu::GpuContext>>) -> (f64, f64) {
    let workers = n_workers();
    engine_gpu::set_gpu_preview_ui_override(None);
    match mode {
        A3Mode::Cpu => {
            env_set("DITHER_GPU_PREVIEW", None);
            env_set("DITHER_GPU_WARMUP", Some("0"));
        }
        A3Mode::OptIn => {
            env_set("DITHER_GPU_PREVIEW", Some("1"));
            env_set("DITHER_GPU_WARMUP", Some("0"));
        }
        A3Mode::Auto => {
            env_set("DITHER_GPU_PREVIEW", None);
            env_set("DITHER_GPU_WARMUP", Some("1"));
        }
    }

    // Same GPU process for all arms: CPU authorship vs auto must not be
    // confounded by "no Metal adapter" vs "adapter + executor alive".
    let state = make_state(DitherModeV2::Bayer4x4, gpu);
    fill_all_l0_raw(&state);

    let origin = set_viewport_with_prefetch(&state, 1.0, 0.0, 0.0);
    let t_open = Instant::now();
    crate::commands::schedule_dirty_viewport_tiles(&state);
    let _ = drain_until_visible(&state, &origin, workers, Duration::from_secs(120));
    let open_ms = t_open.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        count_fresh(&state, 0, CacheStage::Composite, &origin),
        origin.len(),
        "open did not finish"
    );

    // Far pan is outside the origin prefetch ring. Do not kick A2 here: leftover
    // GPU work only taxes the timed pan (A1 download cannot hit these tiles).

    let far = set_viewport_with_prefetch(&state, 1.0, 2048.0, 2048.0);
    let t_pan = Instant::now();
    crate::commands::schedule_dirty_viewport_tiles(&state);
    let _ = drain_until_visible(&state, &far, workers, Duration::from_secs(120));
    let pan_ms = t_pan.elapsed().as_secs_f64() * 1000.0;
    assert_eq!(
        count_fresh(&state, 0, CacheStage::Composite, &far),
        far.len(),
        "pan did not finish"
    );

    env_set("DITHER_GPU_PREVIEW", None);
    env_set("DITHER_GPU_WARMUP", None);
    engine_gpu::set_gpu_preview_ui_override(None);
    (open_ms, pan_ms)
}

/// A3: auto-dispatch vs opt-in vs CPU on open → pan (Bayer4, L0).
///
/// `cargo test -p dither --release preview_latency_diag_auto_dispatch -- --ignored --nocapture --test-threads=1`
/// Optional `DITHER_A3_N` (default 20).
#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag_auto_dispatch -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_auto_dispatch() {
    let n = a3_n();
    println!("\n=== A3 auto-dispatch (n={n}, Bayer4, open origin → pan far) ===\n");

    let gpu = engine_gpu::GpuContext::try_new_blocking().map(Arc::new);
    if gpu.is_none() {
        println!("GPU: no adapter — abort A3");
        return;
    }

    let mut cpu_open = Vec::with_capacity(n);
    let mut cpu_pan = Vec::with_capacity(n);
    let mut opt_open = Vec::with_capacity(n);
    let mut opt_pan = Vec::with_capacity(n);
    let mut auto_open = Vec::with_capacity(n);
    let mut auto_pan = Vec::with_capacity(n);

    for i in 0..n {
        let (o, p) = measure_open_then_pan(A3Mode::Cpu, gpu.clone());
        cpu_open.push(o);
        cpu_pan.push(p);
        let (o, p) = measure_open_then_pan(A3Mode::OptIn, gpu.clone());
        opt_open.push(o);
        opt_pan.push(p);
        let (o, p) = measure_open_then_pan(A3Mode::Auto, gpu.clone());
        auto_open.push(o);
        auto_pan.push(p);
        if i % 5 == 4 {
            println!("  round {}/{n}", i + 1);
        }
    }

    let rows = [
        ("open CPU", sample_stats(&cpu_open)),
        ("open opt-in", sample_stats(&opt_open)),
        ("open auto", sample_stats(&auto_open)),
        ("pan CPU", sample_stats(&cpu_pan)),
        ("pan opt-in", sample_stats(&opt_pan)),
        ("pan auto", sample_stats(&auto_pan)),
    ];
    for (name, s) in &rows {
        print_industrial_row(name, s);
    }

    let cpu_o = sample_stats(&cpu_open);
    let cpu_p = sample_stats(&cpu_pan);
    let auto_o = sample_stats(&auto_open);
    let auto_p = sample_stats(&auto_pan);
    let opt_o = sample_stats(&opt_open);
    let opt_p = sample_stats(&opt_pan);

    let open_ok = engine_gpu::not_worse_than(auto_o.median, auto_o.sigma, cpu_o.median, cpu_o.sigma, n);
    let pan_ok = engine_gpu::not_worse_than(auto_p.median, auto_p.sigma, cpu_p.median, cpu_p.sigma, n);
    println!(
        "\n  gate auto vs CPU open: {}\n  gate auto vs CPU pan: {}\n  opt-in vs CPU open: {}\n  opt-in vs CPU pan: {}\n",
        if open_ok { "PASS (not worse)" } else { "FAIL (worse)" },
        if pan_ok { "PASS (not worse)" } else { "FAIL (worse)" },
        verdict_faster(&opt_o, &cpu_o, "opt-in open"),
        verdict_faster(&opt_p, &cpu_p, "opt-in pan"),
    );
    println!("=== A3 harness complete — paste into gpu-auto-dispatch/A3_EVIDENCE.md ===\n");
    assert!(open_ok && pan_ok, "A3 gate: auto-dispatch worse than CPU");
}

const A5_GPU_BATCH: usize = 32;

fn a5_n() -> usize {
    std::env::var("DITHER_A5_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(5)
}

fn pack_tile_into_rgba(processed: &PixelTile, col: u32, row: u32, rgba: &mut [u8]) {
    let tile_origin_x = col * TILE_SIZE;
    let tile_origin_y = row * TILE_SIZE;
    for ty in 0..TILE_SIZE {
        let img_y = tile_origin_y + ty;
        if img_y >= DOC {
            break;
        }
        for tx in 0..TILE_SIZE {
            let img_x = tile_origin_x + tx;
            if img_x >= DOC {
                break;
            }
            let buf_idx = ((img_y * DOC + img_x) * 4) as usize;
            let tile_x = tx + HALO;
            let tile_y = ty + HALO;
            rgba[buf_idx] = f32_to_u8(processed.at(tile_x, tile_y, 0));
            rgba[buf_idx + 1] = f32_to_u8(processed.at(tile_x, tile_y, 1));
            rgba[buf_idx + 2] = f32_to_u8(processed.at(tile_x, tile_y, 2));
            rgba[buf_idx + 3] = f32_to_u8(processed.at(tile_x, tile_y, 3));
        }
    }
}

fn a5_layer_and_doc(state: &AppState) -> (Layer, engine_project::Document) {
    let snapshot = state.must_active().document_handle.snapshot();
    let layer = snapshot
        .root
        .iter()
        .find_map(|n| match n {
            LayerNode::Leaf(l) if l.id.0 == LAYER => Some(l.clone()),
            _ => None,
        })
        .expect("A5 layer");
    let doc = (*snapshot).clone();
    (layer, doc)
}

/// Current `export_image` filter loop: sequential apply + pack (no encode, no disk).
fn measure_export_cpu_seq(state: &AppState, coords: &[TileCoord]) -> (f64, Vec<u8>) {
    let (layer, doc) = a5_layer_and_doc(state);
    let mut rgba = vec![0u8; (DOC * DOC * 4) as usize];
    let t0 = Instant::now();
    for coord in coords {
        let raw = state
            .tiles
            .tile_cache
            .get_entry(TileKey {
                doc: 1,
                layer: LAYER,
                coord: *coord,
                stage: CacheStage::Raw,
            })
            .expect("raw");
        let processed = apply_filter_to_tile(
            &raw,
            &layer,
            *coord,
            &state.tiles.palette_cache,
            &state.tiles.palette_lut_cache,
            &state.tiles.threshold_cache,
            &doc,
        )
        .expect("apply");
        pack_tile_into_rgba(&processed, coord.x, coord.y, &mut rgba);
    }
    (
        t0.elapsed().as_secs_f64() * 1000.0,
        rgba,
    )
}

/// Parallel CPU apply (same work as export, N worker threads) then pack.
fn measure_export_cpu_par(state: &AppState, coords: &[TileCoord]) -> f64 {
    let (layer, doc) = a5_layer_and_doc(state);
    let workers = n_workers();
    let mut rgba = vec![0u8; (DOC * DOC * 4) as usize];
    let t0 = Instant::now();
    let processed: Vec<(TileCoord, PixelTile)> = std::thread::scope(|s| {
        let chunk = (coords.len() + workers - 1) / workers.max(1);
        let mut joins = Vec::new();
        for piece in coords.chunks(chunk.max(1)) {
            let layer = &layer;
            let doc = &doc;
            joins.push(s.spawn(move || {
                let mut out = Vec::with_capacity(piece.len());
                for coord in piece {
                    let raw = state
                        .tiles
                        .tile_cache
                        .get_entry(TileKey {
                            doc: 1,
                            layer: LAYER,
                            coord: *coord,
                            stage: CacheStage::Raw,
                        })
                        .expect("raw");
                    let tile = apply_filter_to_tile(
                        &raw,
                        layer,
                        *coord,
                        &state.tiles.palette_cache,
                        &state.tiles.palette_lut_cache,
                        &state.tiles.threshold_cache,
                        doc,
                    )
                    .expect("apply");
                    out.push((*coord, tile));
                }
                out
            }));
        }
        joins
            .into_iter()
            .flat_map(|j| j.join().expect("worker"))
            .collect()
    });
    for (coord, tile) in processed {
        pack_tile_into_rgba(&tile, coord.x, coord.y, &mut rgba);
    }
    let _ = rgba;
    t0.elapsed().as_secs_f64() * 1000.0
}

fn measure_export_gpu(
    state: &AppState,
    coords: &[TileCoord],
    graph: &std::sync::Arc<engine_gpu::ComputeGraph>,
) -> f64 {
    let snapshot = state.must_active().document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let doc = snapshot.id.0;
    let ctx = state.gpu.as_ref().expect("gpu");
    let cache = state.gpu_resident.as_ref().expect("resident");
    let executor = state.gpu_executor.as_ref().expect("executor").lock().unwrap();
    let mut rgba = vec![0u8; (DOC * DOC * 4) as usize];
    let t0 = Instant::now();
    for chunk in coords.chunks(A5_GPU_BATCH) {
        let mut tiles = Vec::with_capacity(chunk.len());
        for coord in chunk {
            let raw_key = TileKey {
                doc,
                layer: LAYER,
                coord: *coord,
                stage: CacheStage::Raw,
            };
            let raw = state.tiles.tile_cache.get_entry(raw_key).expect("raw");
            tiles.push(GpuTileWork {
                key: TileKey {
                    stage: CacheStage::Processed,
                    ..raw_key
                },
                coord: *coord,
                generation: doc_gen,
                pixels: raw,
            });
        }
        executor
            .submit_frame_blocking(GpuFrameJob {
                doc_gen,
                graph: std::sync::Arc::clone(graph),
                tiles,
                speculative: false,
            })
            .expect("gpu export frame");
        for coord in chunk {
            let key = TileKey {
                doc,
                layer: LAYER,
                coord: *coord,
                stage: CacheStage::Processed,
            };
            let processed = cache
                .download(ctx, &key)
                .expect("download")
                .expect("slot");
            pack_tile_into_rgba(&processed, coord.x, coord.y, &mut rgba);
        }
    }
    let _ = rgba;
    t0.elapsed().as_secs_f64() * 1000.0
}

/// A5: CPU export cost vs GPU full-doc compute+download vs parallel CPU.
/// No disk. `DITHER_A5_N` (default 5).
#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag_export -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_export() {
    let n = a5_n();
    let coords = l0_grid();
    println!(
        "\n=== A5 export (n={n}, Bayer4, {DOC}², {} tiles, workers={}) ===\n",
        coords.len(),
        n_workers()
    );

    let gpu = engine_gpu::GpuContext::try_new_blocking().map(Arc::new);
    let state = make_state(DitherModeV2::Bayer4x4, gpu.clone());
    fill_all_l0_raw(&state);

    let mut seq = Vec::with_capacity(n);
    let mut par = Vec::with_capacity(n);
    let mut gpu_ms = Vec::with_capacity(n);
    let mut png = Vec::with_capacity(n);
    let mut last_rgba = Vec::new();

    let graph = {
        let snapshot = state.must_active().document_handle.snapshot();
        let layer = snapshot
            .root
            .iter()
            .find_map(|n| match n {
                LayerNode::Leaf(l) if l.id.0 == LAYER => Some(l),
                _ => None,
            })
            .expect("layer");
        let g = compile_layer_graph(&layer.filters).expect("graph");
        assert!(g.is_gpu_only(), "A5 Bayer graph must be gpu_only");
        std::sync::Arc::new(g)
    };

    for i in 0..n {
        let (ms, rgba) = measure_export_cpu_seq(&state, &coords);
        seq.push(ms);
        last_rgba = rgba;
        par.push(measure_export_cpu_par(&state, &coords));
        if gpu.is_some() && state.gpu_executor.is_some() {
            gpu_ms.push(measure_export_gpu(&state, &coords, &graph));
        }
        let t_png = Instant::now();
        let _ = encode_rgba_to_png(&last_rgba, DOC, DOC).expect("png");
        png.push(t_png.elapsed().as_secs_f64() * 1000.0);
        println!("  round {}/{n}", i + 1);
    }

    let seq_s = sample_stats(&seq);
    let par_s = sample_stats(&par);
    let png_s = sample_stats(&png);
    print_industrial_row("export CPU sequential (current)", &seq_s);
    print_industrial_row("export CPU parallel", &par_s);
    print_industrial_row("PNG encode only", &png_s);
    if gpu_ms.is_empty() {
        println!("  export GPU: no adapter — skipped");
    } else {
        let gpu_s = sample_stats(&gpu_ms);
        print_industrial_row("export GPU compute+download+pack", &gpu_s);
        println!(
            "\n  GPU vs CPU seq: {}\n  GPU vs CPU par: {}\n  CPU par vs seq: {}",
            verdict_faster(&gpu_s, &seq_s, "GPU"),
            verdict_faster(&gpu_s, &par_s, "GPU"),
            verdict_faster(&par_s, &seq_s, "CPU parallel"),
        );
        let apply_share = seq_s.median / (seq_s.median + png_s.median);
        println!(
            "  sequential apply share of (apply+png): {:.0}%",
            apply_share * 100.0
        );
        let build_gpu = gpu_s.median + 1.0 < seq_s.median
            && gpu_s.median + 1.0 < par_s.median
            && apply_share > 0.5;
        println!(
            "  A5 build GPU export path: {}\n",
            if build_gpu {
                "YES (GPU beats seq and par; apply-bound)"
            } else {
                "NO (encode-bound, or CPU parallel/seq already better)"
            }
        );
    }

    let ed_state = make_realistic_stack_state(None);
    fill_all_l0_raw(&ed_state);
    let (ed_ms, _) = measure_export_cpu_seq(&ed_state, &coords);
    println!(
        "  ED stack CPU sequential (n=1, same grid): {:.1} ms — GPU export N/A (CpuCheckpoint)\n",
        ed_ms
    );
    println!("=== A5 harness complete — paste into gpu-export-a5/A5_EVIDENCE.md ===\n");
}

fn print_vram_row(label: &str, s: &engine_gpu::GpuVramStats) {
    println!(
        "  {label}: live={}/{} ({:.0}%) peak={} ({:.0}%) free={} pressure_evicts={} budget_miB={}",
        s.live_slots,
        s.max_slots,
        s.occupancy() * 100.0,
        s.peak_live,
        s.peak_occupancy() * 100.0,
        s.free_slots,
        s.pressure_evicts,
        s.budget_bytes / (1024 * 1024),
    );
}

/// A8 input: atlas occupancy under a large Bayer doc (not f16).
#[test]
#[ignore = "diagnostic: cargo test -p dither --release preview_latency_diag_vram -- --ignored --nocapture --test-threads=1"]
fn preview_latency_diag_vram() {
    println!("\n=== A8 VRAM occupancy (Bayer4, {DOC}², 144 L0 tiles) ===\n");
    let Some(gpu) = engine_gpu::GpuContext::try_new_blocking().map(Arc::new) else {
        println!("GPU: no adapter — abort");
        return;
    };
    let state = make_state(DitherModeV2::Bayer4x4, Some(gpu));
    fill_all_l0_raw(&state);
    let coords = l0_grid();
    let graph = {
        let snapshot = state.must_active().document_handle.snapshot();
        let layer = snapshot
            .root
            .iter()
            .find_map(|n| match n {
                LayerNode::Leaf(l) if l.id.0 == LAYER => Some(l),
                _ => None,
            })
            .expect("layer");
        std::sync::Arc::new(compile_layer_graph(&layer.filters).expect("graph"))
    };

    let cache = state.gpu_resident.as_ref().expect("resident");
    print_vram_row("after Raw fill (no GPU jobs)", &cache.vram_stats());

    let origin = set_viewport_with_prefetch(&state, 1.0, 0.0, 0.0);
    crate::commands::schedule_dirty_viewport_tiles(&state);
    let workers = n_workers();
    let _ = drain_until_visible(&state, &origin, workers, Duration::from_secs(60));
    crate::gpu_resident_shadow::enqueue_resident_shadow_viewport(&state);
    std::thread::sleep(Duration::from_millis(400));
    print_vram_row("after origin CPU + A2 prefetch warmup", &cache.vram_stats());

    let _ = measure_export_gpu(&state, &coords, &graph);
    print_vram_row("after full-doc GPU compute (144 tiles)", &cache.vram_stats());

    let _ = measure_export_gpu(&state, &coords, &graph);
    print_vram_row("after second full-doc GPU pass", &cache.vram_stats());

    let s = cache.vram_stats();
    let constrained = s.peak_occupancy() >= 0.95 || s.pressure_evicts > 0;
    println!(
        "\n  A8 f16/sparse justified: {}\n=== paste into gpu-vram-a8/A8_EVIDENCE.md ===\n",
        if constrained {
            "MAYBE (peak ≥95% or pressure evicts)"
        } else {
            "NO (atlas not saturated on this scene)"
        }
    );
}

// Silence unused helpers when only partial industrial runs are compiled in some cfgs.
#[allow(dead_code)]
fn _industrial_helpers_keep() {}
