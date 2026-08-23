pub mod diagnostics;
pub use diagnostics::*;
pub mod panels;
pub use panels::*;
pub mod selection;
pub use selection::*;
pub mod viewport;
pub use viewport::*;
pub mod undo;
pub use undo::*;
pub mod layers;
pub use layers::*;
pub mod filters;
pub use filters::*;
pub mod palette;
pub use palette::*;
pub mod color_lab;
pub use color_lab::*;
pub mod document;
pub use document::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use engine_project::{
    document::DocumentHandle,
    dto::DocumentSnapshotDto,
    types::{LayerId, LayerKind, BlendMode},
    commands::{AddLayerArgs, LayerPropsPatch},
    commands as engine_commands,
};
use engine_tiles::{PixelTile, TileCache, Scheduler};
use engine_tiles::{CacheStage, Priority, RecomputeTask, TileKey};

use crate::panel_manager::PanelManager;
use crate::worker::WorkerWake;
use crate::document_session::{emit_tabs_changed, OpenDocumentsPayload};

pub use crate::services::palette_service::{hex_to_linear, linear_to_hex};
pub use crate::services::document_service::{
    f32_to_u8, encode_rgba_to_png, validate_document_dimensions, place_image_at_origin,
    blank_rgba_f32, MAX_DOCUMENT_DIMENSION, IMAGE_IMPORT_EXTENSIONS, BlankBackground,
    LoadImageResponse, SaveProjectResponse, OpenProjectResponse, ExportPatternRequest,
    ImportPatternRequest, ImportPatternResponse, ExportImageRequest, DocumentResponse,
};
pub use crate::services::palette_service::find_layers_referencing_palette;
pub use crate::commands::color_lab::{oklab_points_from_hexes, oklab_points_from_linear, OklabPointDto};

pub use crate::viewport::ViewportState;

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
    pub gpu: Option<std::sync::Arc<engine_gpu::GpuContext>>,
    pub gpu_resident: Option<std::sync::Arc<engine_gpu::GpuTileCache>>,
    pub gpu_executor: Option<std::sync::Mutex<engine_gpu::GpuExecutor>>,
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub ui: crate::state::UiState,
    pub dock_affinity: Mutex<crate::dock_affinity::DockAffinityController>,
    pub float_drag_mouseup_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub float_drag_mouseup_hook: Mutex<Option<crate::global_mouseup::MouseUpHook>>,
    pub preview_pass_inflight: AtomicUsize,
    pub pending_preview_refresh: Mutex<Option<PendingPreviewRefresh>>,
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

pub(crate) fn reset_tiles_for_new_document(state: &AppState) {
    state.tiles.tile_cache.clear();
    state.tiles.scheduler.clear_all();
    state.tiles.ed_frontier.clear();
    state.tiles.block_representatives.invalidate_all();
    state.tiles.error_residuals.clear();
    if let Ok(mut pending) = state.pending_preview_refresh.lock() {
        *pending = None;
    }
}

pub(crate) fn invalidate_after_document_replace(state: &AppState) {
    use engine_tiles::CacheStage;

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
            .tiles.error_residuals
            .evict_document(session.document_handle.snapshot().id.0);
    } else {
        state.tiles.error_residuals.clear();
    }
}

pub(crate) fn schedule_dirty_viewport_tiles(state: &AppState) {
    use std::sync::atomic::Ordering;

    let viewport = state.ui.viewport.lock().unwrap().clone();
    let Ok(snapshot) = state.active_session().map(|s| s.document_handle.snapshot()) else {
        return;
    };
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

    crate::tile_pipeline::schedule_ed_for_viewport(state);

    let gpu_authored_l0 = crate::gpu_resident_shadow::try_publish_gpu_preview_viewport(state);

    for coord in &viewport.visible_tiles {
        if gpu_authored_l0.contains(coord) {
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
            let task = RecomputeTask {
                key,
                generation: doc_gen,
                layer_generation: 0,
                priority: Priority::Immediate,
            };
            state.tiles.scheduler.enqueue_dedup(task);
            state.worker_wake.notify_one();
        }
    }

    crate::gpu_resident_shadow::enqueue_resident_shadow_viewport(state);
}

fn preview_pass_busy(state: &AppState) -> bool {
    state.preview_pass_inflight.load(Ordering::Acquire) > 0
        || state.tiles.scheduler.queued_len() > 0
}

pub(crate) fn layer_needs_dither_cache_reset(nodes: &[engine_project::LayerNode], layer_id: u32) -> bool {
    use engine_project::filter::FilterParams;
    for node in nodes {
        match node {
            engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                return layer.filters.iter().any(|f| {
                    f.enabled
                        && matches!(
                            f.params,
                            FilterParams::DitherV2(_) | FilterParams::Dither { .. }
                        )
                });
            }
            engine_project::LayerNode::Group(group) => {
                if layer_needs_dither_cache_reset(&group.children, layer_id) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn request_preview_refresh(state: &AppState, layer_id: u32, clear_residuals: bool) {
    if preview_pass_busy(state) {
        let mut pending = state.pending_preview_refresh.lock().unwrap();
        let clear = clear_residuals
            || pending
                .as_ref()
                .map(|p| p.clear_residuals)
                .unwrap_or(false);
        *pending = Some(PendingPreviewRefresh {
            layer_id,
            clear_residuals: clear,
        });
        return;
    }
    run_preview_refresh(state, layer_id, clear_residuals);
}

fn run_preview_refresh(state: &AppState, layer_id: u32, clear_residuals: bool) {
    let Ok(session) = state.active_session() else {
        return;
    };
    if clear_residuals {
        let doc = session.document_handle.snapshot().id.0;
        state
            .tiles.error_residuals
            .evict_layer(doc, engine_project::types::LayerId::new(layer_id));
        state.tiles.block_representatives.evict_layer(doc, layer_id);
    }
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
            doc: session.document_handle.snapshot().id.0,
            layer: layer_id,
        },
    );
    schedule_dirty_viewport_tiles(state);
}

pub(crate) fn on_preview_task_finished(state: &AppState) {
    if preview_pass_busy(state) {
        return;
    }
    let pending = state.pending_preview_refresh.lock().unwrap().take();
    if let Some(p) = pending {
        run_preview_refresh(state, p.layer_id, p.clear_residuals);
    }
}

pub fn is_release_build() -> bool {
    !cfg!(debug_assertions)
}

#[cfg(test)]
pub(crate) fn make_test_app_state() -> Arc<AppState> {
    use engine_project::Document;
    use engine_project::types::DocumentId;

    let state = AppState::empty_process(None, 512 * 1024 * 1024, true);
    state.spawn_session(Document::new(DocumentId::new(1), 800, 600));
    Arc::new(state)
}


#[cfg(test)]
mod tests;
