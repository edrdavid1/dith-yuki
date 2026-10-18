//! Throttled ED pass progress for preview (split-dither-preview Step 1).
//!
//! Emits `ed_progress` into the tile_gen_trace sink and Tauri event `ed-progress`
//! with `{ level, done, total, doc_gen, source_id }`.
//!
//! `PARALLEL_ED_PASSES`: count of ED layers that still have unfinished L0
//! Processed work (not pyramid Composite levels).

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Emitter;

use engine_project::LayerNode;
use engine_tiles::{CacheStage, TileCoord, TileKey, TILE_SIZE};

use crate::commands::AppState;
use crate::tile_pipeline::collect_ed_layer_ids;

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

fn throttle() -> &'static Mutex<Option<ThrottleState>> {
    static T: OnceLock<Mutex<Option<ThrottleState>>> = OnceLock::new();
    T.get_or_init(|| Mutex::new(None))
}

pub fn source_id_cpu_ed() -> &'static str {
    "cpu_ed"
}

fn ed_layer_ids(nodes: &[LayerNode]) -> Vec<u32> {
    let mut ids = Vec::new();
    collect_ed_layer_ids(nodes, &mut ids);
    ids
}

/// `(done, total, doc_gen, parallel_ed_passes)` for L0 Processed across ED layers.
pub fn count_l0_ed_progress(state: &AppState, doc: u32) -> Option<(u32, u32, u64, u32)> {
    let session = state.session(doc).ok()?;
    let snap = session.document_handle.snapshot();
    let doc_gen = snap
        .generations
        .document_gen
        .load(std::sync::atomic::Ordering::Acquire);
    let max_tx = snap.width.saturating_sub(1) / TILE_SIZE;
    let max_ty = snap.height.saturating_sub(1) / TILE_SIZE;
    let layers = ed_layer_ids(&snap.root);
    if layers.is_empty() {
        return Some((0, 0, doc_gen, 0));
    }

    let mut total = 0u32;
    let mut done = 0u32;
    let mut parallel = 0u32;
    for layer_id in layers {
        let mut layer_pending = false;
        for y in 0..=max_ty {
            for x in 0..=max_tx {
                total = total.saturating_add(1);
                let key = TileKey {
                    doc,
                    layer: layer_id,
                    coord: TileCoord {
                        level: 0,
                        x,
                        y,
                    },
                    stage: CacheStage::Processed,
                };
                if engine_tiles::tile_fresh(&state.tiles.tile_cache, key) {
                    done = done.saturating_add(1);
                } else {
                    layer_pending = true;
                }
            }
        }
        if layer_pending {
            parallel = parallel.saturating_add(1);
        }
    }
    Some((done, total, doc_gen, parallel))
}

/// Emit progress if throttle allows (or `force` on completion / pass end).
pub fn maybe_emit<R: tauri::Runtime>(
    state: &AppState,
    app: &tauri::AppHandle<R>,
    doc: u32,
    force: bool,
) {
    let Some((done, total, doc_gen, parallel)) = count_l0_ed_progress(state, doc) else {
        return;
    };
    if total == 0 {
        return;
    }
    let force = force || done >= total;
    {
        let Ok(mut slot) = throttle().lock() else {
            return;
        };
        let now = Instant::now();
        if let Some(prev) = slot.as_ref() {
            if !force
                && prev.last_doc_gen == doc_gen
                && prev.last_done == done
                && now.duration_since(prev.last_emit) < THROTTLE
            {
                return;
            }
            if !force && now.duration_since(prev.last_emit) < THROTTLE {
                return;
            }
        }
        *slot = Some(ThrottleState {
            last_emit: now,
            last_done: done,
            last_doc_gen: doc_gen,
        });
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

    if crate::tile_gen_trace::enabled() {
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
                "pct": (done as f64 / total as f64) * 100.0,
            }),
        );
    }
    let _ = app.emit("ed-progress", &payload);
}

static PASS_TIMERS: OnceLock<Mutex<Vec<(u32, Instant, u32)>>> = OnceLock::new();

pub fn note_ed_pass_start(state: &AppState, doc: u32) {
    let Some((_, total, _, _)) = count_l0_ed_progress(state, doc) else {
        return;
    };
    if total == 0 {
        return;
    }
    let timers = PASS_TIMERS.get_or_init(|| Mutex::new(Vec::new()));
    if let Ok(mut g) = timers.lock() {
        g.retain(|(d, _, _)| *d != doc);
        g.push((doc, Instant::now(), total));
    }
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit_simple(
            "ed_pass_start",
            serde_json::json!({ "doc_id": doc, "total_l0_processed": total }),
        );
    }
}

pub fn note_ed_pass_maybe_complete(state: &AppState, doc: u32) {
    let Some((done, total, doc_gen, _)) = count_l0_ed_progress(state, doc) else {
        return;
    };
    if total == 0 || done < total {
        return;
    }
    let timers = PASS_TIMERS.get_or_init(|| Mutex::new(Vec::new()));
    let started = if let Ok(mut g) = timers.lock() {
        let idx = g.iter().position(|(d, _, _)| *d == doc);
        idx.map(|i| g.remove(i))
    } else {
        None
    };
    let Some((_, t0, _)) = started else {
        return;
    };
    let ms = t0.elapsed().as_secs_f64() * 1000.0;
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit_simple(
            "ed_pass_complete",
            serde_json::json!({
                "doc_id": doc,
                "doc_gen": doc_gen,
                "total_l0_processed": total,
                "elapsed_ms": ms,
                "source_id": source_id_cpu_ed(),
                "done": done,
            }),
        );
    }
}
