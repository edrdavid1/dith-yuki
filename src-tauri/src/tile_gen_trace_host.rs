//! Host sink for tile-generation tracing (`.local-doc` lost-tiles / viewport specs).
//! Writes a **single** JSONL under `profiling/results/<date>/trace_viewport/` when
//! `DITHER_TRACE=1`. Harnesses may override via [`set_out_path_for_tests`].

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use serde_json::{json, Value};
use tauri::State;

use crate::commands::AppState;
use crate::tile_gen_trace::{self, TraceEvent, TraceTile};
use std::sync::Arc;

static OUT_PATH: OnceLock<PathBuf> = OnceLock::new();
static FILE_LOCK: Mutex<()> = Mutex::new(());
static NEXT_RUN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn init() {
    tile_gen_trace::init_from_env();
    install_evict_hook_if_needed();
    if !tile_gen_trace::enabled() {
        return;
    }
    let date = chrono_date();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join(&date);
    // Single sink (fix-disappearing-tiles Step 0): avoid dual ~GB mirrors under
    // `trace/` and `trace_viewport/`. Matrix harnesses call `set_out_path_for_tests`.
    let dir = root.join("trace_viewport");
    if let Err(e) = create_dir_all(&dir) {
        log::warn!(target: "tile_gen_trace", "cannot create {dir:?}: {e}");
        return;
    }
    let path = dir.join("events.jsonl");
    let _ = OUT_PATH.set(path.clone());
    tile_gen_trace::set_sink(Box::new(move |ev| {
        append_event(&ev);
    }));
    log::info!(
        target: "tile_gen_trace",
        "writing events to {}",
        path.display()
    );
}

/// Install per-tile eviction observer (idempotent OnceLock). Safe when tracing off —
/// hook no-ops until `tile_gen_trace::enabled()`.
pub fn install_evict_hook_if_needed() {
    engine_tiles::set_evict_tile_hook(Box::new(|key, generation, reason| {
        if !tile_gen_trace::enabled() {
            return;
        }
        let stage = match key.stage {
            engine_tiles::CacheStage::Raw => "raw",
            engine_tiles::CacheStage::Processed => "processed",
            engine_tiles::CacheStage::Composite => "composite",
        };
        tile_gen_trace::emit(
            "cache_evict",
            Some(key.layer),
            None,
            None,
            Some(tile_gen_trace::TraceTile {
                level: key.coord.level,
                x: key.coord.x,
                y: key.coord.y,
            }),
            Some(generation),
            json!({
                "stage": stage,
                "reason": reason,
                "freed_bytes": engine_tiles::TILE_BYTES,
                "tile_gen": generation,
            }),
        );
    }));
}

fn chrono_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn append_event(ev: &TraceEvent) {
    let Ok(_guard) = FILE_LOCK.lock() else {
        return;
    };
    let Ok(line) = serde_json::to_string(ev) else {
        return;
    };
    let Some(path) = OUT_PATH.get() else {
        return;
    };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "{line}");
    }
}

/// Optional override path for harnesses (must be called before first emit sink use).
pub fn set_out_path_for_tests(path: PathBuf) {
    let _ = OUT_PATH.set(path);
}

#[tauri::command]
pub fn tile_gen_trace_enabled() -> bool {
    tile_gen_trace::enabled()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceFeEventRequest {
    pub event: String,
    #[serde(default)]
    pub run_id: Option<u64>,
    #[serde(default)]
    pub layer_id: Option<u32>,
    #[serde(default)]
    pub doc_gen: Option<u64>,
    #[serde(default)]
    pub layer_gen: Option<u64>,
    #[serde(default)]
    pub level: Option<u8>,
    #[serde(default)]
    pub x: Option<u32>,
    #[serde(default)]
    pub y: Option<u32>,
    #[serde(default)]
    pub gen: Option<u64>,
    #[serde(default)]
    pub details: Option<Value>,
    /// Frontend wall-clock epoch ms (preferred when provided).
    #[serde(default)]
    pub ts_ms: Option<f64>,
}

#[tauri::command]
pub fn tile_gen_trace_fe_event(req: TraceFeEventRequest) {
    if !tile_gen_trace::enabled() {
        return;
    }
    if let Some(run_id) = req.run_id {
        tile_gen_trace::set_run_id(run_id);
    }
    let tile = match (req.level, req.x, req.y) {
        (Some(level), Some(x), Some(y)) => Some(TraceTile { level, x, y }),
        _ => None,
    };
    tile_gen_trace::emit_at(
        &req.event,
        req.ts_ms,
        req.layer_id,
        req.doc_gen,
        req.layer_gen,
        tile,
        req.gen,
        req.details.unwrap_or_else(|| json!({})),
    );
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceBeginRequest {
    #[serde(default)]
    pub scenario: Option<String>,
    #[serde(default)]
    pub seed: Option<u64>,
}

#[tauri::command]
pub fn tile_gen_trace_begin_run(req: TraceBeginRequest) -> u64 {
    if !tile_gen_trace::enabled() {
        return 0;
    }
    let run_id = NEXT_RUN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    tile_gen_trace::begin_run(run_id);
    tile_gen_trace::emit_simple(
        "run_begin",
        json!({
            "scenario": req.scenario,
            "seed": req.seed,
        }),
    );
    run_id
}

/// Programmatic slider-emulator entry: same IPC path as UI (`update_filter`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceStressParamRequest {
    pub doc_id: u32,
    pub layer_id: u32,
    pub filter_id: String,
    pub params: Value,
    #[serde(default)]
    pub run_id: Option<u64>,
}

#[tauri::command]
pub fn tile_gen_trace_stress_param(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    req: TraceStressParamRequest,
) -> Result<(), String> {
    if !tile_gen_trace::enabled() {
        return Err("DITHER_TRACE not enabled".into());
    }
    if let Some(run_id) = req.run_id {
        tile_gen_trace::set_run_id(run_id);
    }
    tile_gen_trace::emit(
        "ui_param_change",
        Some(req.layer_id),
        None,
        None,
        None,
        None,
        json!({ "params": req.params, "via": "stress_cmd" }),
    );
    let svc = crate::services::filter_service::FilterService::new(Arc::clone(&state));
    svc.update_filter(
        &app,
        crate::services::filter_service::UpdateFilterRequest {
            doc_id: req.doc_id,
            layer_id: req.layer_id,
            filter_id: req.filter_id,
            params: req.params,
            opacity: None,
            blend_mode: None,
            enabled: None,
            profile_run_id: None,
        },
    )
    .map_err(|e| format!("{e}"))
}

/// Snapshot for silence / invariant checks from the FE harness.
#[tauri::command]
pub fn tile_gen_trace_busy_snapshot(state: State<'_, Arc<AppState>>) -> Value {
    let queued = state.tiles.scheduler.queued_len();
    let inflight = state
        .preview_pass_inflight
        .load(std::sync::atomic::Ordering::Acquire);
    let pending = state
        .pending_preview_refresh
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|p| json!({
            "layer_id": p.layer_id,
            "clear_residuals": p.clear_residuals,
        })));
    let busy = state
        .full_document_busy
        .load(std::sync::atomic::Ordering::Acquire);
    let (doc_gen, layer_gens) = state
        .active_session()
        .ok()
        .map(|s| {
            let snap = s.document_handle.snapshot();
            let doc_gen = snap
                .generations
                .document_gen
                .load(std::sync::atomic::Ordering::Acquire);
            // Map of layer_id → layer_gen for leaves in the tree.
            let mut map = serde_json::Map::new();
            fn walk(nodes: &[engine_project::LayerNode], snap: &engine_project::Document, out: &mut serde_json::Map<String, Value>) {
                for node in nodes {
                    match node {
                        engine_project::LayerNode::Leaf(layer) => {
                            let g = snap.generations.get_layer_gen(layer.id.0);
                            out.insert(layer.id.0.to_string(), json!(g));
                        }
                        engine_project::LayerNode::Group(group) => {
                            walk(&group.children, snap, out);
                        }
                    }
                }
            }
            walk(&snap.root, &snap, &mut map);
            (Some(doc_gen), Value::Object(map))
        })
        .unwrap_or((None, json!({})));
    json!({
        "queued": queued,
        "inflight": inflight,
        "pending_preview_refresh": pending,
        "full_document_busy": busy,
        "doc_gen": doc_gen,
        "layer_gens": layer_gens,
        "silent": queued == 0 && inflight == 0 && pending.is_none() && busy == 0,
    })
}

#[tauri::command]
pub fn tile_gen_trace_inject_enabled() -> bool {
    std::env::var("DITHER_TRACE_INJECT")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

#[tauri::command]
pub fn tile_gen_trace_inject_spin_enabled() -> bool {
    tile_gen_trace::inject_spin_enabled()
}

/// Offenders for the WORKER_SPIN checker (dequeues without compute / high attempt).
#[tauri::command]
pub fn tile_gen_trace_spin_snapshot() -> Value {
    if !tile_gen_trace::enabled() {
        return json!({ "ok": false, "offenders": [] });
    }
    let offenders: Vec<Value> = tile_gen_trace::spin_offenders()
        .into_iter()
        .map(|o| {
            json!({
                "doc": o.doc,
                "level": o.level,
                "x": o.x,
                "y": o.y,
                "doc_gen": o.doc_gen,
                "dequeues_without_compute": o.dequeues_without_compute,
                "requeue_attempt": o.requeue_attempt,
                "tile_key": format!("{}/{}/{}", o.level, o.x, o.y),
            })
        })
        .collect();
    json!({
        "ok": true,
        "threshold": tile_gen_trace::WORKER_SPIN_THRESHOLD,
        "offenders": offenders,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceLayerGenRequest {
    pub layer_id: u32,
}

#[tauri::command]
pub fn tile_gen_trace_layer_gen(
    state: State<'_, Arc<AppState>>,
    req: TraceLayerGenRequest,
) -> Value {
    let Ok(session) = state.active_session() else {
        return json!({ "doc_gen": null, "layer_gen": null });
    };
    let snap = session.document_handle.snapshot();
    let doc_gen = snap
        .generations
        .document_gen
        .load(std::sync::atomic::Ordering::Acquire);
    let layer_gen = snap.generations.get_layer_gen(req.layer_id);
    json!({ "doc_gen": doc_gen, "layer_gen": layer_gen })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceCacheProbeRequest {
    pub level: u8,
    pub x: u32,
    pub y: u32,
    #[serde(default)]
    pub layer_id: Option<u32>,
}

/// Read-only: is Composite tile present at required generation for visible-set holes.
#[tauri::command]
pub fn tile_gen_trace_cache_probe(
    state: State<'_, Arc<AppState>>,
    req: TraceCacheProbeRequest,
) -> Value {
    if !tile_gen_trace::enabled() {
        return json!({ "ok": false, "reason": "trace_disabled" });
    }
    let Ok(session) = state.active_session() else {
        return json!({ "ok": false, "reason": "no_session" });
    };
    let snap = session.document_handle.snapshot();
    let doc_id = snap.id.0;
    let doc_gen = snap
        .generations
        .document_gen
        .load(std::sync::atomic::Ordering::Acquire);
    let layer = req.layer_id.unwrap_or(0);
    let key = engine_tiles::TileKey {
        doc: doc_id,
        layer,
        coord: engine_tiles::TileCoord {
            level: req.level,
            x: req.x,
            y: req.y,
        },
        stage: engine_tiles::CacheStage::Composite,
    };
    match state.tiles.tile_cache.entries.get(&key) {
        Some(entry) => {
            let dirty = entry
                .dirty
                .load(std::sync::atomic::Ordering::Acquire);
            let gen = entry.generation;
            json!({
                "ok": true,
                "present": true,
                "dirty": dirty,
                "tile_gen": gen,
                "doc_gen": doc_gen,
                "fresh": !dirty && gen >= doc_gen,
            })
        }
        None => json!({
            "ok": true,
            "present": false,
            "dirty": true,
            "tile_gen": null,
            "doc_gen": doc_gen,
            "fresh": false,
        }),
    }
}

/// Trace-only: force dither compute path (`cpu` / `gpu` / `auto`).
/// Sets `DITHER_GPU_PATH` and the GPU-preview UI override so Z1/Z2 can switch live.
#[tauri::command]
pub fn tile_gen_trace_set_compute_path(path: String) -> Result<Value, String> {
    if !tile_gen_trace::enabled() {
        return Err("DITHER_TRACE not enabled".into());
    }
    let normalized = path.to_ascii_lowercase();
    match normalized.as_str() {
        "cpu" => {
            std::env::set_var("DITHER_GPU_PATH", "cpu");
            engine_gpu::set_gpu_preview_ui_override(Some(false));
        }
        "gpu" => {
            std::env::set_var("DITHER_GPU_PATH", "gpu");
            engine_gpu::set_gpu_preview_ui_override(Some(true));
        }
        "auto" => {
            std::env::remove_var("DITHER_GPU_PATH");
            engine_gpu::set_gpu_preview_ui_override(None);
        }
        other => {
            return Err(format!("unknown path `{other}` (want cpu|gpu|auto)"));
        }
    }
    tile_gen_trace::emit_simple(
        "compute_path_set",
        json!({
            "path": normalized,
            "gpu_preview": engine_gpu::gpu_preview_enabled(),
        }),
    );
    Ok(json!({
        "path": normalized,
        "gpu_preview": engine_gpu::gpu_preview_enabled(),
        "force_cpu_env": engine_gpu::force_cpu(),
    }))
}

#[tauri::command]
pub fn tile_gen_trace_get_compute_path() -> Value {
    let path = std::env::var("DITHER_GPU_PATH").unwrap_or_else(|_| "auto".into());
    json!({
        "path": path,
        "gpu_preview": engine_gpu::gpu_preview_enabled(),
        "force_cpu_env": engine_gpu::force_cpu(),
    })
}

/// Last applied backend viewport (for FE divergence checker).
#[tauri::command]
pub fn tile_gen_trace_viewport_snapshot(state: State<'_, Arc<AppState>>) -> Value {
    if !tile_gen_trace::enabled() {
        return json!({ "ok": false });
    }
    let vp = state.ui.viewport.lock().unwrap().clone();
    let (vp_hash, x0, y0, x1, y1) =
        tile_gen_trace::vp_hash_from_coords(vp.level, &vp.visible_tiles);
    json!({
        "ok": true,
        "level": vp.level,
        "zoom": vp.zoom,
        "x": vp.x,
        "y": vp.y,
        "width": vp.width,
        "height": vp.height,
        "x0": x0, "y0": y0, "x1": x1, "y1": y1,
        "visible_count": vp.visible_tiles.len(),
        "vp_hash": vp_hash,
        "tiles": vp.visible_tiles.iter().map(|c| json!({
            "level": c.level, "x": c.x, "y": c.y
        })).collect::<Vec<_>>(),
    })
}
