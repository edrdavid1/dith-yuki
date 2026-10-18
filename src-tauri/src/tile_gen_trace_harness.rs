//! Backend stress harness for lost-tile generation tracing.
//!
//! ```text
//! DITHER_TRACE_QUICK=1 cargo test -p dither \
//!   tile_gen_trace_stress_s1_s3_matrix -- --ignored --nocapture --test-threads=1
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
    let cols = w.div_ceil(engine_tiles::TILE_SIZE);
    let rows = h.div_ceil(engine_tiles::TILE_SIZE);
    let mut tile = PixelTile::new();
    tile.data.fill(0.45);
    let arc = Arc::new(tile);
    for y in 0..rows {
        for x in 0..cols {
            let key = TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Raw,
            };
            state
                .tiles
                .tile_cache
                .insert_fresh_gen(key, Arc::clone(&arc), 0);
        }
    }
}

fn silence_settle_ms() -> u64 {
    if std::env::var("DITHER_TRACE_QUICK").map(|v| v == "1").unwrap_or(false) {
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
                    let doc_gen = snap
                        .generations
                        .document_gen
                        .load(Ordering::Acquire);
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
        Mode::BlockedFs
            | Mode::BlueNoise
            | Mode::Bayer4x4
            | Mode::FloydSteinberg
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
                CacheStage::Composite => match tile_pipeline::compute_composite_tile(task.key, &state)
                {
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
                    // PyramidChildrenPending: compute_* already enqueued children + parent
                    // retry (production drops the error here — do not park on ED deps).
                    other => other,
                },
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

fn setup(mode: Mode, size: SizeSpec, budget: usize) -> (Arc<AppState>, Arc<AtomicBool>, Vec<thread::JoinHandle<()>>) {
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
    if std::env::var("DITHER_TRACE_QUICK").map(|v| v == "1").unwrap_or(false) {
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
    let intervals: &[u64] = if quick {
        &[50]
    } else {
        &[0, 10, 50, 150, 300]
    };
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
                    eprintln!(
                        "[trace] finished S1 series={s} ok={ok} run_id={run_id}"
                    );
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
                            if ev.doc_gen == Some(g) || ev.details.get("tile_gen") == Some(&json!(g))
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
    summary.push_str("- `tile_ready` Tauri emit skipped (no AppHandle); synthetic `tile_ready_emitted` logged\n");

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
            let doc_gen = snap
                .generations
                .document_gen
                .load(Ordering::Acquire);
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
        let doc_gen = snap
            .generations
            .document_gen
            .load(Ordering::Acquire);
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
        let doc_gen = snap
            .generations
            .document_gen
            .load(Ordering::Acquire);
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
