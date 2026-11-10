//! Throttled preview progress for the footer bar.
//!
//! Emits Tauri `ed-progress` with `{ level, done, total, doc_gen, source_id }`.
//!
//! Sources:
//! - `cpu_ed`: wave-relative L0 Processed freshness in the **viewport**
//!   (ED, ordered, and other tiled dither — not the whole document grid)
//! - `gpu_dither`: strip dispatches during GPU family apply
//!
//! A wave starts in [`note_ed_pass_start`]; `total` is the non-fresh viewport L0
//! count at that moment so ordered dither can reach 100% when the view is done.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Emitter;

use engine_project::LayerNode;
use engine_tiles::{CacheStage, TileCoord, TileKey, TILE_SIZE};

use crate::commands::AppState;
use crate::tile_pipeline::collect_preview_progress_layer_ids;

const THROTTLE: Duration = Duration::from_millis(75);

#[derive(Clone, Debug, Serialize)]
pub struct EdProgressPayload {
    pub level: u8,
    pub done: u32,
    pub total: u32,
    pub doc_gen: u64,
    pub source_id: &'static str,
    pub doc_id: u32,
    pub parallel_ed_passes: u32,
}

struct ThrottleState {
    last_emit: Instant,
    last_done: u32,
    last_doc_gen: u64,
}

struct WaveState {
    doc: u32,
    /// Doc gen at arm time (retained for traces / future stale-wave checks).
    #[allow(dead_code)]
    doc_gen: u64,
    /// Non-fresh viewport L0 count when the wave armed.
    total: u32,
}

fn throttle() -> &'static Mutex<Option<ThrottleState>> {
    static T: OnceLock<Mutex<Option<ThrottleState>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

fn wave() -> &'static Mutex<Option<WaveState>> {
    static W: OnceLock<Mutex<Option<WaveState>>> = OnceLock::new();
    W.get_or_init(|| Mutex::new(None))
}

pub fn source_id_cpu_ed() -> &'static str {
    "cpu_ed"
}

pub fn source_id_gpu_dither() -> &'static str {
    "gpu_dither"
}

fn preview_progress_layer_ids(nodes: &[LayerNode]) -> Vec<u32> {
    let mut ids = Vec::new();
    collect_preview_progress_layer_ids(nodes, &mut ids);
    ids
}

/// Expand a visible tile coord to the L0 footprint that must be Processed.
fn push_l0_footprint(coord: TileCoord, max_tx: u32, max_ty: u32, out: &mut Vec<(u32, u32)>) {
    let (x0, y0, x1, y1) = if coord.level == 0 {
        (coord.x, coord.y, coord.x, coord.y)
    } else {
        let shift = coord.level;
        let x0 = coord.x << shift;
        let y0 = coord.y << shift;
        let x1 = ((coord.x + 1) << shift).saturating_sub(1);
        let y1 = ((coord.y + 1) << shift).saturating_sub(1);
        (x0, y0, x1, y1)
    };
    for y in y0..=y1.min(max_ty) {
        for x in x0..=x1.min(max_tx) {
            out.push((x, y));
        }
    }
}

/// Unique L0 coords covering the current viewport (clamped to doc tile grid).
fn viewport_l0_coords(state: &AppState, max_tx: u32, max_ty: u32) -> Vec<(u32, u32)> {
    let Ok(vp) = state.ui.viewport.lock() else {
        return Vec::new();
    };
    let mut coords = Vec::new();
    for coord in &vp.visible_tiles {
        push_l0_footprint(*coord, max_tx, max_ty, &mut coords);
    }
    coords.sort_unstable();
    coords.dedup();
    coords
}

/// `(pending, doc_gen, parallel_layers)` — non-fresh viewport L0 Processed keys.
fn count_viewport_l0_pending(state: &AppState, doc: u32) -> Option<(u32, u64, u32)> {
    let session = state.session(doc).ok()?;
    let snap = session.document_handle.snapshot();
    let doc_gen = snap
        .generations
        .document_gen
        .load(std::sync::atomic::Ordering::Acquire);
    let max_tx = snap.width.saturating_sub(1) / TILE_SIZE;
    let max_ty = snap.height.saturating_sub(1) / TILE_SIZE;
    let layers = preview_progress_layer_ids(&snap.root);
    if layers.is_empty() {
        return Some((0, doc_gen, 0));
    }
    let coords = viewport_l0_coords(state, max_tx, max_ty);
    if coords.is_empty() {
        return Some((0, doc_gen, 0));
    }

    let mut pending = 0u32;
    let mut parallel = 0u32;
    for layer_id in layers {
        let mut layer_pending = false;
        for &(x, y) in &coords {
            let key = TileKey {
                doc,
                layer: layer_id,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Processed,
            };
            if !engine_tiles::tile_fresh(&state.tiles.tile_cache, key) {
                pending = pending.saturating_add(1);
                layer_pending = true;
            }
        }
        if layer_pending {
            parallel = parallel.saturating_add(1);
        }
    }
    Some((pending, doc_gen, parallel))
}

/// Viewport-scoped pending as `done/total` against the armed wave when present.
#[allow(dead_code)] // harnesses / diagnostics
pub fn count_l0_ed_progress(state: &AppState, doc: u32) -> Option<(u32, u32, u64, u32)> {
    let (pending, doc_gen, parallel) = count_viewport_l0_pending(state, doc)?;
    if let Ok(g) = wave().lock() {
        if let Some(w) = g.as_ref() {
            if w.doc == doc && w.total > 0 {
                let done = w.total.saturating_sub(pending.min(w.total));
                return Some((done, w.total, doc_gen, parallel));
            }
        }
    }
    // No wave: report pending as remaining work with unknown total.
    Some((0, pending, doc_gen, parallel))
}

fn should_emit(force: bool, done: u32, doc_gen: u64) -> bool {
    let Ok(mut slot) = throttle().lock() else {
        return false;
    };
    let now = Instant::now();
    if let Some(prev) = slot.as_ref() {
        if !force
            && prev.last_doc_gen == doc_gen
            && prev.last_done == done
            && now.duration_since(prev.last_emit) < THROTTLE
        {
            return false;
        }
        if !force && now.duration_since(prev.last_emit) < THROTTLE {
            return false;
        }
    }
    *slot = Some(ThrottleState {
        last_emit: now,
        last_done: done,
        last_doc_gen: doc_gen,
    });
    true
}

fn emit_payload<R: tauri::Runtime>(app: &tauri::AppHandle<R>, payload: &EdProgressPayload) {
    if crate::tile_gen_trace::enabled() {
        let pct = if payload.total > 0 {
            (payload.done as f64 / payload.total as f64) * 100.0
        } else {
            0.0
        };
        crate::tile_gen_trace::emit_simple(
            "ed_progress",
            serde_json::json!({
                "level": payload.level,
                "done": payload.done,
                "total": payload.total,
                "doc_gen": payload.doc_gen,
                "source_id": payload.source_id,
                "doc_id": payload.doc_id,
                "parallel_ed_passes": payload.parallel_ed_passes,
                "pct": pct,
            }),
        );
    }
    let _ = app.emit("ed-progress", payload);
}

/// Emit progress if throttle allows (or `force` on completion / pass end).
pub fn maybe_emit<R: tauri::Runtime>(
    state: &AppState,
    app: &tauri::AppHandle<R>,
    doc: u32,
    force: bool,
) {
    let Some((pending, doc_gen, parallel)) = count_viewport_l0_pending(state, doc) else {
        return;
    };
    let Ok(mut g) = wave().lock() else {
        return;
    };
    let Some(w) = g.as_mut().filter(|w| w.doc == doc) else {
        return;
    };
    if w.total == 0 {
        return;
    }
    // New dirt mid-wave: grow the denominator so % never goes backwards oddly.
    if pending > w.total {
        w.total = pending;
    }
    let total = w.total;
    let done = total.saturating_sub(pending.min(total));
    let force = force || done >= total || pending == 0;
    // Release lock before throttle/emit.
    drop(g);

    if !should_emit(force, done, doc_gen) {
        return;
    }

    let payload = EdProgressPayload {
        level: 0,
        done,
        total,
        doc_gen,
        source_id: source_id_cpu_ed(),
        doc_id: doc,
        parallel_ed_passes: parallel,
    };
    emit_payload(app, &payload);

    if pending == 0 {
        if let Ok(mut g) = wave().lock() {
            if g.as_ref().is_some_and(|w| w.doc == doc) {
                *g = None;
            }
        }
    }
}

/// GPU strip progress during blocked_fs / blue_noise apply (preview footer %).
pub fn emit_gpu_strip_progress<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    doc: u32,
    doc_gen: u64,
    done: u32,
    total: u32,
) {
    if total == 0 {
        return;
    }
    let force = done >= total || done == 0;
    if !should_emit(force, done, doc_gen) {
        return;
    }
    let payload = EdProgressPayload {
        level: 0,
        done,
        total,
        doc_gen,
        source_id: source_id_gpu_dither(),
        doc_id: doc,
        parallel_ed_passes: 0,
    };
    emit_payload(app, &payload);
}

static PASS_TIMERS: OnceLock<Mutex<Vec<(u32, Instant, u32)>>> = OnceLock::new();

pub fn note_ed_pass_start(state: &AppState, doc: u32) {
    let Some((pending, doc_gen, _)) = count_viewport_l0_pending(state, doc) else {
        return;
    };
    if let Ok(mut g) = wave().lock() {
        *g = Some(WaveState {
            doc,
            doc_gen,
            total: pending,
        });
    }
    if pending == 0 {
        return;
    }
    let timers = PASS_TIMERS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut g) = timers.lock() {
        g.retain(|(d, _, _)| *d != doc);
        g.push((doc, Instant::now(), pending));
    }
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit_simple(
            "ed_pass_start",
            serde_json::json!({
                "doc_id": doc,
                "total_viewport_l0_pending": pending,
                "doc_gen": doc_gen,
            }),
        );
    }
}

/// Clear wave state when the preview busy chip turns off.
pub fn clear_preview_progress_wave(doc: u32) {
    if let Ok(mut g) = wave().lock() {
        if g.as_ref().is_some_and(|w| w.doc == doc) {
            *g = None;
        }
    }
}

pub fn note_ed_pass_maybe_complete(state: &AppState, doc: u32) {
    let Some((pending, doc_gen, _)) = count_viewport_l0_pending(state, doc) else {
        return;
    };
    if pending > 0 {
        return;
    }
    let timers = PASS_TIMERS.get_or_init(|| Mutex::new(Vec::new()));
    let started = if let Ok(mut g) = timers.lock() {
        let idx = g.iter().position(|(d, _, _)| *d == doc);
        idx.map(|i| g.remove(i))
    } else {
        None
    };
    let Some((_, t0, total)) = started else {
        return;
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit_simple(
            "ed_pass_complete",
            serde_json::json!({
                "doc_id": doc,
                "doc_gen": doc_gen,
                "total_viewport_l0": total,
                "elapsed_ms": ms,
                "source_id": source_id_cpu_ed(),
                "done": total,
            }),
        );
    }
    clear_preview_progress_wave(doc);
}
