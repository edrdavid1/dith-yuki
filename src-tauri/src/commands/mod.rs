pub mod diagnostics;
pub use diagnostics::*;
pub mod flexlayout;
pub mod panels;
pub mod selection;
pub use selection::*;
pub mod layers;
pub mod undo;
pub mod viewport;
pub use layers::*;
pub mod filters;
pub use filters::*;
pub mod registry;
pub use registry::*;
pub mod palette;
pub use palette::*;
pub mod color_lab;
pub use color_lab::*;
pub mod document;
pub use document::*;
pub mod proof;
pub use proof::*;
pub mod print_export;
pub use print_export::*;
pub mod batch_export;
pub use batch_export::*;
pub mod pattern_library;
pub use pattern_library::*;
pub mod workspace;
pub use workspace::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use engine_tiles::{CacheStage, Priority, RecomputeTask, TileKey};

use crate::worker::WorkerWake;

/// Re-exports for `commands::tests` / diagnostics. Not all are used by the bin itself.
#[allow(unused_imports)]
pub use crate::services::document_service::{
    blank_rgba_f32, encode_rgba_to_png, f32_to_u8, import_raster_layer, install_raster_document,
    place_image_at_origin, validate_document_dimensions, AsciiClipboardRequest, BlankBackground,
    DocumentResponse, ExportAsciiRequest, ExportImageRequest, ExportPatternRequest,
    ImportPatternRequest, ImportPatternResponse, LoadImageResponse, OpenProjectResponse,
    SaveProjectResponse, ShareProjectCopyOptions, IMAGE_IMPORT_EXTENSIONS, MAX_DOCUMENT_DIMENSION,
};
#[allow(unused_imports)]
pub use crate::services::palette_service::find_layers_referencing_palette;
#[allow(unused_imports)]
pub use crate::services::palette_service::{hex_to_linear, linear_to_hex};

pub(crate) struct PendingPreviewRefresh {
    pub layer_id: u32,
    pub clear_residuals: bool,
}

pub struct AppState {
    pub sessions: Mutex<HashMap<u32, Arc<crate::document_session::DocumentSession>>>,
    pub next_doc_id: AtomicU32,
    pub active_id: Mutex<Option<u32>>,
    pub tiles: crate::state::TileState,
    pub worker_wake: WorkerWake,
    /// H8 last-resort Composite backoff (one sleeper thread; cancel on doc close).
    pub delayed_requeue: Arc<crate::delayed_requeue::DelayedRequeue>,
    pub gpu: Option<std::sync::Arc<engine_gpu::GpuContext>>,
    pub gpu_resident: Option<std::sync::Arc<engine_gpu::GpuTileCache>>,
    pub gpu_executor: Option<std::sync::Mutex<engine_gpu::GpuExecutor>>,
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub ui: crate::state::UiState,
    pub dock_affinity: Mutex<crate::dock_affinity::DockAffinityController>,
    pub preview_pass_inflight: AtomicUsize,
    pub pending_preview_refresh: Mutex<Option<PendingPreviewRefresh>>,
    /// Preview Image|ASCII switch: when true (default), Ascii full-doc filters run.
    pub ascii_preview: AtomicBool,
    /// Refcount of callers inside a full-document ensure (monolithic “Rendering…”).
    pub full_document_busy: AtomicUsize,
    /// FlexLayout persistence (left / right / center models).
    pub flexlayout_persistence: Mutex<crate::flexlayout_persistence::FlexLayoutPersistence>,
    /// Per-side FlexLayout persistence.
    pub flexlayout_left: Mutex<crate::flexlayout_persistence::FlexLayoutPersistence>,
    pub flexlayout_right: Mutex<crate::flexlayout_persistence::FlexLayoutPersistence>,
    pub flexlayout_center: Mutex<crate::flexlayout_persistence::FlexLayoutPersistence>,
    /// Track C: how the CPU tile-cache budget was chosen (`empty_process` tests = override).
    pub ram_budget_source: crate::memory_budget::RamBudgetSource,
    /// Crash-recovery journal (Phase 2). `recovery_dir` set from app_data in setup.
    pub journal: Mutex<crate::journal::JournalRuntime>,
    /// Soft-proof ICC catalog + transform cache (`None` until app_data init).
    pub proof: Mutex<Option<Arc<crate::services::ProofService>>>,
    /// Cancel flag for an in-flight print export (cleared when the job ends).
    pub print_export_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// Cancel flag for an in-flight batch export (cleared when the job ends).
    pub batch_export_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// App-global Workspace / preferences config (`config.json`).
    pub config: Arc<crate::config::ConfigStore>,
}

pub struct QuitGuard {
    pub allow_exit: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentChangedPayload {
    pub kind: String,
    pub layer_id: Option<u32>,
    pub doc_id: Option<u32>,
}

pub(crate) fn emit_document_changed(
    app_handle: &AppHandle,
    kind: &str,
    layer_id: Option<u32>,
    doc_id: Option<u32>,
) {
    let payload = DocumentChangedPayload {
        kind: kind.to_string(),
        layer_id,
        doc_id,
    };
    let _ = app_handle.emit_to(tauri::EventTarget::Any, "document-changed", payload);
}

#[derive(Debug, Clone, Serialize)]
pub struct FullDocumentBusyPayload {
    pub busy: bool,
    pub doc_id: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuDitherStatusPayload {
    pub doc_id: u32,
    pub algorithm: String,
    pub path: String,
    pub backend: String,
    pub elapsed_ms: u32,
    pub strips: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_size: u32,
}

/// Emit last GPU-dither apply status for the preview footer (when experimental flag is on).
pub(crate) fn emit_gpu_dither_status(state: &AppState, doc_id: u32) {
    let Some(status) = engine_project::algorithms::gpu_dither::take_last_gpu_dither_status() else {
        return;
    };
    let Ok(guard) = state.app_handle.lock() else {
        return;
    };
    let Some(ref app) = *guard else {
        return;
    };
    let payload = GpuDitherStatusPayload {
        doc_id,
        algorithm: status.algorithm,
        path: status.path,
        backend: status.backend,
        elapsed_ms: status.elapsed_ms,
        strips: status.strips,
        width: status.width,
        height: status.height,
        pixel_size: status.pixel_size,
    };
    let _ = app.emit_to(tauri::EventTarget::Any, "gpu-dither-status", payload);
}

/// Enter a preview recompute (full-document ensure or filter-driven tile refresh).
/// Emits `full-document-busy` when the first caller enters — drives the corner
/// spinner for GPU families **and** CPU tiled dither.
pub(crate) fn begin_full_document_busy(state: &AppState, doc_id: u32) {
    let prev = state.full_document_busy.fetch_add(1, Ordering::AcqRel);
    if prev == 0 {
        emit_full_document_busy(state, true, doc_id);
    }
}

/// Leave a preview recompute; emit clear when the last caller exits.
pub(crate) fn end_full_document_busy(state: &AppState, doc_id: u32) {
    let prev = state.full_document_busy.fetch_sub(1, Ordering::AcqRel);
    if prev == 1 {
        emit_full_document_busy(state, false, doc_id);
    }
}

/// Drop any held preview-busy refcount and emit idle (safe if already 0).
fn clear_full_document_busy(state: &AppState, doc_id: u32) {
    let prev = state.full_document_busy.swap(0, Ordering::AcqRel);
    if prev > 0 {
        emit_full_document_busy(state, false, doc_id);
    }
}

fn emit_full_document_busy(state: &AppState, busy: bool, doc_id: u32) {
    if crate::tile_gen_trace::enabled() {
        let pending = state
            .pending_preview_refresh
            .lock()
            .ok()
            .map(|g| g.is_some())
            .unwrap_or(false);
        crate::tile_gen_trace::emit(
            "busy_changed",
            None,
            None,
            None,
            None,
            None,
            serde_json::json!({
                "busy": busy,
                "doc_id": doc_id,
                "queued": state.tiles.scheduler.queued_len(),
                "inflight": state.preview_pass_inflight.load(Ordering::Acquire),
                "pending_refresh": pending,
            }),
        );
    }
    let Ok(guard) = state.app_handle.lock() else {
        return;
    };
    let Some(ref app) = *guard else {
        return;
    };
    let payload = FullDocumentBusyPayload { busy, doc_id };
    let _ = app.emit_to(tauri::EventTarget::Any, "full-document-busy", payload);
}

#[allow(dead_code)] // reserved for explicit full-cache reset on new-doc paths
pub(crate) fn reset_tiles_for_new_document(state: &AppState) {
    state.tiles.tile_cache.clear();
    state.tiles.scheduler.clear_all();
    state.tiles.ed_frontier.clear();
    state.tiles.block_representatives.invalidate_all();
    state.tiles.error_residuals.clear();
    state.tiles.full_document.clear();
    if let Ok(mut pending) = state.pending_preview_refresh.lock() {
        *pending = None;
    }
}

pub(crate) fn invalidate_after_document_replace(state: &AppState) {
    let mut keys = Vec::new();
    for entry in state.tiles.tile_cache.entries.iter() {
        let key = *entry.key();
        if matches!(key.stage, CacheStage::Processed | CacheStage::Composite) {
            keys.push(key);
        }
    }
    for key in keys {
        state.tiles.tile_cache.mark_dirty(key);
    }
    state.tiles.block_representatives.invalidate_all();
    if let Ok(session) = state.active_session() {
        state
            .tiles
            .error_residuals
            .evict_document(session.document_handle.snapshot().id.0);
    } else {
        state.tiles.error_residuals.clear();
    }
}

pub(crate) fn schedule_dirty_viewport_tiles(state: &AppState) {
    schedule_dirty_viewport_tiles_with_reason(state, "unspecified");
}

pub(crate) fn schedule_dirty_viewport_tiles_with_reason(state: &AppState, reason: &'static str) {
    use std::sync::atomic::Ordering;

    let viewport = state.ui.viewport.lock().unwrap().clone();
    let Ok(snapshot) = state.active_session().map(|s| s.document_handle.snapshot()) else {
        return;
    };
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let (vp_hash, x0, y0, x1, y1) =
        crate::tile_gen_trace::vp_hash_from_coords(viewport.level, &viewport.visible_tiles);
    let visible_n = viewport.visible_tiles.len();

    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit(
            "schedule_enter",
            None,
            Some(doc_gen),
            None,
            None,
            Some(doc_gen),
            serde_json::json!({
                "reason": reason,
                "level": viewport.level,
                "zoom": viewport.zoom,
                "x0": x0, "y0": y0, "x1": x1, "y1": y1,
                "visible_count": visible_n,
                "vp_hash": vp_hash,
            }),
        );
    }

    crate::tile_pipeline::schedule_ed_for_viewport(state);

    let gpu_authored_l0 = crate::gpu_resident_shadow::try_publish_gpu_preview_viewport(state);

    let mut dirty = 0usize;
    let mut enqueued = 0usize;
    let mut skipped_gpu = 0usize;
    let mut skipped_clean = 0usize;

    for coord in &viewport.visible_tiles {
        if gpu_authored_l0.contains(coord) {
            skipped_gpu += 1;
            continue;
        }

        let key = TileKey {
            doc: snapshot.id.0,
            layer: 0,
            coord: *coord,
            stage: CacheStage::Composite,
        };

        let is_dirty = match state.tiles.tile_cache.entries.get(&key) {
            Some(entry) => entry.dirty.load(Ordering::Acquire),
            None => true,
        };

        if is_dirty {
            dirty += 1;
            let task = RecomputeTask {
                key,
                generation: doc_gen,
                layer_generation: 0,
                priority: Priority::Immediate,
            };
            state.tiles.scheduler.enqueue_dedup(task);
            state.worker_wake.notify_one();
            enqueued += 1;
            if crate::tile_gen_trace::enabled() {
                crate::tile_gen_trace::emit(
                    "tile_enqueued",
                    None,
                    Some(doc_gen),
                    None,
                    Some(crate::tile_gen_trace::tile(coord.level, coord.x, coord.y)),
                    Some(doc_gen),
                    serde_json::json!({
                        "reason": reason,
                        "set": "visible",
                        "vp_hash": vp_hash,
                    }),
                );
            }
        } else {
            skipped_clean += 1;
        }
    }

    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit(
            "schedule_result",
            None,
            Some(doc_gen),
            None,
            None,
            Some(doc_gen),
            serde_json::json!({
                "reason": reason,
                "vp_hash": vp_hash,
                "level": viewport.level,
                "visible_count": visible_n,
                "dirty": dirty,
                "enqueued": enqueued,
                "skipped_gpu_authored": skipped_gpu,
                "skipped_clean_or_cached": skipped_clean,
            }),
        );
    }

    crate::gpu_resident_shadow::enqueue_resident_shadow_viewport(state);
}

fn preview_pass_busy(state: &AppState) -> bool {
    state.preview_pass_inflight.load(Ordering::Acquire) > 0
        || state.tiles.scheduler.queued_len() > 0
}

pub(crate) fn layer_needs_dither_cache_reset(
    nodes: &[engine_project::LayerNode],
    layer_id: u32,
) -> bool {
    use engine_project::filter::FilterParams;
    for node in nodes {
        match node {
            engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                return layer.filters.iter().any(|f| {
                    f.enabled
                        && matches!(
                            f.params,
                            FilterParams::DitherV2(_)
                                | FilterParams::Dither { .. }
                                | FilterParams::Ascii(_)
                        )
                });
            }
            engine_project::LayerNode::Group(group)
                if layer_needs_dither_cache_reset(&group.children, layer_id) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn request_preview_refresh(state: &AppState, layer_id: u32, clear_residuals: bool) {
    if preview_pass_busy(state) {
        let mut pending = state.pending_preview_refresh.lock().unwrap();
        let clear = clear_residuals || pending.as_ref().map(|p| p.clear_residuals).unwrap_or(false);
        *pending = Some(PendingPreviewRefresh {
            layer_id,
            clear_residuals: clear,
        });
        if crate::tile_gen_trace::enabled() {
            crate::tile_gen_trace::emit(
                "refresh_requested",
                Some(layer_id),
                None,
                None,
                None,
                None,
                serde_json::json!({
                    "coalesced": true,
                    "pending_preview_refresh": {
                        "layer_id": layer_id,
                        "clear_residuals": clear,
                    },
                    "queued": state.tiles.scheduler.queued_len(),
                    "inflight": state.preview_pass_inflight.load(Ordering::Acquire),
                }),
            );
        }
        return;
    }
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit(
            "refresh_requested",
            Some(layer_id),
            None,
            None,
            None,
            None,
            serde_json::json!({
                "coalesced": false,
                "pending_preview_refresh": null,
            }),
        );
    }
    run_preview_refresh(state, layer_id, clear_residuals, "param_change");
}

fn run_preview_refresh(
    state: &AppState,
    layer_id: u32,
    clear_residuals: bool,
    reason: &'static str,
) {
    let Ok(session) = state.active_session() else {
        return;
    };
    let doc_id = session.document_handle.snapshot().id.0;
    if clear_residuals {
        state
            .tiles
            .error_residuals
            .evict_layer(doc_id, engine_project::types::LayerId::new(layer_id));
        state
            .tiles
            .block_representatives
            .evict_layer(doc_id, layer_id);
        state.tiles.full_document.invalidate_layer(doc_id, layer_id);
    }
    let dirty_before = state.tiles.tile_cache.entries.len();
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
            doc: doc_id,
            layer: layer_id,
        },
    );
    // Hold the corner spinner for the whole filter refresh wave (CPU tiled ED
    // included). Nested full-document begin/end inside workers nest on top.
    begin_full_document_busy(state, doc_id);
    let queued_before = state.tiles.scheduler.queued_len();
    schedule_dirty_viewport_tiles_with_reason(state, reason);
    crate::ed_progress::note_ed_pass_start(state, doc_id);
    let queued_after = state.tiles.scheduler.queued_len();
    if crate::tile_gen_trace::enabled() {
        let snap = session.document_handle.snapshot();
        let doc_gen = snap
            .generations
            .document_gen
            .load(Ordering::Acquire);
        crate::tile_gen_trace::emit(
            "refresh_started",
            Some(layer_id),
            Some(doc_gen),
            Some(snap.generations.get_layer_gen(layer_id)),
            None,
            Some(doc_gen),
            serde_json::json!({
                "cache_entries": dirty_before,
                "queued_before": queued_before,
                "queued_after": queued_after,
                "enqueued_delta": queued_after.saturating_sub(queued_before),
                "clear_residuals": clear_residuals,
                "schedule_reason": reason,
            }),
        );
    }
    if !preview_pass_busy(state) {
        // Nothing queued (empty viewport / already fresh) — don't leave the chip on.
        end_full_document_busy(state, doc_id);
    }
}

pub(crate) fn on_preview_task_finished(state: &AppState) {
    if preview_pass_busy(state) {
        return;
    }
    // Wave idle: clear the refresh-held busy before a coalesced refresh restarts it.
    if let Ok(session) = state.active_session() {
        let doc_id = session.document_handle.snapshot().id.0;
        clear_full_document_busy(state, doc_id);
    }
    let pending = state.pending_preview_refresh.lock().unwrap().take();
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit(
            "pending_drained",
            pending.as_ref().map(|p| p.layer_id),
            None,
            None,
            None,
            None,
            serde_json::json!({
                "had_pending": pending.is_some(),
                "pending": pending.as_ref().map(|p| serde_json::json!({
                    "layer_id": p.layer_id,
                    "clear_residuals": p.clear_residuals,
                })),
            }),
        );
    }
    if let Some(p) = pending {
        run_preview_refresh(state, p.layer_id, p.clear_residuals, "deferred_refresh");
    }
}

#[cfg(test)]
pub(crate) fn make_test_app_state() -> Arc<AppState> {
    use engine_project::types::DocumentId;
    use engine_project::Document;

    let state = AppState::empty_process(None, 512 * 1024 * 1024, true);
    let mut doc = Document::new(DocumentId::new(1), 800, 600);
    doc.root.push(engine_project::layer::LayerNode::Leaf(
        engine_project::layer::Layer::new(
            engine_project::types::LayerId::new(1),
            engine_project::types::LayerKind::Raster,
            800,
            600,
        ),
    ));
    state.spawn_session(doc);
    Arc::new(state)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod workspace_ipc_tests;
