//! STEP3b — measurement-only: L0 Composite nondeterminism after pixel_size change.
//!
//! Does **not** change product code. Uses harness + production-*shaped* worker
//! (mirrors `worker::tile_worker_loop` without Tauri AppHandle / tile-ready IPC).
//!
//! ```text
//! DITHER_GPU_PATH=cpu DITHER_JOURNAL=0 \
//!   cargo test -p dither --release step3b_ -- --ignored --nocapture --test-threads=1
//!
//! Env:
//!   DITHER_STEP3B_WORKERS=1|2|4|8   (default: available_parallelism clamped 1..8)
//!   DITHER_STEP3B_N=30              (frequency trials; default 30)
//!   DITHER_STEP3B_QUICK=1           → N=5, skip 6k, workers 1+8 only
//!   DITHER_STEP3B_COMPOSITE_IO=1    → log per-L0-Composite input/output hashes
//!   DITHER_STEP3B_WORKER=harness|prod  (default both in matrix tests)
//! ```

#![cfg(test)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use engine_project::filter::{DitherModeV2, DitherParamsV2};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterInstance, FilterKind, FilterParams};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};
use serde_json::json;

use crate::commands::{schedule_dirty_viewport_tiles, AppState};
use crate::services::viewport_service::ViewportService;
use crate::worker::task_is_stale;

#[derive(Clone, Copy)]
struct SizeSpec {
    w: u32,
    h: u32,
    label: &'static str,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WorkerKind {
    /// `second_interaction_harness::harness_worker_loop` shape (simplified).
    Harness,
    /// Mirrors `worker::tile_worker_loop` park/requeue (no AppHandle).
    ProdShaped,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ApplyKind {
    /// Clears strip gates (add_filter / STEP3 golden path).
    ClearGates,
    /// Omits `clear_strip_gates` (mirrors `run_preview_refresh`).
    ProdRefresh,
}

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../profiling/results/2026-10-22/second_interaction")
}

fn quick() -> bool {
    std::env::var("DITHER_STEP3B_QUICK").ok().as_deref() == Some("1")
}

fn workers_from_env(default_n: usize) -> usize {
    std::env::var("DITHER_STEP3B_WORKERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_n)
        .clamp(1, 8)
}

fn n_trials() -> usize {
    if quick() {
        return 5;
    }
    std::env::var("DITHER_STEP3B_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30)
}

fn composite_io_enabled() -> bool {
    std::env::var("DITHER_STEP3B_COMPOSITE_IO").ok().as_deref() == Some("1")
}

fn hash_tile_bytes(tile: &PixelTile) -> String {
    let mut h = blake3::Hasher::new();
    h.update(&(tile.data.len() as u64).to_le_bytes());
    for v in tile.data.iter() {
        h.update(&v.to_le_bytes());
    }
    h.finalize().to_hex().to_string()
}

fn hash_key_bytes(hasher: &mut blake3::Hasher, key: TileKey, tile: &PixelTile) {
    let stage_u8 = match key.stage {
        CacheStage::Raw => 0u8,
        CacheStage::Processed => 1u8,
        CacheStage::Composite => 2u8,
    };
    hasher.update(&[key.coord.level]);
    hasher.update(&key.coord.x.to_le_bytes());
    hasher.update(&key.coord.y.to_le_bytes());
    hasher.update(&[stage_u8]);
    hasher.update(&(tile.data.len() as u64).to_le_bytes());
    for v in tile.data.iter() {
        hasher.update(&v.to_le_bytes());
    }
}

fn visible_composite_key(coord: TileCoord) -> TileKey {
    TileKey {
        doc: 1,
        layer: 0,
        coord,
        stage: CacheStage::Composite,
    }
}

fn processed_key(coord: TileCoord) -> TileKey {
    TileKey {
        doc: 1,
        layer: 1,
        coord,
        stage: CacheStage::Processed,
    }
}

fn raw_key(coord: TileCoord) -> TileKey {
    TileKey {
        doc: 1,
        layer: 1,
        coord,
        stage: CacheStage::Raw,
    }
}

fn current_content_gen(state: &AppState) -> u64 {
    state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .current_content_gen()
}

fn sync_content_epoch(state: &AppState, content_gen: u64) {
    state.tiles.tile_cache.set_content_epoch(content_gen);
}

fn wait_silence(state: &AppState, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let busy = state.full_document_busy.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        let pending = state
            .pending_preview_refresh
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        if queued == 0 && inflight == 0 && !pending && busy == 0 && blocked == 0 {
            thread::sleep(Duration::from_millis(50));
            let queued2 = state.tiles.scheduler.queued_len();
            let inflight2 = state.preview_pass_inflight.load(Ordering::Acquire);
            let busy2 = state.full_document_busy.load(Ordering::Acquire);
            let blocked2 = state.tiles.ed_frontier.blocked_count();
            let pending2 = state
                .pending_preview_refresh
                .lock()
                .map(|g| g.is_some())
                .unwrap_or(false);
            if queued2 == 0 && inflight2 == 0 && !pending2 && busy2 == 0 && blocked2 == 0 {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn all_fresh_now(state: &AppState, content_gen: u64) -> bool {
    sync_content_epoch(state, content_gen);
    let vp = state.ui.viewport.lock().unwrap().clone();
    if vp.visible_tiles.is_empty() {
        return false;
    }
    vp.visible_tiles.iter().all(|coord| {
        engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, visible_composite_key(*coord))
            == Some(content_gen)
    })
}

fn wave_until_quiet(state: &AppState, content_gen: u64, quiet_cap: Duration) -> bool {
    sync_content_epoch(state, content_gen);
    let deadline = Instant::now() + quiet_cap;
    loop {
        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let busy = state.full_document_busy.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        let pending = state
            .pending_preview_refresh
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        if queued == 0 && inflight == 0 && !pending && busy == 0 && blocked == 0 {
            thread::sleep(Duration::from_millis(80));
            if state.tiles.scheduler.queued_len() == 0
                && state.preview_pass_inflight.load(Ordering::Acquire) == 0
                && state.full_document_busy.load(Ordering::Acquire) == 0
                && state.tiles.ed_frontier.blocked_count() == 0
            {
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let _ = wait_silence(state, Duration::from_millis(200));
    all_fresh_now(state, content_gen)
}

fn raw_doc(w: u32, h: u32) -> Document {
    let mut doc = Document::new(DocumentId::new(1), w, h);
    let layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
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
            state.tiles.tile_cache.insert_fresh(key, Arc::new(tile));
        }
    }
}

/// Global ring of Composite IO samples (cfg(test) harness only).
static COMPOSITE_IO_LOG: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());
static COMPOSITE_IO_SEQ: AtomicU64 = AtomicU64::new(0);
static EQUAL_GEN_PROC_FLIP: AtomicU64 = AtomicU64::new(0);

fn clear_io_log() {
    COMPOSITE_IO_LOG.lock().unwrap().clear();
    COMPOSITE_IO_SEQ.store(0, Ordering::Relaxed);
    EQUAL_GEN_PROC_FLIP.store(0, Ordering::Relaxed);
}

fn snapshot_proc_at(state: &AppState, coord: TileCoord) -> serde_json::Value {
    let key = processed_key(coord);
    let raw = raw_key(coord);
    match state.tiles.tile_cache.entries.get(&key) {
        Some(e) => {
            let dirty = e.dirty.load(Ordering::Acquire);
            let gen = e.generation;
            let hash = hash_tile_bytes(&e.tile);
            json!({
                "present": true,
                "gen": gen,
                "dirty": dirty,
                "hash": hash,
                "raw_fallback_would_apply": false,
            })
        }
        None => {
            let raw_hash = state
                .tiles
                .tile_cache
                .get_entry(raw)
                .map(|t| hash_tile_bytes(&t));
            json!({
                "present": false,
                "gen": null,
                "dirty": null,
                "hash": null,
                "raw_present": raw_hash.is_some(),
                "raw_hash": raw_hash,
                "raw_fallback_would_apply": raw_hash.is_some(),
            })
        }
    }
}

fn instrumented_compute_composite(
    state: &AppState,
    key: TileKey,
) -> Result<Arc<PixelTile>, engine_project::error::EngineError> {
    let io = composite_io_enabled() && key.coord.level == 0;
    let before = if io {
        Some(snapshot_proc_at(state, key.coord))
    } else {
        None
    };
    let content_gen = current_content_gen(state);
    let result = crate::tile_pipeline::compute_composite_tile(key, state);
    if let (Some(before), Ok(_)) = (before.as_ref(), result.as_ref()) {
        let after = snapshot_proc_at(state, key.coord);
        let comp_hash = state
            .tiles
            .tile_cache
            .get_entry(key)
            .map(|t| hash_tile_bytes(&t));
        let before_hash = before["hash"].as_str().map(|s| s.to_string());
        let after_hash = after["hash"].as_str().map(|s| s.to_string());
        let before_gen = before["gen"].as_u64();
        let after_gen = after["gen"].as_u64();
        let flipped = before_hash.is_some()
            && after_hash.is_some()
            && before_hash != after_hash
            && before_gen == after_gen
            && before_gen == Some(content_gen);
        if flipped {
            EQUAL_GEN_PROC_FLIP.fetch_add(1, Ordering::Relaxed);
        }
        let seq = COMPOSITE_IO_SEQ.fetch_add(1, Ordering::Relaxed);
        let row = json!({
            "seq": seq,
            "level": key.coord.level,
            "x": key.coord.x,
            "y": key.coord.y,
            "content_gen": content_gen,
            "proc_before": before,
            "proc_after": after,
            "composite_hash": comp_hash,
            "equal_gen_proc_flip": flipped,
            "input_ne_final_proc": flipped,
        });
        if let Ok(mut g) = COMPOSITE_IO_LOG.lock() {
            // Cap memory: keep last 4k samples.
            if g.len() > 4000 {
                let drain = g.len() - 2000;
                g.drain(0..drain);
            }
            g.push(row);
        }
    }
    result
}

/// Differences vs production `tile_worker_loop` are documented in the STEP3b report.
fn harness_worker_loop(state: Arc<AppState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        if let Some(task) = state.tiles.scheduler.dequeue() {
            state.preview_pass_inflight.fetch_add(1, Ordering::AcqRel);
            let snap = match state.session(task.key.doc) {
                Ok(s) => s.document_handle.snapshot(),
                Err(_) => {
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
            };
            if task_is_stale(&task, &snap) {
                state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                crate::commands::on_preview_task_finished(&state);
                continue;
            }
            match task.key.stage {
                CacheStage::Raw => {
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                }
                CacheStage::Processed => {
                    if crate::tile_pipeline::layer_has_error_diffusion(&snap.root, task.key.layer)
                        && task.key.coord.level == 0
                        && !engine_tiles::ed_ready(&state.tiles.tile_cache, task.key, true)
                    {
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
                    match crate::tile_pipeline::compute_processed_tile(task.key, &state) {
                        Err(engine_project::error::EngineError::EdStripBusy) => {
                            let deps =
                                crate::tile_pipeline::ed_strip_busy_park_deps(&state, task.key);
                            if deps.is_empty() {
                                state.tiles.scheduler.enqueue_or_bump(task);
                                state.worker_wake.notify_one();
                            } else {
                                let _ = state.tiles.ed_frontier.block_on(
                                    task,
                                    deps,
                                    &state.tiles.tile_cache,
                                );
                            }
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                        }
                        Ok(_) => {
                            crate::tile_pipeline::wake_ed_frontier_after_insert(&state, task.key);
                            crate::ed_progress::note_ed_pass_maybe_complete(&state, task.key.doc);
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                        }
                        Err(_) => {
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                        }
                    }
                }
                CacheStage::Composite => {
                    match instrumented_compute_composite(&state, task.key) {
                        Err(engine_project::error::EngineError::EdPrefixPending) => {
                            let deps = crate::tile_pipeline::composite_ed_park_deps(
                                &state,
                                &snap.root,
                                task.key.doc,
                                task.key.coord,
                            );
                            if deps.is_empty() {
                                // HARNESS DIFF: immediate requeue (prod uses delayed_requeue).
                                state.tiles.scheduler.enqueue_or_bump(task);
                                state.worker_wake.notify_one();
                            } else {
                                if let Some(ready) = state.tiles.ed_frontier.block_on(
                                    task,
                                    deps,
                                    &state.tiles.tile_cache,
                                ) {
                                    state.tiles.scheduler.enqueue_or_bump(ready);
                                    state.worker_wake.notify_one();
                                }
                            }
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                        }
                        Ok(_) => {
                            crate::tile_pipeline::wake_ed_frontier_after_insert(&state, task.key);
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                        }
                        Err(_) => {
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                        }
                    }
                }
            }
        } else {
            state.worker_wake.wait();
        }
    }
}

/// Production-shaped loop: aligned with `worker.rs` tile_worker_loop (no AppHandle).
fn prod_shaped_worker_loop(state: Arc<AppState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        if let Some(task) = state.tiles.scheduler.dequeue() {
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
                state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                crate::commands::on_preview_task_finished(&state);
                continue;
            }

            let result: Result<Arc<PixelTile>, engine_project::error::EngineError> =
                match task.key.stage {
                    CacheStage::Raw => state.tiles.tile_cache.get_entry(task.key).ok_or_else(|| {
                        engine_project::error::EngineError::invalid_state("raw missing")
                    }),
                    CacheStage::Processed => {
                        if task.key.coord.level > 0
                            && crate::tile_pipeline::layer_has_error_diffusion(
                                &snapshot.root,
                                task.key.layer,
                            )
                        {
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                            continue;
                        }
                        if crate::tile_pipeline::layer_has_error_diffusion(
                            &snapshot.root,
                            task.key.layer,
                        ) && !engine_tiles::ed_ready(&state.tiles.tile_cache, task.key, true)
                        {
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
                        match crate::tile_pipeline::compute_processed_tile(task.key, &state) {
                            Err(engine_project::error::EngineError::EdStripBusy) => {
                                let deps = crate::tile_pipeline::ed_strip_busy_park_deps(
                                    &state, task.key,
                                );
                                if deps.is_empty() {
                                    state.tiles.scheduler.enqueue_or_bump(task);
                                    state.worker_wake.notify_one();
                                } else {
                                    let _ = state.tiles.ed_frontier.block_on(
                                        task,
                                        deps,
                                        &state.tiles.tile_cache,
                                    );
                                }
                                state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                                crate::commands::on_preview_task_finished(&state);
                                continue;
                            }
                            other => other,
                        }
                    }
                    CacheStage::Composite => {
                        match instrumented_compute_composite(&state, task.key) {
                            Err(engine_project::error::EngineError::EdPrefixPending) => {
                                let attempt = crate::tile_gen_trace::bump_requeue_attempt(
                                    task.key.doc,
                                    task.key.coord.level,
                                    task.key.coord.x,
                                    task.key.coord.y,
                                    task.generation,
                                );
                                let deps = crate::tile_pipeline::composite_ed_park_deps(
                                    &state,
                                    &snapshot.root,
                                    task.key.doc,
                                    task.key.coord,
                                );
                                if deps.is_empty() {
                                    let backoff_ms = 1u64 << attempt.min(4);
                                    state.delayed_requeue.schedule(
                                        Arc::clone(&state),
                                        task,
                                        Duration::from_millis(backoff_ms),
                                    );
                                } else if let Some(ready) = state.tiles.ed_frontier.block_on(
                                    task,
                                    deps,
                                    &state.tiles.tile_cache,
                                ) {
                                    state.tiles.scheduler.enqueue_or_bump(ready);
                                    state.worker_wake.notify_one();
                                }
                                state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                                crate::commands::on_preview_task_finished(&state);
                                continue;
                            }
                            other => other,
                        }
                    }
                };

            if let Ok(_tile) = result {
                let snapshot = match state.session(task.key.doc) {
                    Ok(s) => s.document_handle.snapshot(),
                    Err(_) => {
                        state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                        crate::commands::on_preview_task_finished(&state);
                        continue;
                    }
                };
                if !task_is_stale(&task, &snapshot) {
                    match task.key.stage {
                        CacheStage::Raw => {}
                        CacheStage::Processed | CacheStage::Composite => {
                            crate::tile_gen_trace::clear_requeue_attempt(
                                task.key.doc,
                                task.key.coord.level,
                                task.key.coord.x,
                                task.key.coord.y,
                                task.generation,
                            );
                            if task.key.stage == CacheStage::Processed && task.key.coord.level == 0
                            {
                                crate::ed_progress::note_ed_pass_maybe_complete(
                                    &state,
                                    task.key.doc,
                                );
                            }
                            // Production: wake is inside compute_* insert paths;
                            // also wake here for frontier waiters (matches post-insert).
                            crate::tile_pipeline::wake_ed_frontier_after_insert(&state, task.key);
                        }
                    }
                }
            }
            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
            crate::commands::on_preview_task_finished(&state);
        } else {
            state.worker_wake.wait();
        }
    }
}

fn setup(
    size: SizeSpec,
    fit: bool,
    workers: usize,
    kind: WorkerKind,
) -> (Arc<AppState>, Arc<AtomicBool>, Vec<thread::JoinHandle<()>>) {
    let budget = 1024 * 1024 * 1024;
    // Avoid Metal adapter thrash across hundreds of trials (SIGKILL under load).
    let gpu = if std::env::var("DITHER_GPU_PATH").ok().as_deref() == Some("cpu") {
        None
    } else {
        engine_gpu::GpuContext::try_new_blocking().map(Arc::new)
    };
    let state = Arc::new(AppState::empty_process(gpu, budget, true));
    state.spawn_session(raw_doc(size.w, size.h));
    fill_raw(&state, size.w, size.h);

    let (zoom, cw, ch) = if fit {
        let cw = 717.0_f64;
        let ch = 725.0_f64;
        let zoom = (cw / size.w as f64).min(ch / size.h as f64).min(1.0);
        (zoom, cw, ch)
    } else {
        let zoom = (1200.0 / size.w as f64)
            .min(800.0 / size.h as f64)
            .min(1.0);
        (zoom, 1200.0, 800.0)
    };
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(zoom, 0.0, 0.0, cw, ch);

    let stop = Arc::new(AtomicBool::new(false));
    let mut joins = Vec::new();
    for _ in 0..workers {
        let st = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        joins.push(thread::spawn(move || match kind {
            WorkerKind::Harness => harness_worker_loop(st, stop),
            WorkerKind::ProdShaped => prod_shaped_worker_loop(st, stop),
        }));
    }
    let _ = wait_silence(&state, Duration::from_secs(60));
    (state, stop, joins)
}

fn teardown(state: &AppState, stop: &AtomicBool, joins: Vec<thread::JoinHandle<()>>) {
    stop.store(true, Ordering::Relaxed);
    for _ in 0..8 {
        state.worker_wake.notify_all();
    }
    // Join with timeout so abandoned Condvar waiters do not accumulate (SIGKILL).
    for j in joins {
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = j.join();
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }
}

fn apply_dither(
    state: &AppState,
    mode: DitherModeV2,
    pixel_size: u8,
    levels: u16,
    apply: ApplyKind,
) -> (bool, u64) {
    state.must_active().document_handle.mutate(|doc| {
        if let LayerNode::Leaf(layer) = &mut doc.root[0] {
            layer.filters.clear();
            layer.filters.push(FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: mode.clone(),
                    pixel_size,
                    levels,
                    ..DitherParamsV2::default()
                }),
            ));
        }
        doc.increment_generation();
    });
    state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .increment_layer_gen(1);
    let content_gen = current_content_gen(state);
    sync_content_epoch(state, content_gen);
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: 1 },
    );
    state
        .tiles
        .error_residuals
        .evict_layer(1, LayerId::new(1));
    state.tiles.block_representatives.clear_dithered();
    state.tiles.full_document.invalidate_layer(1, 1);
    if apply == ApplyKind::ClearGates {
        state.tiles.clear_strip_gates();
    }
    crate::ed_progress::note_ed_pass_start(state, 1);
    schedule_dirty_viewport_tiles(state);
    let ok = wave_until_quiet(state, content_gen, Duration::from_secs(180));
    (ok, content_gen)
}

fn digest_l0_processed(state: &AppState) -> (String, u32, u32) {
    use engine_tiles::TILE_SIZE;
    let snap = state.must_active().document_handle.snapshot();
    let content_gen = snap.generations.current_content_gen();
    sync_content_epoch(state, content_gen);
    let cols = snap.width.div_ceil(TILE_SIZE);
    let rows = snap.height.div_ceil(TILE_SIZE);
    let mut hasher = blake3::Hasher::new();
    let mut n = 0u32;
    let mut stale = 0u32;
    for y in 0..rows {
        for x in 0..cols {
            let key = processed_key(TileCoord { level: 0, x, y });
            if engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, key) != Some(content_gen)
            {
                stale += 1;
                continue;
            }
            if let Some(tile) = state.tiles.tile_cache.get_entry(key) {
                hash_key_bytes(&mut hasher, key, &tile);
                n += 1;
            } else {
                stale += 1;
            }
        }
    }
    (hasher.finalize().to_hex().to_string(), n, stale)
}

fn digest_l0_composites_under_visible(state: &AppState) -> (String, u32, u32) {
    let content_gen = current_content_gen(state);
    sync_content_epoch(state, content_gen);
    let vp = state.ui.viewport.lock().unwrap().clone();
    let mut keys = Vec::new();
    for c in &vp.visible_tiles {
        if c.level == 0 {
            keys.push(visible_composite_key(*c));
            continue;
        }
        let child_level = c.level - 1;
        for dy in 0..2u32 {
            for dx in 0..2u32 {
                keys.push(visible_composite_key(TileCoord {
                    level: child_level,
                    x: c.x * 2 + dx,
                    y: c.y * 2 + dy,
                }));
            }
        }
    }
    keys.sort_by_key(|k| (k.coord.level, k.coord.y, k.coord.x));
    keys.dedup();
    let mut hasher = blake3::Hasher::new();
    let mut n = 0u32;
    let mut stale = 0u32;
    for key in &keys {
        if engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, *key) != Some(content_gen) {
            stale += 1;
            continue;
        }
        if let Some(tile) = state.tiles.tile_cache.get_entry(*key) {
            hash_key_bytes(&mut hasher, *key, &tile);
            n += 1;
        } else {
            stale += 1;
        }
    }
    (hasher.finalize().to_hex().to_string(), n, stale)
}

fn digest_visible_composite(state: &AppState) -> (String, u32, u32) {
    let content_gen = current_content_gen(state);
    sync_content_epoch(state, content_gen);
    let vp = state.ui.viewport.lock().unwrap().clone();
    let mut keys: Vec<_> = vp
        .visible_tiles
        .iter()
        .map(|c| visible_composite_key(*c))
        .collect();
    keys.sort_by_key(|k| (k.coord.level, k.coord.y, k.coord.x));
    let mut hasher = blake3::Hasher::new();
    let mut n = 0u32;
    let mut stale = 0u32;
    for key in &keys {
        if engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, *key) != Some(content_gen) {
            stale += 1;
            continue;
        }
        if let Some(tile) = state.tiles.tile_cache.get_entry(*key) {
            hash_key_bytes(&mut hasher, *key, &tile);
            n += 1;
        } else {
            stale += 1;
        }
    }
    (hasher.finalize().to_hex().to_string(), n, stale)
}

fn hashes_now(state: &AppState) -> serde_json::Value {
    let (proc, proc_n, proc_stale) = digest_l0_processed(state);
    let (l0c, l0c_n, l0c_stale) = digest_l0_composites_under_visible(state);
    let (vis, vis_n, vis_stale) = digest_visible_composite(state);
    json!({
        "processed_l0": proc,
        "processed_l0_tiles": proc_n,
        "processed_l0_stale": proc_stale,
        "l0_composite": l0c,
        "l0_composite_tiles": l0c_n,
        "l0_composite_stale": l0c_stale,
        "visible_composite": vis,
        "visible_composite_tiles": vis_n,
        "visible_composite_stale": vis_stale,
        "content_gen": current_content_gen(state),
        "equal_gen_proc_flips": EQUAL_GEN_PROC_FLIP.load(Ordering::Relaxed),
    })
}

fn reschedule_visible(state: &AppState) {
    schedule_dirty_viewport_tiles(state);
}

/// One transition trial: optional prelude → target params → quiet → hashes → delayed rehash.
fn trial_transition(
    size: SizeSpec,
    fit: bool,
    workers: usize,
    kind: WorkerKind,
    apply: ApplyKind,
    prelude: Option<(DitherModeV2, u8, u16)>,
    target: (DitherModeV2, u8, u16),
    converge: bool,
) -> serde_json::Value {
    clear_io_log();
    let (state, stop, joins) = setup(size, fit, workers, kind);
    if let Some((m, ps, lv)) = prelude {
        let _ = apply_dither(&state, m, ps, lv, apply);
    }
    let (ok, _gen) = apply_dither(&state, target.0.clone(), target.1, target.2, apply);
    let h0 = hashes_now(&state);
    let mut delayed = Vec::new();
    if converge {
        for ms in [500u64, 2000, 5000] {
            thread::sleep(Duration::from_millis(ms));
            // Re-request visibles without invalidation — schedule only if dirty.
            reschedule_visible(&state);
            let _ = wait_silence(&state, Duration::from_secs(30));
            let h = hashes_now(&state);
            delayed.push(json!({
                "wait_ms": ms,
                "l0_composite": h["l0_composite"],
                "visible_composite": h["visible_composite"],
                "processed_l0": h["processed_l0"],
                "same_l0c_as_t0": h["l0_composite"] == h0["l0_composite"],
                "same_vis_as_t0": h["visible_composite"] == h0["visible_composite"],
            }));
        }
    }
    let flips = EQUAL_GEN_PROC_FLIP.load(Ordering::Relaxed);
    let io_samples = COMPOSITE_IO_LOG.lock().unwrap().len();
    let io_flip_rows: Vec<_> = COMPOSITE_IO_LOG
        .lock()
        .unwrap()
        .iter()
        .filter(|r| r["equal_gen_proc_flip"].as_bool() == Some(true))
        .take(8)
        .cloned()
        .collect();
    teardown(&state, &stop, joins);
    json!({
        "ok": ok,
        "hashes_t0": h0,
        "converge": delayed,
        "equal_gen_proc_flips": flips,
        "io_samples": io_samples,
        "io_flip_examples": io_flip_rows,
    })
}

/// Establish reference hash: cold target ×3 on 1 session; use last hash if stable.
fn cold_reference(
    size: SizeSpec,
    fit: bool,
    workers: usize,
    kind: WorkerKind,
    apply: ApplyKind,
    target: (DitherModeV2, u8, u16),
) -> Option<(String, String, String)> {
    let (state, stop, joins) = setup(size, fit, workers, kind);
    let mut last = None;
    for _ in 0..3 {
        let _ = apply_dither(&state, target.0.clone(), target.1, target.2, apply);
        let h = hashes_now(&state);
        let cur = (
            h["processed_l0"].as_str()?.to_string(),
            h["l0_composite"].as_str()?.to_string(),
            h["visible_composite"].as_str()?.to_string(),
        );
        if let Some(prev) = &last {
            if prev != &cur {
                teardown(&state, &stop, joins);
                return None; // cold itself unstable
            }
        }
        last = Some(cur);
    }
    teardown(&state, &stop, joins);
    last
}

/// Frequency of L1-visible mismatch with stable L0 Composite (pyramid stale parent).
///
/// One worker pool per cell (avoids setup thrash / SIGKILL). Env filters:
///   DITHER_STEP3B_KIND=Harness|ProdShaped
///   DITHER_STEP3B_WORKERS=1|2|4|8
///   DITHER_STEP3B_SIZE=4000|6k
#[test]
#[ignore]
fn step3b_l1_mismatch_frequency() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    let n = n_trials();
    let kind_filter = std::env::var("DITHER_STEP3B_KIND").ok();
    let workers_filter = std::env::var("DITHER_STEP3B_WORKERS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok());
    let size_filter = std::env::var("DITHER_STEP3B_SIZE").ok();

    let worker_sets: Vec<usize> = if let Some(w) = workers_filter {
        vec![w.clamp(1, 8)]
    } else if quick() {
        vec![1, 8]
    } else {
        vec![1, 2, 4, 8]
    };
    let kinds: Vec<WorkerKind> = match kind_filter.as_deref() {
        Some("Harness") => vec![WorkerKind::Harness],
        Some("ProdShaped") => vec![WorkerKind::ProdShaped],
        _ => vec![WorkerKind::Harness, WorkerKind::ProdShaped],
    };
    let sizes: Vec<(SizeSpec, bool)> = match size_filter.as_deref() {
        Some("6k") => vec![(
            SizeSpec {
                w: 6016,
                h: 4352,
                label: "6016x4352",
            },
            true,
        )],
        Some("4000") => vec![(
            SizeSpec {
                w: 4000,
                h: 3000,
                label: "4000x3000",
            },
            false,
        )],
        _ if quick() => vec![(
            SizeSpec {
                w: 4000,
                h: 3000,
                label: "4000x3000",
            },
            false,
        )],
        _ => vec![
            (
                SizeSpec {
                    w: 4000,
                    h: 3000,
                    label: "4000x3000",
                },
                false,
            ),
            (
                SizeSpec {
                    w: 6016,
                    h: 4352,
                    label: "6016x4352",
                },
                true,
            ),
        ],
    };
    let mode = DitherModeV2::FloydSteinberg;

    // Merge into existing JSON so per-cell shell runs accumulate.
    let path = out_dir().join("step3b_l1_frequency.json");
    let mut report = if path.exists() {
        serde_json::from_slice(&std::fs::read(&path).unwrap_or_default())
            .unwrap_or_else(|_| json!({ "rows": [], "n": n }))
    } else {
        json!({ "rows": [], "n": n })
    };
    if report.get("rows").is_none() {
        report["rows"] = json!([]);
    }

    for &(size, fit) in &sizes {
        for &kind in &kinds {
            for &workers in &worker_sets {
                let mut l1_mismatch = 0usize;
                let mut l0_mismatch = 0usize;
                let mut ok_n = 0usize;
                let mut persist_b = 0usize;
                let mut heal_a = 0usize;

                // One pool for the whole cell.
                let (state, stop, joins) = setup(size, fit, workers, kind);
                let _ = apply_dither(&state, mode.clone(), 8, 16, ApplyKind::ClearGates);
                let _ = apply_dither(&state, mode.clone(), 8, 16, ApplyKind::ClearGates);
                let href = hashes_now(&state);
                let ref_l0 = href["l0_composite"].as_str().unwrap_or("").to_string();
                let ref_vis = href["visible_composite"].as_str().unwrap_or("").to_string();
                let ref_proc = href["processed_l0"].as_str().unwrap_or("").to_string();

                for trial in 0..n {
                    let reps = if trial % 3 == 0 { 3 } else { 1 };
                    for _ in 0..reps {
                        let _ = apply_dither(&state, mode.clone(), 1, 16, ApplyKind::ClearGates);
                    }
                    let (ok, _) =
                        apply_dither(&state, mode.clone(), 8, 16, ApplyKind::ClearGates);
                    if !ok {
                        continue;
                    }
                    ok_n += 1;
                    let h0 = hashes_now(&state);
                    let proc = h0["processed_l0"].as_str().unwrap_or("");
                    let l0c = h0["l0_composite"].as_str().unwrap_or("");
                    let vis = h0["visible_composite"].as_str().unwrap_or("");
                    let l0_bad = proc == ref_proc && l0c != ref_l0;
                    let l1_bad = proc == ref_proc && l0c == ref_l0 && vis != ref_vis;
                    if l0_bad {
                        l0_mismatch += 1;
                    }
                    if l1_bad {
                        l1_mismatch += 1;
                    }
                    if l0_bad || l1_bad {
                        let bad0 = (
                            h0["l0_composite"].clone(),
                            h0["visible_composite"].clone(),
                        );
                        thread::sleep(Duration::from_millis(500));
                        reschedule_visible(&state);
                        let _ = wait_silence(&state, Duration::from_secs(30));
                        thread::sleep(Duration::from_millis(2000));
                        reschedule_visible(&state);
                        let _ = wait_silence(&state, Duration::from_secs(30));
                        let h2 = hashes_now(&state);
                        let healed = h2["visible_composite"].as_str() == Some(ref_vis.as_str())
                            && h2["l0_composite"].as_str() == Some(ref_l0.as_str());
                        if healed {
                            heal_a += 1;
                        } else if h2["visible_composite"] == bad0.1 && h2["l0_composite"] == bad0.0
                        {
                            persist_b += 1;
                        } else {
                            heal_a += 1;
                        }
                    }
                    eprintln!(
                        "[step3b_l1] {} {:?} w={workers} trial={}/{} l0_bad={l0_bad} l1_bad={l1_bad} vis={}…",
                        size.label,
                        kind,
                        trial + 1,
                        n,
                        &vis[..vis.len().min(16)],
                    );
                }
                teardown(&state, &stop, joins);

                let row = json!({
                    "size": size.label,
                    "fit": fit,
                    "worker_kind": format!("{kind:?}"),
                    "workers": workers,
                    "n": n,
                    "ok_n": ok_n,
                    "l0_mismatch_rate": if ok_n==0 {0.0} else {l0_mismatch as f64 / ok_n as f64},
                    "l1_only_mismatch_rate": if ok_n==0 {0.0} else {l1_mismatch as f64 / ok_n as f64},
                    "l0_mismatch": l0_mismatch,
                    "l1_only_mismatch": l1_mismatch,
                    "on_mismatch_healed_a": heal_a,
                    "on_mismatch_persist_b": persist_b,
                    "session": "reuse_pool_per_cell",
                    "reference": {
                        "processed_l0": ref_proc,
                        "l0_composite": ref_l0,
                        "visible_composite": ref_vis,
                    },
                });
                // Replace prior row for same cell if re-run.
                if let Some(arr) = report["rows"].as_array_mut() {
                    arr.retain(|r| {
                        !(r["size"] == row["size"]
                            && r["worker_kind"] == row["worker_kind"]
                            && r["workers"] == row["workers"])
                    });
                    arr.push(row);
                }
                let _ = std::fs::create_dir_all(out_dir());
                let _ = std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap_or_default());
                eprintln!(
                    "[step3b_l1] cell done {} {:?} w={workers} l1_rate={:.3}",
                    size.label,
                    kind,
                    if ok_n == 0 {
                        0.0
                    } else {
                        l1_mismatch as f64 / ok_n as f64
                    }
                );
            }
        }
    }

    eprintln!("[step3b_l1] wrote {}", path.display());
}

/// Minimal scenario set (ProdShaped, w=8, 4000): which transitions flake?
#[test]
#[ignore]
fn step3b_scenario_minimal_set() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    let scen_n = if quick() { 8 } else { 20 };
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let workers = 8;
    let kind = WorkerKind::ProdShaped;
    let scenarios: Vec<(&str, Option<(DitherModeV2, u8, u16)>, (DitherModeV2, u8, u16))> = vec![
        (
            "ps_1_to_8",
            Some((DitherModeV2::FloydSteinberg, 1, 16)),
            (DitherModeV2::FloydSteinberg, 8, 16),
        ),
        (
            "ps_8_to_9",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::FloydSteinberg, 9, 16),
        ),
        (
            "levels_16_to_32",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::FloydSteinberg, 8, 32),
        ),
        (
            "mode_fs_to_bayer",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::Bayer4x4, 8, 16),
        ),
        (
            "same_params_recompute",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::FloydSteinberg, 8, 16),
        ),
    ];
    let mut out = json!({ "scenarios": [], "n": scen_n, "workers": workers });
    for (name, prelude, target) in scenarios {
        let (state, stop, joins) = setup(size, false, workers, kind);
        // cold reference for target
        let _ = apply_dither(
            &state,
            target.0.clone(),
            target.1,
            target.2,
            ApplyKind::ClearGates,
        );
        let _ = apply_dither(
            &state,
            target.0.clone(),
            target.1,
            target.2,
            ApplyKind::ClearGates,
        );
        let href = hashes_now(&state);
        let ref_l0 = href["l0_composite"].as_str().unwrap_or("").to_string();
        let ref_vis = href["visible_composite"].as_str().unwrap_or("").to_string();
        let ref_proc = href["processed_l0"].as_str().unwrap_or("").to_string();

        let mut l1 = 0usize;
        let mut l0 = 0usize;
        let mut ok_n = 0usize;
        for i in 0..scen_n {
            if let Some((m, ps, lv)) = &prelude {
                let _ = apply_dither(&state, m.clone(), *ps, *lv, ApplyKind::ClearGates);
            }
            let (ok, _) = apply_dither(
                &state,
                target.0.clone(),
                target.1,
                target.2,
                ApplyKind::ClearGates,
            );
            if !ok {
                continue;
            }
            ok_n += 1;
            let h = hashes_now(&state);
            let proc = h["processed_l0"].as_str().unwrap_or("");
            let l0c = h["l0_composite"].as_str().unwrap_or("");
            let vis = h["visible_composite"].as_str().unwrap_or("");
            if proc == ref_proc && l0c != ref_l0 {
                l0 += 1;
            }
            if proc == ref_proc && l0c == ref_l0 && vis != ref_vis {
                l1 += 1;
            }
            eprintln!(
                "[step3b_scen] {name} {i}/{scen_n} l0_bad={} l1_bad={} vis={}…",
                proc == ref_proc && l0c != ref_l0,
                proc == ref_proc && l0c == ref_l0 && vis != ref_vis,
                &vis[..vis.len().min(16)]
            );
        }
        teardown(&state, &stop, joins);
        out["scenarios"].as_array_mut().unwrap().push(json!({
            "name": name,
            "ok_n": ok_n,
            "l0_mismatch": l0,
            "l1_only_mismatch": l1,
            "l1_rate": if ok_n==0 {0.0} else {l1 as f64 / ok_n as f64},
            "l0_rate": if ok_n==0 {0.0} else {l0 as f64 / ok_n as f64},
        }));
    }
    let path = out_dir().join("step3b_scenarios.json");
    let _ = std::fs::write(&path, serde_json::to_vec_pretty(&out).unwrap_or_default());
    eprintln!("[step3b_scen] wrote {}", path.display());
}

#[test]
#[ignore]
fn step3b_worker_diff_and_frequency_matrix() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    std::env::set_var("DITHER_STEP3B_COMPOSITE_IO", "1");

    let n = n_trials();
    let worker_sets: Vec<usize> = if quick() {
        vec![1, 8]
    } else {
        vec![1, 2, 4, 8]
    };
    let sizes: Vec<(SizeSpec, bool)> = if quick() {
        vec![(
            SizeSpec {
                w: 4000,
                h: 3000,
                label: "4000x3000",
            },
            false,
        )]
    } else {
        vec![
            (
                SizeSpec {
                    w: 4000,
                    h: 3000,
                    label: "4000x3000",
                },
                false,
            ),
            (
                SizeSpec {
                    w: 6016,
                    h: 4352,
                    label: "6016x4352",
                },
                true,
            ),
        ]
    };

    let mode = DitherModeV2::FloydSteinberg;
    let target = (mode.clone(), 8u8, 16u16);
    let mut report = json!({
        "date": "2026-10-22",
        "gpu_path": "cpu",
        "n_trials": n,
        "rows": [],
        "scenarios": [],
    });

    for &(size, fit) in &sizes {
        for &kind in &[WorkerKind::Harness, WorkerKind::ProdShaped] {
            for &apply in &[ApplyKind::ClearGates, ApplyKind::ProdRefresh] {
                // Reference once per (size, kind, apply) at workers=max available for stability.
                let ref_w = workers_from_env(4);
                let reference = cold_reference(size, fit, ref_w, kind, apply, target.clone());
                eprintln!(
                    "[step3b] ref size={} fit={fit} kind={kind:?} apply={apply:?} ref={:?}",
                    size.label,
                    reference
                        .as_ref()
                        .map(|(p, l0, _)| format!("proc={}… l0c={}…", &p[..16], &l0[..16]))
                );

                for &workers in &worker_sets {
                    let mut mismatch = 0usize;
                    let mut ok_n = 0usize;
                    let mut converge_a = 0usize; // changed after wait
                    let mut converge_b = 0usize; // stayed wrong/same
                    let mut flip_trials = 0usize;
                    let mut first_mismatch_hashes = None;
                    for trial in 0..n {
                        let row = trial_transition(
                            size,
                            fit,
                            workers,
                            kind,
                            apply,
                            Some((mode.clone(), 1, 16)),
                            target.clone(),
                            trial == 0, // full converge probe on first trial only
                        );
                        if !row["ok"].as_bool().unwrap_or(false) {
                            continue;
                        }
                        ok_n += 1;
                        let l0c = row["hashes_t0"]["l0_composite"].as_str().unwrap_or("");
                        let proc = row["hashes_t0"]["processed_l0"].as_str().unwrap_or("");
                        let flips = row["equal_gen_proc_flips"].as_u64().unwrap_or(0);
                        if flips > 0 {
                            flip_trials += 1;
                        }
                        let is_mismatch = match &reference {
                            Some((rp, rl0, _)) => proc == rp.as_str() && l0c != rl0.as_str(),
                            None => false,
                        };
                        // Also treat as mismatch if cold ref missing: compare to mode later.
                        if is_mismatch {
                            mismatch += 1;
                            if first_mismatch_hashes.is_none() {
                                first_mismatch_hashes = Some(row["hashes_t0"].clone());
                            }
                        }
                        if trial == 0 {
                            let stayed = row["converge"]
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .all(|c| c["same_l0c_as_t0"].as_bool().unwrap_or(true))
                                })
                                .unwrap_or(true);
                            if is_mismatch && !stayed {
                                converge_a += 1;
                            } else if is_mismatch && stayed {
                                converge_b += 1;
                            } else if !stayed {
                                converge_a += 1; // hash moved without mismatch-vs-ref
                            } else {
                                converge_b += 1;
                            }
                        }
                        eprintln!(
                            "[step3b] {} {:?} {:?} w={workers} trial={}/{} mismatch={} flips={} l0c={}…",
                            size.label,
                            kind,
                            apply,
                            trial + 1,
                            n,
                            is_mismatch,
                            flips,
                            &l0c[..l0c.len().min(16)],
                        );
                    }
                    let rate = if ok_n == 0 {
                        0.0
                    } else {
                        mismatch as f64 / ok_n as f64
                    };
                    report["rows"].as_array_mut().unwrap().push(json!({
                        "size": size.label,
                        "fit": fit,
                        "worker_kind": format!("{kind:?}"),
                        "apply": format!("{apply:?}"),
                        "workers": workers,
                        "n": n,
                        "ok_n": ok_n,
                        "mismatch_vs_cold_ref": mismatch,
                        "discrepancy_rate": rate,
                        "trials_with_equal_gen_proc_flip": flip_trials,
                        "converge_probe": {
                            "note": "first trial only; a=hash changed after wait, b=unchanged",
                            "a_changed": converge_a,
                            "b_unchanged": converge_b,
                        },
                        "reference": reference.as_ref().map(|(p,l0,v)| json!({
                            "processed_l0": p,
                            "l0_composite": l0,
                            "visible_composite": v,
                        })),
                        "first_mismatch_hashes": first_mismatch_hashes,
                    }));
                }
            }
        }
    }

    // Minimal reproducing set (prod-shaped, clear-gates, workers=8, 4000).
    let size4 = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let scen_n = if quick() { 5 } else { 15 };
    let scenarios: Vec<(&str, Option<(DitherModeV2, u8, u16)>, (DitherModeV2, u8, u16))> = vec![
        (
            "ps_1_to_8",
            Some((DitherModeV2::FloydSteinberg, 1, 16)),
            (DitherModeV2::FloydSteinberg, 8, 16),
        ),
        (
            "ps_8_to_9",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::FloydSteinberg, 9, 16),
        ),
        (
            "levels_16_to_32",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::FloydSteinberg, 8, 32),
        ),
        (
            "mode_fs_to_bayer",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::Bayer4x4, 8, 16),
        ),
        (
            "same_params_recompute",
            Some((DitherModeV2::FloydSteinberg, 8, 16)),
            (DitherModeV2::FloydSteinberg, 8, 16),
        ),
    ];
    for (name, prelude, target) in scenarios {
        let ref_h = cold_reference(
            size4,
            false,
            8,
            WorkerKind::ProdShaped,
            ApplyKind::ClearGates,
            target.clone(),
        );
        let mut mismatch = 0usize;
        let mut ok_n = 0usize;
        for _ in 0..scen_n {
            let row = trial_transition(
                size4,
                false,
                8,
                WorkerKind::ProdShaped,
                ApplyKind::ClearGates,
                prelude.clone(),
                target.clone(),
                false,
            );
            if !row["ok"].as_bool().unwrap_or(false) {
                continue;
            }
            ok_n += 1;
            let l0c = row["hashes_t0"]["l0_composite"].as_str().unwrap_or("");
            let proc = row["hashes_t0"]["processed_l0"].as_str().unwrap_or("");
            if let Some((rp, rl0, _)) = &ref_h {
                if proc == rp && l0c != rl0 {
                    mismatch += 1;
                }
            }
        }
        report["scenarios"].as_array_mut().unwrap().push(json!({
            "name": name,
            "n": scen_n,
            "ok_n": ok_n,
            "mismatch_vs_cold_ref": mismatch,
            "rate": if ok_n==0 { 0.0 } else { mismatch as f64 / ok_n as f64 },
            "reference_ok": ref_h.is_some(),
        }));
        eprintln!(
            "[step3b] scenario={name} mismatch={mismatch}/{ok_n} ref_ok={}",
            ref_h.is_some()
        );
    }

    let path = out_dir().join("step3b_matrix.json");
    let _ = std::fs::create_dir_all(out_dir());
    let _ = std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap_or_default());
    eprintln!("[step3b] wrote {}", path.display());
}

/// Same-session: ps=1 once, then ps=8 × N without teardown (matches STEP3 diag shape).
#[test]
#[ignore]
fn step3b_same_session_ps1_then_ps8_x30() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    std::env::set_var("DITHER_STEP3B_COMPOSITE_IO", "1");
    let n = if quick() { 10 } else { 30 };
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let mode = DitherModeV2::FloydSteinberg;
    let mut report = json!({ "rows": [] });

    for &(kind, apply) in &[
        (WorkerKind::Harness, ApplyKind::ClearGates),
        (WorkerKind::ProdShaped, ApplyKind::ClearGates),
        (WorkerKind::ProdShaped, ApplyKind::ProdRefresh),
        (WorkerKind::Harness, ApplyKind::ProdRefresh),
    ] {
        clear_io_log();
        let workers = 8;
        let (state, stop, joins) = setup(size, false, workers, kind);
        let _ = apply_dither(&state, mode.clone(), 1, 16, apply);
        let mut hashes = Vec::new();
        for i in 1..=n {
            let (ok, _) = apply_dither(&state, mode.clone(), 8, 16, apply);
            let h = hashes_now(&state);
            // converge probe on first only
            let mut converge = json!(null);
            if i == 1 {
                let h0 = h.clone();
                let mut delays = Vec::new();
                for ms in [500u64, 2000, 5000] {
                    thread::sleep(Duration::from_millis(ms));
                    reschedule_visible(&state);
                    let _ = wait_silence(&state, Duration::from_secs(30));
                    let hx = hashes_now(&state);
                    delays.push(json!({
                        "wait_ms": ms,
                        "same_l0c": hx["l0_composite"] == h0["l0_composite"],
                        "l0_composite": hx["l0_composite"],
                    }));
                }
                converge = json!(delays);
            }
            eprintln!(
                "[step3b_same] {:?} {:?} run={i}/{n} ok={ok} flips={} l0c={}…",
                kind,
                apply,
                EQUAL_GEN_PROC_FLIP.load(Ordering::Relaxed),
                h["l0_composite"].as_str().unwrap_or("")[..16].to_string(),
            );
            hashes.push(json!({
                "run": i,
                "ok": ok,
                "hashes": h,
                "converge": converge,
                "flips_cumulative": EQUAL_GEN_PROC_FLIP.load(Ordering::Relaxed),
            }));
        }
        let l0_set: std::collections::BTreeSet<String> = hashes
            .iter()
            .filter_map(|r| r["hashes"]["l0_composite"].as_str().map(|s| s.to_string()))
            .collect();
        let proc_set: std::collections::BTreeSet<String> = hashes
            .iter()
            .filter_map(|r| r["hashes"]["processed_l0"].as_str().map(|s| s.to_string()))
            .collect();
        teardown(&state, &stop, joins);
        report["rows"].as_array_mut().unwrap().push(json!({
            "worker_kind": format!("{kind:?}"),
            "apply": format!("{apply:?}"),
            "workers": workers,
            "n": n,
            "distinct_l0_composite": l0_set.len(),
            "distinct_processed_l0": proc_set.len(),
            "l0_hashes": l0_set,
            "proc_stable": proc_set.len() <= 1,
            "equal_gen_proc_flips": EQUAL_GEN_PROC_FLIP.load(Ordering::Relaxed),
            "runs": hashes,
        }));
    }

    let path = out_dir().join("step3b_same_session.json");
    let _ = std::fs::write(&path, serde_json::to_vec_pretty(&report).unwrap_or_default());
    eprintln!("[step3b_same] wrote {}", path.display());
}

#[test]
#[ignore]
fn step3b_converge_and_io_proof() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    std::env::set_var("DITHER_STEP3B_COMPOSITE_IO", "1");

    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let mode = DitherModeV2::FloydSteinberg;
    let target = (mode.clone(), 8u8, 16u16);
    let reference = cold_reference(
        size,
        false,
        8,
        WorkerKind::ProdShaped,
        ApplyKind::ClearGates,
        target.clone(),
    );
    eprintln!("[step3b_io] cold_ref={:?}", reference.as_ref().map(|(_, l0, _)| &l0[..16]));

    // Keep trying until we catch a mismatch or hit 20 attempts.
    let mut caught = None;
    for attempt in 1..=20 {
        let row = trial_transition(
            size,
            false,
            8,
            WorkerKind::ProdShaped,
            ApplyKind::ClearGates,
            Some((mode.clone(), 1, 16)),
            target.clone(),
            true,
        );
        let l0c = row["hashes_t0"]["l0_composite"].as_str().unwrap_or("").to_string();
        let proc = row["hashes_t0"]["processed_l0"].as_str().unwrap_or("").to_string();
        let mismatch = reference
            .as_ref()
            .map(|(rp, rl0, _)| proc == *rp && l0c != *rl0)
            .unwrap_or(false);
        eprintln!(
            "[step3b_io] attempt={attempt} mismatch={mismatch} flips={} converge={:?}",
            row["equal_gen_proc_flips"],
            row["converge"]
                .as_array()
                .map(|a| a
                    .iter()
                    .map(|c| c["same_l0c_as_t0"].as_bool().unwrap_or(true))
                    .collect::<Vec<_>>())
        );
        if mismatch || row["equal_gen_proc_flips"].as_u64().unwrap_or(0) > 0 {
            caught = Some(row);
            break;
        }
        if attempt == 20 {
            caught = Some(row);
        }
    }

    let path = out_dir().join("step3b_io_proof.json");
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({
            "reference": reference,
            "caught": caught,
        }))
        .unwrap_or_default(),
    );
    eprintln!("[step3b_io] wrote {}", path.display());
}
