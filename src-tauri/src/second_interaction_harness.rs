//! Measurement-only: first apply (add_filter path) vs later updates (refresh+busy).
//!
//! Does **not** change product behavior. Models production asymmetry:
//! - #1: mutate + `schedule_dirty_viewport_tiles` (no `begin_full_document_busy`)
//! - #2+: `request_preview_refresh` (busy until wave idle)
//!
//! ```text
//! DITHER_TRACE=1 DITHER_JOURNAL=0 \
//!   cargo test -p dither --release second_interaction_matrix -- \
//!   --ignored --nocapture --test-threads=1
//!
//! Env:
//!   DITHER_SECOND_IX_QUICK=1     → 1920×1080
//!   DITHER_SECOND_IX_SIZE=6k    → 6016×4000 (~24 MPix)
//!   DITHER_SECOND_IX_SIZE=4k    → 4000×3000 (default)
//!   DITHER_SECOND_IX_FIT=1      → fit-view canvas (~live 717×725 → L3 on 6k)
//!   DITHER_EXPERIMENT_PARK_COMPOSITE=1 → harness: park Composite until deps ready
//!   DITHER_GPU_PATH=cpu|gpu|auto
//!   DITHER_SECOND_IX_NO_WARM=1  → skip blocked_fs pipeline warm before matrix
//! ```

#![cfg(test)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use engine_project::filter::{DitherModeV2, DitherParamsV2};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterInstance, FilterKind, FilterParams};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};
use serde_json::json;

use crate::commands::{request_preview_refresh, schedule_dirty_viewport_tiles, AppState};
use crate::services::viewport_service::ViewportService;
use crate::tile_gen_trace;

#[derive(Clone, Copy)]
struct SizeSpec {
    w: u32,
    h: u32,
    label: &'static str,
}

fn size_from_env() -> SizeSpec {
    if std::env::var("DITHER_SECOND_IX_QUICK").ok().as_deref() == Some("1") {
        return SizeSpec {
            w: 1920,
            h: 1080,
            label: "1920x1080",
        };
    }
    match std::env::var("DITHER_SECOND_IX_SIZE")
        .unwrap_or_else(|_| "4k".into())
        .as_str()
    {
        // Match live 6K fit ED grid: 24×17 L0 = 408 tiles (not 6016×4000 → 384).
        // L3 footprint then has 3×3 = 9 visible tiles at fit.
        "6k" => SizeSpec {
            w: 6016,
            h: 4352,
            label: "6016x4352",
        },
        _ => SizeSpec {
            w: 4000,
            h: 3000,
            label: "4000x3000",
        },
    }
}

fn profile_label() -> &'static str {
    if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    }
}

fn out_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join("2026-10-22")
        .join("second_interaction");
    let _ = std::fs::create_dir_all(&dir);
    dir
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
            state
                .tiles
                .tile_cache
                .insert_fresh_gen(key, Arc::new(tile), 0);
        }
    }
}

fn wait_silence(state: &AppState, timeout: Duration) -> bool {
    let start = Instant::now();
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
            thread::sleep(Duration::from_millis(80));
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
        if start.elapsed() > timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(20));
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

fn visible_fresh_counts(state: &AppState) -> (usize, usize) {
    let vp = state.ui.viewport.lock().unwrap().clone();
    let n = vp.visible_tiles.len();
    let mut fresh = 0usize;
    for coord in &vp.visible_tiles {
        if engine_tiles::tile_fresh(&state.tiles.tile_cache, visible_composite_key(*coord)) {
            fresh += 1;
        }
    }
    (fresh, n)
}

/// Per-visible Composite: missing | dirty | gen | fresh_eq_content.
fn visible_stamp_diag(state: &AppState, content_gen: u64) -> serde_json::Value {
    sync_content_epoch(state, content_gen);
    let vp = state.ui.viewport.lock().unwrap().clone();
    let mut rows = Vec::new();
    let mut eq = 0usize;
    let mut fresh_ge = 0usize;
    let mut missing = 0usize;
    let mut dirty = 0usize;
    let mut behind = 0usize;
    for coord in &vp.visible_tiles {
        let key = visible_composite_key(*coord);
        let (status, gen) = match state.tiles.tile_cache.entries.get(&key) {
            None => {
                missing += 1;
                ("missing", None)
            }
            Some(e) => {
                let d = e.dirty.load(Ordering::Acquire);
                let g = e.generation;
                if d {
                    dirty += 1;
                    ("dirty", Some(g))
                } else if g < content_gen {
                    behind += 1;
                    ("behind", Some(g))
                } else if g == content_gen {
                    eq += 1;
                    fresh_ge += 1;
                    ("eq_content", Some(g))
                } else {
                    fresh_ge += 1;
                    ("ahead", Some(g))
                }
            }
        };
        rows.push(json!({
            "x": coord.x, "y": coord.y, "level": coord.level,
            "status": status, "gen": gen,
        }));
    }
    json!({
        "visible": vp.visible_tiles.len(),
        "eq_content_gen": eq,
        "fresh_ge_epoch": fresh_ge,
        "missing": missing,
        "dirty": dirty,
        "behind": behind,
        "tiles": rows,
    })
}

fn current_content_gen(state: &AppState) -> u64 {
    state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .current_content_gen()
}

/// Sync cache epoch after mutate (mirrors `compute_composite_tile` / serve).
fn sync_content_epoch(state: &AppState, content_gen: u64) {
    state.tiles.tile_cache.set_content_epoch(content_gen);
}

/// May include stale placeholders if epoch/gen tracking lags — keep as diagnostic.
fn wait_first_visible_fresh(state: &AppState, t0: Instant, timeout: Duration) -> Option<f64> {
    let deadline = t0 + timeout;
    loop {
        let (fresh, n) = visible_fresh_counts(state);
        if n > 0 && fresh > 0 {
            return Some(t0.elapsed().as_secs_f64() * 1000.0);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Time to first visible Composite with `body_gen == content_gen` (true fresh pixels).
fn wait_first_fresh_visible(
    state: &AppState,
    t0: Instant,
    content_gen: u64,
    timeout: Duration,
) -> Option<f64> {
    sync_content_epoch(state, content_gen);
    let deadline = t0 + timeout;
    loop {
        let vp = state.ui.viewport.lock().unwrap().clone();
        for coord in &vp.visible_tiles {
            if engine_tiles::tile_ready_emit_gen(
                &state.tiles.tile_cache,
                visible_composite_key(*coord),
            ) == Some(content_gen)
            {
                return Some(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Sample: all visible Composite stamp `content_gen` (no long blocking wait).
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

/// Busy hold while polling; also tracks ED done via `count_l0_ed_progress`.
fn wave_until_quiet(
    state: &AppState,
    t0: Instant,
    content_gen: u64,
    quiet_cap: Duration,
) -> serde_json::Value {
    sync_content_epoch(state, content_gen);
    let mut first_visible_ms = None;
    let mut first_fresh_vis_ms = None;
    let mut busy_on: Option<Instant> = None;
    let mut busy_hold_ms = None;
    let mut ed_done_ms = None;
    let mut ed_total = None;
    let deadline = Instant::now() + quiet_cap;

    loop {
        if first_visible_ms.is_none() {
            let (fresh, n) = visible_fresh_counts(state);
            if n > 0 && fresh > 0 {
                first_visible_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }
        if first_fresh_vis_ms.is_none() {
            let vp = state.ui.viewport.lock().unwrap().clone();
            for coord in &vp.visible_tiles {
                if engine_tiles::tile_ready_emit_gen(
                    &state.tiles.tile_cache,
                    visible_composite_key(*coord),
                ) == Some(content_gen)
                {
                    first_fresh_vis_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
                    break;
                }
            }
        }

        let busy = state.full_document_busy.load(Ordering::Acquire);
        if busy > 0 && busy_on.is_none() {
            busy_on = Some(Instant::now());
        }
        if busy == 0 {
            if let Some(on) = busy_on.take() {
                busy_hold_ms = Some(on.elapsed().as_secs_f64() * 1000.0);
            }
        }

        if let Some((done, total, _, _)) = crate::ed_progress::count_l0_ed_progress(state, 1) {
            ed_total = Some(total);
            if total > 0 && done >= total && ed_done_ms.is_none() {
                ed_done_ms = Some(t0.elapsed().as_secs_f64() * 1000.0);
            }
        }

        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        let pending = state
            .pending_preview_refresh
            .lock()
            .map(|g| g.is_some())
            .unwrap_or(false);
        if queued == 0 && inflight == 0 && !pending && busy == 0 && blocked == 0 {
            thread::sleep(Duration::from_millis(80));
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
                break;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }

    let quiet_ok = wait_silence(state, Duration::from_millis(200));
    let stamp = visible_stamp_diag(state, content_gen);
    json!({
        "first_visible_ms": first_visible_ms,
        "first_visible_ms_note": "may_be_stale_pixels_if_fe_placeholder; harness has no FE atlas",
        "first_fresh_vis_ms": first_fresh_vis_ms,
        "all_fresh_at_quiet": all_fresh_now(state, content_gen),
        "busy_hold_ms": busy_hold_ms,
        "ed_done_ms": ed_done_ms,
        "ed_total_l0": ed_total,
        "quiet_ok": quiet_ok,
        "stamp_at_quiet": stamp,
    })
}

fn viewport_schedule_snapshot(state: &AppState) -> serde_json::Value {
    let vp = state.ui.viewport.lock().unwrap().clone();
    let xs: Vec<u32> = vp.visible_tiles.iter().map(|c| c.x).collect();
    let ys: Vec<u32> = vp.visible_tiles.iter().map(|c| c.y).collect();
    json!({
        "visible_count": vp.visible_tiles.len(),
        "level": vp.level,
        "zoom": vp.zoom,
        "x0": xs.iter().copied().min(),
        "x1": xs.iter().copied().max(),
        "y0": ys.iter().copied().min(),
        "y1": ys.iter().copied().max(),
        "queued": state.tiles.scheduler.queued_len(),
        "inflight": state.preview_pass_inflight.load(Ordering::Acquire),
    })
}

/// Per-wave counters owned by harness workers (measurement only).
struct HarnessCounters {
    dequeue: AtomicU64,
    dequeue_l0: AtomicU64,
    dequeue_processed: AtomicU64,
    dequeue_composite: AtomicU64,
    requeue_input_not_ready: AtomicU64,
    requeue_other: AtomicU64,
    park_composite_early: AtomicU64,
    stale_discard: AtomicU64,
}

impl HarnessCounters {
    fn new() -> Self {
        Self {
            dequeue: AtomicU64::new(0),
            dequeue_l0: AtomicU64::new(0),
            dequeue_processed: AtomicU64::new(0),
            dequeue_composite: AtomicU64::new(0),
            requeue_input_not_ready: AtomicU64::new(0),
            requeue_other: AtomicU64::new(0),
            park_composite_early: AtomicU64::new(0),
            stale_discard: AtomicU64::new(0),
        }
    }
    fn reset(&self) {
        for a in [
            &self.dequeue,
            &self.dequeue_l0,
            &self.dequeue_processed,
            &self.dequeue_composite,
            &self.requeue_input_not_ready,
            &self.requeue_other,
            &self.park_composite_early,
            &self.stale_discard,
        ] {
            a.store(0, Ordering::Relaxed);
        }
    }
    fn snapshot(&self) -> serde_json::Value {
        json!({
            "dequeue": self.dequeue.load(Ordering::Relaxed),
            "dequeue_l0": self.dequeue_l0.load(Ordering::Relaxed),
            "dequeue_processed": self.dequeue_processed.load(Ordering::Relaxed),
            "dequeue_composite": self.dequeue_composite.load(Ordering::Relaxed),
            "requeue_input_not_ready": self.requeue_input_not_ready.load(Ordering::Relaxed),
            "requeue_other": self.requeue_other.load(Ordering::Relaxed),
            "park_composite_early": self.park_composite_early.load(Ordering::Relaxed),
            "stale_discard": self.stale_discard.load(Ordering::Relaxed),
        })
    }
}

fn experiment_park_composite() -> bool {
    std::env::var("DITHER_EXPERIMENT_PARK_COMPOSITE")
        .ok()
        .as_deref()
        == Some("1")
}

/// Production-shaped ED/Composite handling (unlike the old drop-on-error loop).
/// Optional early-park for Composite when `DITHER_EXPERIMENT_PARK_COMPOSITE=1`.
fn harness_worker_loop(
    state: Arc<AppState>,
    stop: Arc<AtomicBool>,
    counters: Arc<HarnessCounters>,
) {
    let park_early = experiment_park_composite();
    while !stop.load(Ordering::Relaxed) {
        if let Some(task) = state.tiles.scheduler.dequeue() {
            counters.dequeue.fetch_add(1, Ordering::Relaxed);
            if task.key.coord.level == 0 {
                counters.dequeue_l0.fetch_add(1, Ordering::Relaxed);
            }
            match task.key.stage {
                CacheStage::Processed => {
                    counters.dequeue_processed.fetch_add(1, Ordering::Relaxed);
                }
                CacheStage::Composite => {
                    counters.dequeue_composite.fetch_add(1, Ordering::Relaxed);
                }
                CacheStage::Raw => {}
            }

            state.preview_pass_inflight.fetch_add(1, Ordering::AcqRel);
            let snap = match state.session(task.key.doc) {
                Ok(s) => s.document_handle.snapshot(),
                Err(_) => {
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
            };
            if crate::worker::task_is_stale(&task, &snap) {
                counters.stale_discard.fetch_add(1, Ordering::Relaxed);
                state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                crate::commands::on_preview_task_finished(&state);
                continue;
            }

            // Experiment B: park Composite before compute if deps not ready.
            if park_early && task.key.stage == CacheStage::Composite {
                let deps = crate::tile_pipeline::composite_ed_park_deps(
                    &state,
                    &snap.root,
                    task.key.doc,
                    task.key.coord,
                );
                let deps_pending = deps
                    .iter()
                    .any(|d| !engine_tiles::tile_fresh(&state.tiles.tile_cache, *d));
                if deps_pending && !deps.is_empty() {
                    counters
                        .park_composite_early
                        .fetch_add(1, Ordering::Relaxed);
                    if let Some(ready) =
                        state
                            .tiles
                            .ed_frontier
                            .block_on(task, deps, &state.tiles.tile_cache)
                    {
                        state.tiles.scheduler.enqueue_or_bump(ready);
                        state.worker_wake.notify_one();
                    }
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
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
                    match crate::tile_pipeline::compute_composite_tile(task.key, &state) {
                        Err(engine_project::error::EngineError::EdPrefixPending) => {
                            let deps = crate::tile_pipeline::composite_ed_park_deps(
                                &state,
                                &snap.root,
                                task.key.doc,
                                task.key.coord,
                            );
                            if deps.is_empty() {
                                counters.requeue_other.fetch_add(1, Ordering::Relaxed);
                                state.tiles.scheduler.enqueue_or_bump(task);
                                state.worker_wake.notify_one();
                            } else {
                                counters
                                    .requeue_input_not_ready
                                    .fetch_add(1, Ordering::Relaxed);
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

fn fit_viewport_enabled() -> bool {
    std::env::var("DITHER_SECOND_IX_FIT").ok().as_deref() == Some("1")
}

fn setup(
    size: SizeSpec,
) -> (
    Arc<AppState>,
    Arc<AtomicBool>,
    Arc<HarnessCounters>,
    Vec<thread::JoinHandle<()>>,
) {
    let budget = 1024 * 1024 * 1024;
    let gpu = engine_gpu::GpuContext::try_new_blocking().map(Arc::new);
    let state = Arc::new(AppState::empty_process(gpu, budget, true));
    state.spawn_session(raw_doc(size.w, size.h));
    fill_raw(&state, size.w, size.h);

    // Fit-view: canvas matches live-ish 717×725; zoom-to-fit. With 6016×4352
    // (408 L0 tiles) this yields L3 visible≈9 like the live capture.
    let (zoom, ox, oy, cw, ch) = if fit_viewport_enabled() {
        let cw = 717.0_f64;
        let ch = 725.0_f64;
        let zoom = (cw / size.w as f64).min(ch / size.h as f64).min(1.0);
        (zoom, 0.0, 0.0, cw, ch)
    } else {
        let zoom = (1200.0 / size.w as f64).min(800.0 / size.h as f64).min(1.0);
        (zoom, 0.0, 0.0, 1200.0, 800.0)
    };
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(zoom, ox, oy, cw, ch);
    {
        let vp = state.ui.viewport.lock().unwrap().clone();
        eprintln!(
            "[second_ix] viewport level={} zoom={:.6} visible={} bbox=({:?}..{:?},{:?}..{:?}) canvas={cw}x{ch} fit={}",
            vp.level,
            vp.zoom,
            vp.visible_tiles.len(),
            vp.visible_tiles.iter().map(|c| c.x).min(),
            vp.visible_tiles.iter().map(|c| c.x).max(),
            vp.visible_tiles.iter().map(|c| c.y).min(),
            vp.visible_tiles.iter().map(|c| c.y).max(),
            fit_viewport_enabled(),
        );
    }

    let counters = Arc::new(HarnessCounters::new());
    let stop = Arc::new(AtomicBool::new(false));
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 8);
    let mut joins = Vec::new();
    for _ in 0..cores {
        let st = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        let ctr = Arc::clone(&counters);
        joins.push(thread::spawn(move || harness_worker_loop(st, stop, ctr)));
    }
    // Workers must be running before initial silence (Raw already filled).
    let _ = wait_silence(&state, Duration::from_secs(60));
    (state, stop, counters, joins)
}

fn teardown(state: &AppState, stop: &AtomicBool, joins: Vec<thread::JoinHandle<()>>) {
    stop.store(true, Ordering::Relaxed);
    state.worker_wake.notify_all();
    for j in joins {
        std::mem::forget(j);
    }
}

fn cache_snapshot(state: &AppState) -> serde_json::Value {
    json!({
        "tile_entries": state.tiles.tile_cache.entries.len(),
        "tile_used_mb": state.tiles.tile_cache.used_bytes_count() as f64 / (1024.0 * 1024.0),
        "residuals": state.tiles.error_residuals.len(),
        "ed_blocked": state.tiles.ed_frontier.blocked_count(),
        "content_epoch": state.tiles.tile_cache.content_epoch(),
        "fd_busy": state.full_document_busy.load(Ordering::Acquire),
        "queued": state.tiles.scheduler.queued_len(),
        "inflight": state.preview_pass_inflight.load(Ordering::Acquire),
    })
}

fn cache_snapshot_before(state: &AppState) -> serde_json::Value {
    cache_snapshot(state)
}

/// Production-shaped first apply: add filter + schedule (NO request_preview_refresh).
fn interaction_add(
    state: &AppState,
    counters: &HarnessCounters,
    mode: DitherModeV2,
    pixel_size: u8,
    levels: u16,
) -> serde_json::Value {
    counters.reset();
    let t0 = Instant::now();
    let busy_before = state.full_document_busy.load(Ordering::Acquire);
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
    let t_mutate = t0.elapsed().as_secs_f64() * 1000.0;
    let cache_before = cache_snapshot_before(state);

    let t_inv = Instant::now();
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: 1 },
    );
    let needs_ed_reset = matches!(
        &mode,
        DitherModeV2::BlockedFs | DitherModeV2::BlueNoise | DitherModeV2::FloydSteinberg
    );
    if needs_ed_reset {
        state.tiles.error_residuals.evict_layer(1, LayerId::new(1));
        state.tiles.block_representatives.clear_dithered();
        state.tiles.clear_strip_gates();
        state.tiles.full_document.invalidate_layer(1, 1);
    }
    let t_invalidate = t_inv.elapsed().as_secs_f64() * 1000.0;

    let t_sch = Instant::now();
    crate::ed_progress::note_ed_pass_start(state, 1);
    // Mirror FilterService::add_filter — schedule only, no busy.
    schedule_dirty_viewport_tiles(state);
    let t_schedule = t_sch.elapsed().as_secs_f64() * 1000.0;
    let after_schedule = viewport_schedule_snapshot(state);
    let queued_after = after_schedule["queued"].as_u64().unwrap_or(0);
    let enqueued_delta = queued_after; // from empty pre-schedule in harness

    let wave = wave_until_quiet(state, t0, content_gen, Duration::from_secs(120));
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let (fresh_at_end, visible_at_end) = visible_fresh_counts(state);
    let busy_after_wave = state.full_document_busy.load(Ordering::Acquire);
    let mode_s = format!("{mode:?}");

    json!({
        "ix": 1,
        "kind": "add_filter",
        "arms_busy": false,
        "busy_before": busy_before,
        "busy_after_wave": busy_after_wave,
        "fe_would_show_rendering": false,
        "wall_ms": wall_ms,
        "content_gen": content_gen,
        "cache_before": cache_before,
        "mutate_ms": t_mutate,
        "invalidate_ms": t_invalidate,
        "schedule_ms": t_schedule,
        "after_schedule": after_schedule,
        "enqueued_delta": enqueued_delta,
        "schedule_enqueued_visible": after_schedule["visible_count"],
        "wave": wave,
        "first_visible_ms": wave["first_visible_ms"],
        "first_visible_ms_note": wave["first_visible_ms_note"],
        "first_fresh_vis_ms": wave["first_fresh_vis_ms"],
        "busy_hold_ms": wave["busy_hold_ms"],
        "ed_done_ms": wave["ed_done_ms"],
        "ed_total_l0": wave["ed_total_l0"],
        "quiet_ok": wave["quiet_ok"],
        "all_fresh_at_quiet": wave["all_fresh_at_quiet"],
        "stamp_at_quiet": wave["stamp_at_quiet"],
        "visible_fresh_at_end": fresh_at_end,
        "visible_count_at_end": visible_at_end,
        "worker_counters": counters.snapshot(),
        "cache_after": cache_snapshot(state),
        "pixel_size": pixel_size,
        "levels": levels,
        "mode": mode_s,
        "park_composite_experiment": experiment_park_composite(),
    })
}

/// Production-shaped param edit: request_preview_refresh (busy).
fn interaction_update(
    state: &AppState,
    counters: &HarnessCounters,
    ix: u32,
    mode: DitherModeV2,
    pixel_size: u8,
    levels: u16,
    fe_had_preview_frame: bool,
) -> serde_json::Value {
    counters.reset();
    let t0 = Instant::now();
    let busy_before = state.full_document_busy.load(Ordering::Acquire);
    let queued_before = state.tiles.scheduler.queued_len();
    state.must_active().document_handle.mutate(|doc| {
        if let LayerNode::Leaf(layer) = &mut doc.root[0] {
            if let Some(f) = layer.filters.first_mut() {
                f.params = FilterParams::DitherV2(DitherParamsV2 {
                    mode: mode.clone(),
                    pixel_size,
                    levels,
                    ..DitherParamsV2::default()
                });
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
    let content_gen = current_content_gen(state);
    sync_content_epoch(state, content_gen);
    let t_mutate = t0.elapsed().as_secs_f64() * 1000.0;
    let cache_before = cache_snapshot_before(state);

    let t_ref = Instant::now();
    let clear = matches!(
        &mode,
        DitherModeV2::BlockedFs
            | DitherModeV2::BlueNoise
            | DitherModeV2::FloydSteinberg
            | DitherModeV2::Bayer4x4
    );
    request_preview_refresh(state, 1, clear);
    let t_refresh = t_ref.elapsed().as_secs_f64() * 1000.0;
    let busy_armed = state.full_document_busy.load(Ordering::Acquire) > 0;
    let after_schedule = viewport_schedule_snapshot(state);
    let queued_after = after_schedule["queued"].as_u64().unwrap_or(0);
    let enqueued_delta = queued_after.saturating_sub(queued_before as u64);

    let wave = wave_until_quiet(state, t0, content_gen, Duration::from_secs(120));
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let (fresh_at_end, visible_at_end) = visible_fresh_counts(state);
    let mode_s = format!("{mode:?}");

    // FE: Rendering… iff busy && !hadPreviewFrame (PreviewWindow.tsx).
    let fe_would_show_rendering = busy_armed && !fe_had_preview_frame;

    json!({
        "ix": ix,
        "kind": "update_filter",
        "arms_busy": true,
        "busy_before": busy_before,
        "busy_armed": busy_armed,
        "fe_had_preview_frame": fe_had_preview_frame,
        "fe_would_show_rendering": fe_would_show_rendering,
        "wall_ms": wall_ms,
        "content_gen": content_gen,
        "cache_before": cache_before,
        "mutate_ms": t_mutate,
        "refresh_schedule_ms": t_refresh,
        "after_schedule": after_schedule,
        "enqueued_delta": enqueued_delta,
        "schedule_enqueued_visible": after_schedule["visible_count"],
        "wave": wave,
        "first_visible_ms": wave["first_visible_ms"],
        "first_visible_ms_note": wave["first_visible_ms_note"],
        "first_fresh_vis_ms": wave["first_fresh_vis_ms"],
        "busy_hold_ms": wave["busy_hold_ms"],
        "ed_done_ms": wave["ed_done_ms"],
        "ed_total_l0": wave["ed_total_l0"],
        "quiet_ok": wave["quiet_ok"],
        "all_fresh_at_quiet": wave["all_fresh_at_quiet"],
        "stamp_at_quiet": wave["stamp_at_quiet"],
        "visible_fresh_at_end": fresh_at_end,
        "visible_count_at_end": visible_at_end,
        "worker_counters": counters.snapshot(),
        "cache_after": cache_snapshot(state),
        "pixel_size": pixel_size,
        "levels": levels,
        "mode": mode_s,
        "park_composite_experiment": experiment_park_composite(),
    })
}

fn interaction_palette_update(
    state: &AppState,
    counters: &HarnessCounters,
    ix: u32,
    fe_had_preview_frame: bool,
) -> serde_json::Value {
    use engine_color::palette::LinearColor;
    counters.reset();
    let t0 = Instant::now();
    let queued_before = state.tiles.scheduler.queued_len();
    // Attach a palette to the dither filter (content change → full refresh).
    state.must_active().document_handle.mutate(|doc| {
        let pid = doc.add_palette(
            format!("step2_pal_{ix}"),
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
            ],
        );
        if let LayerNode::Leaf(layer) = &mut doc.root[0] {
            if let Some(f) = layer.filters.first_mut() {
                if let FilterParams::DitherV2(ref mut p) = f.params {
                    p.palette_id = Some(pid);
                }
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
    let content_gen = current_content_gen(state);
    sync_content_epoch(state, content_gen);
    let cache_before = cache_snapshot_before(state);
    request_preview_refresh(state, 1, true);
    let busy_armed = state.full_document_busy.load(Ordering::Acquire) > 0;
    let after_schedule = viewport_schedule_snapshot(state);
    let enqueued_delta = after_schedule["queued"]
        .as_u64()
        .unwrap_or(0)
        .saturating_sub(queued_before as u64);
    let wave = wave_until_quiet(state, t0, content_gen, Duration::from_secs(120));
    json!({
        "ix": ix,
        "kind": "palette_change",
        "arms_busy": true,
        "busy_armed": busy_armed,
        "fe_had_preview_frame": fe_had_preview_frame,
        "fe_would_show_rendering": busy_armed && !fe_had_preview_frame,
        "content_gen": content_gen,
        "cache_before": cache_before,
        "after_schedule": after_schedule,
        "enqueued_delta": enqueued_delta,
        "wave": wave,
        "first_fresh_vis_ms": wave["first_fresh_vis_ms"],
        "busy_hold_ms": wave["busy_hold_ms"],
        "ed_done_ms": wave["ed_done_ms"],
        "ed_total_l0": wave["ed_total_l0"],
        "quiet_ok": wave["quiet_ok"],
        "all_fresh_at_quiet": wave["all_fresh_at_quiet"],
        "stamp_at_quiet": wave["stamp_at_quiet"],
        "worker_counters": counters.snapshot(),
        "cache_after": cache_snapshot(state),
        "park_composite_experiment": experiment_park_composite(),
    })
}

fn run_sequence(mode: DitherModeV2, label: &str, edits: &[(u8, u16)]) -> serde_json::Value {
    let size = size_from_env();
    let (state, stop, counters, joins) = setup(size);
    let timeout = Duration::from_secs(30);
    assert!(wait_silence(&state, timeout), "pre-silence");

    let mut rows = Vec::new();
    let (ps0, lv0) = edits[0];
    let r1 = interaction_add(&state, &counters, mode.clone(), ps0, lv0);
    eprintln!(
        "[second_ix] {label} #1 add first_fresh={:?} ed_done={:?} ed_l0={:?} delta={:?} requeue_inr={:?} quiet={:?} all_fresh={:?} vis={}",
        r1["first_fresh_vis_ms"],
        r1["ed_done_ms"],
        r1["ed_total_l0"],
        r1["enqueued_delta"],
        r1["worker_counters"]["requeue_input_not_ready"],
        r1["quiet_ok"],
        r1["all_fresh_at_quiet"],
        r1["after_schedule"]["visible_count"],
    );
    rows.push(r1);

    let mut fe_had_frame = false;
    for (i, &(ps, lv)) in edits.iter().enumerate().skip(1) {
        let ix = (i + 1) as u32;
        let r = interaction_update(&state, &counters, ix, mode.clone(), ps, lv, fe_had_frame);
        eprintln!(
            "[second_ix] {label} #{ix} update ps={ps} lv={lv} first_fresh={:?} busy_hold={:?} ed_done={:?} delta={:?} requeue_inr={:?} rendering={} quiet={:?} all_fresh={:?}",
            r["first_fresh_vis_ms"],
            r["busy_hold_ms"],
            r["ed_done_ms"],
            r["enqueued_delta"],
            r["worker_counters"]["requeue_input_not_ready"],
            r["fe_would_show_rendering"],
            r["quiet_ok"],
            r["all_fresh_at_quiet"],
        );
        if r["arms_busy"].as_bool() == Some(true) && r["quiet_ok"].as_bool() == Some(true) {
            fe_had_frame = true;
        }
        rows.push(r);
    }

    teardown(&state, &stop, joins);
    json!({
        "label": label,
        "mode": format!("{mode:?}"),
        "size": size.label,
        "fit_viewport": fit_viewport_enabled(),
        "park_composite": experiment_park_composite(),
        "profile": profile_label(),
        "gpu_path_env": std::env::var("DITHER_GPU_PATH").unwrap_or_else(|_| "auto".into()),
        "journal": std::env::var("DITHER_JOURNAL").unwrap_or_else(|_| "default".into()),
        "interactions": rows,
    })
}

#[test]
#[ignore]
fn second_interaction_matrix() {
    tile_gen_trace::force_enable(true);
    let events = out_dir().join(format!("events_{}.jsonl", profile_label()));
    crate::tile_gen_trace_host::set_out_path_for_tests(events);

    if std::env::var("DITHER_SECOND_IX_NO_WARM").ok().as_deref() != Some("1") {
        // Default: leave cold (no warm) so #2 GPU can show pipeline=new.
        // Set DITHER_SECOND_IX_WARM=1 to pre-warm for contrast.
        if std::env::var("DITHER_SECOND_IX_WARM").ok().as_deref() == Some("1") {
            let _ = engine_gpu::warm_blocked_fs_pipelines();
        }
    }

    // Sequences: #1 baseline + #2–#8 edits (7 updates = 8 total).
    // A: pixel_size 1→8→9→8→9→8→9→8 (cheaper after #1 if #1 was ps=1)
    // B: levels only 16→32→16→32…
    // C: alternate ps 8↔9 from the start (after add at 8)
    let mut report = json!({
        "date": "2026-10-22",
        "note": "Measurement-only. #1=add_filter path; #2+=request_preview_refresh. FE Rendering simulated.",
        "scenarios": [],
    });

    let mut scenarios: Vec<(DitherModeV2, &str, Vec<(u8, u16)>)> = vec![
        (
            DitherModeV2::FloydSteinberg,
            "fs_same_ps8",
            // Isolates sequence #: same params every tick after add at ps=8.
            vec![
                (8, 16),
                (8, 16),
                (8, 16),
                (8, 16),
                (8, 16),
                (8, 16),
                (8, 16),
                (8, 16),
            ],
        ),
        (
            DitherModeV2::FloydSteinberg,
            "fs_ps_alt",
            vec![
                (1, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
            ],
        ),
        (
            DitherModeV2::FloydSteinberg,
            "fs_levels_alt",
            vec![
                (8, 16),
                (8, 32),
                (8, 16),
                (8, 32),
                (8, 16),
                (8, 32),
                (8, 16),
                (8, 32),
            ],
        ),
        (
            DitherModeV2::Bayer4x4,
            "bayer_ps_alt",
            vec![
                (1, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
            ],
        ),
        (
            DitherModeV2::BlockedFs,
            "blocked_fs_ps_alt",
            vec![
                (1, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
            ],
        ),
        (
            DitherModeV2::BlueNoise,
            "blue_noise_ps_alt",
            vec![
                (1, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
                (9, 16),
                (8, 16),
            ],
        ),
    ];

    let quick = std::env::var("DITHER_SECOND_IX_QUICK").ok().as_deref() == Some("1");
    let only = std::env::var("DITHER_SECOND_IX_ONLY").ok();
    if let Some(name) = only.as_deref() {
        scenarios.retain(|(_, label, _)| *label == name);
    } else if quick {
        scenarios.truncate(2); // fs_same_ps8 + fs_ps_alt
    }
    let max_ix = std::env::var("DITHER_SECOND_IX_MAX_IX")
        .ok()
        .and_then(|s| s.parse::<usize>().ok());
    if let Some(n) = max_ix {
        for (_, _, edits) in scenarios.iter_mut() {
            if edits.len() > n {
                edits.truncate(n);
            }
        }
    }

    for (mode, label, edits) in scenarios {
        let row = run_sequence(mode, label, &edits);
        report["scenarios"].as_array_mut().unwrap().push(row);
    }

    let path = out_dir().join(format!("matrix_{}.json", profile_label()));
    let md_path = out_dir().join(format!("matrix_{}.md", profile_label()));
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );

    let mut md = String::from("# Second-interaction matrix\n\n");
    md.push_str(&format!(
        "profile={} size={} gpu_path={}\n\n",
        profile_label(),
        size_from_env().label,
        std::env::var("DITHER_GPU_PATH").unwrap_or_else(|_| "auto".into())
    ));
    md.push_str("| scenario | ix | kind | wall_ms | first_vis_ms (maybe stale) | first_fresh_vis_ms | queued_after | rendering? |\n");
    md.push_str("|---|---:|---|---:|---:|---:|---:|---|\n");
    if let Some(arr) = report["scenarios"].as_array() {
        for sc in arr {
            let label = sc["label"].as_str().unwrap_or("?");
            if let Some(ixs) = sc["interactions"].as_array() {
                for r in ixs {
                    md.push_str(&format!(
                        "| {} | {} | {} | {:.0} | {} | {} | {} | {} |\n",
                        label,
                        r["ix"],
                        r["kind"].as_str().unwrap_or("?"),
                        r["wall_ms"].as_f64().unwrap_or(0.0),
                        r["first_visible_ms"]
                            .as_f64()
                            .map(|v| format!("{v:.0}"))
                            .unwrap_or_else(|| "-".into()),
                        r["first_fresh_vis_ms"]
                            .as_f64()
                            .map(|v| format!("{v:.0}"))
                            .unwrap_or_else(|| "-".into()),
                        r["after_schedule"]["queued"],
                        r["fe_would_show_rendering"],
                    ));
                }
            }
        }
    }
    let _ = std::fs::write(&md_path, md);
    eprintln!(
        "[second_ix] wrote {} and {}",
        path.display(),
        md_path.display()
    );
}

#[test]
#[ignore]
fn second_interaction_ab_cpu_no_journal() {
    // A/B: force CPU GPU path + journal off (set by caller env). Same FS sequence.
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    tile_gen_trace::force_enable(true);
    let events = out_dir().join(format!("events_ab_cpu_{}.jsonl", profile_label()));
    crate::tile_gen_trace_host::set_out_path_for_tests(events);

    let edits = vec![
        (1u8, 16u16),
        (8, 16),
        (9, 16),
        (8, 16),
        (9, 16),
        (8, 16),
        (9, 16),
        (8, 16),
    ];
    let row = run_sequence(DitherModeV2::FloydSteinberg, "ab_fs_cpu_no_journal", &edits);
    let path = out_dir().join(format!("ab_cpu_{}.json", profile_label()));
    let _ = std::fs::write(&path, serde_json::to_vec_pretty(&row).unwrap_or_default());
    eprintln!("[second_ix] A/B wrote {}", path.display());
}

/// STEP2: fit-view L3 on 6k — add ps=8 → #2–#4 ps alt → #5 levels → #6 palette.
#[test]
#[ignore]
fn second_interaction_fit_l3_step2() {
    std::env::set_var("DITHER_SECOND_IX_SIZE", "6k");
    std::env::set_var("DITHER_SECOND_IX_FIT", "1");
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    // Default: park experiment off (production-like requeue path in harness worker).
    std::env::remove_var("DITHER_EXPERIMENT_PARK_COMPOSITE");

    tile_gen_trace::force_enable(true);
    let events = out_dir().join(format!("events_fit_l3_{}.jsonl", profile_label()));
    crate::tile_gen_trace_host::set_out_path_for_tests(events);

    let size = size_from_env();
    let (state, stop, counters, joins) = setup(size);
    assert!(wait_silence(&state, Duration::from_secs(60)), "pre-silence");

    let mode = DitherModeV2::FloydSteinberg;
    let mut rows = Vec::new();
    let r1 = interaction_add(&state, &counters, mode.clone(), 8, 16);
    eprintln!(
        "[fit_l3] #1 add first_fresh={:?} ed_l0={:?} delta={:?} vis={:?} lvl={:?} requeue={:?}",
        r1["first_fresh_vis_ms"],
        r1["ed_total_l0"],
        r1["enqueued_delta"],
        r1["after_schedule"]["visible_count"],
        r1["after_schedule"]["level"],
        r1["worker_counters"]["requeue_input_not_ready"],
    );
    rows.push(r1);

    let mut fe_had = false;
    for (ix, (ps, lv)) in [(9u8, 16u16), (8, 16), (9, 16), (9, 32)]
        .into_iter()
        .enumerate()
    {
        let ix = (ix + 2) as u32;
        let r = interaction_update(&state, &counters, ix, mode.clone(), ps, lv, fe_had);
        eprintln!(
            "[fit_l3] #{ix} ps={ps} lv={lv} first_fresh={:?} busy={:?} ed_done={:?} delta={:?} requeue={:?} all_fresh={:?}",
            r["first_fresh_vis_ms"],
            r["busy_hold_ms"],
            r["ed_done_ms"],
            r["enqueued_delta"],
            r["worker_counters"]["requeue_input_not_ready"],
            r["all_fresh_at_quiet"],
        );
        // FE sets hadPreviewFrame after the first busy cycle ends (not quiet_ok).
        if r["busy_hold_ms"].as_f64().is_some() {
            fe_had = true;
        }
        rows.push(r);
    }
    let r6 = interaction_palette_update(&state, &counters, 6, fe_had);
    eprintln!(
        "[fit_l3] #6 palette first_fresh={:?} busy={:?} ed_done={:?} delta={:?} requeue={:?}",
        r6["first_fresh_vis_ms"],
        r6["busy_hold_ms"],
        r6["ed_done_ms"],
        r6["enqueued_delta"],
        r6["worker_counters"]["requeue_input_not_ready"],
    );
    rows.push(r6);

    teardown(&state, &stop, joins);
    let report = json!({
        "date": "2026-10-22",
        "label": "fit_l3_step2",
        "size": size.label,
        "fit_viewport": true,
        "park_composite": false,
        "interactions": rows,
    });
    let path = out_dir().join(format!("fit_l3_step2_{}.json", profile_label()));
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    eprintln!("[fit_l3] wrote {}", path.display());
}

/// STEP2 Task3: A/B park-composite churn (3 runs each). Flag default off = A.
#[test]
#[ignore]
fn second_interaction_churn_ab() {
    std::env::set_var("DITHER_SECOND_IX_SIZE", "6k");
    std::env::set_var("DITHER_SECOND_IX_FIT", "1");
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    tile_gen_trace::force_enable(true);

    fn one_run(label: &str) -> serde_json::Value {
        let size = size_from_env();
        let (state, stop, counters, joins) = setup(size);
        let _ = wait_silence(&state, Duration::from_secs(60));
        let mode = DitherModeV2::FloydSteinberg;
        let r1 = interaction_add(&state, &counters, mode.clone(), 8, 16);
        let r2 = interaction_update(&state, &counters, 2, mode, 9, 16, false);
        teardown(&state, &stop, joins);
        json!({
            "label": label,
            "park": experiment_park_composite(),
            "add": {
                "first_fresh_vis_ms": r1["first_fresh_vis_ms"],
                "ed_done_ms": r1["ed_done_ms"],
                "ed_total_l0": r1["ed_total_l0"],
                "enqueued_delta": r1["enqueued_delta"],
                "worker_counters": r1["worker_counters"],
                "quiet_ok": r1["quiet_ok"],
            },
            "update2": {
                "first_fresh_vis_ms": r2["first_fresh_vis_ms"],
                "busy_hold_ms": r2["busy_hold_ms"],
                "ed_done_ms": r2["ed_done_ms"],
                "ed_total_l0": r2["ed_total_l0"],
                "enqueued_delta": r2["enqueued_delta"],
                "worker_counters": r2["worker_counters"],
                "quiet_ok": r2["quiet_ok"],
            },
        })
    }

    let mut runs = Vec::new();
    for i in 1..=3 {
        std::env::remove_var("DITHER_EXPERIMENT_PARK_COMPOSITE");
        let r = one_run(&format!("A_baseline_{i}"));
        eprintln!(
            "[churn_ab] A#{i} fresh2={:?} busy={:?} requeue={:?}",
            r["update2"]["first_fresh_vis_ms"],
            r["update2"]["busy_hold_ms"],
            r["update2"]["worker_counters"]["requeue_input_not_ready"]
        );
        runs.push(r);
    }
    for i in 1..=3 {
        std::env::set_var("DITHER_EXPERIMENT_PARK_COMPOSITE", "1");
        let r = one_run(&format!("B_park_{i}"));
        eprintln!(
            "[churn_ab] B#{i} fresh2={:?} busy={:?} requeue={:?} park_early={:?}",
            r["update2"]["first_fresh_vis_ms"],
            r["update2"]["busy_hold_ms"],
            r["update2"]["worker_counters"]["requeue_input_not_ready"],
            r["update2"]["worker_counters"]["park_composite_early"]
        );
        runs.push(r);
    }
    std::env::remove_var("DITHER_EXPERIMENT_PARK_COMPOSITE");

    let path = out_dir().join(format!("churn_ab_{}.json", profile_label()));
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({ "runs": runs })).unwrap_or_default(),
    );
    eprintln!("[churn_ab] wrote {}", path.display());
}

// ─── STEP3 golden hashes (measurement / accuracy) ───────────────────────────

fn hash_tile_bytes(hasher: &mut blake3::Hasher, key: TileKey, tile: &PixelTile) {
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

/// Blake3 over all L0 Processed tiles + all visible Composite tiles (sorted keys).
///
/// Only hashes tiles that are cache-fresh at `content_gen`. Stale/dirty bodies
/// (still present after mark_dirty) must not enter the digest — that was the
/// Step0 false "nondeterminism" when quiet landed before all visibles stamped.
fn golden_hashes_now(state: &AppState) -> serde_json::Value {
    use engine_tiles::TILE_SIZE;
    let snap = state.must_active().document_handle.snapshot();
    let w = snap.width;
    let h = snap.height;
    let cols = w.div_ceil(TILE_SIZE);
    let rows = h.div_ceil(TILE_SIZE);
    let content_gen = snap.generations.current_content_gen();
    sync_content_epoch(state, content_gen);

    let mut proc_keys = Vec::new();
    for y in 0..rows {
        for x in 0..cols {
            proc_keys.push(TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Processed,
            });
        }
    }
    proc_keys.sort_by_key(|k| (k.coord.y, k.coord.x));

    let mut proc_hasher = blake3::Hasher::new();
    let mut proc_n = 0u32;
    let mut proc_missing = 0u32;
    let mut proc_stale = 0u32;
    for key in &proc_keys {
        if engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, *key) != Some(content_gen) {
            if state.tiles.tile_cache.entries.contains_key(key) {
                proc_stale += 1;
            } else {
                proc_missing += 1;
            }
            continue;
        }
        if let Some(tile) = state.tiles.tile_cache.get_entry(*key) {
            hash_tile_bytes(&mut proc_hasher, *key, &tile);
            proc_n += 1;
        } else {
            proc_missing += 1;
        }
    }

    let vp = state.ui.viewport.lock().unwrap().clone();
    let mut comp_keys: Vec<TileKey> = vp
        .visible_tiles
        .iter()
        .map(|c| visible_composite_key(*c))
        .collect();
    comp_keys.sort_by_key(|k| (k.coord.level, k.coord.y, k.coord.x));

    let mut comp_hasher = blake3::Hasher::new();
    let mut comp_n = 0u32;
    let mut comp_missing = 0u32;
    let mut comp_stale = 0u32;
    for key in &comp_keys {
        if engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, *key) != Some(content_gen) {
            if state.tiles.tile_cache.entries.contains_key(key) {
                comp_stale += 1;
            } else {
                comp_missing += 1;
            }
            continue;
        }
        if let Some(tile) = state.tiles.tile_cache.get_entry(*key) {
            hash_tile_bytes(&mut comp_hasher, *key, &tile);
            comp_n += 1;
        } else {
            comp_missing += 1;
        }
    }

    let complete = proc_missing == 0
        && proc_stale == 0
        && comp_missing == 0
        && comp_stale == 0
        && comp_n == comp_keys.len() as u32;

    json!({
        "processed_l0": proc_hasher.finalize().to_hex().to_string(),
        "processed_l0_tiles": proc_n,
        "processed_l0_missing": proc_missing,
        "processed_l0_stale": proc_stale,
        "composite_visible": comp_hasher.finalize().to_hex().to_string(),
        "composite_visible_tiles": comp_n,
        "composite_visible_missing": comp_missing,
        "composite_visible_stale": comp_stale,
        "complete": complete,
        "content_gen": content_gen,
        "viewport_level": vp.level,
        "visible_count": vp.visible_tiles.len(),
    })
}

fn apply_dither_and_quiet(
    state: &AppState,
    counters: &HarnessCounters,
    mode: DitherModeV2,
    pixel_size: u8,
    levels: u16,
) -> bool {
    counters.reset();
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
    state.tiles.error_residuals.evict_layer(1, LayerId::new(1));
    state.tiles.block_representatives.clear_dithered();
    state.tiles.clear_strip_gates();
    state.tiles.full_document.invalidate_layer(1, 1);
    crate::ed_progress::note_ed_pass_start(state, 1);
    schedule_dirty_viewport_tiles(state);
    let wave = wave_until_quiet(state, Instant::now(), content_gen, Duration::from_secs(180));
    // Must have every visible Composite stamped at this content_gen — quiet alone
    // can land while dirty/stale bodies remain in the cache (get_entry still serves them).
    let all_fresh = wave["all_fresh_at_quiet"].as_bool().unwrap_or(false);
    if !all_fresh {
        // One reschedule pass for dirty visibles, then wait again.
        schedule_dirty_viewport_tiles(state);
        let wave2 = wave_until_quiet(state, Instant::now(), content_gen, Duration::from_secs(180));
        return wave2["all_fresh_at_quiet"].as_bool().unwrap_or(false);
    }
    all_fresh
}

/// STEP3 Step0: golden hashes ×3 for determinism (before product changes).
#[test]
#[ignore]
fn golden_hashes_baseline_step0() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    std::env::remove_var("DITHER_EXPERIMENT_PARK_COMPOSITE");
    std::env::remove_var("DITHER_PARK_COMPOSITE");

    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".into());

    let modes: &[(DitherModeV2, &str)] = &[
        (DitherModeV2::FloydSteinberg, "floyd_steinberg"),
        (DitherModeV2::BlockedFs, "blocked_fs"),
        (DitherModeV2::BlueNoise, "blue_noise"),
        (DitherModeV2::Bayer4x4, "bayer4x4"),
    ];
    let configs: &[(u8, u16)] = &[(1, 16), (8, 16), (9, 16), (1, 32), (8, 32), (9, 32)];
    let sizes: &[(SizeSpec, bool)] = &[
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
    ];

    let quick = std::env::var("DITHER_GOLDEN_QUICK").ok().as_deref() == Some("1");
    let runs = if quick { 1 } else { 3 };

    let mut report = json!({
        "date": "2026-10-22",
        "git_commit": commit,
        "profile": profile_label(),
        "gpu_path": "cpu",
        "runs_per_config": runs,
        "configs": [],
        "determinism_ok": true,
        "mismatches": [],
    });

    for (size, fit) in sizes {
        if *fit {
            std::env::set_var("DITHER_SECOND_IX_FIT", "1");
        } else {
            std::env::remove_var("DITHER_SECOND_IX_FIT");
        }
        let (state, stop, counters, joins) = setup(*size);
        assert!(
            wait_silence(&state, Duration::from_secs(120)),
            "pre-silence {}",
            size.label
        );

        for (mode, mode_label) in modes {
            let mode_list = if quick {
                // Quick: FS only.
                if *mode_label != "floyd_steinberg" {
                    continue;
                }
                true
            } else {
                true
            };
            let _ = mode_list;
            for &(ps, lv) in configs {
                if quick && !(ps == 8 && lv == 16) {
                    continue;
                }
                let mut run_hashes = Vec::new();
                for run_i in 1..=runs {
                    let ok = apply_dither_and_quiet(&state, &counters, mode.clone(), ps, lv);
                    let hashes = golden_hashes_now(&state);
                    let complete = hashes["complete"].as_bool().unwrap_or(false);
                    eprintln!(
                        "[golden] {} {} ps={ps} lv={lv} run={run_i} ok={ok} complete={complete} proc={} comp={} comp_stale={} proc_stale={}",
                        size.label,
                        mode_label,
                        hashes["processed_l0"],
                        hashes["composite_visible"],
                        hashes["composite_visible_stale"],
                        hashes["processed_l0_stale"],
                    );
                    if !ok || !complete {
                        report["determinism_ok"] = json!(false);
                        report["mismatches"].as_array_mut().unwrap().push(json!({
                            "kind": "incomplete_wave",
                            "size": size.label,
                            "mode": mode_label,
                            "pixel_size": ps,
                            "levels": lv,
                            "run": run_i,
                            "ok": ok,
                            "hash": hashes,
                        }));
                    }
                    run_hashes.push(hashes);
                }
                let stable = run_hashes.windows(2).all(|w| {
                    w[0]["processed_l0"] == w[1]["processed_l0"]
                        && w[0]["composite_visible"] == w[1]["composite_visible"]
                        && w[0]["complete"] == w[1]["complete"]
                }) && run_hashes
                    .iter()
                    .all(|h| h["complete"].as_bool().unwrap_or(false));
                if !stable {
                    report["determinism_ok"] = json!(false);
                    report["mismatches"].as_array_mut().unwrap().push(json!({
                        "kind": "hash_mismatch",
                        "size": size.label,
                        "mode": mode_label,
                        "pixel_size": ps,
                        "levels": lv,
                        "runs": run_hashes,
                    }));
                }
                report["configs"].as_array_mut().unwrap().push(json!({
                    "size": size.label,
                    "fit": fit,
                    "mode": mode_label,
                    "pixel_size": ps,
                    "levels": lv,
                    "stable": stable,
                    "hash": run_hashes.last(),
                    "runs": if runs > 1 { run_hashes } else { vec![] },
                }));
            }
        }
        teardown(&state, &stop, joins);
    }

    let path = out_dir().join("golden_hashes_baseline.json");
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    eprintln!(
        "[golden] wrote {} determinism_ok={}",
        path.display(),
        report["determinism_ok"]
    );
    assert!(
        report["determinism_ok"].as_bool().unwrap_or(false),
        "golden hashes are non-deterministic — stop before product changes"
    );
}

/// STEP3 Step0/4: fit L3 metrics ×3 (add ps=8 → update ps=9).
#[test]
#[ignore]
fn fit_l3_metrics_baseline_step0() {
    std::env::set_var("DITHER_SECOND_IX_SIZE", "6k");
    std::env::set_var("DITHER_SECOND_IX_FIT", "1");
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    std::env::remove_var("DITHER_EXPERIMENT_PARK_COMPOSITE");
    // Kill-switch default for park: leave unset until Step2 ships.

    let mut runs = Vec::new();
    for i in 1..=3 {
        let size = size_from_env();
        let (state, stop, counters, joins) = setup(size);
        let _ = wait_silence(&state, Duration::from_secs(60));
        let mode = DitherModeV2::FloydSteinberg;
        let r1 = interaction_add(&state, &counters, mode.clone(), 8, 16);
        let r2 = interaction_update(&state, &counters, 2, mode, 9, 16, false);
        teardown(&state, &stop, joins);
        let row = json!({
            "run": i,
            "add": {
                "first_fresh_ms": r1["first_fresh_vis_ms"],
                "ed_done_ms": r1["ed_done_ms"],
                "requeue_inr": r1["worker_counters"]["requeue_input_not_ready"],
                "dequeue": r1["worker_counters"]["dequeue"],
            },
            "update2": {
                "first_fresh_ms": r2["first_fresh_vis_ms"],
                "busy_hold_ms": r2["busy_hold_ms"],
                "ed_done_ms": r2["ed_done_ms"],
                "requeue_inr": r2["worker_counters"]["requeue_input_not_ready"],
                "dequeue": r2["worker_counters"]["dequeue"],
            },
        });
        eprintln!(
            "[metrics_baseline] run={i} fresh2={:?} busy={:?} requeue={:?}",
            row["update2"]["first_fresh_ms"],
            row["update2"]["busy_hold_ms"],
            row["update2"]["requeue_inr"]
        );
        runs.push(row);
    }
    let path = out_dir().join(format!("metrics_baseline_fit_l3_{}.json", profile_label()));
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&json!({ "runs": runs })).unwrap_or_default(),
    );
    eprintln!("[metrics_baseline] wrote {}", path.display());
}

/// Diagnose Step0 Composite nondeterminism: cold ps=8 vs transition ps=1→8,
/// plus L0 Composite digest under the visible L1 set.
#[test]
#[ignore]
fn golden_diagnose_composite_nondeterminism() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    std::env::remove_var("DITHER_SECOND_IX_FIT");
    std::env::remove_var("DITHER_EXPERIMENT_PARK_COMPOSITE");
    std::env::remove_var("DITHER_PARK_COMPOSITE");

    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let mode = DitherModeV2::FloydSteinberg;

    // A: cold start → ps=8 only, 3 runs on one worker set
    {
        let (state, stop, counters, joins) = setup(size);
        assert!(wait_silence(&state, Duration::from_secs(60)));
        for run_i in 1..=3 {
            let ok = apply_dither_and_quiet(&state, &counters, mode.clone(), 8, 16);
            let h = golden_hashes_now(&state);
            let l0c = hash_visible_l0_composites(&state);
            eprintln!(
                "[diag] cold_ps8 run={run_i} ok={ok} complete={} proc={} comp={} l0comp={}",
                h["complete"], h["processed_l0"], h["composite_visible"], l0c
            );
        }
        teardown(&state, &stop, joins);
    }

    // B: ps=1 then ps=8×3 (transition)
    {
        let (state, stop, counters, joins) = setup(size);
        assert!(wait_silence(&state, Duration::from_secs(60)));
        let _ = apply_dither_and_quiet(&state, &counters, mode.clone(), 1, 16);
        for run_i in 1..=3 {
            let ok = apply_dither_and_quiet(&state, &counters, mode.clone(), 8, 16);
            let h = golden_hashes_now(&state);
            let l0c = hash_visible_l0_composites(&state);
            eprintln!(
                "[diag] after_ps1 run={run_i} ok={ok} complete={} proc={} comp={} l0comp={}",
                h["complete"], h["processed_l0"], h["composite_visible"], l0c
            );
        }
        teardown(&state, &stop, joins);
    }

    // C: matrix-like ps=1×3 then ps=8×3 then ps=9×3 (reproduce full-golden flake)
    {
        let (state, stop, counters, joins) = setup(size);
        assert!(wait_silence(&state, Duration::from_secs(60)));
        for run_i in 1..=3 {
            let ok = apply_dither_and_quiet(&state, &counters, mode.clone(), 1, 16);
            let h = golden_hashes_now(&state);
            eprintln!(
                "[diag] matrix_ps1 run={run_i} ok={ok} complete={} comp={}",
                h["complete"], h["composite_visible"]
            );
        }
        for run_i in 1..=3 {
            let ok = apply_dither_and_quiet(&state, &counters, mode.clone(), 8, 16);
            let h = golden_hashes_now(&state);
            let l0c = hash_visible_l0_composites(&state);
            eprintln!(
                "[diag] matrix_ps8 run={run_i} ok={ok} complete={} proc={} comp={} l0comp={}",
                h["complete"], h["processed_l0"], h["composite_visible"], l0c
            );
        }
        for run_i in 1..=3 {
            let ok = apply_dither_and_quiet(&state, &counters, mode.clone(), 9, 16);
            let h = golden_hashes_now(&state);
            let l0c = hash_visible_l0_composites(&state);
            eprintln!(
                "[diag] matrix_ps9 run={run_i} ok={ok} complete={} proc={} comp={} l0comp={}",
                h["complete"], h["processed_l0"], h["composite_visible"], l0c
            );
        }
        teardown(&state, &stop, joins);
    }
}

fn hash_visible_l0_composites(state: &AppState) -> String {
    let content_gen = current_content_gen(state);
    sync_content_epoch(state, content_gen);
    let vp = state.ui.viewport.lock().unwrap().clone();
    let mut keys = Vec::new();
    for c in &vp.visible_tiles {
        // L1 parent → 4 L0 children
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
            hash_tile_bytes(&mut hasher, *key, &tile);
            n += 1;
        }
    }
    format!("{}:n={n}:stale={stale}", hasher.finalize().to_hex())
}
