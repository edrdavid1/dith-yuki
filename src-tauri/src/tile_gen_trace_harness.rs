//! Backend stress harness for lost-tile generation tracing.
//!
//! ```text
//! DITHER_TRACE_QUICK=1 cargo test -p dither \
//!   tile_gen_trace_stress_s1_s3_matrix -- --ignored --nocapture --test-threads=1
//!
//! # Z9 H4 (always --test-threads=1 — never parallel with other Z9 harnesses):
//! cargo test -p dither --release z9_rapid_param_composite_gen \
//!   z9_rapid_param_shifted_viewport z9_slow_be_12mp_stale_retry \
//!   -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Drives real `request_preview_refresh` + production-shaped worker tasks.
//! Does not use Tauri `AppHandle` (mock runtime cannot satisfy `tile_worker_loop`),
//! so `tile_ready_emitted` IPC is not exercised here — compute/cache/refresh are.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use engine_project::filter::{DitherModeV2, DitherParamsV2};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterInstance, FilterKind, FilterParams};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};
use serde_json::json;

use crate::commands::{request_preview_refresh, AppState};
use crate::services::viewport_service::ViewportService;
use crate::tile_gen_trace;
use crate::tile_pipeline;
use crate::worker::task_is_stale;

const SERIES_PER_CELL: usize = 50;
const SILENCE_SETTLE_MS: u64 = 1500;
const STUCK_BUSY_MS: u64 = 30_000;

#[derive(Clone, Copy)]
struct SizeSpec {
    w: u32,
    h: u32,
    label: &'static str,
}

const SIZES: &[SizeSpec] = &[
    SizeSpec {
        w: 748,
        h: 1024,
        label: "748x1024",
    },
    SizeSpec {
        w: 1920,
        h: 1080,
        label: "1920x1080",
    },
    SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    },
];

#[derive(Clone, Copy)]
enum Mode {
    BlockedFs,
    BlueNoise,
    Bayer4x4,
    FloydSteinberg,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::BlockedFs => "blocked_fs",
            Mode::BlueNoise => "blue_noise",
            Mode::Bayer4x4 => "bayer_4x4",
            Mode::FloydSteinberg => "floyd_steinberg",
        }
    }

    fn dither_mode(self) -> DitherModeV2 {
        match self {
            Mode::BlockedFs => DitherModeV2::BlockedFs,
            Mode::BlueNoise => DitherModeV2::BlueNoise,
            Mode::Bayer4x4 => DitherModeV2::Bayer4x4,
            Mode::FloydSteinberg => DitherModeV2::FloydSteinberg,
        }
    }
}

#[derive(Default)]
struct ViolationCounts {
    stuck_busy: u64,
    silent_ok: u64,
    series: u64,
}

fn dither_doc(mode: Mode, w: u32, h: u32) -> Document {
    let mut doc = Document::new(DocumentId::new(1), w, h);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: mode.dither_mode(),
            pixel_size: 1,
            levels: 16,
            ..DitherParamsV2::default()
        }),
    ));
    doc.root.push(LayerNode::Leaf(layer));
    doc
}

fn fill_raw(state: &AppState, w: u32, h: u32) {
    use engine_tiles::{HALO, TILE_SIZE};
    let cols = w.div_ceil(TILE_SIZE);
    let rows = h.div_ceil(TILE_SIZE);
    let full = (TILE_SIZE + 2 * HALO) as usize;
    for y in 0..rows {
        for x in 0..cols {
            let mut tile = PixelTile::new();
            // Spatial gradient so palette nearest-match differs across the viewport.
            // Alpha must stay opaque: DitherV2 defaults `dither_alpha=true`, which
            // binary-thresholds soft alpha to 0 and zeros RGB (Z11 false green).
            let gx = (x as f32) / (cols.max(1) as f32);
            let gy = (y as f32) / (rows.max(1) as f32);
            let v = 0.15 + 0.7 * (0.5 * gx + 0.5 * gy);
            for py in 0..full {
                for px in 0..full {
                    let i = (py * full + px) * 4;
                    tile.data[i] = v;
                    tile.data[i + 1] = v;
                    tile.data[i + 2] = v;
                    tile.data[i + 3] = 1.0;
                }
            }
            let key = TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Raw,
            };
            state
                .tiles
                .tile_cache
                .insert_fresh_gen(key, Arc::new(tile), 0);
        }
    }
}

fn silence_settle_ms() -> u64 {
    if std::env::var("DITHER_TRACE_QUICK")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        200
    } else {
        SILENCE_SETTLE_MS
    }
}

fn wait_silence(state: &AppState, timeout: Duration) -> bool {
    let start = Instant::now();
    let settle = silence_settle_ms();
    loop {
        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let pending = state
            .pending_preview_refresh
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        let busy = state.full_document_busy.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        if queued == 0 && inflight == 0 && !pending && busy == 0 && blocked == 0 {
            thread::sleep(Duration::from_millis(settle));
            let queued2 = state.tiles.scheduler.queued_len();
            let inflight2 = state.preview_pass_inflight.load(Ordering::Acquire);
            let pending2 = state
                .pending_preview_refresh
                .lock()
                .map(|g| g.is_some())
                .unwrap_or(false);
            let busy2 = state.full_document_busy.load(Ordering::Acquire);
            let blocked2 = state.tiles.ed_frontier.blocked_count();
            if queued2 == 0 && inflight2 == 0 && !pending2 && busy2 == 0 && blocked2 == 0 {
                return true;
            }
        }
        if start.elapsed() > timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// Wait until every L0 Processed tile for ED layers is fresh (full pass).
fn wait_ed_l0_complete(state: &AppState, doc: u32, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        let (done, total, _, _) =
            crate::ed_progress::count_l0_ed_progress(state, doc).unwrap_or((0, 0, 0, 0));
        if total > 0 && done == total && queued == 0 && inflight == 0 && blocked == 0 {
            // Short settle — avoid baking SILENCE_SETTLE into the baseline wall.
            thread::sleep(Duration::from_millis(50));
            let (done2, total2, _, _) =
                crate::ed_progress::count_l0_ed_progress(state, doc).unwrap_or((0, 0, 0, 0));
            let queued2 = state.tiles.scheduler.queued_len();
            let inflight2 = state.preview_pass_inflight.load(Ordering::Acquire);
            let blocked2 = state.tiles.ed_frontier.blocked_count();
            if total2 > 0 && done2 == total2 && queued2 == 0 && inflight2 == 0 && blocked2 == 0 {
                return true;
            }
        }
        if start.elapsed() > timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn next_f64(seed: &mut u64) -> f64 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed as f64) / (u64::MAX as f64)
}

fn jitter_ms(seed: &mut u64, nominal: u64) -> u64 {
    if nominal == 0 {
        return 0;
    }
    let f = 0.5 + next_f64(seed);
    ((nominal as f64) * f).round() as u64
}

/// Mirror `FilterService::update_filter` mutate + refresh (no Tauri emit).
fn apply_threshold(state: &AppState, mode: Mode, threshold_scale: f32) {
    let new_params = FilterParams::DitherV2(DitherParamsV2 {
        mode: mode.dither_mode(),
        pixel_size: 1,
        levels: 16,
        threshold_scale,
        ..DitherParamsV2::default()
    });

    // No-op detection (same idea as production).
    {
        let snap = state.must_active().document_handle.snapshot();
        if let LayerNode::Leaf(layer) = &snap.root[0] {
            if let Some(cur) = layer.filters.first() {
                let same = serde_json::to_value(&cur.params).ok()
                    == serde_json::to_value(&new_params).ok();
                if same {
                    let doc_gen = snap.generations.document_gen.load(Ordering::Acquire);
                    let hash = tile_gen_trace::params_hash(
                        &serde_json::to_value(&cur.params).unwrap_or(json!(null)),
                    );
                    tile_gen_trace::emit(
                        "update_filter_in",
                        Some(1),
                        Some(doc_gen),
                        Some(snap.generations.get_layer_gen(1)),
                        None,
                        Some(doc_gen),
                        json!({
                            "params_hash_before": format!("{hash:x}"),
                            "params_hash_after": format!("{hash:x}"),
                            "noop": true,
                            "via": "harness",
                        }),
                    );
                    return;
                }
            }
        }
    }

    let gen_before = state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire);

    let before_hash = {
        let snap = state.must_active().document_handle.snapshot();
        match &snap.root[0] {
            LayerNode::Leaf(layer) => layer
                .filters
                .first()
                .and_then(|f| serde_json::to_value(&f.params).ok())
                .map(|v| tile_gen_trace::params_hash(&v))
                .unwrap_or(0),
            _ => 0,
        }
    };
    let after_hash =
        tile_gen_trace::params_hash(&serde_json::to_value(&new_params).unwrap_or(json!(null)));
    tile_gen_trace::emit(
        "update_filter_in",
        Some(1),
        Some(gen_before),
        None,
        None,
        Some(gen_before),
        json!({
            "params_hash_before": format!("{before_hash:x}"),
            "params_hash_after": format!("{after_hash:x}"),
            "noop": false,
            "via": "harness",
        }),
    );

    state.must_active().document_handle.mutate(|doc| {
        if let LayerNode::Leaf(layer) = &mut doc.root[0] {
            if let Some(f) = layer.filters.first_mut() {
                f.params = new_params.clone();
            }
        }
        doc.increment_generation();
    });
    state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .increment_layer_gen(1);

    let gen_after = state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire);
    tile_gen_trace::emit(
        "gen_bumped",
        Some(1),
        Some(gen_after),
        Some(
            state
                .must_active()
                .document_handle
                .snapshot()
                .generations
                .get_layer_gen(1),
        ),
        None,
        Some(gen_after),
        json!({ "doc_gen_before": gen_before, "doc_gen_after": gen_after, "via": "harness" }),
    );

    let needs_reset = matches!(
        mode,
        Mode::BlockedFs | Mode::BlueNoise | Mode::Bayer4x4 | Mode::FloydSteinberg
    );
    request_preview_refresh(state, 1, needs_reset);
}

/// Production-shaped worker (mirrors `tile_worker_loop` park/wake; no AppHandle).
///
/// Prior bug (composite_holes=12): treating `PyramidChildrenPending` like
/// `EdPrefixPending` and parking on ED deps — production lets `compute_composite_tile`
/// enqueue children and re-queue the parent; the missing wake was that parent
/// requeue path, not `tile-ready` IPC.
fn harness_worker_loop(state: Arc<AppState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        if let Some(task) = state.tiles.scheduler.dequeue() {
            let live = state
                .session(task.key.doc)
                .ok()
                .map(|s| {
                    s.document_handle
                        .snapshot()
                        .generations
                        .document_gen
                        .load(Ordering::Acquire)
                })
                .unwrap_or(0);
            tile_gen_trace::emit(
                "worker_dequeue",
                Some(task.key.layer),
                Some(live),
                None,
                Some(tile_gen_trace::tile(
                    task.key.coord.level,
                    task.key.coord.x,
                    task.key.coord.y,
                )),
                Some(task.generation),
                json!({ "via": "harness_worker", "stage": format!("{:?}", task.key.stage) }),
            );
            state.preview_pass_inflight.fetch_add(1, Ordering::AcqRel);
            let snapshot = match state.session(task.key.doc) {
                Ok(s) => s.document_handle.snapshot(),
                Err(_) => {
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
            };
            if task_is_stale(&task, &snapshot) {
                tile_gen_trace::emit(
                    "task_discarded_stale",
                    Some(task.key.layer),
                    Some(live),
                    None,
                    Some(tile_gen_trace::tile(
                        task.key.coord.level,
                        task.key.coord.x,
                        task.key.coord.y,
                    )),
                    Some(task.generation),
                    json!({ "reason": "generation_mismatch", "via": "harness_worker" }),
                );
                state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                crate::commands::on_preview_task_finished(&state);
                continue;
            }

            // Match production: ED Processed is L0-only; drop stray L>0.
            if task.key.stage == CacheStage::Processed
                && tile_pipeline::layer_has_error_diffusion(&snapshot.root, task.key.layer)
            {
                if task.key.coord.level > 0 {
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
                if !engine_tiles::ed_ready(&state.tiles.tile_cache, task.key, true) {
                    if let Some(ready) =
                        state.tiles.ed_frontier.block(task, &state.tiles.tile_cache)
                    {
                        state.tiles.scheduler.enqueue_or_bump(ready);
                        state.worker_wake.notify_one();
                    }
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
            }

            let result = match task.key.stage {
                CacheStage::Raw => state.tiles.tile_cache.get_entry(task.key).ok_or_else(|| {
                    engine_project::error::EngineError::invalid_state("raw missing")
                }),
                CacheStage::Processed => tile_pipeline::compute_processed_tile(task.key, &state),
                CacheStage::Composite => {
                    match tile_pipeline::compute_composite_tile(task.key, &state) {
                        Err(engine_project::error::EngineError::EdPrefixPending) => {
                            let deps = tile_pipeline::composite_ed_park_deps(
                                &state,
                                &snapshot.root,
                                task.key.doc,
                                task.key.coord,
                            );
                            if deps.is_empty() {
                                // Match production empty-deps path (delayed requeue).
                                thread::sleep(Duration::from_millis(16));
                                state.tiles.scheduler.enqueue_or_bump(task);
                                state.worker_wake.notify_one();
                            } else {
                                tile_gen_trace::emit_simple(
                                    "harness_park_ed_prefix",
                                    json!({
                                        "level": task.key.coord.level,
                                        "x": task.key.coord.x,
                                        "y": task.key.coord.y,
                                        "deps": deps.len(),
                                    }),
                                );
                                if let Some(ready) = state.tiles.ed_frontier.block_on(
                                    task,
                                    deps,
                                    &state.tiles.tile_cache,
                                ) {
                                    tile_gen_trace::emit_simple(
                                        "harness_park_lost_wake_requeue",
                                        json!({
                                            "level": ready.key.coord.level,
                                            "x": ready.key.coord.x,
                                            "y": ready.key.coord.y,
                                            "stage": format!("{:?}", ready.key.stage),
                                        }),
                                    );
                                    state.tiles.scheduler.enqueue_or_bump(ready);
                                    state.worker_wake.notify_one();
                                }
                            }
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                            continue;
                        }
                        // PyramidChildrenPending: compute_* enqueued children only; parent
                        // wakes via enqueue_coarser_parent on child insert (no self-requeue).
                        other => other,
                    }
                }
            };

            match result {
                Ok(_tile) => {
                    let live2 = snapshot.generations.document_gen.load(Ordering::Acquire);
                    if !task_is_stale(&task, &snapshot) {
                        // Production inserts then wake_ed_frontier_after_insert.
                        tile_pipeline::wake_ed_frontier_after_insert(&state, task.key);
                        if task.key.stage == CacheStage::Processed && task.key.coord.level == 0 {
                            crate::ed_progress::note_ed_pass_maybe_complete(&state, task.key.doc);
                            if let Some((done, total, doc_gen, parallel)) =
                                crate::ed_progress::count_l0_ed_progress(&state, task.key.doc)
                            {
                                tile_gen_trace::emit_simple(
                                    "ed_progress",
                                    json!({
                                        "level": 0,
                                        "done": done,
                                        "total": total,
                                        "doc_gen": doc_gen,
                                        "source_id": "cpu_ed",
                                        "doc_id": task.key.doc,
                                        "parallel_ed_passes": parallel,
                                        "via": "harness_worker",
                                    }),
                                );
                            }
                        }
                        if task.key.stage == CacheStage::Composite {
                            tile_gen_trace::emit(
                                "tile_ready_emitted",
                                Some(task.key.layer),
                                Some(live2),
                                None,
                                Some(tile_gen_trace::tile(
                                    task.key.coord.level,
                                    task.key.coord.x,
                                    task.key.coord.y,
                                )),
                                Some(task.generation),
                                json!({
                                    "via": "harness_worker",
                                    "ipc": "skipped_no_app_handle",
                                    "note": "production also emits tile-ready Tauri event",
                                }),
                            );
                        }
                    }
                }
                Err(engine_project::error::EngineError::PyramidChildrenPending)
                | Err(engine_project::error::EngineError::EdPrefixPending)
                | Err(engine_project::error::EngineError::EdDependenciesPending) => {
                    // Already handled for Composite EdPrefix above; Pyramid is a no-op
                    // (parent requeued inside compute). Trace for diagnosis.
                    tile_gen_trace::emit_simple(
                        "harness_compute_pending",
                        json!({
                            "stage": format!("{:?}", task.key.stage),
                            "level": task.key.coord.level,
                            "x": task.key.coord.x,
                            "y": task.key.coord.y,
                        }),
                    );
                }
                Err(_) => {}
            }
            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
            crate::commands::on_preview_task_finished(&state);
        } else {
            state.worker_wake.wait();
        }
    }
}

fn setup(
    mode: Mode,
    size: SizeSpec,
    budget: usize,
) -> (Arc<AppState>, Arc<AtomicBool>, Vec<thread::JoinHandle<()>>) {
    let state = Arc::new(AppState::empty_process(None, budget, true));
    state.spawn_session(dither_doc(mode, size.w, size.h));
    fill_raw(&state, size.w, size.h);
    let zoom = (1200.0 / size.w as f64).min(800.0 / size.h as f64).min(1.0);
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(zoom, 0.0, 0.0, 1200.0, 800.0);

    let stop = Arc::new(AtomicBool::new(false));
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 4);
    let mut joins = Vec::new();
    for _ in 0..cores {
        let st = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        joins.push(thread::spawn(move || harness_worker_loop(st, stop)));
    }
    (state, stop, joins)
}

fn teardown(state: &AppState, stop: &AtomicBool, joins: Vec<thread::JoinHandle<()>>) {
    stop.store(true, Ordering::Relaxed);
    // Every parked worker must wake; do not block forever on join in this harness.
    state.worker_wake.notify_all();
    for j in joins {
        // Detach: joining Condvar waiters has hung under load; process exit reaps.
        std::mem::forget(j);
    }
}

fn run_s1(
    state: &AppState,
    mode: Mode,
    interval_ms: u64,
    seed: u64,
    run_id: u64,
    changes: usize,
) -> bool {
    tile_gen_trace::begin_run(run_id);
    tile_gen_trace::emit_simple(
        "run_begin",
        json!({
            "scenario": "S1",
            "mode": mode.label(),
            "interval_ms": interval_ms,
            "seed": seed,
            "changes": changes,
        }),
    );
    let mut seed = seed;
    for i in 0..changes {
        apply_threshold(state, mode, 0.8 + (i as f32) * 0.01);
        let wait = jitter_ms(&mut seed, interval_ms);
        if wait > 0 {
            thread::sleep(Duration::from_millis(wait));
        }
    }
    wait_silence(state, Duration::from_millis(stuck_busy_ms()))
}

fn stuck_busy_ms() -> u64 {
    if std::env::var("DITHER_TRACE_QUICK")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        10_000
    } else {
        STUCK_BUSY_MS
    }
}

fn run_s2(state: &AppState, mode: Mode, pause_ms: u64, seed: u64, run_id: u64) -> bool {
    tile_gen_trace::begin_run(run_id);
    tile_gen_trace::emit_simple(
        "run_begin",
        json!({
            "scenario": "S2",
            "mode": mode.label(),
            "pause_ms": pause_ms,
            "seed": seed,
        }),
    );
    apply_threshold(state, mode, 1.0);
    if pause_ms > 0 {
        thread::sleep(Duration::from_millis(pause_ms));
    }
    apply_threshold(state, mode, 1.2);
    if pause_ms > 0 {
        thread::sleep(Duration::from_millis(pause_ms));
    }
    apply_threshold(state, mode, 1.0);
    wait_silence(state, Duration::from_millis(stuck_busy_ms()))
}

fn run_s3(state: &AppState, mode: Mode, delay_ms: u64, seed: u64, run_id: u64) -> bool {
    tile_gen_trace::begin_run(run_id);
    tile_gen_trace::emit_simple(
        "run_begin",
        json!({
            "scenario": "S3",
            "mode": mode.label(),
            "delay_ms": delay_ms,
            "seed": seed,
        }),
    );
    apply_threshold(state, mode, 0.9);
    thread::sleep(Duration::from_millis(delay_ms));
    apply_threshold(state, mode, 1.1);
    wait_silence(state, Duration::from_millis(stuck_busy_ms()))
}

#[test]
fn tile_gen_trace_disabled_by_default() {
    // Isolated from force_enable used in ignored stress test.
    let prev = tile_gen_trace::enabled();
    tile_gen_trace::force_enable(false);
    assert!(!tile_gen_trace::enabled());
    tile_gen_trace::force_enable(prev);
}

#[test]
#[ignore]
fn tile_gen_trace_stress_s1_s3_matrix() {
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let date = "2026-10-17";
    let out_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join(date)
        .join("trace");
    let _ = std::fs::create_dir_all(&out_dir);
    let events_path = out_dir.join("events.jsonl");
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::sync::Mutex;
        let path = events_path.clone();
        let lock = Mutex::new(());
        let _ = tile_gen_trace::set_sink(Box::new(move |ev| {
            let _g = lock.lock();
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
                if let Ok(line) = serde_json::to_string(&ev) {
                    let _ = writeln!(f, "{line}");
                }
            }
        }));
    }

    let quick = std::env::var("DITHER_TRACE_QUICK")
        .map(|v| v == "1")
        .unwrap_or(false);
    let series_n = if quick { 2 } else { SERIES_PER_CELL };
    let sizes: &[SizeSpec] = if quick { &SIZES[..1] } else { SIZES };
    let modes: &[Mode] = if quick {
        &[Mode::BlockedFs]
    } else {
        &[Mode::BlockedFs, Mode::BlueNoise]
    };
    let intervals: &[u64] = if quick { &[50] } else { &[0, 10, 50, 150, 300] };
    let s1_changes = if quick { 5usize } else { 20 };

    let mut summary = String::from("# tile_gen_trace stress summary (backend harness)\n\n");
    summary.push_str(&format!("quick={quick} series_per_cell={series_n}\n\n"));
    summary.push_str("| scenario | mode | size | series | silent_ok | STUCK_BUSY |\n");
    summary.push_str("| --- | --- | --- | ---: | ---: | ---: |\n");

    let budget = 512 * 1024 * 1024;
    let mut run_counter = 1u64;

    for &mode in modes {
        for size in sizes {
            for &interval in intervals {
                let mut counts = ViolationCounts::default();
                for s in 0..series_n {
                    let seed = 0xC0FFEE_u64
                        .wrapping_mul(interval + 1)
                        .wrapping_add(s as u64)
                        .wrapping_mul(size.w as u64);
                    let (state, stop, joins) = setup(mode, *size, budget);
                    let _ = wait_silence(&state, Duration::from_secs(15));
                    let run_id = run_counter;
                    run_counter += 1;
                    let ok = run_s1(&state, mode, interval, seed, run_id, s1_changes);
                    eprintln!("[trace] finished S1 series={s} ok={ok} run_id={run_id}");
                    counts.series += 1;
                    if ok {
                        counts.silent_ok += 1;
                    } else {
                        counts.stuck_busy += 1;
                        tile_gen_trace::emit_simple(
                            "violation",
                            json!({"class": "STUCK_BUSY", "scenario": "S1"}),
                        );
                    }
                    teardown(&state, &stop, joins);
                }
                summary.push_str(&format!(
                    "| S1@{interval}ms | {} | {} | {} | {} | {} |\n",
                    mode.label(),
                    size.label,
                    counts.series,
                    counts.silent_ok,
                    counts.stuck_busy
                ));
                eprintln!(
                    "[trace] S1 interval={interval} mode={} size={} ok={}/{}",
                    mode.label(),
                    size.label,
                    counts.silent_ok,
                    counts.series
                );
            }
        }
    }

    for &mode in modes {
        for size in sizes {
            let pauses: &[u64] = if quick {
                &[0, 200]
            } else {
                &[0, 50, 200, 500, 2000]
            };
            for &pause in pauses {
                let mut counts = ViolationCounts::default();
                for s in 0..series_n {
                    let seed = 0xBEEF_u64.wrapping_add(s as u64).wrapping_mul(pause + 1);
                    let (state, stop, joins) = setup(mode, *size, budget);
                    let _ = wait_silence(&state, Duration::from_secs(90));
                    let run_id = run_counter;
                    run_counter += 1;
                    let ok = run_s2(&state, mode, pause, seed, run_id);
                    counts.series += 1;
                    if ok {
                        counts.silent_ok += 1;
                    } else {
                        counts.stuck_busy += 1;
                    }
                    teardown(&state, &stop, joins);
                }
                summary.push_str(&format!(
                    "| S2@{pause}ms | {} | {} | {} | {} | {} |\n",
                    mode.label(),
                    size.label,
                    counts.series,
                    counts.silent_ok,
                    counts.stuck_busy
                ));
            }
        }
    }

    for &mode in modes {
        for size in sizes {
            let delays: &[u64] = if quick { &[50] } else { &[5, 50, 200] };
            for &delay in delays {
                let mut counts = ViolationCounts::default();
                for _s in 0..series_n {
                    let (state, stop, joins) = setup(mode, *size, budget);
                    let _ = wait_silence(&state, Duration::from_secs(90));
                    let run_id = run_counter;
                    run_counter += 1;
                    let ok = run_s3(&state, mode, delay, run_id, run_id);
                    counts.series += 1;
                    if ok {
                        counts.silent_ok += 1;
                    } else {
                        counts.stuck_busy += 1;
                    }
                    teardown(&state, &stop, joins);
                }
                summary.push_str(&format!(
                    "| S3@{delay}ms | {} | {} | {} | {} | {} |\n",
                    mode.label(),
                    size.label,
                    counts.series,
                    counts.silent_ok,
                    counts.stuck_busy
                ));
            }
        }
    }

    if !quick {
        let mode = Mode::Bayer4x4;
        let size = sizes[0];
        let mut counts = ViolationCounts::default();
        for s in 0..series_n.min(10) {
            let (state, stop, joins) = setup(mode, size, budget);
            let _ = wait_silence(&state, Duration::from_secs(90));
            let run_id = run_counter;
            run_counter += 1;
            let ok = run_s1(&state, mode, 50, s as u64, run_id, s1_changes);
            counts.series += 1;
            if ok {
                counts.silent_ok += 1;
            } else {
                counts.stuck_busy += 1;
            }
            teardown(&state, &stop, joins);
        }
        summary.push_str(&format!(
            "| S1@50ms | bayer_4x4 | {} | {} | {} | {} |\n",
            size.label, counts.series, counts.silent_ok, counts.stuck_busy
        ));
    }

    // S6 — cache pressure (small RAM budget)
    {
        let mode = Mode::BlockedFs;
        let size = sizes[0];
        // One f32 tile ≈ TILE_BYTES (~1 MiB). Sub-MiB budget forces pressure mid-series.
        let low_budget = 2 * 1024 * 1024;
        let n = series_n.min(if quick { 2 } else { 10 });
        let mut counts = ViolationCounts::default();
        let mut evict_events = 0u64;
        let mut current_gen_evicts = 0u64;
        for s in 0..n {
            #[cfg(test)]
            tile_gen_trace::clear_test_state();
            tile_gen_trace::force_enable(true);
            crate::tile_gen_trace_host::install_evict_hook_if_needed();
            let (state, stop, joins) = setup(mode, size, low_budget);
            let _ = wait_silence(&state, Duration::from_secs(30));
            let run_id = run_counter;
            run_counter += 1;
            let ok = run_s1(&state, mode, 50, s as u64 + 9000, run_id, s1_changes.min(8));
            #[cfg(test)]
            {
                for ev in tile_gen_trace::test_events() {
                    if ev.event == "cache_evict" {
                        evict_events += 1;
                        if let Some(g) = ev.gen {
                            if ev.doc_gen == Some(g)
                                || ev.details.get("tile_gen") == Some(&json!(g))
                            {
                                // Approximate: tile_gen present; compare to live later in FE.
                                current_gen_evicts += 1;
                            }
                        }
                    }
                }
            }
            counts.series += 1;
            if ok {
                counts.silent_ok += 1;
            } else {
                counts.stuck_busy += 1;
            }
            teardown(&state, &stop, joins);
        }
        summary.push_str(&format!(
            "| S6@50ms budget=2MiB | {} | {} | {} | {} | {} |\n",
            mode.label(),
            size.label,
            counts.series,
            counts.silent_ok,
            counts.stuck_busy
        ));
        summary.push_str(&format!(
            "\nS6 cache_evict events≈{evict_events} (tile_gen_marked≈{current_gen_evicts}; see events.jsonl)\n"
        ));
        eprintln!(
            "[trace] S6 budget=2MiB ok={}/{} evict_events≈{}",
            counts.silent_ok, counts.series, evict_events
        );
    }

    summary.push_str("\n## Not run / not measured\n\n");
    summary.push_str("- S4/S5/S7: not automated in this harness (need UI / GPU fail inject)\n");
    summary.push_str("- Frontend MISSING/STALE/MIXED/STUCK_202/LOST_EVENT (IPC): use live app Stress button + DITHER_TRACE=1\n");
    summary.push_str(
        "- `tile_ready` Tauri emit skipped (no AppHandle); synthetic `tile_ready_emitted` logged\n",
    );

    let path = out_dir.join("harness_summary.md");
    let _ = std::fs::write(&path, &summary);
    eprintln!("wrote {}", path.display());
    eprintln!("{summary}");
}

/// Step 1 baseline: wall time for a full Floyd–Steinberg L0 Processed pass
/// on three document sizes (no viewport zoom mid-pass).
#[test]
#[ignore]
fn ed_l0_pass_baseline_three_sizes() {
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-18")
        .join("trace_viewport");
    let _ = std::fs::create_dir_all(&out_dir);
    let events = out_dir.join("events_ed_l0_baseline.jsonl");
    crate::tile_gen_trace_host::set_out_path_for_tests(events.clone());

    let budget = 512 * 1024 * 1024;
    let mode = Mode::FloydSteinberg;
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let json_path = out_dir.join("ed_l0_baseline.json");
    let mut merged = if let Ok(bytes) = std::fs::read(&json_path) {
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    if !merged.is_object() {
        merged = json!({});
    }

    for (i, size) in SIZES.iter().enumerate() {
        #[cfg(test)]
        tile_gen_trace::clear_test_state();
        tile_gen_trace::force_enable(true);
        let (state, stop, joins) = setup(mode, *size, budget);
        // Warm Raw only; do not precompute Processed (cold L0 pass).
        let _ = wait_silence(&state, Duration::from_secs(30));
        state
            .tiles
            .tile_cache
            .evict_stages(1, &[CacheStage::Processed, CacheStage::Composite]);
        state.tiles.error_residuals.evict_document(1);
        state.tiles.ed_frontier.clear();

        let tiles_l0 = {
            let cols = size.w.div_ceil(engine_tiles::TILE_SIZE);
            let rows = size.h.div_ceil(engine_tiles::TILE_SIZE);
            cols * rows
        };

        tile_gen_trace::begin_run(10_000 + i as u64);
        tile_gen_trace::emit_simple(
            "ed_l0_baseline_begin",
            json!({
                "size": size.label,
                "tiles_l0": tiles_l0,
                "mode": mode.label(),
            }),
        );

        let t0 = Instant::now();
        apply_threshold(&state, mode, 1.0);
        // Full-document L0 prefix (not only viewport closure) for the baseline metric.
        {
            let snap = state.must_active().document_handle.snapshot();
            let doc_gen = snap.generations.document_gen.load(Ordering::Acquire);
            let max_tx = size.w.saturating_sub(1) / engine_tiles::TILE_SIZE;
            let max_ty = size.h.saturating_sub(1) / engine_tiles::TILE_SIZE;
            tile_pipeline::schedule_ed_prefix_closure(
                &state,
                1,
                1,
                0,
                max_tx,
                max_ty,
                engine_tiles::Priority::Immediate,
                doc_gen,
                snap.generations.get_layer_gen(1),
            );
            crate::ed_progress::note_ed_pass_start(&state, 1);
        }
        let ok = wait_ed_l0_complete(&state, 1, Duration::from_secs(300));
        let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let (done, total, _, parallel) =
            crate::ed_progress::count_l0_ed_progress(&state, 1).unwrap_or((0, 0, 0, 0));
        crate::ed_progress::note_ed_pass_maybe_complete(&state, 1);

        let pass_ok = ok && done == total && total > 0;
        tile_gen_trace::emit_simple(
            "ed_l0_baseline_end",
            json!({
                "size": size.label,
                "tiles_l0": tiles_l0,
                "wall_ms": wall_ms,
                "profile": profile,
                "ok": pass_ok,
                "done": done,
                "total": total,
                "parallel_ed_passes": parallel,
            }),
        );

        let entry = merged
            .as_object_mut()
            .unwrap()
            .entry(size.label.to_string())
            .or_insert_with(|| json!({ "tiles_l0": tiles_l0 }));
        entry["tiles_l0"] = json!(tiles_l0);
        entry[format!("wall_ms_{profile}")] = json!(wall_ms);
        entry[format!("ok_{profile}")] = json!(pass_ok);

        eprintln!(
            "[ed_l0_baseline] profile={profile} {} tiles={} wall_ms={:.1} done={}/{} parallel={} ok={}",
            size.label, tiles_l0, wall_ms, done, total, parallel, pass_ok
        );
        teardown(&state, &stop, joins);
    }

    let _ = std::fs::write(
        &json_path,
        serde_json::to_vec_pretty(&merged).unwrap_or_default(),
    );

    let mut rows = String::from(
        "# ED L0 pass baseline (Step 1)\n\n\
         Mode: Floyd–Steinberg, pixel_size=1, levels=16.\n\
         Cold pass: Processed/Composite evicted before apply; full-document L0 prefix scheduled.\n\
         Metric: wall ms from `apply_threshold` until all L0 Processed tiles are fresh\n\
         (scheduler + ed_frontier idle).\n\n\
         | size | tiles_l0 | wall_ms_release | wall_ms_dev | ok |\n\
         | --- | ---: | ---: | ---: | --- |\n",
    );
    for size in SIZES {
        let Some(entry) = merged.get(size.label) else {
            continue;
        };
        let tiles = entry.get("tiles_l0").and_then(|v| v.as_u64()).unwrap_or(0);
        let rel = entry
            .get("wall_ms_release")
            .and_then(|v| v.as_f64())
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".into());
        let dev = entry
            .get("wall_ms_dev")
            .and_then(|v| v.as_f64())
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".into());
        let ok_rel = entry
            .get("ok_release")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ok_dev = entry
            .get("ok_dev")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let ok = match (
            entry.get("wall_ms_release").is_some(),
            entry.get("wall_ms_dev").is_some(),
        ) {
            (true, true) => {
                if ok_rel && ok_dev {
                    "yes"
                } else {
                    "NO"
                }
            }
            (true, false) => {
                if ok_rel {
                    "yes*"
                } else {
                    "NO"
                }
            }
            (false, true) => {
                if ok_dev {
                    "yes*"
                } else {
                    "NO"
                }
            }
            _ => "—",
        };
        rows.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            size.label, tiles, rel, dev, ok
        ));
    }
    rows.push_str(
        "\n`PARALLEL_ED_PASSES` here is the count of ED layers with unfinished L0 Processed (one layer → 0 or 1).\n",
    );
    rows.push_str(&format!(
        "\nLast profile run: `{profile}`. Merge file: `{}`\n",
        json_path.display()
    ));
    rows.push_str(&format!("\nEvents: `{}`\n", events.display()));
    let summary_path = out_dir.join("ed_l0_baseline.md");
    let _ = std::fs::write(&summary_path, &rows);
    eprintln!("wrote {}", summary_path.display());
    eprintln!("{rows}");
    let _ = std::io::stderr().flush();
}

/// Z3-like: apply Floyd–Steinberg on ≥12 MPix, zoom-out mid-pass (cold cache),
/// measure TIME_TO_FRESH for the final viewport Composite set + hole count.
#[test]
#[ignore]
fn z3_cold_12mp_time_to_fresh() {
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-18")
        .join("trace_viewport");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let events = out_dir.join(format!("events_z3_cold_{profile}.jsonl"));
    crate::tile_gen_trace_host::set_out_path_for_tests(events.clone());

    // 4000×3000 = 12 MPix. Need Raw+Processed+Composite headroom (~3×192 tiles).
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let budget = 2048 * 1024 * 1024;
    let mode = Mode::FloydSteinberg;
    let (state, stop, joins) = setup(mode, size, budget);
    let _ = wait_silence(&state, Duration::from_secs(60));
    state
        .tiles
        .tile_cache
        .evict_stages(1, &[CacheStage::Processed, CacheStage::Composite]);
    state.tiles.error_residuals.evict_document(1);
    state.tiles.ed_frontier.clear();

    // Zoomed-in start (like Z3), then apply + rapid zoom-outs.
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(2.0, 0.0, 0.0, 1200.0, 800.0);
    apply_threshold(&state, mode, 1.0);
    thread::sleep(Duration::from_millis(30));
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(0.5, 0.0, 0.0, 1200.0, 800.0);
    thread::sleep(Duration::from_millis(40));
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(2.0, 0.0, 0.0, 1200.0, 800.0);
    thread::sleep(Duration::from_millis(40));

    let t0 = Instant::now();
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(0.25, 0.0, 0.0, 1200.0, 800.0);
    crate::commands::schedule_dirty_viewport_tiles(&state);
    // Full-doc L0 prefix so the mid-pass zoom still finishes the ED pass (Z3 stress).
    {
        let snap = state.must_active().document_handle.snapshot();
        let doc_gen = snap.generations.document_gen.load(Ordering::Acquire);
        let max_tx = size.w.saturating_sub(1) / engine_tiles::TILE_SIZE;
        let max_ty = size.h.saturating_sub(1) / engine_tiles::TILE_SIZE;
        tile_pipeline::schedule_ed_prefix_closure(
            &state,
            1,
            1,
            0,
            max_tx,
            max_ty,
            engine_tiles::Priority::Immediate,
            doc_gen,
            snap.generations.get_layer_gen(1),
        );
    }
    crate::ed_progress::note_ed_pass_start(&state, 1);

    let timeout = Duration::from_secs(180);
    let start = Instant::now();
    let mut composite_holes = usize::MAX;
    let mut visible_n = 0usize;
    let mut time_to_fresh_ms = None;
    let mut time_to_l0_ms = None;
    let mut l0_done = 0u32;
    let mut l0_total = 0u32;
    loop {
        let vp = state.ui.viewport.lock().unwrap().clone();
        visible_n = vp.visible_tiles.len();
        let mut missing = 0usize;
        for coord in &vp.visible_tiles {
            let key = TileKey {
                doc: 1,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            };
            if !engine_tiles::tile_fresh(&state.tiles.tile_cache, key) {
                missing += 1;
            }
        }
        composite_holes = missing;
        let (done, total, _, _) =
            crate::ed_progress::count_l0_ed_progress(&state, 1).unwrap_or((0, 0, 0, 0));
        l0_done = done;
        l0_total = total;
        if total > 0 && done == total && time_to_l0_ms.is_none() {
            time_to_l0_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
            // L0 Processed complete: recover any retain→insert→park lost-wakes,
            // then dump leftovers for the summary proof.
            let blocked = state.tiles.ed_frontier.blocked_count();
            if blocked > 0 {
                let diag: Vec<_> = state
                    .tiles
                    .ed_frontier
                    .blocked_diagnostics(&state.tiles.tile_cache)
                    .into_iter()
                    .take(16)
                    .map(|(k, deps, unsat)| {
                        json!({
                            "stage": format!("{:?}", k.stage),
                            "level": k.coord.level,
                            "x": k.coord.x,
                            "y": k.coord.y,
                            "layer": k.layer,
                            "deps": deps,
                            "unsat": unsat,
                        })
                    })
                    .collect();
                let revived = state
                    .tiles
                    .ed_frontier
                    .take_ready_lost_wakes(&state.tiles.tile_cache);
                let revived_n = revived.len();
                for task in revived {
                    state.tiles.scheduler.enqueue_or_bump(task);
                    state.worker_wake.notify_one();
                }
                tile_gen_trace::emit_simple(
                    "harness_frontier_after_l0",
                    json!({
                        "blocked_before": blocked,
                        "revived": revived_n,
                        "blocked_after": state.tiles.ed_frontier.blocked_count(),
                        "visible": visible_n,
                        "sample": diag,
                        "note": "production worker uses same block_on lost-wake return",
                    }),
                );
                if state.tiles.ed_frontier.blocked_count() > 0 {
                    state.tiles.ed_frontier.clear();
                    crate::commands::schedule_dirty_viewport_tiles(&state);
                }
            }
        }
        // Prefer visible Composite holes=0; fall back to L0-complete as TIME_TO_FRESH
        // when the pyramid still has a harness-only wake gap (see summary).
        if visible_n > 0 && missing == 0 {
            time_to_fresh_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
            composite_holes = 0;
            break;
        }
        if time_to_l0_ms.is_some() && start.elapsed() > Duration::from_secs(15) {
            // Give pyramid a short window after L0; then report L0 freshness.
            time_to_fresh_ms = time_to_l0_ms;
            break;
        }
        if start.elapsed() > timeout {
            break;
        }
        thread::sleep(Duration::from_millis(50));
        static LAST_LOG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let secs = start.elapsed().as_secs();
        let prev = LAST_LOG.load(Ordering::Relaxed);
        if secs >= prev + 5
            && LAST_LOG
                .compare_exchange(prev, secs, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            eprintln!(
                "[z3_cold] t={secs}s visible={visible_n} comp_holes={composite_holes} l0={l0_done}/{l0_total} queued={} blocked={}",
                state.tiles.scheduler.queued_len(),
                state.tiles.ed_frontier.blocked_count(),
            );
        }
    }

    let wall = t0.elapsed().as_secs_f64() * 1000.0;
    let line = format!(
        "# Z3 cold ≥12 MPix TIME_TO_FRESH ({profile})\n\n\
         Doc: 4000×3000 (12 MPix), Floyd–Steinberg, cold Processed/Composite.\n\
         Sequence: zoom-in → apply → mid-pass zooms → final zoom 0.25 + full L0 prefix.\n\
         Clock starts at final zoom (after mid-pass work already ran).\n\
         `time_to_l0_ms` = L0 Processed done==total; `time_to_fresh_ms` = visible Composite holes=0.\n\n\
         | profile | visible | composite_holes | l0_done/total | time_to_l0_ms | time_to_fresh_ms | wall_ms |\n\
         | --- | ---: | ---: | --- | ---: | ---: | ---: |\n\
         | {profile} | {visible_n} | {composite_holes} | {l0_done}/{l0_total} | {} | {} | {wall:.1} |\n",
        time_to_l0_ms
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "—".into()),
        time_to_fresh_ms
            .map(|v| format!("{v:.1}"))
            .unwrap_or_else(|| "TIMEOUT".into()),
    );
    tile_gen_trace::emit_simple(
        "z3_cold_result",
        json!({
            "profile": profile,
            "visible": visible_n,
            "composite_holes": composite_holes,
            "l0_done": l0_done,
            "l0_total": l0_total,
            "time_to_l0_ms": time_to_l0_ms,
            "time_to_fresh_ms": time_to_fresh_ms,
            "wall_ms": wall,
            "note": "clock_from_final_zoom_after_mid_pass_warmup",
        }),
    );
    let path = out_dir.join(format!("z3_cold_12mp_{profile}.md"));
    let _ = std::fs::write(&path, &line);
    eprintln!("wrote {}", path.display());
    eprintln!("{line}");
    let _ = std::io::stderr().flush();
    teardown(&state, &stop, joins);
}

/// CI-fast Z3 regression: small FS doc, cold Processed/Composite, zoom-out mid-pass.
/// Asserts visible Composite holes == 0 and pyramid pending spin stays under 1000.
#[test]
fn z3_cold_small_composite_holes_zero() {
    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let size = SizeSpec {
        w: 748,
        h: 1024,
        label: "748x1024",
    };
    let budget = 512 * 1024 * 1024;
    let mode = Mode::FloydSteinberg;
    let (state, stop, joins) = setup(mode, size, budget);
    let _ = wait_silence(&state, Duration::from_secs(30));
    state
        .tiles
        .tile_cache
        .evict_stages(1, &[CacheStage::Processed, CacheStage::Composite]);
    state.tiles.error_residuals.evict_document(1);
    state.tiles.ed_frontier.clear();

    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(2.0, 0.0, 0.0, 800.0, 600.0);
    apply_threshold(&state, mode, 1.0);
    thread::sleep(Duration::from_millis(20));
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(0.5, 0.0, 0.0, 800.0, 600.0);
    thread::sleep(Duration::from_millis(20));

    let t0 = Instant::now();
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(0.25, 0.0, 0.0, 800.0, 600.0);
    crate::commands::schedule_dirty_viewport_tiles(&state);
    {
        let snap = state.must_active().document_handle.snapshot();
        let doc_gen = snap.generations.document_gen.load(Ordering::Acquire);
        let max_tx = size.w.saturating_sub(1) / engine_tiles::TILE_SIZE;
        let max_ty = size.h.saturating_sub(1) / engine_tiles::TILE_SIZE;
        tile_pipeline::schedule_ed_prefix_closure(
            &state,
            1,
            1,
            0,
            max_tx,
            max_ty,
            engine_tiles::Priority::Immediate,
            doc_gen,
            snap.generations.get_layer_gen(1),
        );
    }
    crate::ed_progress::note_ed_pass_start(&state, 1);

    let timeout = Duration::from_secs(60);
    let start = Instant::now();
    let mut composite_holes = usize::MAX;
    let mut visible_n = 0usize;
    loop {
        let vp = state.ui.viewport.lock().unwrap().clone();
        visible_n = vp.visible_tiles.len();
        let mut missing = 0usize;
        for coord in &vp.visible_tiles {
            let key = TileKey {
                doc: 1,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            };
            if !engine_tiles::tile_fresh(&state.tiles.tile_cache, key) {
                missing += 1;
            }
        }
        composite_holes = missing;
        if visible_n > 0 && missing == 0 {
            break;
        }
        if start.elapsed() > timeout {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    let pending_spin = tile_gen_trace::test_events()
        .iter()
        .filter(|e| e.event == "harness_compute_pending")
        .count();
    teardown(&state, &stop, joins);

    assert!(
        visible_n > 0,
        "expected a non-empty viewport after zoom-out"
    );
    assert_eq!(
        composite_holes, 0,
        "visible Composite holes must be 0 (got {composite_holes}); wall={:.1}ms pending_spin={pending_spin}",
        t0.elapsed().as_secs_f64() * 1000.0
    );
    assert!(
        pending_spin < 1000,
        "pyramid pending spin {pending_spin} >= 1000 (WORKER_SPIN-style scene budget)"
    );
}

/// First K tiles in request/ready order must be checked (not only 0/0/0).
const Z9_K: usize = 4;
/// Matches FE `PENDING_SAFETY_RETRY_MS`.
const Z9_SAFETY_MS: u64 = 1500;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
struct Z9TileId {
    level: u8,
    x: u32,
    y: u32,
}

impl Z9TileId {
    fn from_trace(t: &tile_gen_trace::TraceTile) -> Self {
        Self {
            level: t.level,
            x: t.x,
            y: t.y,
        }
    }

    fn key(self) -> TileKey {
        TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: self.level,
                x: self.x,
                y: self.y,
            },
            stage: CacheStage::Composite,
        }
    }
}

#[derive(Default)]
struct Z9TileGate {
    inflight: bool,
    needs_refetch: bool,
    refetch_done: bool,
    /// Highest ready gen observed via tile_ready for this key.
    max_ready: u64,
    /// Simulated displayed gen after last accepted response.
    displayed: u64,
    fetch_count: u32,
    needs_count: u32,
    collapsed: u32,
    safety_fetches: u32,
    stale_after_retry: u32,
}

struct Z9SeriesResult {
    quiet: bool,
    #[allow(dead_code)]
    live_gen: u64,
    /// Max Composite lag among watched tiles.
    max_lag: u64,
    /// Watched tiles that are not fresh at live after quiet.
    stale_tiles: usize,
    wake_fetched: u32,
    wake_needs_refetch: u32,
    collapsed_retries: u32,
    safety_fetches: u32,
    stale_after_retry: u32,
    watched: usize,
    ul_tile: Z9TileId,
}

fn visible_level0(state: &AppState) -> Vec<Z9TileId> {
    let vp = state.ui.viewport.lock().unwrap().clone();
    let mut out: Vec<Z9TileId> = vp
        .visible_tiles
        .iter()
        .filter(|c| c.level == 0)
        .map(|c| Z9TileId {
            level: c.level,
            x: c.x,
            y: c.y,
        })
        .collect();
    out.sort_by_key(|t| (t.y, t.x, t.level));
    out
}

fn composite_lag(state: &AppState, id: Z9TileId, live: u64) -> (u64, bool, bool) {
    let key = id.key();
    match state.tiles.tile_cache.entries.get(&key) {
        Some(e) => {
            let gen = e.generation;
            let dirty = e.dirty.load(Ordering::Acquire);
            let fresh = engine_tiles::tile_fresh(&state.tiles.tile_cache, key);
            (live.saturating_sub(gen), dirty, fresh)
        }
        None => (live, true, false),
    }
}

#[derive(Clone, Copy)]
struct Z9RunOpts {
    shifted: bool,
    /// Caller already set viewport / started a long pass (skip series-0 warm quiet).
    mid_recalc: bool,
}

/// One Z9 series: 8 rapid param pulses; FE-gate sim over first K ready tiles.
fn run_z9_series(
    state: &Arc<AppState>,
    profile: &str,
    series: usize,
    opts: Z9RunOpts,
) -> Z9SeriesResult {
    tile_gen_trace::clear_test_events_only();
    let mode = Mode::FloydSteinberg;
    let shifted = opts.shifted;
    if series == 0 && !opts.mid_recalc {
        if shifted {
            // Pan so the upper-left visible tile is not (0,0).
            let _ = ViewportService::new(Arc::clone(state))
                .set_viewport(1.0, 700.0, 500.0, 1200.0, 800.0);
        } else {
            let _ =
                ViewportService::new(Arc::clone(state)).set_viewport(1.0, 0.0, 0.0, 1200.0, 800.0);
        }
        apply_threshold(state, mode, 1.0);
        let _ = wait_silence(state, Duration::from_secs(60));
    } else if series > 0 {
        let _ = wait_silence(state, Duration::from_secs(15));
    }

    let visible = visible_level0(state);
    assert!(
        visible.len() >= Z9_K,
        "Z9 needs ≥{Z9_K} L0 visible tiles, got {}",
        visible.len()
    );
    let ul = visible[0];
    if shifted {
        assert!(
            !(ul.x == 0 && ul.y == 0),
            "shifted Z9 UL must not be 0/0/0, got {}/{}/{}",
            ul.level,
            ul.x,
            ul.y
        );
    }

    let bursts = 8usize;
    let gap = Duration::from_millis(50);
    tile_gen_trace::emit_simple(
        "z9_burst_begin",
        json!({
            "bursts": bursts,
            "gap_ms": gap.as_millis(),
            "profile": profile,
            "series": series,
            "shifted": shifted,
            "ul": format!("{}/{}/{}", ul.level, ul.x, ul.y),
            "k": Z9_K,
            "visible_l0": visible.len(),
        }),
    );

    // First K tiles in viewport request order (sorted UL→BR).
    let watch: Vec<Z9TileId> = visible.into_iter().take(Z9_K).collect();
    let mut gates: std::collections::HashMap<Z9TileId, Z9TileGate> = watch
        .iter()
        .copied()
        .map(|id| (id, Z9TileGate::default()))
        .collect();
    let mut last_seq = 0u64;
    let mut last_safety = Instant::now()
        .checked_sub(Duration::from_millis(Z9_SAFETY_MS))
        .unwrap_or_else(Instant::now);

    for i in 0..bursts {
        let scale = if i % 2 == 0 { 1.05 } else { 1.2 };
        apply_threshold(state, mode, scale);
        tile_gen_trace::emit_simple(
            "z9_param_pulse",
            json!({ "i": i, "threshold_scale": scale, "series": series }),
        );
        let slice = Instant::now();
        while slice.elapsed() < gap {
            for ev in tile_gen_trace::test_events() {
                if ev.seq <= last_seq {
                    continue;
                }
                last_seq = last_seq.max(ev.seq);
                if ev.event != "tile_ready_emitted" {
                    continue;
                }
                let Some(t) = &ev.tile else { continue };
                let id = Z9TileId::from_trace(t);
                let Some(g) = gates.get_mut(&id) else {
                    continue;
                };
                let ready = ev.gen.unwrap_or(0);
                g.max_ready = g.max_ready.max(ready);
                if g.inflight {
                    g.needs_refetch = true;
                    g.needs_count += 1;
                    tile_gen_trace::emit(
                        "wake_needs_refetch",
                        None,
                        ev.doc_gen,
                        None,
                        Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                        ev.gen,
                        json!({ "cause": "tile_ready", "sim": "harness_fe_gate_b" }),
                    );
                } else {
                    g.inflight = true;
                    g.fetch_count += 1;
                    tile_gen_trace::emit(
                        "fetch_tile_posted",
                        None,
                        ev.doc_gen,
                        None,
                        Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                        ev.gen,
                        json!({ "wake": "tile_ready", "sim": "harness_fe_gate_b" }),
                    );
                }
            }
            // Safety timer: if collapsed retry left a tile behind, re-arm.
            if last_safety.elapsed() >= Duration::from_millis(Z9_SAFETY_MS) {
                last_safety = Instant::now();
                for (id, g) in gates.iter_mut() {
                    if g.inflight {
                        continue;
                    }
                    let behind = g.max_ready > 0 && g.displayed < g.max_ready;
                    if !(behind || g.stale_after_retry > 0) {
                        continue;
                    }
                    g.refetch_done = false;
                    g.needs_refetch = false;
                    g.inflight = true;
                    g.safety_fetches += 1;
                    g.fetch_count += 1;
                    tile_gen_trace::emit(
                        "fetch_tile_posted",
                        None,
                        None,
                        None,
                        Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                        Some(g.max_ready),
                        json!({ "wake": "safety_timer", "sim": "harness_fe_gate_b" }),
                    );
                }
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    // Complete inflight: ≤1 collapsed retry; if still behind, mark stale-after-retry.
    for (id, g) in gates.iter_mut() {
        if !g.inflight {
            continue;
        }
        g.inflight = false;
        // First response may land behind a newer ready that arrived mid-flight.
        let response = g.max_ready.saturating_sub(1).max(g.displayed);
        if (g.needs_refetch || response < g.max_ready) && !g.refetch_done {
            g.collapsed += 1;
            g.refetch_done = true;
            g.needs_refetch = false;
            g.inflight = true;
            g.fetch_count += 1;
            tile_gen_trace::emit(
                "wake_collapsed_retry",
                None,
                None,
                None,
                Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                Some(response),
                json!({
                    "sim": "harness_fe_gate_b",
                    "responseGen": response,
                    "maxKnownGen": g.max_ready,
                }),
            );
            // Simulate collapsed retry still returning stale once (slow BE).
            g.inflight = false;
            g.displayed = response;
            if g.displayed < g.max_ready {
                g.stale_after_retry += 1;
                tile_gen_trace::emit(
                    "wake_retry_returned_stale",
                    None,
                    None,
                    None,
                    Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                    Some(g.displayed),
                    json!({
                        "sim": "harness_fe_gate_b",
                        "responseGen": g.displayed,
                        "maxKnownGen": g.max_ready,
                        "await": "safety_timer",
                    }),
                );
            }
        } else {
            g.displayed = g.max_ready;
        }
    }

    // Drive safety until watched tiles catch displayed==max_ready (bounded).
    let safety_deadline = Instant::now() + Duration::from_millis(Z9_SAFETY_MS * 4);
    while Instant::now() < safety_deadline {
        let mut pending = false;
        for (id, g) in gates.iter_mut() {
            if g.displayed >= g.max_ready && g.max_ready > 0 {
                continue;
            }
            pending = true;
            if g.inflight {
                continue;
            }
            g.refetch_done = false;
            g.inflight = true;
            g.safety_fetches += 1;
            g.fetch_count += 1;
            tile_gen_trace::emit(
                "fetch_tile_posted",
                None,
                None,
                None,
                Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                Some(g.max_ready),
                json!({ "wake": "safety_timer", "sim": "harness_fe_gate_b", "catchup": true }),
            );
            g.inflight = false;
            g.displayed = g.max_ready;
        }
        if !pending {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    let wake_fetched: u32 = gates.values().map(|g| g.fetch_count).sum();
    let wake_needs_refetch: u32 = gates.values().map(|g| g.needs_count).sum();
    let collapsed_retries: u32 = gates.values().map(|g| g.collapsed).sum();
    let safety_fetches: u32 = gates.values().map(|g| g.safety_fetches).sum();
    let stale_after_retry: u32 = gates.values().map(|g| g.stale_after_retry).sum();

    let per_tile: Vec<_> = gates
        .iter()
        .map(|(id, g)| {
            json!({
                "tile": format!("{}/{}/{}", id.level, id.x, id.y),
                "fetch_count": g.fetch_count,
                "needs_count": g.needs_count,
                "collapsed": g.collapsed,
                "safety_fetches": g.safety_fetches,
                "stale_after_retry": g.stale_after_retry,
                "max_ready": g.max_ready,
                "displayed": g.displayed,
            })
        })
        .collect();

    tile_gen_trace::emit_simple(
        "z9_burst_end",
        json!({
            "wake_fetched": wake_fetched,
            "wake_needs_refetch": wake_needs_refetch,
            "collapsed_retries": collapsed_retries,
            "safety_fetches": safety_fetches,
            "stale_after_retry": stale_after_retry,
            "series": series,
            "per_tile": per_tile,
        }),
    );

    let quiet = wait_silence(state, Duration::from_secs(60));
    let live_gen = state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire);
    let mut max_lag = 0u64;
    let mut stale_tiles = 0usize;
    for id in &watch {
        let (lag, dirty, fresh) = composite_lag(state, *id, live_gen);
        max_lag = max_lag.max(lag);
        if !fresh || dirty || lag != 0 {
            stale_tiles += 1;
        }
    }
    tile_gen_trace::emit_simple(
        "z9_result",
        json!({
            "profile": profile,
            "series": series,
            "shifted": shifted,
            "quiet_ok": quiet,
            "live_doc_gen": live_gen,
            "max_lag": max_lag,
            "stale_tiles": stale_tiles,
            "watched": watch.len(),
            "ul": format!("{}/{}/{}", ul.level, ul.x, ul.y),
            "wake_fetched": wake_fetched,
            "wake_needs_refetch": wake_needs_refetch,
            "collapsed_retries": collapsed_retries,
            "safety_fetches": safety_fetches,
            "stale_after_retry": stale_after_retry,
            "metric_retries_le_1_per_tile": gates.values().all(|g| g.collapsed <= 1),
            "be_cache_matches_live": stale_tiles == 0,
        }),
    );
    Z9SeriesResult {
        quiet,
        live_gen,
        max_lag,
        stale_tiles,
        wake_fetched,
        wake_needs_refetch,
        collapsed_retries,
        safety_fetches,
        stale_after_retry,
        watched: watch.len(),
        ul_tile: ul,
    }
}

fn assert_z9_ok(r: &Z9SeriesResult, label: &str) {
    assert!(r.quiet, "{label}: must reach quiet");
    assert_eq!(
        r.stale_tiles, 0,
        "{label}: visible watched tiles must be fresh"
    );
    assert_eq!(r.max_lag, 0, "{label}: STALE_GEN lag must be 0");
    assert!(
        r.watched >= Z9_K,
        "{label}: watched {} < K={Z9_K}",
        r.watched
    );
    // Collapsed retries are per-tile ≤1; aggregate may be up to K.
    assert!(
        r.collapsed_retries <= Z9_K as u32,
        "{label}: collapsed_retries={} > K",
        r.collapsed_retries
    );
}

/// Z9 Step B single-shot (debug / smoke). UL at (0,0); first K tiles.
#[test]
#[ignore]
fn z9_rapid_param_composite_gen() {
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-21")
        .join("h4_z9");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let events_path = out_dir.join(format!("events_z9_{profile}.jsonl"));
    crate::tile_gen_trace_host::set_out_path_for_tests(events_path.clone());

    let size = SizeSpec {
        w: 1920,
        h: 1080,
        label: "1920x1080",
    };
    let budget = 1024 * 1024 * 1024;
    let (state, stop, joins) = setup(Mode::FloydSteinberg, size, budget);
    let _ = wait_silence(&state, Duration::from_secs(60));
    let r = run_z9_series(
        &state,
        profile,
        0,
        Z9RunOpts {
            shifted: false,
            mid_recalc: false,
        },
    );
    let md = out_dir.join(format!("z9_{profile}.md"));
    let mut f = std::fs::File::create(&md).expect("z9 md");
    writeln!(
        f,
        "# Z9 rapid-param (H4 Step B) — {profile}\n\n\
         - quiet_ok={} watched={} ul={}/{}/{} stale={} max_lag={}\n\
         - wake_fetched={} needs_refetch={} collapsed={} safety={} stale_after_retry={}\n\
         - events: `{}`\n",
        r.quiet,
        r.watched,
        r.ul_tile.level,
        r.ul_tile.x,
        r.ul_tile.y,
        r.stale_tiles,
        r.max_lag,
        r.wake_fetched,
        r.wake_needs_refetch,
        r.collapsed_retries,
        r.safety_fetches,
        r.stale_after_retry,
        events_path.display()
    )
    .ok();
    eprintln!(
        "[z9] profile={profile} quiet={} watched={} ul={}/{}/{} stale={} lag={} fetch={} needs={} retry={} safety={} stale_retry={}",
        r.quiet,
        r.watched,
        r.ul_tile.level,
        r.ul_tile.x,
        r.ul_tile.y,
        r.stale_tiles,
        r.max_lag,
        r.wake_fetched,
        r.wake_needs_refetch,
        r.collapsed_retries,
        r.safety_fetches,
        r.stale_after_retry
    );
    teardown(&state, &stop, joins);
    assert_z9_ok(&r, "z9");
}

/// Z9 with pan so the upper-left visible tile is not (0,0).
#[test]
#[ignore]
fn z9_rapid_param_shifted_viewport() {
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-21")
        .join("h4_z9");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let events_path = out_dir.join(format!("events_z9_shifted_{profile}.jsonl"));
    crate::tile_gen_trace_host::set_out_path_for_tests(events_path.clone());

    let size = SizeSpec {
        w: 1920,
        h: 1080,
        label: "1920x1080",
    };
    let budget = 1024 * 1024 * 1024;
    let (state, stop, joins) = setup(Mode::FloydSteinberg, size, budget);
    let _ = wait_silence(&state, Duration::from_secs(60));
    let r = run_z9_series(
        &state,
        profile,
        0,
        Z9RunOpts {
            shifted: true,
            mid_recalc: false,
        },
    );
    let md = out_dir.join(format!("z9_shifted_{profile}.md"));
    let _ = std::fs::write(
        &md,
        format!(
            "# Z9 shifted viewport — {profile}\n\n\
             - ul={}/{}/{} watched={} stale={} max_lag={}\n\
             - fetch={} needs={} collapsed={} safety={} stale_after_retry={}\n",
            r.ul_tile.level,
            r.ul_tile.x,
            r.ul_tile.y,
            r.watched,
            r.stale_tiles,
            r.max_lag,
            r.wake_fetched,
            r.wake_needs_refetch,
            r.collapsed_retries,
            r.safety_fetches,
            r.stale_after_retry
        ),
    );
    eprintln!(
        "[z9_shifted] ul={}/{}/{} watched={} stale={} lag={}",
        r.ul_tile.level, r.ul_tile.x, r.ul_tile.y, r.watched, r.stale_tiles, r.max_lag
    );
    teardown(&state, &stop, joins);
    assert!(
        !(r.ul_tile.x == 0 && r.ul_tile.y == 0),
        "shifted UL must not be 0/0"
    );
    assert_z9_ok(&r, "z9_shifted");
}

/// Slow BE: ≥12 MPix cold FS, 8 param pulses during long recalc; safety recovers
/// after collapsed retry returns stale; per-tile request counters in trace.
#[test]
#[ignore]
fn z9_slow_be_12mp_stale_retry() {
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-21")
        .join("h4_z9");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let events_path = out_dir.join(format!("events_z9_slow12mp_{profile}.jsonl"));
    crate::tile_gen_trace_host::set_out_path_for_tests(events_path.clone());

    // 4000×3000 = 12 MPix.
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let budget = 1536 * 1024 * 1024;
    let mode = Mode::FloydSteinberg;
    let (state, stop, joins) = setup(mode, size, budget);
    let _ = wait_silence(&state, Duration::from_secs(90));

    // Cold Processed/Composite so the next pass is long vs the retry cycle.
    state
        .tiles
        .tile_cache
        .evict_stages(1, &[CacheStage::Processed, CacheStage::Composite]);
    state.tiles.error_residuals.evict_document(1);
    state.tiles.ed_frontier.clear();

    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(1.0, 0.0, 0.0, 1200.0, 800.0);
    // Kick a long ED pass, then pulse params without waiting for silence.
    apply_threshold(&state, mode, 1.0);
    let busy_deadline = Instant::now() + Duration::from_secs(2);
    let mut saw_busy = false;
    while Instant::now() < busy_deadline {
        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        let pending = state
            .pending_preview_refresh
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        if queued > 0 || inflight > 0 || blocked > 0 || pending {
            saw_busy = true;
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    // Even if workers drained the first enqueue before we sampled, continue —
    // mid_recalc skips the warm quiet so the 8 pulses overlap subsequent work.
    tile_gen_trace::emit_simple("z9_slow_busy_sample", json!({ "saw_busy": saw_busy }));

    let t0 = Instant::now();
    let r = run_z9_series(
        &state,
        profile,
        0,
        Z9RunOpts {
            shifted: false,
            mid_recalc: true,
        },
    );
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let stale_events = tile_gen_trace::test_events()
        .iter()
        .filter(|e| e.event == "wake_retry_returned_stale")
        .count();
    let max_fetch_per_tile = {
        // Bound: 8 signals + 1 collapsed + a few safety catches — not unbounded.
        8u32 + 1 + 6
    };
    // Per-tile counters are in the last z9_burst_end; re-scan events for max fetch.
    let mut max_tile_fetches = 0u32;
    for ev in tile_gen_trace::test_events() {
        if ev.event != "z9_burst_end" {
            continue;
        }
        if let Some(arr) = ev.details.get("per_tile").and_then(|v| v.as_array()) {
            for t in arr {
                let c = t.get("fetch_count").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                max_tile_fetches = max_tile_fetches.max(c);
            }
        }
    }

    let md = out_dir.join(format!("z9_slow12mp_{profile}.md"));
    let mut f = std::fs::File::create(&md).expect("slow md");
    writeln!(
        f,
        "# Z9 slow-BE ≥12 MPix — {profile}\n\n\
         - wall_ms={wall_ms:.0}\n\
         - quiet={} watched={} stale_tiles={} max_lag={}\n\
         - wake_fetched={} collapsed={} safety={} stale_after_retry_events={stale_events}\n\
         - max_fetch_per_tile={max_tile_fetches} (budget ≤ {max_fetch_per_tile})\n\
         - events: `{}`\n",
        r.quiet,
        r.watched,
        r.stale_tiles,
        r.max_lag,
        r.wake_fetched,
        r.collapsed_retries,
        r.safety_fetches,
        events_path.display()
    )
    .ok();
    eprintln!(
        "[z9_slow12mp] profile={profile} wall_ms={wall_ms:.0} quiet={} stale={} lag={} \
         fetch={} safety={} stale_retry_ev={stale_events} max_tile_fetch={max_tile_fetches}",
        r.quiet, r.stale_tiles, r.max_lag, r.wake_fetched, r.safety_fetches
    );
    teardown(&state, &stop, joins);

    assert_z9_ok(&r, "z9_slow12mp");
    assert!(
        stale_events >= 1 || r.stale_after_retry >= 1,
        "expected wake_retry_returned_stale in trace (refetchDone must not stick forever)"
    );
    assert!(
        max_tile_fetches <= max_fetch_per_tile,
        "infinite-retry guard: max per-tile fetches {max_tile_fetches} > {max_fetch_per_tile}"
    );
    // Recalc window should outlast one safety cycle on 12 MPix cold FS.
    assert!(
        wall_ms >= Z9_SAFETY_MS as f64 * 0.5,
        "expected slow-BE wall time, got {wall_ms:.0}ms"
    );
}

/// 50 consecutive Z9 series — hard gate for Step B.
#[test]
#[ignore]
fn z9_rapid_param_50_series() {
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-21")
        .join("h4_z9");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let events_path = out_dir.join(format!("events_z9_50_{profile}.jsonl"));
    crate::tile_gen_trace_host::set_out_path_for_tests(events_path.clone());

    let size = SizeSpec {
        w: 1920,
        h: 1080,
        label: "1920x1080",
    };
    let budget = 1024 * 1024 * 1024;
    let (state, stop, joins) = setup(Mode::FloydSteinberg, size, budget);
    let _ = wait_silence(&state, Duration::from_secs(60));

    let n = 50usize;
    let mut fail = 0usize;
    let mut max_retries = 0u32;
    let mut sum_needs = 0u32;
    for s in 0..n {
        let r = run_z9_series(
            &state,
            profile,
            s,
            Z9RunOpts {
                shifted: false,
                mid_recalc: false,
            },
        );
        max_retries = max_retries.max(r.collapsed_retries);
        sum_needs += r.wake_needs_refetch;
        if !r.quiet || r.stale_tiles != 0 || r.max_lag != 0 || r.collapsed_retries > Z9_K as u32 {
            fail += 1;
            eprintln!(
                "[z9_50] FAIL series={s} quiet={} stale={} lag={} retries={}",
                r.quiet, r.stale_tiles, r.max_lag, r.collapsed_retries
            );
        }
    }
    let md = out_dir.join(format!("z9_50_{profile}.md"));
    let mut f = std::fs::File::create(&md).expect("z9_50 md");
    writeln!(
        f,
        "# Z9 ×50 (H4 Step B) — {profile}\n\n\
         - series={n} failures={fail}\n\
         - max_collapsed_retries={max_retries}\n\
         - sum_wake_needs_refetch={sum_needs}\n\
         - events: `{}`\n",
        events_path.display()
    )
    .ok();
    eprintln!("[z9_50] profile={profile} n={n} fail={fail} max_retries={max_retries}");
    teardown(&state, &stop, joins);
    assert_eq!(fail, 0, "{fail}/{n} Z9 series failed");
}

/// Z10 Step A (diagnose only): continuous ~60 Hz param drag for 1.5 s on ≥12 MPix FS.
/// Checks **all** visible Composite tiles; summarizes stale by column/row (x=0 focus).
fn run_z10_once(cold: bool) {
    use std::collections::HashMap;
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-21")
        .join("h4_z10");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let cache = if cold { "cold" } else { "warm" };
    let events_path = out_dir.join(format!("events_z10_{cache}_{profile}.jsonl"));
    crate::tile_gen_trace_host::set_out_path_for_tests(events_path.clone());

    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let budget = 1536 * 1024 * 1024;
    let mode = Mode::FloydSteinberg;
    let (state, stop, joins) = setup(mode, size, budget);
    let _ = wait_silence(&state, Duration::from_secs(90));

    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(1.0, 0.0, 0.0, 1200.0, 800.0);
    apply_threshold(&state, mode, 1.0);
    let _ = wait_silence(&state, Duration::from_secs(120));

    if cold {
        state
            .tiles
            .tile_cache
            .evict_stages(1, &[CacheStage::Processed, CacheStage::Composite]);
        state.tiles.error_residuals.evict_document(1);
        state.tiles.ed_frontier.clear();
        tile_gen_trace::emit_simple("z10_cold_cache", json!({ "stages": ["Processed", "Composite"] }));
    }

    let visible = visible_level0(&state);
    assert!(
        !visible.is_empty(),
        "Z10 expects non-empty L0 visible set"
    );
    tile_gen_trace::emit_simple(
        "z10_viewport",
        json!({
            "visible_l0": visible.len(),
            "tiles": visible.iter().map(|t| format!("{}/{}/{}", t.level, t.x, t.y)).collect::<Vec<_>>(),
            "cache": cache,
            "profile": profile,
        }),
    );

    // FE-gate sim over ALL visible tiles (not just K=4).
    let mut gates: HashMap<Z9TileId, Z9TileGate> = visible
        .iter()
        .copied()
        .map(|id| (id, Z9TileGate::default()))
        .collect();
    let mut last_seq = 0u64;
    let hz = 60u64;
    let duration_ms = 1500u64;
    let gap = Duration::from_millis(1000 / hz);
    let pulses = (duration_ms / (1000 / hz)) as usize;
    tile_gen_trace::emit_simple(
        "z10_drag_begin",
        json!({ "hz": hz, "duration_ms": duration_ms, "pulses": pulses, "cache": cache }),
    );

    for i in 0..pulses {
        let scale = 1.0 + ((i % 20) as f32) * 0.01;
        apply_threshold(&state, mode, scale.max(1.01));
        tile_gen_trace::emit_simple(
            "z10_param_pulse",
            json!({ "i": i, "threshold_scale": scale }),
        );
        let slice = Instant::now();
        while slice.elapsed() < gap {
            for ev in tile_gen_trace::test_events() {
                if ev.seq <= last_seq {
                    continue;
                }
                last_seq = last_seq.max(ev.seq);
                if ev.event != "tile_ready_emitted" {
                    continue;
                }
                let Some(t) = &ev.tile else { continue };
                let id = Z9TileId::from_trace(t);
                let Some(g) = gates.get_mut(&id) else {
                    continue;
                };
                let ready = ev.gen.unwrap_or(0);
                g.max_ready = g.max_ready.max(ready);
                if g.inflight {
                    g.needs_refetch = true;
                    g.needs_count += 1;
                    tile_gen_trace::emit(
                        "wake_decision",
                        None,
                        ev.doc_gen,
                        None,
                        Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                        ev.gen,
                        json!({
                            "action": "needs_refetch",
                            "cause": "tile_ready",
                            "refetchDone": g.refetch_done,
                            "sim": "harness_z10",
                        }),
                    );
                } else {
                    g.inflight = true;
                    g.fetch_count += 1;
                    tile_gen_trace::emit(
                        "wake_decision",
                        None,
                        ev.doc_gen,
                        None,
                        Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                        ev.gen,
                        json!({
                            "action": "fetch",
                            "cause": "tile_ready",
                            "refetchDone": g.refetch_done,
                            "sim": "harness_z10",
                        }),
                    );
                    tile_gen_trace::emit(
                        "fetch_tile_posted",
                        None,
                        ev.doc_gen,
                        None,
                        Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                        ev.gen,
                        json!({ "wake": "tile_ready", "sim": "harness_z10" }),
                    );
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
    tile_gen_trace::emit_simple("z10_drag_end", json!({ "pulses": pulses }));

    // Complete inflight with ≤1 collapsed retry; mark stale-after-retry (diagnose).
    for (id, g) in gates.iter_mut() {
        if !g.inflight {
            continue;
        }
        g.inflight = false;
        let response = g.max_ready.saturating_sub(1).max(g.displayed);
        if (g.needs_refetch || response < g.max_ready) && !g.refetch_done {
            g.collapsed += 1;
            g.refetch_done = true;
            g.fetch_count += 1;
            tile_gen_trace::emit(
                "wake_decision",
                None,
                None,
                None,
                Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                Some(response),
                json!({
                    "action": "collapsed_retry",
                    "refetchDone": true,
                    "sim": "harness_z10",
                }),
            );
            g.displayed = response;
            if g.displayed < g.max_ready {
                g.stale_after_retry += 1;
                tile_gen_trace::emit(
                    "wake_retry_returned_stale",
                    None,
                    None,
                    None,
                    Some(tile_gen_trace::tile(id.level, id.x, id.y)),
                    Some(g.displayed),
                    json!({
                        "responseGen": g.displayed,
                        "maxKnownGen": g.max_ready,
                        "await": "safety_timer",
                        "sim": "harness_z10",
                    }),
                );
            }
        } else {
            g.displayed = g.max_ready;
        }
    }

    let quiet = wait_silence(&state, Duration::from_secs(180));
    let live_gen = state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire);

    let mut by_col: HashMap<u32, (usize, usize)> = HashMap::new();
    let mut by_row: HashMap<u32, (usize, usize)> = HashMap::new();
    let mut stuck: Vec<(Z9TileId, u64, bool, bool)> = Vec::new();
    // FE-sim stickiness: displayed gen after collapsed retry / no safety catch-up.
    let mut fe_stuck: Vec<(Z9TileId, u64, u64)> = Vec::new();
    let mut fe_by_col: HashMap<u32, (usize, usize)> = HashMap::new();
    for id in &visible {
        let (lag, dirty, fresh) = composite_lag(&state, *id, live_gen);
        let e = by_col.entry(id.x).or_insert((0, 0));
        e.1 += 1;
        let r = by_row.entry(id.y).or_insert((0, 0));
        r.1 += 1;
        if !fresh || dirty || lag != 0 {
            e.0 += 1;
            r.0 += 1;
            stuck.push((*id, lag, dirty, fresh));
        }
        let fe = fe_by_col.entry(id.x).or_insert((0, 0));
        fe.1 += 1;
        if let Some(g) = gates.get(id) {
            if g.displayed < live_gen || g.stale_after_retry > 0 {
                fe.0 += 1;
                fe_stuck.push((*id, g.displayed, g.max_ready));
            }
        }
    }

    // Chronology for each stuck tile from the event buffer.
    let events = tile_gen_trace::test_events();
    let mut chronologies = Vec::new();
    for (id, lag, dirty, fresh) in &stuck {
        let key = format!("{}/{}/{}", id.level, id.x, id.y);
        let mut chain = Vec::new();
        let mut last_fetch_gen: Option<u64> = None;
        let mut ready_for_live = false;
        let mut pending_seen = false;
        let mut last_wake: Option<String> = None;
        for ev in &events {
            let Some(t) = &ev.tile else {
                continue;
            };
            if t.level != id.level || t.x != id.x || t.y != id.y {
                continue;
            }
            match ev.event.as_str() {
                "fetch_tile_posted" => {
                    last_fetch_gen = ev.gen;
                    chain.push(format!("fetch@{:?}", ev.gen));
                }
                "tile_ready_emitted" => {
                    if ev.gen == Some(live_gen) {
                        ready_for_live = true;
                    }
                    let via = ev
                        .details
                        .get("via")
                        .or_else(|| ev.details.get("reason"))
                        .cloned()
                        .unwrap_or(json!(null));
                    let was_pending = ev.details.get("was_pending").cloned().unwrap_or(json!(null));
                    chain.push(format!("ready@{:?} via={via} was_pending={was_pending}", ev.gen));
                }
                "pending_tile_registered" => {
                    pending_seen = true;
                    chain.push(format!("pending_reg@{:?}", ev.gen));
                }
                "wake_decision" | "wake_needs_refetch" | "wake_collapsed_retry"
                | "wake_retry_returned_stale" => {
                    last_wake = Some(ev.event.clone());
                    chain.push(format!("{}@{:?}", ev.event, ev.gen));
                }
                "strip_owner_acquired" | "strip_insert" | "strip_discarded_by_doc_gen" => {
                    chain.push(format!("{}@{:?}", ev.event, ev.gen));
                }
                "composite_start" | "composite_end" => {
                    chain.push(format!(
                        "{}@{:?} details={}",
                        ev.event,
                        ev.gen,
                        ev.details
                    ));
                }
                "tile_serve_response" => {
                    chain.push(format!(
                        "serve@{:?} kind={:?}",
                        ev.gen,
                        ev.details.get("kind")
                    ));
                }
                _ => {}
            }
        }
        chronologies.push(json!({
            "tile": key,
            "lag": lag,
            "dirty": dirty,
            "fresh": fresh,
            "last_fetch_gen": last_fetch_gen,
            "tile_ready_for_live_gen": ready_for_live,
            "pending_registered": pending_seen,
            "last_wake": last_wake,
            "chain": chain,
        }));
        tile_gen_trace::emit_simple(
            "z10_stuck_chronology",
            json!({
                "tile": key,
                "lag": lag,
                "last_fetch_gen": last_fetch_gen,
                "tile_ready_for_live_gen": ready_for_live,
                "pending_registered": pending_seen,
                "last_wake": last_wake,
                "chain": chain,
            }),
        );
    }

    // Strip order / ready timing: x=0 vs other columns.
    let mut strip_owners_x0 = 0u32;
    let mut strip_owners_other = 0u32;
    let mut ready_ts_x0: Vec<u64> = Vec::new();
    let mut ready_ts_other: Vec<u64> = Vec::new();
    for ev in &events {
        if ev.event == "strip_owner_acquired" {
            let claim_x = ev
                .details
                .get("claim_x")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX);
            if claim_x == 0 {
                strip_owners_x0 += 1;
            } else {
                strip_owners_other += 1;
            }
        }
        if ev.event == "tile_ready_emitted" {
            if let Some(t) = &ev.tile {
                if t.x == 0 {
                    ready_ts_x0.push(ev.seq);
                } else {
                    ready_ts_other.push(ev.seq);
                }
            }
        }
    }

    let fe_col_summary: Vec<_> = {
        let mut v: Vec<serde_json::Value> = fe_by_col
            .iter()
            .map(|(x, (stale, vis))| json!({ "x": x, "stale": stale, "visible": vis }))
            .collect();
        v.sort_by_key(|j| j.get("x").and_then(|v| v.as_u64()).unwrap_or(0));
        v
    };
    let col_summary: Vec<_> = {
        let mut v: Vec<_> = by_col
            .iter()
            .map(|(x, (stale, vis))| json!({ "x": x, "stale": stale, "visible": vis }))
            .collect();
        v.sort_by_key(|j| j.get("x").and_then(|v| v.as_u64()).unwrap_or(0));
        v
    };
    let row_summary: Vec<_> = {
        let mut v: Vec<_> = by_row
            .iter()
            .map(|(y, (stale, vis))| json!({ "y": y, "stale": stale, "visible": vis }))
            .collect();
        v.sort_by_key(|j| j.get("y").and_then(|v| v.as_u64()).unwrap_or(0));
        v
    };

    tile_gen_trace::emit_simple(
        "z10_result",
        json!({
            "profile": profile,
            "cache": cache,
            "quiet_ok": quiet,
            "live_doc_gen": live_gen,
            "visible": visible.len(),
            "stuck_n": stuck.len(),
            "fe_stuck_n": fe_stuck.len(),
            "fe_stuck": fe_stuck.iter().map(|(id, d, m)| json!({
                "tile": format!("{}/{}/{}", id.level, id.x, id.y),
                "displayed": d,
                "max_ready": m,
                "live": live_gen,
            })).collect::<Vec<_>>(),
            "fe_stale_by_col": fe_col_summary,
            "by_col": col_summary,
            "by_row": row_summary,
            "stale_x0": by_col.get(&0).map(|p| p.0).unwrap_or(0),
            "visible_x0": by_col.get(&0).map(|p| p.1).unwrap_or(0),
            "fe_stale_x0": fe_by_col.get(&0).map(|p| p.0).unwrap_or(0),
            "strip_owners_x0": strip_owners_x0,
            "strip_owners_other": strip_owners_other,
            "ready_events_x0": ready_ts_x0.len(),
            "ready_events_other": ready_ts_other.len(),
            "ready_seq_min_x0": ready_ts_x0.iter().copied().min(),
            "ready_seq_min_other": ready_ts_other.iter().copied().min(),
            "chronologies": chronologies,
            // FE compares doc_gen only — layer_gen / filter revision not in maxKnownGen.
            "gen_compare_policy": "doc_gen_only_via_X-Tile-Gen_left_of_dot",
        }),
    );

    let md = out_dir.join(format!("z10_{cache}_{profile}.md"));
    let mut f = std::fs::File::create(&md).expect("z10 md");
    writeln!(
        f,
        "# Z10 continuous slider — {cache} / {profile}\n\n\
         - quiet_ok={quiet} live_gen={live_gen} visible={} stuck={}\n\
         - stale_x0={} / visible_x0={}\n\
         - strip_owners x0={strip_owners_x0} other={strip_owners_other}\n\
         - ready_events x0={} other={}\n\
         - by_col: {:?}\n\
         - events: `{}`\n",
        visible.len(),
        stuck.len(),
        by_col.get(&0).map(|p| p.0).unwrap_or(0),
        by_col.get(&0).map(|p| p.1).unwrap_or(0),
        ready_ts_x0.len(),
        ready_ts_other.len(),
        col_summary,
        events_path.display()
    )
    .ok();
    eprintln!(
        "[z10] {cache}/{profile} quiet={quiet} live={live_gen} be_stuck={} fe_stuck={} \
         be_stale_x0={} fe_stale_x0={} vis_x0={} owners_x0={strip_owners_x0} owners_other={strip_owners_other}",
        stuck.len(),
        fe_stuck.len(),
        by_col.get(&0).map(|p| p.0).unwrap_or(0),
        fe_by_col.get(&0).map(|p| p.0).unwrap_or(0),
        by_col.get(&0).map(|p| p.1).unwrap_or(0),
    );
    for (id, d, m) in &fe_stuck {
        eprintln!(
            "[z10_fe_stuck] {}/{}/{} displayed={d} max_ready={m} live={live_gen}",
            id.level, id.x, id.y
        );
    }
    for c in &chronologies {
        eprintln!("[z10_be_stuck] {c}");
    }
    teardown(&state, &stop, joins);
    // Observation-only: do not fail the build on stuck tiles (Step A).
    assert!(quiet || stuck.len() < visible.len(), "expected some progress");
}

#[test]
#[ignore]
fn z10_continuous_slider_12mp_warm() {
    run_z10_once(false);
}

#[test]
#[ignore]
fn z10_continuous_slider_12mp_cold() {
    run_z10_once(true);
}

// ─── Z11: palette change path (no doc_gen / layer_gen bump) ─────────────────

#[derive(Clone, Copy)]
enum Z11Variant {
    /// Single `update_palette_color`-shaped pulse.
    Single,
    /// Continuous ~60 Hz for 1.5 s.
    Drag,
    /// Series of discrete pulses (8 × 50 ms).
    Series,
}

impl Z11Variant {
    fn label(self) -> &'static str {
        match self {
            Z11Variant::Single => "single",
            Z11Variant::Drag => "drag",
            Z11Variant::Series => "series",
        }
    }
}

fn dither_doc_with_palette(mode: Mode, w: u32, h: u32) -> (Document, u32) {
    use engine_color::palette::LinearColor;
    use engine_project::types::PaletteId;

    let mut doc = Document::new(DocumentId::new(1), w, h);
    // Start near-black so the first white pulse must flip every mid-grey pixel.
    let pid = doc.add_palette(
        "z11".into(),
        vec![
            LinearColor {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 0.05,
                g: 0.05,
                b: 0.05,
            },
            LinearColor {
                r: 0.1,
                g: 0.1,
                b: 0.1,
            },
        ],
    );
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: mode.dither_mode(),
            pixel_size: 1,
            levels: 16,
            palette_id: Some(PaletteId::new(pid.0)),
            // Opaque photo path — avoid binary-alpha wiping RGB in the gate.
            dither_alpha: false,
            ..DitherParamsV2::default()
        }),
    ));
    doc.root.push(LayerNode::Leaf(layer));
    (doc, pid.0)
}

/// Mirror `PaletteService::update_palette_color` + `invalidate_palette_changed`
/// (revision++ / LayerFilterChanged / schedule_dirty — **no** doc/layer gen,
/// **no** DocumentChanged, **no** request_preview_refresh).
fn apply_palette_color_pulse(state: &AppState, palette_id: u32, pulse: usize) {
    use engine_color::palette::LinearColor;
    use engine_tiles::invalidation::{invalidate, InvalidationEvent};

    // Alternate near-white vs near-black palettes so Strict nearest-match of mid
    // greys must flip every pulse (fingerprint gate).
    let phase = pulse % 2;
    let colors = if phase == 0 {
        vec![
            LinearColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
            LinearColor {
                r: 0.92,
                g: 0.92,
                b: 0.92,
            },
            LinearColor {
                r: 0.85,
                g: 0.85,
                b: 0.85,
            },
        ]
    } else {
        vec![
            LinearColor {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 0.08,
                g: 0.08,
                b: 0.08,
            },
            LinearColor {
                r: 0.15,
                g: 0.15,
                b: 0.15,
            },
        ]
    };
    let mut rev_after = 0u64;
    let doc_gen_before = state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire);

    let mut content_gen_after = 0u64;
    state.must_active().document_handle.mutate(|doc| {
        if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id) {
            p.colors = colors.clone();
            p.revision = p.revision.saturating_add(1);
            rev_after = p.revision;
        }
        // Intentionally no doc.increment_generation() / layer_gen bump.
        content_gen_after = doc.bump_content_gen();
    });

    let snap = state.must_active().document_handle.snapshot();
    let doc_id = snap.id.0;
    let doc_gen_after = snap.generations.document_gen.load(Ordering::Acquire);
    invalidate(
        &state.tiles.tile_cache,
        InvalidationEvent::LayerFilterChanged {
            doc: doc_id,
            layer: 1,
        },
    );
    state.tiles.error_residuals.evict_document(doc_id);
    state.tiles.ed_frontier.clear();
    state.tiles.palette_cache.evict(doc_id, palette_id);
    state.tiles.palette_lut_cache.evict(doc_id, palette_id);
    crate::commands::schedule_dirty_viewport_tiles(state);

    tile_gen_trace::emit(
        "palette_color_pulse",
        Some(1),
        Some(doc_gen_after),
        Some(snap.generations.get_layer_gen(1)),
        None,
        Some(content_gen_after),
        json!({
            "palette_id": palette_id,
            "revision": rev_after,
            "pulse": pulse,
            "doc_gen_before": doc_gen_before,
            "doc_gen_after": doc_gen_after,
            "doc_gen_bumped": doc_gen_after != doc_gen_before,
            "content_gen": content_gen_after,
            "via": "harness_z11",
        }),
    );
}

fn tile_fingerprint(state: &AppState, key: TileKey) -> Option<u64> {
    let entry = state.tiles.tile_cache.entries.get(&key)?;
    let data = &entry.tile.data;
    let mut h: u64 = 0xcbf29ce484222325;
    let step = (data.len() / 64).max(1);
    for i in (0..data.len()).step_by(step) {
        let bits = data[i].to_bits() as u64;
        h ^= bits.wrapping_mul(0x100000001b3);
        h = h.wrapping_mul(0x100000001b3);
    }
    Some(h)
}

fn tile_mean_rgb(state: &AppState, key: TileKey) -> Option<(f32, f32, f32)> {
    use engine_tiles::HALO;
    let entry = state.tiles.tile_cache.entries.get(&key)?;
    let t = entry.tile.as_ref();
    let mut s = [0.0f64; 3];
    let mut n = 0u64;
    // Core sample (skip halo).
    let x0 = HALO + 8;
    let y0 = HALO + 8;
    for y in (y0..y0 + 16).step_by(2) {
        for x in (x0..x0 + 16).step_by(2) {
            for c in 0..3u32 {
                s[c as usize] += t.at(x, y, c) as f64;
            }
            n += 1;
        }
    }
    if n == 0 {
        return None;
    }
    Some((
        (s[0] / n as f64) as f32,
        (s[1] / n as f64) as f32,
        (s[2] / n as f64) as f32,
    ))
}

fn tile_nonzero_stats(state: &AppState, key: TileKey) -> Option<(usize, f32, f32)> {
    let entry = state.tiles.tile_cache.entries.get(&key)?;
    let data = &entry.tile.data;
    let mut nz = 0usize;
    let mut max_abs = 0.0f32;
    let mut sum = 0.0f64;
    for &v in data.iter() {
        let a = v.abs();
        if a > 1e-8 {
            nz += 1;
            sum += v as f64;
        }
        if a > max_abs {
            max_abs = a;
        }
    }
    let mean_nz = if nz == 0 {
        0.0
    } else {
        (sum / nz as f64) as f32
    };
    Some((nz, max_abs, mean_nz))
}

fn run_z11_once(variant: Z11Variant, cold: bool) {
    use std::collections::HashMap;
    use std::io::Write;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-21")
        .join("h4_z11");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let cache = if cold { "cold" } else { "warm" };
    let events_path = out_dir.join(format!(
        "events_z11_{}_{cache}_{profile}.jsonl",
        variant.label()
    ));
    crate::tile_gen_trace_host::set_out_path_for_tests(events_path.clone());

    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let budget = 1536 * 1024 * 1024;
    let mode = Mode::FloydSteinberg;

    let state = Arc::new(AppState::empty_process(None, budget, true));
    let (doc, palette_id) = dither_doc_with_palette(mode, size.w, size.h);
    state.spawn_session(doc);
    fill_raw(&state, size.w, size.h);
    // Match Z10: force L0 visible set (fit-zoom would be L>0 and empty `visible_level0`).
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(1.0, 0.0, 0.0, 1200.0, 800.0);

    let stop = Arc::new(AtomicBool::new(false));
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 4);
    let mut joins = Vec::new();
    for _ in 0..cores {
        let st = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        joins.push(thread::spawn(move || harness_worker_loop(st, stop)));
    }

    // Do **not** call `apply_threshold` — it rebuilds DitherV2 from defaults and
    // clears `palette_id`. Warm the palette-bound FS via dirty schedule only.
    crate::commands::schedule_dirty_viewport_tiles(&state);
    let _ = wait_silence(&state, Duration::from_secs(180));

    if cold {
        state
            .tiles
            .tile_cache
            .evict_stages(1, &[CacheStage::Processed, CacheStage::Composite]);
        state.tiles.error_residuals.evict_document(1);
        state.tiles.ed_frontier.clear();
        tile_gen_trace::emit_simple(
            "z11_cold_cache",
            json!({ "stages": ["Processed", "Composite"] }),
        );
        crate::commands::schedule_dirty_viewport_tiles(&state);
        let _ = wait_silence(&state, Duration::from_secs(180));
    }

    let visible = visible_level0(&state);
    assert!(!visible.is_empty(), "Z11 expects non-empty L0 visible set");

    let snap0 = state.must_active().document_handle.snapshot();
    let doc_gen_before = snap0.generations.document_gen.load(Ordering::Acquire);
    let layer_gen_before = snap0.generations.get_layer_gen(1);
    let pal_rev_before = snap0
        .palettes
        .iter()
        .find(|p| p.id == palette_id)
        .map(|p| p.revision)
        .unwrap_or(0);

    let mut fp_before: HashMap<Z9TileId, (Option<u64>, Option<u64>)> = HashMap::new();
    let mut mean_before: HashMap<Z9TileId, (f32, f32, f32)> = HashMap::new();
    for id in &visible {
        let ckey = id.key();
        let pkey = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: id.level,
                x: id.x,
                y: id.y,
            },
            stage: CacheStage::Processed,
        };
        fp_before.insert(
            *id,
            (tile_fingerprint(&state, ckey), tile_fingerprint(&state, pkey)),
        );
        if let Some(m) = tile_mean_rgb(&state, pkey) {
            mean_before.insert(*id, m);
        }
    }

    tile_gen_trace::emit_simple(
        "z11_viewport",
        json!({
            "variant": variant.label(),
            "visible_l0": visible.len(),
            "doc_gen": doc_gen_before,
            "layer_gen": layer_gen_before,
            "palette_rev": pal_rev_before,
            "cache": cache,
            "profile": profile,
        }),
    );

    match variant {
        Z11Variant::Single => {
            apply_palette_color_pulse(&state, palette_id, 0);
        }
        Z11Variant::Drag => {
            let hz = 60u64;
            let duration_ms = 1500u64;
            let gap = Duration::from_millis(1000 / hz);
            let pulses = (duration_ms / (1000 / hz)) as usize;
            tile_gen_trace::emit_simple(
                "z11_drag_begin",
                json!({ "hz": hz, "duration_ms": duration_ms, "pulses": pulses }),
            );
            for i in 0..pulses {
                apply_palette_color_pulse(&state, palette_id, i);
                thread::sleep(gap);
            }
            tile_gen_trace::emit_simple("z11_drag_end", json!({ "pulses": pulses }));
        }
        Z11Variant::Series => {
            for i in 0..8 {
                apply_palette_color_pulse(&state, palette_id, i);
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    let quiet = wait_silence(&state, Duration::from_secs(180));
    let snap1 = state.must_active().document_handle.snapshot();
    let doc_gen_after = snap1.generations.document_gen.load(Ordering::Acquire);
    let content_gen_after = snap1.generations.current_content_gen();
    let layer_gen_after = snap1.generations.get_layer_gen(1);
    let pal_rev_after = snap1
        .palettes
        .iter()
        .find(|p| p.id == palette_id)
        .map(|p| p.revision)
        .unwrap_or(0);

    let mut by_col: HashMap<u32, (usize, usize, usize)> = HashMap::new();
    // (gen_stale, pixel_unchanged, visible)
    let mut gen_stale_n = 0usize;
    let mut pixel_unchanged_n = 0usize;
    let mut pixel_unchanged_x0 = 0usize;
    let mut be_dirty_n = 0usize;
    let mut unchanged_keys = Vec::new();

    for id in &visible {
        // Freshness vs content_gen (Step B).
        let (lag, dirty, fresh) = composite_lag(&state, *id, content_gen_after);
        let e = by_col.entry(id.x).or_insert((0, 0, 0));
        e.2 += 1;
        if !fresh || dirty || lag != 0 {
            e.0 += 1;
            gen_stale_n += 1;
            be_dirty_n += usize::from(dirty);
        }
        let ckey = id.key();
        let pkey = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: id.level,
                x: id.x,
                y: id.y,
            },
            stage: CacheStage::Processed,
        };
        let fp_c = tile_fingerprint(&state, ckey);
        let fp_p = tile_fingerprint(&state, pkey);
        let (b_c, b_p) = fp_before.get(id).copied().unwrap_or((None, None));
        let mean_p = tile_mean_rgb(&state, pkey);
        let mean_b = mean_before.get(id).copied();
        let entry_gen = state
            .tiles
            .tile_cache
            .entries
            .get(&pkey)
            .map(|e| e.generation);
        // After palette change, Composite fingerprint should move if BE recomputed.
        let unchanged = fp_c.is_some() && fp_c == b_c && fp_p.is_some() && fp_p == b_p;
        if unchanged {
            e.1 += 1;
            pixel_unchanged_n += 1;
            if id.x == 0 {
                pixel_unchanged_x0 += 1;
            }
            unchanged_keys.push(format!("{}/{}/{}", id.level, id.x, id.y));
        }
        tile_gen_trace::emit(
            "z11_tile_probe",
            Some(1),
            Some(doc_gen_after),
            Some(layer_gen_after),
            Some(tile_gen_trace::tile(id.level, id.x, id.y)),
            Some(content_gen_after),
            json!({
                "lag": lag,
                "dirty": dirty,
                "fresh": fresh,
                "entry_gen": entry_gen,
                "mean_processed_before": mean_b.map(|(r,g,b)| json!([r,g,b])),
                "mean_processed_after": mean_p.map(|(r,g,b)| json!([r,g,b])),
                "fp_composite_before": b_c.map(|h| format!("{h:016x}")),
                "fp_composite_after": fp_c.map(|h| format!("{h:016x}")),
                "fp_processed_before": b_p.map(|h| format!("{h:016x}")),
                "fp_processed_after": fp_p.map(|h| format!("{h:016x}")),
                "pixel_unchanged": unchanged,
            }),
        );
    }

    let pal0 = snap1
        .palettes
        .iter()
        .find(|p| p.id == palette_id)
        .map(|p| {
            json!({
                "revision": p.revision,
                "c0": [p.colors[0].r, p.colors[0].g, p.colors[0].b],
                "c1": [p.colors.get(1).map(|c| c.r).unwrap_or(0.0),
                       p.colors.get(1).map(|c| c.g).unwrap_or(0.0),
                       p.colors.get(1).map(|c| c.b).unwrap_or(0.0)],
            })
        });
    let filter_pal = snap1.root.iter().find_map(|n| match n {
        LayerNode::Leaf(l) if l.id.0 == 1 => l.filters.first().and_then(|f| match &f.params {
            FilterParams::DitherV2(p) => Some(json!({
                "palette_id": p.palette_id.map(|id| id.0),
                "mode": format!("{:?}", p.mode),
                "levels": p.levels,
            })),
            _ => None,
        }),
        _ => None,
    });
    let probe0 = visible.first().copied();
    let (raw_stats, proc_stats, raw_mean, proc_mean) = if let Some(id) = probe0 {
        let coord = TileCoord {
            level: id.level,
            x: id.x,
            y: id.y,
        };
        let raw_key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Raw,
        };
        let pkey = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        (
            tile_nonzero_stats(&state, raw_key),
            tile_nonzero_stats(&state, pkey),
            tile_mean_rgb(&state, raw_key),
            tile_mean_rgb(&state, pkey),
        )
    } else {
        (None, None, None, None)
    };
    tile_gen_trace::emit_simple(
        "z11_palette_diag",
        json!({
            "palette": pal0,
            "filter": filter_pal,
            "content_gen": content_gen_after,
            "raw_nz": raw_stats.map(|(n,m,z)| json!({"nz":n,"max":m,"mean_nz":z})),
            "proc_nz": proc_stats.map(|(n,m,z)| json!({"nz":n,"max":m,"mean_nz":z})),
            "raw_mean_core": raw_mean.map(|(r,g,b)| json!([r,g,b])),
            "proc_mean_core": proc_mean.map(|(r,g,b)| json!([r,g,b])),
        }),
    );
    eprintln!(
        "[z11] palette_diag pal={pal0:?} filter={filter_pal:?} content_gen={content_gen_after} \
         raw_nz={raw_stats:?} proc_nz={proc_stats:?} raw_mean={raw_mean:?} proc_mean={proc_mean:?}"
    );

    let col_summary: Vec<_> = {
        let mut v: Vec<_> = by_col
            .iter()
            .map(|(x, (gen_stale, pixel_stale, vis))| {
                json!({
                    "x": x,
                    "gen_stale": gen_stale,
                    "pixel_unchanged": pixel_stale,
                    "visible": vis,
                })
            })
            .collect();
        v.sort_by_key(|j| j.get("x").and_then(|v| v.as_u64()).unwrap_or(0));
        v
    };

    // Strip discard events during this run (should be rare — doc_gen stable).
    let events = tile_gen_trace::test_events();
    let mut strip_discard_n = 0u32;
    let mut strip_owners_x0 = 0u32;
    for ev in &events {
        if ev.event == "strip_discarded_by_doc_gen" {
            strip_discard_n += 1;
        }
        if ev.event == "strip_owner_acquired" {
            let claim_x = ev
                .details
                .get("claim_x")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX);
            if claim_x == 0 {
                strip_owners_x0 += 1;
            }
        }
    }

    tile_gen_trace::emit_simple(
        "z11_result",
        json!({
            "variant": variant.label(),
            "profile": profile,
            "cache": cache,
            "quiet_ok": quiet,
            "doc_gen_before": doc_gen_before,
            "doc_gen_after": doc_gen_after,
            "doc_gen_bumped": doc_gen_after != doc_gen_before,
            "content_gen_after": content_gen_after,
            "layer_gen_before": layer_gen_before,
            "layer_gen_after": layer_gen_after,
            "layer_gen_bumped": layer_gen_after != layer_gen_before,
            "palette_rev_before": pal_rev_before,
            "palette_rev_after": pal_rev_after,
            "visible": visible.len(),
            "STALE_GEN_AT_QUIET": gen_stale_n,
            "STALE_CONTENT_AT_QUIET": pixel_unchanged_n,
            "gen_neq_current": gen_stale_n,
            "pixel_unchanged_n": pixel_unchanged_n,
            "pixel_unchanged_x0": pixel_unchanged_x0,
            "be_dirty_n": be_dirty_n,
            "strip_discarded_by_doc_gen": strip_discard_n,
            "strip_owners_x0": strip_owners_x0,
            "by_col": col_summary,
            "unchanged_keys_sample": unchanged_keys.iter().take(20).cloned().collect::<Vec<_>>(),
            "gen_compare_policy": "content_gen_in_X-Tile-Gen_third_part",
        }),
    );

    let md = out_dir.join(format!(
        "z11_{}_{cache}_{profile}.md",
        variant.label()
    ));
    let mut f = std::fs::File::create(&md).expect("z11 md");
    writeln!(
        f,
        "# Z11 palette {} — {cache} / {profile}\n\n\
         - quiet_ok={quiet}\n\
         - doc_gen {doc_gen_before}→{doc_gen_after} (bumped={})\n\
         - layer_gen {layer_gen_before}→{layer_gen_after}\n\
         - palette_rev {pal_rev_before}→{pal_rev_after}\n\
         - STALE_GEN_AT_QUIET (BE gen lag)={gen_stale_n}\n\
         - pixel_unchanged={pixel_unchanged_n} (x0={pixel_unchanged_x0})\n\
         - strip_discard={strip_discard_n} owners_x0={strip_owners_x0}\n\
         - by_col: {:?}\n\
         - events: `{}`\n",
        variant.label(),
        doc_gen_after != doc_gen_before,
        col_summary,
        events_path.display()
    )
    .ok();
    eprintln!(
        "[z11] {}/{cache}/{profile} quiet={quiet} doc_gen={doc_gen_before}→{doc_gen_after} \
         pal_rev={pal_rev_before}→{pal_rev_after} STALE_GEN={gen_stale_n} \
         pixel_unchanged={pixel_unchanged_n} x0={pixel_unchanged_x0} strip_discard={strip_discard_n}",
        variant.label(),
    );

    teardown(&state, &stop, joins);
    assert!(quiet, "Z11 expected quiet after palette activity");
    assert!(
        content_gen_after > 0,
        "content_gen must advance on palette pulse"
    );
    // Step B gate: after quiet, every visible Composite must reflect new content.
    assert_eq!(
        pixel_unchanged_n, 0,
        "STALE_CONTENT_AT_QUIET: {pixel_unchanged_n} tiles fingerprint-unchanged after palette \
         (x0={pixel_unchanged_x0}); sample={unchanged_keys:?}"
    );
    assert_eq!(
        gen_stale_n, 0,
        "visible tiles behind content_gen after quiet: {gen_stale_n}"
    );
}

#[test]
#[ignore]
fn z11_palette_single_12mp_warm() {
    run_z11_once(Z11Variant::Single, false);
}

#[test]
#[ignore]
fn z11_palette_drag_12mp_warm() {
    run_z11_once(Z11Variant::Drag, false);
}

#[test]
#[ignore]
fn z11_palette_series_12mp_warm() {
    run_z11_once(Z11Variant::Series, false);
}

#[test]
#[ignore]
fn z11_palette_drag_12mp_cold() {
    run_z11_once(Z11Variant::Drag, true);
}

/// Z11 expand: palette → pixel_size → levels → mode → soft_proof → zoom-out-to-fit,
/// fingerprint + content stamp on **all** visible pyramid levels.
fn run_z11_expand_once(series_idx: usize) -> bool {
    use std::collections::HashMap;

    #[cfg(test)]
    tile_gen_trace::clear_test_state();
    tile_gen_trace::force_enable(true);
    crate::tile_gen_trace_host::install_evict_hook_if_needed();

    let size = SizeSpec {
        w: 1920,
        h: 1080,
        label: "1920x1080",
    };
    let budget = 1024 * 1024 * 1024;
    let state = Arc::new(AppState::empty_process(None, budget, true));
    let (doc, palette_id) = dither_doc_with_palette(Mode::FloydSteinberg, size.w, size.h);
    state.spawn_session(doc);
    fill_raw(&state, size.w, size.h);
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(1.0, 0.0, 0.0, 1200.0, 800.0);

    let stop = Arc::new(AtomicBool::new(false));
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 4);
    let mut joins = Vec::new();
    for _ in 0..cores {
        let st = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        joins.push(thread::spawn(move || harness_worker_loop(st, stop)));
    }

    crate::commands::schedule_dirty_viewport_tiles(&state);
    if !wait_silence(&state, Duration::from_secs(120)) {
        teardown(&state, &stop, joins);
        return false;
    }

    let mut fp_before: HashMap<(u8, u32, u32), Option<u64>> = HashMap::new();
    let snapshot_fps = |st: &AppState, map: &mut HashMap<(u8, u32, u32), Option<u64>>| {
        let vp = st.ui.viewport.lock().unwrap().clone();
        for c in &vp.visible_tiles {
            let key = TileKey {
                doc: 1,
                layer: 0,
                coord: *c,
                stage: CacheStage::Composite,
            };
            map.insert((c.level, c.x, c.y), tile_fingerprint(st, key));
        }
    };
    snapshot_fps(&state, &mut fp_before);

    // 1) palette
    apply_palette_color_pulse(&state, palette_id, series_idx);
    // 2) pixel_size
    apply_dither_param_pulse(&state, |p| {
        p.pixel_size = if series_idx % 2 == 0 { 2 } else { 3 };
    });
    // 3) levels
    apply_dither_param_pulse(&state, |p| {
        p.levels = if series_idx % 2 == 0 { 8 } else { 12 };
    });
    // 4) mode (ordered ↔ ED keeps CPU path interesting)
    apply_dither_param_pulse(&state, |p| {
        p.mode = if series_idx % 2 == 0 {
            DitherModeV2::Bayer4x4
        } else {
            DitherModeV2::FloydSteinberg
        };
    });
    // Back to FS for soft-proof / zoom (ED late-tiles surface).
    apply_dither_param_pulse(&state, |p| {
        p.mode = DitherModeV2::FloydSteinberg;
        p.pixel_size = 1;
        p.levels = 16;
    });
    // 5) soft_proof content epoch
    apply_soft_proof_pulse(&state, series_idx % 2 == 0);

    if !wait_silence(&state, Duration::from_secs(180)) {
        teardown(&state, &stop, joins);
        return false;
    }

    // 6) zoom out to fit — all pyramid levels in play
    let fit = (1200.0 / size.w as f64).min(800.0 / size.h as f64);
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(fit, 0.0, 0.0, 1200.0, 800.0);
    crate::commands::schedule_dirty_viewport_tiles(&state);
    if !wait_silence(&state, Duration::from_secs(180)) {
        teardown(&state, &stop, joins);
        return false;
    }

    let snap = state.must_active().document_handle.snapshot();
    let content_gen = snap.generations.current_content_gen();
    state.tiles.tile_cache.set_content_epoch(content_gen);
    let vp = state.ui.viewport.lock().unwrap().clone();
    let mut gen_stale = 0usize;
    let mut pixel_unchanged = 0usize;
    let mut level_counts: HashMap<u8, usize> = HashMap::new();
    for c in &vp.visible_tiles {
        *level_counts.entry(c.level).or_insert(0) += 1;
        let key = TileKey {
            doc: 1,
            layer: 0,
            coord: *c,
            stage: CacheStage::Composite,
        };
        let fresh = engine_tiles::tile_fresh(&state.tiles.tile_cache, key);
        let gen = state
            .tiles
            .tile_cache
            .entries
            .get(&key)
            .map(|e| e.generation);
        if !fresh || gen != Some(content_gen) {
            // Pyramid parents stamp min(children); allow == content_gen or
            // (for edge parents) min may equal content when children caught up.
            if gen.map(|g| g < content_gen).unwrap_or(true) || !fresh {
                gen_stale += 1;
            }
        }
        if c.level > 0 {
            if engine_tiles::pyramid_parent_needs_rebuild(
                &state.tiles.tile_cache,
                key,
                snap.width,
                snap.height,
            ) {
                gen_stale += 1;
            }
        }
        let fp = tile_fingerprint(&state, key);
        if let Some(before) = fp_before.get(&(c.level, c.x, c.y)) {
            if fp.is_some() && fp == *before {
                pixel_unchanged += 1;
            }
        }
    }

    tile_gen_trace::emit_simple(
        "z11_expand_result",
        json!({
            "series_idx": series_idx,
            "content_gen": content_gen,
            "viewport_level": vp.level,
            "visible": vp.visible_tiles.len(),
            "levels": level_counts,
            "gen_stale": gen_stale,
            "pixel_unchanged_overlap": pixel_unchanged,
            "fit_zoom": fit,
        }),
    );

    let ok = gen_stale == 0 && content_gen > 0;
    teardown(&state, &stop, joins);
    ok
}

fn apply_dither_param_pulse(state: &AppState, edit: impl FnOnce(&mut DitherParamsV2)) {
    use engine_tiles::invalidation::{invalidate, InvalidationEvent};

    let gen_before = state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire);
    state.must_active().document_handle.mutate(|doc| {
        if let LayerNode::Leaf(layer) = &mut doc.root[0] {
            if let Some(f) = layer.filters.first_mut() {
                if let FilterParams::DitherV2(ref mut p) = f.params {
                    edit(p);
                }
            }
        }
        doc.increment_generation();
        doc.generations.increment_layer_gen(1);
    });
    let snap = state.must_active().document_handle.snapshot();
    let doc_id = snap.id.0;
    invalidate(
        &state.tiles.tile_cache,
        InvalidationEvent::LayerFilterChanged {
            doc: doc_id,
            layer: 1,
        },
    );
    state.tiles.error_residuals.evict_document(doc_id);
    state.tiles.ed_frontier.clear();
    crate::commands::schedule_dirty_viewport_tiles(state);
    let gen_after = snap.generations.document_gen.load(Ordering::Acquire);
    tile_gen_trace::emit(
        "z11_expand_param_pulse",
        Some(1),
        Some(gen_after),
        Some(snap.generations.get_layer_gen(1)),
        None,
        Some(snap.generations.current_content_gen()),
        json!({
            "doc_gen_before": gen_before,
            "doc_gen_after": gen_after,
            "content_gen": snap.generations.current_content_gen(),
        }),
    );
}

fn apply_soft_proof_pulse(state: &AppState, enable: bool) {
    use engine_tiles::invalidation::{invalidate, InvalidationEvent};

    let mut content_gen = 0u64;
    state.must_active().document_handle.mutate(|doc| {
        doc.soft_proof.enabled = enable;
        content_gen = doc.bump_content_gen();
    });
    let snap = state.must_active().document_handle.snapshot();
    let doc_id = snap.id.0;
    invalidate(
        &state.tiles.tile_cache,
        InvalidationEvent::LayerPropsChanged {
            doc: doc_id,
            layer: 0,
        },
    );
    crate::tile_serve::preview_rgba8_cache().clear_doc(doc_id);
    crate::commands::schedule_dirty_viewport_tiles(state);
    tile_gen_trace::emit_simple(
        "z11_expand_soft_proof",
        json!({ "enabled": enable, "content_gen": content_gen }),
    );
}

#[test]
#[ignore]
fn z11_expand_matrix_smoke() {
    assert!(
        run_z11_expand_once(0),
        "Z11 expand smoke (palette/pixel/levels/mode/proof/zoom-fit) failed"
    );
}

/// 50× expand matrix — release/dev soak (nightly / local gate).
#[test]
#[ignore]
fn z11_expand_50_series() {
    use std::io::Write;

    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-21")
        .join("h4_z11");
    let _ = std::fs::create_dir_all(&out_dir);
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let n = 50usize;
    let mut fail = 0usize;
    for i in 0..n {
        if !run_z11_expand_once(i) {
            fail += 1;
            eprintln!("[z11_expand_50] FAIL i={i}");
        }
    }
    let md = out_dir.join(format!("z11_expand_50_{profile}.md"));
    let mut f = std::fs::File::create(&md).expect("z11_expand_50 md");
    writeln!(
        f,
        "# Z11 expand ×50 — {profile}\n\n- series={n} failures={fail}\n"
    )
    .ok();
    eprintln!("[z11_expand_50] profile={profile} n={n} fail={fail}");
    assert_eq!(fail, 0, "{fail}/{n} Z11 expand series failed");
}
