//! Tauri command handlers for document operations.
//!
//! This module registers the public IPC endpoints that the frontend calls.
//! Each command acquires the DocumentHandle and TileCache from app state,
//! delegates to engine-project for mutations, and returns DTOs for serialization.

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

// ============================================================================
// Data Structures for Command Arguments
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AddLayerRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub kind: String, // "raster" or "adjustment"
    pub parent_group: Option<u32>,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetLayerPropsRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub name: Option<String>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<String>,
    pub visible: Option<bool>,
    pub offset: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderLayerRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub new_parent: Option<u32>,
    pub new_index: usize,
}

/// Patch DTO for layer property updates (design spec).
/// All fields are optional; only set values are applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerPropsPatchDto {
    pub name: Option<String>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<String>,
    pub visible: Option<bool>,
}

/// Flat layer node DTO for frontend layer panel consumption.
/// Groups have `children: Some(vec![...])`, leaves have `children: None`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerNodeDto {
    pub id: u32,
    pub name: String,
    pub kind: String,         // "raster" | "adjustment" | "group"
    pub blend_mode: String,
    pub opacity: f32,
    pub visible: bool,
    pub children: Option<Vec<LayerNodeDto>>,
}

// ============================================================================
// Command Responses
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct LayerIdResponse {
    pub layer_id: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentResponse {
    pub snapshot: DocumentSnapshotDto,
}

// ============================================================================
// App State Structure
// ============================================================================

// ViewportState is defined in the viewport module.
pub use crate::viewport::ViewportState;

/// Cross-window selection state. Updated via selection-changed events.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SelectionState {
    pub selected_layer_id: Option<u32>,
    pub selected_filter_id: Option<String>,
}

/// Payload emitted with the `selection-changed` Tauri event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectionChangedPayload {
    pub selected_layer_id: Option<u32>,
    pub selected_filter_id: Option<String>,
}

/// Coalesced preview refresh requested while a tile pass is still in-flight.
pub(crate) struct PendingPreviewRefresh {
    pub layer_id: u32,
    pub clear_residuals: bool,
}

/// Shared application state for Tauri commands.
pub struct AppState {
    pub sessions: Mutex<HashMap<u32, Arc<crate::document_session::DocumentSession>>>,
    pub next_doc_id: AtomicU32,
    pub active_id: Mutex<Option<u32>>,
    pub tile_cache: TileCache,
    pub scheduler: Scheduler,
    pub viewport: Mutex<ViewportState>,
    pub worker_wake: WorkerWake,
    pub palette_cache: engine_color::palette_cache::PaletteKdCache,
    pub palette_lut_cache: engine_color::palette_lut::PaletteLutCache,
    pub threshold_cache: engine_color::threshold_map::ThresholdMapCache,
    pub error_residuals: engine_project::filters::ErrorResidualsStore,
    pub block_representatives: engine_tiles::BlockRepresentativeCache,
    /// ED wavefront: blocked Processed/Composite waiting on deps.
    pub ed_frontier: engine_tiles::EdFrontier,
    /// Track D: optional GPU compute context (None = CPU-only / no adapter).
    pub gpu: Option<std::sync::Arc<engine_gpu::GpuContext>>,
    /// Path B: GPU-resident tile cache (VRAM slots); None without adapter.
    pub gpu_resident: Option<std::sync::Arc<engine_gpu::GpuTileCache>>,
    /// Path B: dedicated submit thread; None without adapter.
    pub gpu_executor: Option<std::sync::Mutex<engine_gpu::GpuExecutor>>,
    /// Set once in app setup — used to emit `tile-ready` from GPU preview publish.
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub panel_manager: Mutex<PanelManager>,
    pub selection: Mutex<SelectionState>,
    pub dock_affinity: Mutex<crate::dock_affinity::DockAffinityController>,
    /// Cancels the active global mouseup watcher (set on end/cancel).
    pub float_drag_mouseup_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Keeps macOS NSEvent monitors alive for the active float-drag session.
    pub float_drag_mouseup_hook: Mutex<Option<crate::global_mouseup::MouseUpHook>>,
    /// Workers currently executing a dequeued task (including stale discards).
    pub preview_pass_inflight: AtomicUsize,
    /// Latest filter-driven preview refresh to run once the current pass drains.
    pub pending_preview_refresh: Mutex<Option<PendingPreviewRefresh>>,
}

/// Cmd+Q / Dock Quit must not terminate until the frontend save dialog finishes.
pub struct QuitGuard {
    pub allow_exit: AtomicBool,
}

// ============================================================================
// Helpers
// ============================================================================

/// Payload for the document-changed event.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentChangedPayload {
    pub kind: String,
    pub layer_id: Option<u32>,
    pub doc_id: Option<u32>,
}

/// Helper to emit document-changed to all windows (including floating panels).
/// `doc_id` should be the document that changed (not “whoever is active now”).
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

/// Parse a 6-character hex string to LinearColor.
/// Case-insensitive. Returns Err for invalid format.
fn hex_to_linear(hex: &str) -> Result<engine_color::palette::LinearColor, String> {
    use engine_color::palette::{srgb_to_linear, LinearColor};

    if hex.len() != 6 {
        return Err("Hex color must be exactly 6 characters".to_string());
    }
    let r = u8::from_str_radix(&hex[0..2], 16)
        .map_err(|_| "Invalid hex character in red channel".to_string())?;
    let g = u8::from_str_radix(&hex[2..4], 16)
        .map_err(|_| "Invalid hex character in green channel".to_string())?;
    let b = u8::from_str_radix(&hex[4..6], 16)
        .map_err(|_| "Invalid hex character in blue channel".to_string())?;
    Ok(LinearColor {
        r: srgb_to_linear(r),
        g: srgb_to_linear(g),
        b: srgb_to_linear(b),
    })
}

/// Convert LinearColor to 6-character uppercase hex string.
fn linear_to_hex(color: &engine_color::palette::LinearColor) -> String {
    use engine_color::palette::linear_to_srgb;

    let r = linear_to_srgb(color.r);
    let g = linear_to_srgb(color.g);
    let b = linear_to_srgb(color.b);
    format!("{:02X}{:02X}{:02X}", r, g, b)
}

/// Recursively find all layer IDs whose filters reference the given palette.
///
/// Walks the layer tree (including nested groups) and checks each layer's filter
/// stack for DitherV2 filters with a matching `palette_id` or PaletteQuantize
/// filters referencing the given palette.
fn find_layers_referencing_palette(
    nodes: &[engine_project::layer::LayerNode],
    palette_id: engine_project::types::PaletteId,
) -> Vec<engine_project::types::LayerId> {
    use engine_project::filter::FilterParams;
    use engine_project::layer::LayerNode;

    let mut result = Vec::new();
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                let references_palette = layer.filters.iter().any(|filter| {
                    match &filter.params {
                        FilterParams::DitherV2(params) => params.palette_id == Some(palette_id),
                        FilterParams::PaletteQuantize { palette_id: pid, .. } => *pid == palette_id,
                        _ => false,
                    }
                });
                if references_palette {
                    result.push(layer.id);
                }
            }
            LayerNode::Group(group) => {
                // Recurse into group children
                let mut child_results = find_layers_referencing_palette(&group.children, palette_id);
                result.append(&mut child_results);
            }
        }
    }
    result
}

/// Invalidate all layers whose filters reference the given palette_id.
///
/// Steps:
/// 1. Snapshot document
/// 2. Walk layer tree, find filters with matching palette_id
/// 3. For each affected layer, fire InvalidationEvent::LayerFilterChanged
/// 4. Schedule dirty viewport tiles
///
/// If no FilterInstance references the modified PaletteId, this is a no-op.
/// Does not block on tile recomputation; invalidation and scheduling complete synchronously.
fn invalidate_palette_changed(palette_id: engine_project::types::PaletteId, state: &AppState) {
    let Ok(snapshot) = state.active_session().map(|s| s.document_handle.snapshot()) else {
        return;
    };
    let affected_layers = find_layers_referencing_palette(&snapshot.root, palette_id);

    for layer_id in &affected_layers {
        engine_tiles::invalidation::invalidate(
            &state.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: snapshot.id.0, layer: layer_id.0,
            },
        );
    }

    if !affected_layers.is_empty() {
        schedule_dirty_viewport_tiles(state);
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================

#[tauri::command]
pub fn allow_app_exit(gate: State<'_, Arc<QuitGuard>>) {
    gate.allow_exit.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub fn confirm_app_quit(app: AppHandle, gate: State<'_, Arc<QuitGuard>>) {
    gate.allow_exit.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// Create a new document.
#[tauri::command]
pub fn new_document(
    width: u32,
    height: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    use engine_project::types::DocumentId;
    
    let new_doc = engine_project::Document::new(DocumentId::new(state.alloc_doc_id()), width, height);
    let session = state.spawn_session(new_doc);
    let doc_id = session.id.0;
    crate::undo::clear_history(&state, Some(&app_handle), doc_id)?;
    emit_tabs_changed(Some(&app_handle), &state);
    
    let snapshot = session.document_handle.snapshot();
    let dto = engine_project::dto::document_to_dto(&snapshot);
    
    Ok(DocumentResponse { snapshot: dto })
}

/// Get current document snapshot.
#[tauri::command]
pub fn get_document_snapshot(
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    let Ok(session) = state.active_session() else {
        let empty = engine_project::Document::new(engine_project::types::DocumentId::new(0), 0, 0);
        let dto = engine_project::dto::document_to_dto(&empty);
        return Ok(DocumentResponse { snapshot: dto });
    };
    let snapshot = session.document_handle.snapshot();
    let dto = engine_project::dto::document_to_dto(&snapshot);
    Ok(DocumentResponse { snapshot: dto })
}/// Get the layer tree as a flat DTO structure for frontend consumption.
///
/// Returns the full layer hierarchy as `Vec<LayerNodeDto>`, where groups
/// have `children: Some(vec![...])` and leaves have `children: None`.
#[tauri::command]
pub fn get_layer_tree(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<LayerNodeDto>, String> {
    let Ok(session) = state.active_session() else {
        return Ok(vec![]);
    };
    let snapshot = session.document_handle.snapshot();
    let tree = layer_nodes_to_dto(&snapshot.root);
    Ok(tree)
}/// Convert internal LayerNode tree to flat LayerNodeDto tree.
fn layer_nodes_to_dto(nodes: &[engine_project::LayerNode]) -> Vec<LayerNodeDto> {
    nodes.iter().map(layer_node_to_flat_dto).collect()
}

/// Convert a single LayerNode to a flat LayerNodeDto.
fn layer_node_to_flat_dto(node: &engine_project::LayerNode) -> LayerNodeDto {
    match node {
        engine_project::LayerNode::Leaf(layer) => {
            let kind = match layer.kind {
                LayerKind::Raster => "raster",
                LayerKind::Adjustment => "adjustment",
            };
            LayerNodeDto {
                id: layer.id.0,
                name: layer.name.clone(),
                kind: kind.to_string(),
                blend_mode: layer.blend_mode.to_string(),
                opacity: layer.opacity,
                visible: layer.visible,
                children: None,
            }
        }
        engine_project::LayerNode::Group(group) => {
            let children = layer_nodes_to_dto(&group.children);
            LayerNodeDto {
                id: group.id.0,
                name: group.name.clone(),
                kind: "group".to_string(),
                blend_mode: group.blend_mode.to_string(),
                opacity: group.opacity,
                visible: group.visible,
                children: Some(children),
            }
        }
    }
}

/// Add a new layer to the document.
#[tauri::command]
pub fn add_layer(
    req: AddLayerRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<LayerIdResponse, String> {
    let doc_id = req.doc_id;
    let kind = match req.kind.as_str() {
        "raster" => LayerKind::Raster,
        "adjustment" => LayerKind::Adjustment,
        _ => return Err("Invalid layer kind".to_string()),
    };

    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let width = snapshot.width;
        let height = snapshot.height;
        let engine_doc_id = snapshot.id;
        drop(snapshot);

        let args = AddLayerArgs {
            kind,
            parent_group: req.parent_group.map(LayerId::new),
            index: req.index,
            width,
            height,
        };

        match engine_commands::add_layer(&state.require_session(doc_id)?.document_handle, &state.tile_cache, engine_doc_id, args) {
            Ok(layer_id) => {
                emit_document_changed(&app_handle, "layer_added", Some(layer_id.0), Some(doc_id));
                Ok(LayerIdResponse { layer_id: layer_id.0 })
            }
            Err(e) => Err(format!("Failed to add layer: {:?}", e)),
        }
    })
}

/// Remove a layer from the document.
#[tauri::command]
pub fn remove_layer(
    doc_id: u32,
    layer_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let engine_doc_id = snapshot.id;
        drop(snapshot);

        match engine_commands::remove_layer(
            &state.require_session(doc_id)?.document_handle,
            &state.tile_cache,
            engine_doc_id,
            LayerId::new(layer_id),
        ) {
            Ok(_) => {
                emit_document_changed(&app_handle, "layer_removed", Some(layer_id), Some(doc_id));
                Ok(())
            }
            Err(e) => Err(format!("Failed to remove layer: {:?}", e)),
        }
    })
}

/// Set layer properties (name, opacity, blend mode, visibility, offset).
#[tauri::command]
pub fn set_layer_props(
    req: SetLayerPropsRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let doc_id = req.doc_id;
    let blend_mode = req.blend_mode.as_ref().map(|bm| {
        match bm.as_str() {
            "normal" => BlendMode::Normal,
            "multiply" => BlendMode::Multiply,
            "screen" => BlendMode::Screen,
            "overlay" => BlendMode::Overlay,
            "darken" => BlendMode::Darken,
            "lighten" => BlendMode::Lighten,
            "color_dodge" => BlendMode::ColorDodge,
            "color_burn" => BlendMode::ColorBurn,
            "hard_light" => BlendMode::HardLight,
            "soft_light" => BlendMode::SoftLight,
            "difference" => BlendMode::Difference,
            "exclusion" => BlendMode::Exclusion,
            _ => BlendMode::Normal,
        }
    });

    // Determine if this is a visual property change (requires Composite scheduling)
    let is_visual_change = req.opacity.is_some() || req.blend_mode.is_some() || req.visible.is_some();

    let layer_id = req.layer_id;

    let patch = LayerPropsPatch {
        name: req.name,
        opacity: req.opacity,
        blend_mode,
        visible: req.visible,
        offset: req.offset,
    };

    let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
    let engine_doc_id = snapshot.id;
    drop(snapshot);

    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        match engine_commands::set_layer_props(
            &state.require_session(doc_id)?.document_handle,
            &state.tile_cache,
            engine_doc_id,
            LayerId::new(layer_id),
            patch,
        ) {
            Ok(_) => {
                if is_visual_change && state.active_id() == Some(doc_id) {
                    schedule_dirty_viewport_tiles(&state);
                }
                emit_document_changed(&app_handle, "layer_changed", Some(layer_id), Some(doc_id));
                Ok(())
            }
            Err(e) => Err(format!("Failed to set layer props: {:?}", e)),
        }
    })
}

/// Reorder a layer (move to new parent/position).
#[tauri::command]
pub fn reorder_layer(
    req: ReorderLayerRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let doc_id = req.doc_id;
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let engine_doc_id = snapshot.id;
        drop(snapshot);

        match engine_commands::reorder_layer(
            &state.require_session(doc_id)?.document_handle,
            &state.tile_cache,
            engine_doc_id,
            LayerId::new(req.layer_id),
            req.new_parent.map(LayerId::new),
            req.new_index,
        ) {
            Ok(_) => {
                emit_document_changed(&app_handle, "layer_reordered", None, Some(doc_id));
                Ok(())
            }
            Err(e) => Err(format!("Failed to reorder layer: {:?}", e)),
        }
    })
}

// ============================================================================
// Filter Commands
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AddFilterRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub kind: String,
    pub params: serde_json::Value,
}

fn json_f32_adjust(params: &serde_json::Value, key: &str, default: f32) -> f32 {
    let slot = params
        .get("Adjust")
        .filter(|v| v.is_object())
        .unwrap_or(params);
    slot.get(key)
        .and_then(|v| v.as_f64())
        .map(|v| v as f32)
        .unwrap_or(default)
}

fn parse_adjust_params(
    params: &serde_json::Value,
    contrast: f32,
    brightness: f32,
    saturation: f32,
    blur: f32,
    sharpness: f32,
    noise: f32,
) -> engine_project::FilterParams {
    engine_project::FilterParams::Adjust {
        contrast: json_f32_adjust(params, "contrast", contrast),
        brightness: json_f32_adjust(params, "brightness", brightness),
        saturation: json_f32_adjust(params, "saturation", saturation),
        blur: json_f32_adjust(params, "blur", blur),
        sharpness: json_f32_adjust(params, "sharpness", sharpness),
        noise: json_f32_adjust(params, "noise", noise),
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateFilterRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub filter_id: String,
    pub params: serde_json::Value,
    #[serde(default)]
    pub opacity: Option<f32>,
    #[serde(default)]
    pub blend_mode: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoveFilterRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub filter_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterIdResponse {
    pub filter_id: String,
}

/// Add a filter to a layer.
#[tauri::command]
pub fn add_filter(
    req: AddFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<FilterIdResponse, String> {
    let doc_id = req.doc_id;
    use engine_project::{FilterKind, FilterParams, FilterInstance};
    use engine_project::filters::curves::CurveChannel;
    use engine_project::filter::{DitherMode, DiffusionKernel};
    use engine_project::filters::glitch::GlitchType;
    
    let kind = match req.kind.as_str() {
        "Curves" => FilterKind::Curves,
        "Levels" => FilterKind::Levels,
        "Dither" => FilterKind::Dither,
        "DitherV2" => FilterKind::Dither, // DitherV2 uses Dither kind with DitherV2 params
        "PaletteQuantize" => FilterKind::PaletteQuantize,
        "Glitch" => FilterKind::Glitch,
        "Glow" => FilterKind::Glow,
        "Crt" => FilterKind::Crt,
        "Adjust" => FilterKind::Adjust,
        _ => return Err("Invalid filter kind".to_string()),
    };

    // Parse params based on kind
    let params = match req.kind.as_str() {
        "DitherV2" => {
            // Parse DitherV2 params from JSON
            let dither_params: engine_project::filter::DitherParamsV2 =
                serde_json::from_value(req.params.clone())
                    .map_err(|e| format!("Invalid DitherV2 params: {}", e))?;
            dither_params.validate().map_err(|e| format!("{}", e))?;
            FilterParams::DitherV2(dither_params)
        }
        _ => match kind {
        FilterKind::Curves => {
            let channel = match req.params.get("channel").and_then(|v| v.as_str()).unwrap_or("All") {
                "Red" => CurveChannel::Red,
                "Green" => CurveChannel::Green,
                "Blue" => CurveChannel::Blue,
                "Luminance" => CurveChannel::Luminance,
                _ => CurveChannel::All,
            };
            if let Some(curve) = req.params.get("curve").and_then(|v| v.as_array()) {
                let curve_vec: Vec<(f32, f32)> = curve
                    .iter()
                    .filter_map(|v| {
                        if let Some(arr) = v.as_array() {
                            if arr.len() == 2 {
                                let x = arr[0].as_f64().map(|v| v as f32)?;
                                let y = arr[1].as_f64().map(|v| v as f32)?;
                                return Some((x, y));
                            }
                        }
                        None
                    })
                    .collect();
                FilterParams::Curves { curve: curve_vec, channel }
            } else {
                FilterParams::Curves { curve: vec![(0.0, 0.0), (1.0, 1.0)], channel }
            }
        }
        FilterKind::Levels => {
            let input_black = req.params.get("input_black").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let input_white = req.params.get("input_white").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let gamma = req.params.get("gamma").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let output_black = req.params.get("output_black").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let output_white = req.params.get("output_white").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let channel_r = req.params.get("channel_r").and_then(|v| v.as_bool()).unwrap_or(true);
            let channel_g = req.params.get("channel_g").and_then(|v| v.as_bool()).unwrap_or(true);
            let channel_b = req.params.get("channel_b").and_then(|v| v.as_bool()).unwrap_or(true);
            FilterParams::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
                channel_r,
                channel_g,
                channel_b,
            }
        }
        FilterKind::Dither => {
            let mode = match req.params.get("mode").and_then(|v| v.as_str()).unwrap_or("ErrorDiffusion") {
                "Bayer" => {
                    let matrix_size = req.params.get("matrix_size").and_then(|v| v.as_u64()).unwrap_or(4) as u8;
                    DitherMode::Bayer { matrix_size }
                }
                "ThresholdMap" => {
                    let path = req.params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    DitherMode::ThresholdMap { path }
                }
                _ => {
                    let name = req.params.get("kernel").and_then(|v| v.as_str()).unwrap_or("FloydSteinberg");
                    let kernel = DiffusionKernel::from_ui_name(name).unwrap_or(DiffusionKernel::FloydSteinberg);
                    DitherMode::ErrorDiffusion { kernel }
                }
            };
            let color_depth = req.params.get("color_depth").and_then(|v| v.as_u64()).unwrap_or(4) as u8;
            if !(1..=8).contains(&color_depth) {
                return Err("Color depth must be 1-8 bits".to_string());
            }
            FilterParams::Dither { mode, color_depth }
        }
        FilterKind::PaletteQuantize => {
            let palette_id = req.params.get("palette_id").and_then(|v| v.as_u64())
                .ok_or_else(|| "palette_id is required for PaletteQuantize".to_string())? as u32;
            let diffusion = req.params.get("diffusion").and_then(|v| v.as_str()).map(|s| {
                DiffusionKernel::from_ui_name(s).unwrap_or(DiffusionKernel::FloydSteinberg)
            });
            FilterParams::PaletteQuantize {
                palette_id: engine_project::PaletteId::new(palette_id),
                diffusion,
            }
        }
        FilterKind::Glitch => {
            let glitch_type = match req.params.get("glitch_type").and_then(|v| v.as_str()).unwrap_or("RGBShift") {
                "BlockDisplace" => GlitchType::BlockDisplace,
                _ => GlitchType::RGBShift,
            };
            let intensity = req.params.get("intensity").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let seed = req.params.get("seed").and_then(|v| v.as_u64()).unwrap_or(42);
            FilterParams::Glitch { glitch_type, intensity, seed }
        }
        FilterKind::Glow => {
            let radius = req.params.get("radius").and_then(|v| v.as_f64()).unwrap_or(2.0) as f32;
            let intensity = req.params.get("intensity").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let threshold = req.params.get("threshold").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            FilterParams::Glow { radius, intensity, threshold }
        }
        FilterKind::Crt => {
            let period = req.params.get("period").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
            let strength = req.params.get("strength").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let mask_strength = req.params.get("mask_strength").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            FilterParams::Crt { period, strength, mask_strength }
        }
        FilterKind::Adjust => parse_adjust_params(&req.params, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        FilterKind::Placeholder => FilterParams::Placeholder("unknown".to_string()),
    } }; // closes inner `match kind` and outer `match req.kind.as_str()`

    let filter = FilterInstance::new(kind, params);
    
    // Validate the filter parameters before adding
    filter.validate().map_err(|e| format!("{}", e))?;
    
    let filter_id = filter.id.to_string();

    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        // Add filter to layer in document
        let layer_id = req.layer_id;
        let mut found = false;

        // Clear error residuals for the affected layer when adding a DitherV2 filter
        // (Req 10.4: clear on filter parameter change)
        if matches!(&filter.params, FilterParams::DitherV2(_)) {
            state
                .error_residuals
                .evict_layer(doc_id, engine_project::types::LayerId::new(layer_id));
            state.block_representatives.clear_dithered();
        }

        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            // Find layer (recursing into groups) and add filter
            fn find_and_add_filter(nodes: &mut Vec<engine_project::LayerNode>, layer_id: u32, filter: FilterInstance) -> bool {
                for node in nodes.iter_mut() {
                    match node {
                        engine_project::LayerNode::Leaf(layer) => {
                            if layer.id.0 == layer_id {
                                layer.add_filter_instance(filter);
                                return true;
                            }
                        }
                        engine_project::LayerNode::Group(group) => {
                            if find_and_add_filter(&mut group.children, layer_id, filter.clone()) {
                                return true;
                            }
                        }
                    }
                }
                false
            }
            
            found = find_and_add_filter(&mut doc.root, layer_id, filter);
            if found {
                doc.increment_generation();
            }
        });

        if !found {
            return Err(format!("Layer {} not found", layer_id));
        }

        // Increment layer generation (requirement 10.1)
        {
            let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
            snapshot.generations.increment_layer_gen(layer_id);
        }

        // Invalidate tile cache for the affected layer (Processed + Composite cascade)
        let doc = state.require_session(doc_id)?.document_handle.snapshot().id.0;
        engine_tiles::invalidation::invalidate(
            &state.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc, layer: layer_id,
            },
        );

        // Schedule viewport-visible dirty tiles for immediate recomputation
        schedule_dirty_viewport_tiles(&state);

        emit_document_changed(&app_handle, "filter_added", Some(layer_id), Some(doc_id));

        Ok(FilterIdResponse { filter_id })
    })
}

/// Remove a filter from a layer.
#[tauri::command]
pub fn remove_filter(
    req: RemoveFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let doc_id = req.doc_id;
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let mut found = false;

        state.require_session(doc_id)?.document_handle.mutate(|doc| {
        fn find_and_remove_filter(nodes: &mut Vec<engine_project::LayerNode>, layer_id: u32, filter_id: &str) -> bool {
            for node in nodes.iter_mut() {
                match node {
                    engine_project::LayerNode::Leaf(layer) => {
                        if layer.id.0 == layer_id {
                            if let Some(idx) = layer.filters.iter().position(|f| f.id.to_string() == filter_id) {
                                layer.filters.remove(idx);
                                return true;
                            }
                            // Layer found but filter not found
                            return false;
                        }
                    }
                    engine_project::LayerNode::Group(group) => {
                        if find_and_remove_filter(&mut group.children, layer_id, filter_id) {
                            return true;
                        }
                    }
                }
            }
            false
        }

        found = find_and_remove_filter(&mut doc.root, req.layer_id, &req.filter_id);
        if found {
            doc.increment_generation();
        }
    });

    if !found {
        return Err(format!(
            "Filter '{}' not found on layer {}",
            req.filter_id, req.layer_id
        ));
    }

    request_preview_refresh(
        &state,
        req.layer_id,
        layer_needs_dither_cache_reset(&state.require_session(doc_id)?.document_handle.snapshot().root, req.layer_id),
    );

    emit_document_changed(&app_handle, "filter_removed", Some(req.layer_id), Some(doc_id));

        Ok(())
    })
}

// ============================================================================
// Reorder Filter Command
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderFilterRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub filter_id: String,
    pub new_index: usize,
}

/// Reorder a filter within a layer's filter stack.
#[tauri::command]
pub fn reorder_filter(
    req: ReorderFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let doc_id = req.doc_id;
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let mut success = false;

        state.require_session(doc_id)?.document_handle.mutate(|doc| {
        fn find_and_reorder(nodes: &mut Vec<engine_project::LayerNode>, layer_id: u32, filter_id: &str, new_index: usize) -> bool {
            for node in nodes.iter_mut() {
                match node {
                    engine_project::LayerNode::Leaf(layer) => {
                        if layer.id.0 == layer_id {
                            let current_idx = layer.filters.iter().position(|f| f.id.to_string() == filter_id);
                            if let Some(idx) = current_idx {
                                let clamped_new = new_index.min(layer.filters.len() - 1);
                                if idx != clamped_new {
                                    let filter = layer.filters.remove(idx);
                                    layer.filters.insert(clamped_new, filter);
                                }
                                return true;
                            }
                            return false;
                        }
                    }
                    engine_project::LayerNode::Group(group) => {
                        if find_and_reorder(&mut group.children, layer_id, filter_id, new_index) {
                            return true;
                        }
                    }
                }
            }
            false
        }

        success = find_and_reorder(&mut doc.root, req.layer_id, &req.filter_id, req.new_index);
        if success {
            doc.increment_generation();
        }
    });

    if !success {
        return Err(format!(
            "Filter '{}' not found on layer {}",
            req.filter_id, req.layer_id
        ));
    }

    // Invalidate and schedule recomputation since filter order affects output
    request_preview_refresh(
        &state,
        req.layer_id,
        layer_needs_dither_cache_reset(&state.require_session(doc_id)?.document_handle.snapshot().root, req.layer_id),
    );

    emit_document_changed(&app_handle, "filter_reordered", Some(req.layer_id), Some(doc_id));

        Ok(())
    })
}

// ============================================================================
// Update Filter Command
// ============================================================================

/// Update filter parameters on a layer.
#[tauri::command]
pub fn update_filter(
    req: UpdateFilterRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let doc_id = req.doc_id;
    use engine_project::{FilterKind, FilterParams, FilterInstance};
    use engine_project::filters::curves::CurveChannel;
    use engine_project::filter::{DitherMode, DiffusionKernel};
    use engine_project::filters::glitch::GlitchType;
    use engine_project::types::FilterInstanceId;

    // Parse the filter_id string into a UUID
    let uuid = uuid::Uuid::parse_str(&req.filter_id)
        .map_err(|e| format!("Invalid filter_id: {}", e))?;
    let filter_id = FilterInstanceId(uuid);

    // First, get the filter's kind so we know how to parse params
    let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
    let (filter_kind, is_dither_v2, existing_params) = {
        fn find_filter_kind(
            nodes: &[engine_project::LayerNode],
            layer_id: u32,
            filter_id: FilterInstanceId,
        ) -> Option<(FilterKind, bool, FilterParams)> {
            for node in nodes.iter() {
                match node {
                    engine_project::LayerNode::Leaf(layer) => {
                        if layer.id.0 == layer_id {
                            if let Some(filter) = layer.find_filter(filter_id) {
                                let is_dither_v2 = matches!(&filter.params, FilterParams::DitherV2(_));
                                return Some((filter.kind, is_dither_v2, filter.params.clone()));
                            }
                        }
                    }
                    engine_project::LayerNode::Group(group) => {
                        if let Some(result) = find_filter_kind(&group.children, layer_id, filter_id) {
                            return Some(result);
                        }
                    }
                }
            }
            None
        }
        let (kind, is_dither_v2, params) = find_filter_kind(&snapshot.root, req.layer_id, filter_id)
            .ok_or_else(|| format!(
                "Filter {} not found on layer {}",
                req.filter_id, req.layer_id
            ))?;
        (kind, is_dither_v2, params)
    };
    drop(snapshot);

    let params_empty = req.params.as_object().map(|o| o.is_empty()).unwrap_or(false);

    // Parse new params based on the filter's kind
    let new_params = if params_empty {
        existing_params
    } else if is_dither_v2 || (filter_kind == FilterKind::Dither && req.params.get("levels").is_some()) {
        // DitherV2 params: parse from JSON directly
        let dither_params: engine_project::filter::DitherParamsV2 =
            serde_json::from_value(req.params.clone())
                .map_err(|e| format!("Invalid DitherV2 params: {}", e))?;
        dither_params.validate().map_err(|e| format!("{}", e))?;
        FilterParams::DitherV2(dither_params)
    } else {
        match filter_kind {
        FilterKind::Curves => {
            let channel = match req.params.get("channel").and_then(|v| v.as_str()).unwrap_or("All") {
                "Red" => CurveChannel::Red,
                "Green" => CurveChannel::Green,
                "Blue" => CurveChannel::Blue,
                "Luminance" => CurveChannel::Luminance,
                _ => CurveChannel::All,
            };
            if let Some(curve) = req.params.get("curve").and_then(|v| v.as_array()) {
                let curve_vec: Vec<(f32, f32)> = curve
                    .iter()
                    .filter_map(|v| {
                        if let Some(arr) = v.as_array() {
                            if arr.len() == 2 {
                                let x = arr[0].as_f64().map(|v| v as f32)?;
                                let y = arr[1].as_f64().map(|v| v as f32)?;
                                return Some((x, y));
                            }
                        }
                        None
                    })
                    .collect();
                FilterParams::Curves { curve: curve_vec, channel }
            } else {
                FilterParams::Curves { curve: vec![(0.0, 0.0), (1.0, 1.0)], channel }
            }
        }
        FilterKind::Levels => {
            let input_black = req.params.get("input_black").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let input_white = req.params.get("input_white").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let gamma = req.params.get("gamma").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let output_black = req.params.get("output_black").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let output_white = req.params.get("output_white").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let channel_r = req.params.get("channel_r").and_then(|v| v.as_bool()).unwrap_or(true);
            let channel_g = req.params.get("channel_g").and_then(|v| v.as_bool()).unwrap_or(true);
            let channel_b = req.params.get("channel_b").and_then(|v| v.as_bool()).unwrap_or(true);
            FilterParams::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
                channel_r,
                channel_g,
                channel_b,
            }
        }
        FilterKind::Dither => {
            let mode = match req.params.get("mode").and_then(|v| v.as_str()).unwrap_or("ErrorDiffusion") {
                "Bayer" => {
                    let matrix_size = req.params.get("matrix_size").and_then(|v| v.as_u64()).unwrap_or(4) as u8;
                    DitherMode::Bayer { matrix_size }
                }
                "ThresholdMap" => {
                    let path = req.params.get("path").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    DitherMode::ThresholdMap { path }
                }
                _ => {
                    let name = req.params.get("kernel").and_then(|v| v.as_str()).unwrap_or("FloydSteinberg");
                    let kernel = DiffusionKernel::from_ui_name(name).unwrap_or(DiffusionKernel::FloydSteinberg);
                    DitherMode::ErrorDiffusion { kernel }
                }
            };
            let color_depth = req.params.get("color_depth").and_then(|v| v.as_u64()).unwrap_or(4) as u8;
            FilterParams::Dither { mode, color_depth }
        }
        FilterKind::PaletteQuantize => {
            let palette_id = req.params.get("palette_id").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            let diffusion = req.params.get("diffusion").and_then(|v| v.as_str()).map(|s| {
                DiffusionKernel::from_ui_name(s).unwrap_or(DiffusionKernel::FloydSteinberg)
            });
            FilterParams::PaletteQuantize {
                palette_id: engine_project::PaletteId::new(palette_id),
                diffusion,
            }
        }
        FilterKind::Glitch => {
            let glitch_type = match req.params.get("glitch_type").and_then(|v| v.as_str()).unwrap_or("RGBShift") {
                "BlockDisplace" => GlitchType::BlockDisplace,
                _ => GlitchType::RGBShift,
            };
            let intensity = req.params.get("intensity").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let seed = req.params.get("seed").and_then(|v| v.as_u64()).unwrap_or(42);
            FilterParams::Glitch { glitch_type, intensity, seed }
        }
        FilterKind::Glow => {
            let radius = req.params.get("radius").and_then(|v| v.as_f64()).unwrap_or(2.0) as f32;
            let intensity = req.params.get("intensity").and_then(|v| v.as_f64()).unwrap_or(1.0) as f32;
            let threshold = req.params.get("threshold").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            FilterParams::Glow { radius, intensity, threshold }
        }
        FilterKind::Crt => {
            let period = req.params.get("period").and_then(|v| v.as_u64()).unwrap_or(2) as u8;
            let strength = req.params.get("strength").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
            let mask_strength = req.params.get("mask_strength").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            FilterParams::Crt { period, strength, mask_strength }
        }
        FilterKind::Adjust => {
            let (ec, eb, es, ebl, esh, en) = match existing_params {
                FilterParams::Adjust {
                    contrast,
                    brightness,
                    saturation,
                    blur,
                    sharpness,
                    noise,
                } => (contrast, brightness, saturation, blur, sharpness, noise),
                _ => (0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            };
            parse_adjust_params(&req.params, ec, eb, es, ebl, esh, en)
        }
        FilterKind::Placeholder => FilterParams::Placeholder("unknown".to_string()),
    } }; // closes inner `match filter_kind` and outer `if is_dither_v2 ... else`

    // Validate new params before applying
    let mut temp_filter = FilterInstance::new(filter_kind, new_params.clone());
    if let Some(opacity) = req.opacity {
        temp_filter.opacity = opacity;
    }
    let parsed_blend = if let Some(ref name) = req.blend_mode {
        let mode = engine_project::BlendMode::from_name(name).ok_or_else(|| {
            format!("Invalid or reserved blend mode: {}", name)
        })?;
        temp_filter.blend_mode = mode;
        Some(mode)
    } else {
        None
    };
    temp_filter.validate().map_err(|e| format!("Invalid parameters: {}", e))?;

    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        // Apply the update within a document mutation
        let layer_id = req.layer_id;
        let mut found = false;
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
        fn update_filter_in_nodes(
            nodes: &mut Vec<engine_project::LayerNode>,
            layer_id: u32,
            filter_id: engine_project::types::FilterInstanceId,
            new_params: FilterParams,
            opacity: Option<f32>,
            blend_mode: Option<engine_project::BlendMode>,
            enabled: Option<bool>,
        ) -> bool {
            for node in nodes.iter_mut() {
                match node {
                    engine_project::LayerNode::Leaf(layer) => {
                        if layer.id.0 == layer_id {
                            if let Some(filter) = layer.find_filter_mut(filter_id) {
                                // Update requires_full_row based on new params
                                filter.requires_full_row =
                                    engine_project::FilterInstance::params_require_full_row(
                                        &new_params,
                                    );
                                filter.params = new_params;
                                if let Some(opacity) = opacity {
                                    filter.opacity = opacity;
                                }
                                if let Some(blend_mode) = blend_mode {
                                    filter.blend_mode = blend_mode;
                                }
                                if let Some(enabled) = enabled {
                                    filter.enabled = enabled;
                                }
                                return true;
                            }
                        }
                    }
                    engine_project::LayerNode::Group(group) => {
                        if update_filter_in_nodes(
                            &mut group.children,
                            layer_id,
                            filter_id,
                            new_params.clone(),
                            opacity,
                            blend_mode,
                            enabled,
                        ) {
                            return true;
                        }
                    }
                }
            }
            false
        }

        found = update_filter_in_nodes(
            &mut doc.root,
            layer_id,
            filter_id,
            new_params.clone(),
            req.opacity,
            parsed_blend,
            req.enabled,
        );
        if found {
            doc.increment_generation();
        }
    });

    if !found {
        return Err(format!(
            "Filter {} not found on layer {} during update",
            req.filter_id, req.layer_id
        ));
    }

    // Increment layer generation (requirement 10.1)
    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        snapshot.generations.increment_layer_gen(layer_id);
    }

    // Coalesce full invalidate/clear while a previous tile pass is still running.
    request_preview_refresh(
        &state,
        layer_id,
        layer_needs_dither_cache_reset(&state.require_session(doc_id)?.document_handle.snapshot().root, layer_id),
    );

    emit_document_changed(&app_handle, "filter_updated", Some(layer_id), Some(doc_id));

        Ok(())
    })
}

// ============================================================================
// Load Image / Create Document
// ============================================================================

/// Shared dimension cap for `load_image` and `create_document` (inclusive).
pub const MAX_DOCUMENT_DIMENSION: u32 = 8192;

/// Same raster types as Open Image (`load_image`).
pub const IMAGE_IMPORT_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp"];

/// Background fill for a blank document (`create_document`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BlankBackground {
    Transparent,
    White,
}

/// Response from the load_image / create_document commands.
#[derive(Debug, Clone, Serialize)]
pub struct LoadImageResponse {
    pub doc_id: u32,
    pub width: u32,
    pub height: u32,
    pub tile_count: u32,
}

/// Validate both axes in `1..=MAX_DOCUMENT_DIMENSION`. No panic on bad input.
pub fn validate_document_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width == 0 || height == 0 {
        return Err("Invalid state: image has zero dimensions".to_string());
    }
    if width > MAX_DOCUMENT_DIMENSION || height > MAX_DOCUMENT_DIMENSION {
        return Err(format!(
            "Invalid state: image dimensions {}x{} exceed maximum {}x{}",
            width, height, MAX_DOCUMENT_DIMENSION, MAX_DOCUMENT_DIMENSION
        ));
    }
    Ok(())
}

/// Decode a raster file to RGBA f32 (`u8 as f32 / 255.0`), same path as `load_image`.
fn decode_image_to_rgba_f32(path: &str) -> Result<(u32, u32, Vec<f32>), String> {
    let img = image::open(path).map_err(|e| format!("IO error: {e}"))?;
    let img_rgba = img.to_rgba8();
    let width = img_rgba.width();
    let height = img_rgba.height();
    validate_document_dimensions(width, height)?;

    let pixel_count = (width as usize) * (height as usize);
    let mut rgba_f32 = Vec::with_capacity(pixel_count * 4);
    for pixel in img_rgba.pixels() {
        rgba_f32.push(pixel[0] as f32 / 255.0);
        rgba_f32.push(pixel[1] as f32 / 255.0);
        rgba_f32.push(pixel[2] as f32 / 255.0);
        rgba_f32.push(pixel[3] as f32 / 255.0);
    }
    Ok((width, height, rgba_f32))
}

/// Place `src` at the document origin. Clip if larger; transparent remainder if smaller. No scale.
pub fn place_image_at_origin(
    src: &[f32],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<f32> {
    let mut dst = vec![0.0; (dst_w as usize) * (dst_h as usize) * 4];
    let copy_w = src_w.min(dst_w) as usize;
    let copy_h = src_h.min(dst_h) as usize;
    let src_stride = src_w as usize * 4;
    let dst_stride = dst_w as usize * 4;
    let row_bytes = copy_w * 4;
    for y in 0..copy_h {
        let src_row = y * src_stride;
        let dst_row = y * dst_stride;
        dst[dst_row..dst_row + row_bytes].copy_from_slice(&src[src_row..src_row + row_bytes]);
    }
    dst
}

/// RGBA f32 buffer in the same numeric space as `load_image` (`u8 as f32 / 255.0`).
pub fn blank_rgba_f32(width: u32, height: u32, background: BlankBackground) -> Vec<f32> {
    let n = (width as usize).saturating_mul(height as usize).saturating_mul(4);
    match background {
        BlankBackground::Transparent => vec![0.0; n],
        BlankBackground::White => vec![1.0; n],
    }
}

/// Decompose a raster buffer, replace the live document (one leaf, `project_path = None`).
/// Does not emit events or record Recent Files — callers do that.
fn install_raster_document(
    state: &AppState,
    width: u32,
    height: u32,
    rgba_f32: &[f32],
    app: Option<&AppHandle>,
) -> Result<LoadImageResponse, String> {
    use engine_project::types::DocumentId;
    use engine_tiles::decompose::decompose_image_to_tiles_at_generation;

    let doc_id = state.alloc_doc_id();
    let live_gen = 1u64;
    let layer_id = 1u32;
    let grid = decompose_image_to_tiles_at_generation(
        rgba_f32, width, height, doc_id, layer_id, &state.tile_cache, live_gen,
    )
    .map_err(|e| format!("Tile decomposition error: {}", e))?;

    let mut new_doc = engine_project::Document::new(DocumentId::new(doc_id), width, height);
    let layer = engine_project::layer::Layer::new(
        engine_project::types::LayerId::new(1),
        engine_project::types::LayerKind::Raster,
        width,
        height,
    );
    new_doc.root.push(engine_project::layer::LayerNode::Leaf(layer));
    new_doc.increment_generation();
    new_doc.generations.set_document_gen(live_gen);

    let session = state.spawn_session(new_doc);
    state.evict_inactive_for_pressure_if_needed();
    crate::undo::clear_history(state, app, doc_id)?;
    emit_tabs_changed(app, state);

    schedule_dirty_viewport_tiles(state);

    let _ = session;

    Ok(LoadImageResponse {
        doc_id,
        width,
        height,
        tile_count: grid.cols * grid.rows,
    })
}

/// Load an image from disk, decode it, split into tiles, and create a document.
#[tauri::command]
pub async fn load_image(
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<LoadImageResponse, String> {
    let recent_path = path.clone();

    // Do heavy I/O and CPU work in a blocking thread
    let (width, height, rgba_f32) = tauri::async_runtime::spawn_blocking(move || {
        decode_image_to_rgba_f32(&path)
    })
    .await
    .map_err(|e| format!("Load error: {}", e))??;

    let response = install_raster_document(&state, width, height, &rgba_f32, Some(&app_handle))?;

    emit_document_changed(&app_handle, "image_loaded", None, Some(response.doc_id));
    crate::recent_files::record_from_app(
        &app_handle,
        &recent_path,
        crate::recent_files::RecentFileKind::Image,
    );

    Ok(response)
}

/// Create a blank in-memory raster document. Does **not** record Recent Files.
#[tauri::command]
pub async fn create_document(
    width: u32,
    height: u32,
    background: BlankBackground,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<LoadImageResponse, String> {
    validate_document_dimensions(width, height)?;

    let rgba_f32 = tauri::async_runtime::spawn_blocking(move || {
        blank_rgba_f32(width, height, background)
    })
    .await
    .map_err(|e| format!("Create error: {e}"))?;

    let response = install_raster_document(&state, width, height, &rgba_f32, Some(&app_handle))?;
    emit_document_changed(&app_handle, "document_created", None, Some(response.doc_id));
    Ok(response)
}

/// Add a decoded raster as a new layer at the document origin (clip, no scale).
fn import_raster_layer(
    state: &AppState,
    doc_id: u32,
    src_w: u32,
    src_h: u32,
    src_rgba: &[f32],
    app: Option<&AppHandle>,
) -> Result<LayerIdResponse, String> {
    use engine_tiles::decompose::decompose_image_to_tiles_at_generation;

    let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
    if snapshot.root.is_empty() {
        return Err("No document open".to_string());
    }
    let dst_w = snapshot.width;
    let dst_h = snapshot.height;
    let engine_doc_id = snapshot.id;
    let insert_index = snapshot.root.len();
    drop(snapshot);

    let placed = place_image_at_origin(src_rgba, src_w, src_h, dst_w, dst_h);

    crate::undo::with_document_undo(state, app, doc_id, || {
        let args = AddLayerArgs {
            kind: LayerKind::Raster,
            parent_group: None,
            index: insert_index,
            width: dst_w,
            height: dst_h,
        };
        let layer_id = engine_commands::add_layer(
            &state.require_session(doc_id)?.document_handle,
            &state.tile_cache,
            engine_doc_id,
            args,
        )
        .map_err(|e| format!("Failed to add layer: {e:?}"))?;

        let live_gen = state
            .require_session(doc_id)?
            .document_handle
            .snapshot()
            .generations
            .current_document_gen();
        decompose_image_to_tiles_at_generation(
            &placed, dst_w, dst_h, doc_id, layer_id.0, &state.tile_cache, live_gen,
        )
            .map_err(|e| format!("Tile decomposition error: {e}"))?;

        state.evict_inactive_for_pressure_if_needed();

        if let Some(handle) = app {
            emit_document_changed(handle, "layer_added", Some(layer_id.0), Some(doc_id));
        }
        if state.active_id() == Some(doc_id) {
            schedule_dirty_viewport_tiles(state);
        }
        Ok(LayerIdResponse {
            layer_id: layer_id.0,
        })
    })
}

/// Import an image as a new raster layer without replacing the document.
#[tauri::command]
pub async fn import_image_layer(
    doc_id: u32,
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<LayerIdResponse, String> {
    use engine_io::sandbox;

    let resolved = sandbox::resolve_user_path(&path, IMAGE_IMPORT_EXTENSIONS)
        .map_err(|e| format!("Path error: {e}"))?;
    let resolved_str = resolved.to_string_lossy().into_owned();

    let (width, height, rgba_f32) = tauri::async_runtime::spawn_blocking(move || {
        decode_image_to_rgba_f32(&resolved_str)
    })
    .await
    .map_err(|e| format!("Load error: {e}"))??;

    import_raster_layer(&state, doc_id, width, height, &rgba_f32, Some(&app_handle))
}

// ============================================================================
// Project (.dyproj) Commands
// ============================================================================

/// Response from save_project / save_project_as.
#[derive(Debug, Clone, Serialize)]
pub struct SaveProjectResponse {
    pub path: String,
    pub size_warning: bool,
}

/// Response from open_project.
#[derive(Debug, Clone, Serialize)]
pub struct OpenProjectResponse {
    pub doc_id: u32,
    pub width: u32,
    pub height: u32,
    pub path: String,
}

/// Save the current document to an existing project path, or `path` if provided.
#[tauri::command]
pub async fn save_project(
    doc_id: u32,
    path: Option<String>,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<SaveProjectResponse, String> {
    let target = match path {
        Some(p) => p,
        None => {
            let session = state.require_session(doc_id)?;
            let guard = session
                .project_path
                .lock()
                .map_err(|e| format!("Lock error: {e}"))?;
            guard
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .ok_or_else(|| "Save As required: no project path set".to_string())?
        }
    };
    save_project_as(doc_id, target, app_handle, state).await
}

/// Save the current document to `path` and remember it as the project path.
#[tauri::command]
pub async fn save_project_as(
    doc_id: u32,
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<SaveProjectResponse, String> {
    use engine_io::sandbox;
    use engine_project::serialize::{read_png_file, save_project_to_path};
    use engine_project::serialize::ProjectError;

    let resolved = sandbox::resolve_export_path(&path, &["dyproj"])
        .map_err(|e| format!("Path error: {e}"))?;

    let session = state.require_session(doc_id)?;
    let _io_guard = session.begin_io();
    let snapshot = session.document_handle.snapshot();
    let doc = (*snapshot).clone();
    drop(snapshot);

    let state_arc = state.inner().clone();
    let resolved_clone = resolved.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        save_project_to_path(
            &resolved_clone,
            &doc,
            &state_arc.tile_cache,
            env!("CARGO_PKG_VERSION"),
            |p| read_png_file(p),
        )
    })
    .await
    .map_err(|e| format!("Save error: {e}"))?
    .map_err(|e| match e {
        ProjectError::IncompleteRaw { doc_id, layer_id } => format!(
            "Cannot save: image tiles missing from memory for document {doc_id} layer {layer_id} — reopen the file"
        ),
        other => format!("Save error: {other}"),
    })?;

    if let Ok(mut guard) = session.project_path.lock() {
        *guard = Some(resolved.clone());
    }

    let stored = resolved.to_string_lossy().into_owned();
    crate::recent_files::record_from_app(
        &app_handle,
        &stored,
        crate::recent_files::RecentFileKind::Project,
    );

    crate::undo::mark_clean(&state);
    crate::undo::emit_dirty(Some(&app_handle), &state);

    Ok(SaveProjectResponse {
        path: stored,
        size_warning: result.size_warning,
    })
}

/// Open a `.dyproj`, replacing the current document (same single-doc model as `load_image`).
#[tauri::command]
pub async fn open_project(
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<OpenProjectResponse, String> {
    use engine_io::sandbox;
    use engine_project::serialize::open_project_from_bytes;
    use engine_project::types::DocumentId;
    use engine_tiles::TileCache;
    use std::fs;

    let resolved = sandbox::resolve_user_path(&path, &["dyproj"])
        .map_err(|e| format!("Path error: {e}"))?;

    let zip_bytes = tauri::async_runtime::spawn_blocking({
        let resolved = resolved.clone();
        move || fs::read(&resolved).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Open error: {e}"))??;

    let runtime_id = state.alloc_doc_id();
    let staging = TileCache::new(state.tile_cache.budget_bytes_count());
    let opened = tauri::async_runtime::spawn_blocking(move || {
        open_project_from_bytes(&zip_bytes, &staging, DocumentId::new(runtime_id))
            .map(|r| (r, staging))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Open error: {e}"))??;

    let (opened, staging) = opened;
    let live_gen = 1u64;

    for entry in staging.entries.iter() {
        let key = *entry.key();
        let tile = entry.value().tile.clone();
        let _ = state.tile_cache.insert_fresh_gen(key, tile, live_gen);
    }

    let width = opened.document.width;
    let height = opened.document.height;
    let mut new_doc = opened.document;
    new_doc.increment_generation();
    new_doc.generations.set_document_gen(live_gen);
    let session = state.spawn_session(new_doc);
    state.evict_inactive_for_pressure_if_needed();
    let doc_id = runtime_id;
    crate::undo::clear_history(&state, Some(&app_handle), doc_id)?;
    crate::undo::mark_clean_doc(&state, doc_id);

    schedule_dirty_viewport_tiles(&state);

    if let Ok(mut guard) = session.project_path.lock() {
        *guard = Some(resolved.clone());
    }

    emit_document_changed(&app_handle, "project_opened", None, Some(doc_id));
    emit_tabs_changed(Some(&app_handle), &state);

    let stored = resolved.to_string_lossy().into_owned();
    crate::recent_files::record_from_app(
        &app_handle,
        &stored,
        crate::recent_files::RecentFileKind::Project,
    );

    Ok(OpenProjectResponse {
        doc_id: runtime_id,
        width,
        height,
        path: stored,
    })
}

// ============================================================================
// Pattern (.dyuki) Commands
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct ExportPatternRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub filter_instance_ids: Option<Vec<String>>,
    pub path: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImportPatternRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub path: String,
    pub target_layer_id: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPatternResponse {
    pub filter_ids: Vec<String>,
    pub palette_ids: Vec<u32>,
}

/// Export a layer's filter stack (or a subset, in stack order) as `.dyuki`.
#[tauri::command]
pub fn export_pattern(
    req: ExportPatternRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let doc_id = req.doc_id;
    use engine_io::sandbox;
    use engine_project::serialize::{
        export_pattern_from_document, read_png_file, write_pattern_to_path, PatternExportMeta,
    };
    use engine_project::types::FilterInstanceId;
    use uuid::Uuid;

    let resolved = sandbox::resolve_export_path(&req.path, &["dyuki"])
        .map_err(|e| format!("Path error: {e}"))?;

    let ids: Option<Vec<FilterInstanceId>> = match &req.filter_instance_ids {
        None => None,
        Some(list) if list.is_empty() => None,
        Some(list) => {
            let parsed = list
                .iter()
                .map(|s| {
                    Uuid::parse_str(s)
                        .map(FilterInstanceId)
                        .map_err(|e| format!("Invalid filter id '{s}': {e}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Some(parsed)
        }
    };

    let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
    let zip = export_pattern_from_document(
        &snapshot,
        engine_project::types::LayerId::new(req.layer_id),
        ids.as_deref(),
        &PatternExportMeta {
            name: req.name.unwrap_or_default(),
            description: req.description,
            author: None,
        },
        env!("CARGO_PKG_VERSION"),
        |p| read_png_file(p),
    )
    .map_err(|e| e.to_string())?;

    write_pattern_to_path(&resolved, &zip).map_err(|e| e.to_string())?;
    Ok(())
}

/// Import a `.dyuki` onto a leaf layer (append; always-new palettes and filters).
#[tauri::command]
pub fn import_pattern(
    req: ImportPatternRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ImportPatternResponse, String> {
    let doc_id = req.doc_id;
    use engine_io::sandbox;
    use engine_project::filter::FilterParams;
    use engine_project::serialize::import_pattern_into_document;
    use std::fs;

    let resolved = sandbox::resolve_user_path(&req.path, &["dyuki"])
        .map_err(|e| format!("Path error: {e}"))?;

    let zip_bytes = fs::read(&resolved).map_err(|e| format!("Read error: {e}"))?;

    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let mut imported_filters_are_dither = false;
        let mut err: Option<String> = None;
        let mut out: Option<engine_project::serialize::ImportPatternResult> = None;
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
        match import_pattern_into_document(
            &zip_bytes,
            doc,
            engine_project::types::LayerId::new(req.target_layer_id),
            env!("CARGO_PKG_VERSION"),
        ) {
            Ok(r) => {
                imported_filters_are_dither = {
                    fn has_dither(nodes: &[engine_project::LayerNode], layer_id: u32) -> bool {
                        for node in nodes {
                            match node {
                                engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                                    return layer
                                        .filters
                                        .iter()
                                        .any(|f| matches!(f.params, FilterParams::DitherV2(_)));
                                }
                                engine_project::LayerNode::Group(g) => {
                                    if has_dither(&g.children, layer_id) {
                                        return true;
                                    }
                                }
                                _ => {}
                            }
                        }
                        false
                    }
                    has_dither(&doc.root, req.target_layer_id)
                };
                doc.increment_generation();
                out = Some(r);
            }
            Err(e) => err = Some(e.to_string()),
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    let result = out.ok_or_else(|| "Import failed".to_string())?;

    if imported_filters_are_dither {
        state.error_residuals.evict_layer(
            doc_id,
            engine_project::types::LayerId::new(req.target_layer_id),
        );
        state.block_representatives.clear_dithered();
    }

    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        snapshot
            .generations
            .increment_layer_gen(req.target_layer_id);
    }

    let doc = state.require_session(doc_id)?.document_handle.snapshot().id.0;
    engine_tiles::invalidation::invalidate(
        &state.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc, layer: req.target_layer_id,
        },
    );
    schedule_dirty_viewport_tiles(&state);
    emit_document_changed(&app_handle, "pattern_imported", Some(req.target_layer_id), Some(doc_id));

        Ok(ImportPatternResponse {
            filter_ids: result.filter_ids.iter().map(|id| id.to_string()).collect(),
            palette_ids: result.palette_ids.iter().map(|id| id.0).collect(),
        })
    })
}



// ============================================================================
// Tile Scheduling Helpers
// ============================================================================

/// Drop all tiles and in-flight work before writing a replacement document's Raw tiles.
/// Does **not** run on undo/redo — those must keep Raw pixels for restored layers.
pub(crate) fn reset_tiles_for_new_document(state: &AppState) {
    state.tile_cache.clear();
    state.scheduler.clear_all();
    state.ed_frontier.clear();
    state.block_representatives.invalidate_all();
    state.error_residuals.clear();
    if let Ok(mut pending) = state.pending_preview_refresh.lock() {
        *pending = None;
    }
}

/// Mark Processed and Composite dirty after undo/redo restore (Raw stays).
/// Advances live `document_gen` past any cached generation so worker inserts win.
pub(crate) fn invalidate_after_document_replace(state: &AppState) {
    use engine_tiles::CacheStage;

    let mut keys = Vec::new();
    for entry in state.tile_cache.entries.iter() {
        let key = *entry.key();
        if matches!(key.stage, CacheStage::Processed | CacheStage::Composite) {
            keys.push(key);
        }
    }
    for key in keys {
        state.tile_cache.mark_dirty(key);
    }
    state.block_representatives.invalidate_all();
    if let Ok(session) = state.active_session() {
        state
            .error_residuals
            .evict_document(session.document_handle.snapshot().id.0);
    } else {
        state.error_residuals.clear();
    }
}

/// Schedule viewport-visible dirty tiles for immediate recomputation.
///
/// Reads the current viewport state, iterates over visible tile coordinates, and
/// enqueues Immediate-priority recompute tasks for any Composite-stage tile that
/// is currently marked dirty in the cache. This ensures the user sees updated tiles
/// promptly after a filter or layer property change.
///
/// The `tile-ready` event is emitted by the worker loop upon successful recomputation
/// (requirements 2.4, 10.4, 10.6).
pub(crate) fn schedule_dirty_viewport_tiles(state: &AppState) {
    use std::sync::atomic::Ordering;

    let viewport = state.viewport.lock().unwrap().clone();
    let Ok(snapshot) = state.active_session().map(|s| s.document_handle.snapshot()) else {
        return;
    };
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

    // ED: enqueue causal prefix for visible tiles (same Immediate lane + inheritance).
    crate::tile_pipeline::schedule_ed_for_viewport(state);

    // G10: exclusive GPU L0 Composite when eligible; else CPU schedule as usual.
    let gpu_authored_l0 = crate::gpu_resident_shadow::try_publish_gpu_preview_viewport(state);

    for coord in &viewport.visible_tiles {
        // Pyramid L>0 always CPU. Skip L0 coords the GPU just published.
        if gpu_authored_l0.contains(coord) {
            continue;
        }

        let key = TileKey {
            doc: snapshot.id.0,
            layer: 0,
            coord: *coord,
            stage: CacheStage::Composite,
        };

        let is_dirty = match state.tile_cache.entries.get(&key) {
            Some(entry) => entry.dirty.load(Ordering::Acquire),
            None => true, // Missing tile also needs computation
        };

        if is_dirty {
            let task = RecomputeTask {
                key,
                generation: doc_gen,
                layer_generation: 0,
                priority: Priority::Immediate,
            };
            state.scheduler.enqueue_dedup(task);
            state.worker_wake.notify_one();
        }
    }

    crate::gpu_resident_shadow::enqueue_resident_shadow_viewport(state);
}

fn preview_pass_busy(state: &AppState) -> bool {
    state.preview_pass_inflight.load(Ordering::Acquire) > 0
        || state.scheduler.queued_len() > 0
}

/// True if this layer still has an enabled dither filter (ordered or ED).
/// Changing an earlier filter (Adjust/Curves/…) must drop residuals / mega-pixel
/// block cache so later dithering is recomputed from the new input.
fn layer_needs_dither_cache_reset(nodes: &[engine_project::LayerNode], layer_id: u32) -> bool {
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

/// Run (or coalesce) the expensive filter preview pass: residuals clear +
/// layer invalidate + viewport schedule.
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
            .error_residuals
            .evict_layer(doc, engine_project::types::LayerId::new(layer_id));
        state.block_representatives.evict_layer(doc, layer_id);
    }
    engine_tiles::invalidation::invalidate(
        &state.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: session.document_handle.snapshot().id.0, layer: layer_id },
    );
    schedule_dirty_viewport_tiles(state);
}

/// After a worker finishes (or discards) a task, flush a coalesced preview
/// refresh if the queues are idle.
pub(crate) fn on_preview_task_finished(state: &AppState) {
    if preview_pass_busy(state) {
        return;
    }
    let pending = state.pending_preview_refresh.lock().unwrap().take();
    if let Some(p) = pending {
        run_preview_refresh(state, p.layer_id, p.clear_residuals);
    }
}

/// Convert an f32 pixel value (0.0-1.0) to u8 (0-255), clamped.
fn f32_to_u8(val: f32) -> u8 {
    (val * 255.0).clamp(0.0, 255.0) as u8
}

/// Encode an RGBA u8 buffer as PNG bytes.
fn encode_rgba_to_png(buffer: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;
    use std::io::Cursor;

    let mut png_data: Vec<u8> = Vec::new();
    let cursor = Cursor::new(&mut png_data);
    let encoder = PngEncoder::new(cursor);
    encoder
        .write_image(buffer, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("PNG encoding error: {}", e))?;

    Ok(png_data)
}

/// Find the first visible leaf layer in the document tree.
fn find_first_visible_layer(nodes: &[engine_project::LayerNode]) -> Option<&engine_project::Layer> {
    for node in nodes {
        match node {
            engine_project::LayerNode::Leaf(layer) => {
                if layer.visible {
                    return Some(layer);
                }
            }
            engine_project::LayerNode::Group(group) => {
                if group.visible {
                    if let Some(layer) = find_first_visible_layer(&group.children) {
                        return Some(layer);
                    }
                }
            }
        }
    }
    None
}

// ============================================================================
// Export Image Command
// ============================================================================

/// Request body for the export_image command.
#[derive(Debug, Clone, Deserialize)]
pub struct ExportImageRequest {
    pub doc_id: u32,
    pub path: String,
    pub format: String,       // "PNG" or "JPEG"
    pub quality: Option<u8>,  // 1-100, default 90 for JPEG
    /// `"greedy_meshing"` | `"contour_tracing"`; ignored for raster.
    #[serde(default)]
    pub svg_algorithm: Option<String>,
}

/// Export the current document (with all filters applied at full resolution) to a file.
#[tauri::command]
pub async fn export_image(
    req: ExportImageRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    use engine_project::filters::apply::apply_filter_to_tile;
    use engine_tiles::{TILE_SIZE, HALO, TileCoord, CacheStage, TileKey};
    use std::fs;
    use std::io::Cursor;

    // 1. Validate format
    if req.format != "PNG" && req.format != "JPEG" && req.format != "SVG" {
        return Err("Invalid parameters: format must be PNG, JPEG, or SVG".to_string());
    }

    // 2. Resolve session by requested doc_id (not "active only") — SessionGone vs RawIncomplete.
    let session = state.session(req.doc_id).map_err(|_| {
        format!(
            "Document was closed (id {}); cannot export",
            req.doc_id
        )
    })?;
    let _io_guard = session.begin_io();
    let snapshot = session.document_handle.snapshot();
    let img_width = snapshot.width;
    let img_height = snapshot.height;

    // 3. Compute tile grid and read tiles from cache
    let cols = (img_width + TILE_SIZE - 1) / TILE_SIZE;
    let rows = (img_height + TILE_SIZE - 1) / TILE_SIZE;
    let layer_id = 1u32;

    let mut tiles: Vec<Vec<Arc<PixelTile>>> = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let mut row_tiles: Vec<Arc<PixelTile>> = Vec::with_capacity(cols as usize);
        for col in 0..cols {
            let key = TileKey {
                doc: req.doc_id,
                layer: layer_id,
                coord: TileCoord { level: 0, x: col, y: row },
                stage: CacheStage::Raw,
            };
            match state.tile_cache.get_entry(key) {
                Some(tile) => row_tiles.push(tile),
                None => {
                    return Err(format!(
                        "Cannot export: image tiles missing from memory for document {} layer {} — reopen the file",
                        req.doc_id, layer_id
                    ));
                }
            }
        }
        tiles.push(row_tiles);
    }

    // 4. Get the document snapshot and clone the layer
    let layer_clone = find_first_visible_layer(&snapshot.root).cloned();
    let doc_snapshot = (*snapshot).clone();
    drop(snapshot);

    // 5. Do heavy rendering and I/O in a blocking thread
    let req_format = req.format.clone();
    let req_path = req.path.clone();
    let req_quality = req.quality;
    let req_svg_algorithm = req.svg_algorithm.clone();
    let state_clone = state.inner().clone();

    tauri::async_runtime::spawn_blocking(move || {
        let mut rgba_buffer: Vec<u8> = vec![0u8; (img_width * img_height * 4) as usize];

        for row in 0..rows {
            for col in 0..cols {
                let tile = &tiles[row as usize][col as usize];

                // Apply filters if we have a visible layer
                let processed_tile = if let Some(ref layer) = layer_clone {
                    let coord = TileCoord { level: 0, x: col, y: row };
                    apply_filter_to_tile(
                        tile,
                        layer,
                        coord,
                        &state_clone.palette_cache,
                        &state_clone.palette_lut_cache,
                        &state_clone.threshold_cache,
                        &doc_snapshot,
                    )
                        .map_err(|e| format!("Render error: {:?}", e))?
                } else {
                    let mut copy = engine_tiles::PixelTile::new();
                    for y in 0u32..260 {
                        for x in 0u32..260 {
                            for c in 0..4 {
                                copy.set(x, y, c, tile.at(x, y, c));
                            }
                        }
                    }
                    copy
                };

                // Copy tile pixels to rgba_buffer (f32 → u8 conversion)
                let tile_origin_x = col * TILE_SIZE;
                let tile_origin_y = row * TILE_SIZE;

                for ty in 0..TILE_SIZE {
                    let img_y = tile_origin_y + ty;
                    if img_y >= img_height {
                        break;
                    }
                    for tx in 0..TILE_SIZE {
                        let img_x = tile_origin_x + tx;
                        if img_x >= img_width {
                            break;
                        }
                        let tile_x = tx + HALO;
                        let tile_y = ty + HALO;
                        let buf_idx = ((img_y * img_width + img_x) * 4) as usize;

                        rgba_buffer[buf_idx] = f32_to_u8(processed_tile.at(tile_x, tile_y, 0));
                        rgba_buffer[buf_idx + 1] = f32_to_u8(processed_tile.at(tile_x, tile_y, 1));
                        rgba_buffer[buf_idx + 2] = f32_to_u8(processed_tile.at(tile_x, tile_y, 2));
                        rgba_buffer[buf_idx + 3] = f32_to_u8(processed_tile.at(tile_x, tile_y, 3));
                    }
                }
            }
        }

        // 6. Encode and write based on format
        match req_format.as_str() {
            "PNG" => {
                let png_bytes = encode_rgba_to_png(&rgba_buffer, img_width, img_height)?;
                fs::write(&req_path, &png_bytes)
                    .map_err(|e| format!("IO error: {}", e))?;
            }
            "JPEG" => {
                use image::codecs::jpeg::JpegEncoder;
                use image::ImageEncoder;

                // Convert RGBA to RGB for JPEG (JPEG doesn't support alpha)
                let mut rgb_buffer: Vec<u8> = Vec::with_capacity((img_width * img_height * 3) as usize);
                for pixel in rgba_buffer.chunks_exact(4) {
                    rgb_buffer.push(pixel[0]); // R
                    rgb_buffer.push(pixel[1]); // G
                    rgb_buffer.push(pixel[2]); // B
                }

                let quality = req_quality.unwrap_or(90);
                let mut jpeg_data: Vec<u8> = Vec::new();
                let cursor = Cursor::new(&mut jpeg_data);
                let encoder = JpegEncoder::new_with_quality(cursor, quality);
                encoder
                    .write_image(&rgb_buffer, img_width, img_height, image::ExtendedColorType::Rgb8)
                    .map_err(|e| format!("JPEG encoding error: {}", e))?;

                fs::write(&req_path, &jpeg_data)
                    .map_err(|e| format!("IO error: {}", e))?;
            }
            "SVG" => {
                use engine_io::{write_svg_file, SvgAlgorithm, SvgExportOptions};
                let algorithm = match req_svg_algorithm.as_deref() {
                    Some("contour_tracing") => SvgAlgorithm::ContourTracing,
                    _ => SvgAlgorithm::GreedyMeshing,
                };
                let opts = SvgExportOptions {
                    algorithm,
                    tolerance: 0,
                };
                write_svg_file(&req_path, img_width, img_height, &rgba_buffer, &opts)
                    .map_err(|e| format!("SVG export error: {}", e))?;
            }
            _ => unreachable!(), // Already validated above
        }

        Ok::<(), String>(())
    }).await.map_err(|e| format!("Export error: {}", e))?
}

// ============================================================================
// Palette Commands
// ============================================================================

/// DTO for palette data sent to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct PaletteDto {
    pub id: u32,
    pub name: String,
    pub colors: Vec<[u8; 3]>,    // sRGB u8 for backward compatibility
    pub hex_colors: Vec<String>, // Hex strings for new UI
    pub color_count: usize,
}

/// Request body for adding a palette manually.
#[derive(Debug, Clone, Deserialize)]
pub struct AddPaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub name: String,
    pub colors: Vec<[u8; 3]>, // sRGB
}

/// Request body for generating a palette from a layer.
#[derive(Debug, Clone, Deserialize)]
pub struct GeneratePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub target_count: u16,
    pub method: String, // "MedianCut" or "KMeans"
    #[serde(default)]
    pub chroma_weight: f32,
    #[serde(default)]
    pub contrast_weight: f32,
}

/// Convert a document palette to a PaletteDto (linear→sRGB for display).
fn palette_to_dto(palette: &engine_color::palette::Palette) -> PaletteDto {
    use engine_color::palette::linear_to_srgb;
    let colors: Vec<[u8; 3]> = palette
        .colors
        .iter()
        .map(|c| [linear_to_srgb(c.r), linear_to_srgb(c.g), linear_to_srgb(c.b)])
        .collect();
    let hex_colors: Vec<String> = palette
        .colors
        .iter()
        .map(|c| linear_to_hex(c))
        .collect();
    let color_count = colors.len();
    PaletteDto {
        id: palette.id,
        name: palette.name.clone(),
        colors,
        hex_colors,
        color_count,
    }
}

/// List all palettes in the document.
#[tauri::command]
pub fn list_palettes(state: State<'_, Arc<AppState>>) -> Result<Vec<PaletteDto>, String> {
    let Ok(session) = state.active_session() else {
        return Ok(vec![]);
    };
    let snapshot = session.document_handle.snapshot();
    let dtos: Vec<PaletteDto> = snapshot.palettes.iter().map(palette_to_dto).collect();
    Ok(dtos)
}/// Preview DTO for a built-in retro palette (no Document write).
#[derive(Debug, Clone, Serialize)]
pub struct BuiltinPaletteDto {
    pub id: String,
    pub name: String,
    pub colors: Vec<[u8; 3]>,
    pub color_count: usize,
}

/// List built-in retro palette presets from `engine-color` (UI must not hardcode RGB).
#[tauri::command]
pub fn list_builtin_palettes() -> Result<Vec<BuiltinPaletteDto>, String> {
    use engine_color::palette::BUILTIN_PRESETS;

    Ok(BUILTIN_PRESETS
        .iter()
        .map(|p| BuiltinPaletteDto {
            id: p.id.to_string(),
            name: p.name.to_string(),
            colors: p
                .colors_srgb
                .iter()
                .map(|&(r, g, b)| [r, g, b])
                .collect(),
            color_count: p.colors_srgb.len(),
        })
        .collect())
}

/// Import a built-in preset into the Document as a new palette (same path as `add_palette`).
#[tauri::command]
pub fn import_builtin_palette(
    doc_id: u32,
    id: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    use engine_color::palette::{find_preset, srgb_to_linear, LinearColor};

    let preset = find_preset(&id).ok_or_else(|| {
        format!(
            "Unknown builtin palette id '{}'. Use list_builtin_palettes for valid ids.",
            id
        )
    })?;

    let linear_colors: Vec<LinearColor> = preset
        .colors_srgb
        .iter()
        .map(|&(r, g, b)| LinearColor {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
        })
        .collect();

    let mut palette_id_raw = 0u32;
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            let pid = doc.add_palette(preset.name.to_string(), linear_colors);
            palette_id_raw = pid.0;
            doc.increment_generation();
        });

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == palette_id_raw)
            .ok_or_else(|| "Failed to find newly imported builtin palette".to_string())?;
        Ok(palette_to_dto(palette))
    })
}

/// Lightweight color returned by generators (draft only — no Document write).
#[derive(Debug, Clone, Serialize)]
pub struct GeneratedColorDto {
    pub hex: String,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

fn normalize_hex_arg(hex: &str) -> Result<String, String> {
    let trimmed = hex.trim().trim_start_matches('#').to_uppercase();
    if trimmed.len() != 6 {
        return Err("Hex color must be exactly 6 characters (optionally prefixed with #)".to_string());
    }
    // Validate hex digits early
    if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Hex color contains invalid characters".to_string());
    }
    Ok(trimmed)
}

fn lin_rgb_to_generated(c: engine_color::LinRgb) -> GeneratedColorDto {
    use engine_color::palette::{linear_to_srgb, LinearColor};
    let lc = LinearColor {
        r: c.r,
        g: c.g,
        b: c.b,
    };
    let hex = linear_to_hex(&lc);
    GeneratedColorDto {
        hex: format!("#{}", hex),
        r: linear_to_srgb(c.r),
        g: linear_to_srgb(c.g),
        b: linear_to_srgb(c.b),
    }
}

fn hex_arg_to_lin_rgb(hex: &str) -> Result<engine_color::LinRgb, String> {
    let normalized = normalize_hex_arg(hex)?;
    let lc = hex_to_linear(&normalized)?;
    Ok(engine_color::LinRgb {
        r: lc.r,
        g: lc.g,
        b: lc.b,
    })
}

/// Generate an Oklab ramp between two hex colors. Draft-only — does **not** call `add_palette`.
#[tauri::command]
pub fn generate_ramp_palette(
    from_hex: String,
    to_hex: String,
    steps: u32,
) -> Result<Vec<GeneratedColorDto>, String> {
    if !(1..=64).contains(&steps) {
        return Err("steps must be between 1 and 64".to_string());
    }
    let from = hex_arg_to_lin_rgb(&from_hex)?;
    let to = hex_arg_to_lin_rgb(&to_hex)?;
    let ramp = engine_color::generate_ramp(from, to, steps as usize);
    Ok(ramp.into_iter().map(lin_rgb_to_generated).collect())
}

/// Generate a harmony palette from a base hex + rule. Draft-only — no Document write.
#[tauri::command]
pub fn generate_harmony_palette(
    base_hex: String,
    rule: String,
    count: u32,
    analogous_spread: Option<f32>,
) -> Result<Vec<GeneratedColorDto>, String> {
    use engine_color::HarmonyRule;

    if !(1..=32).contains(&count) {
        return Err("count must be between 1 and 32".to_string());
    }
    let harmony_rule = match rule.as_str() {
        "Monochromatic" => HarmonyRule::Monochromatic,
        "Analogous" => HarmonyRule::Analogous,
        "Complementary" => HarmonyRule::Complementary,
        "Triadic" => HarmonyRule::Triadic,
        "SplitComplementary" => HarmonyRule::SplitComplementary,
        other => {
            return Err(format!(
                "Unknown harmony rule '{}'. Expected Monochromatic|Analogous|Complementary|Triadic|SplitComplementary",
                other
            ))
        }
    };
    let base = hex_arg_to_lin_rgb(&base_hex)?;
    let colors = if let Some(spread) = analogous_spread {
        if !spread.is_finite() || spread < 0.0 || spread > std::f32::consts::PI {
            return Err("analogous_spread must be a finite value in [0, π] radians".to_string());
        }
        engine_color::generate_harmony_with_spread(base, harmony_rule, count as usize, spread)
    } else {
        engine_color::generate_harmony(base, harmony_rule, count as usize)
    };
    Ok(colors.into_iter().map(lin_rgb_to_generated).collect())
}

/// Oklab coordinates for one palette color. Conversion is Rust-only (`oklab.rs`).
#[derive(Debug, Clone, Serialize)]
pub struct OklabPointDto {
    pub l: f32,
    pub a: f32,
    pub b: f32,
    pub srgb_hex: String,
}

fn oklab_point_from_lin_rgb(rgb: engine_color::LinRgb, srgb_hex: String) -> OklabPointDto {
    let lab = engine_color::linear_to_oklab(rgb);
    OklabPointDto {
        l: lab.l,
        a: lab.a,
        b: lab.b,
        srgb_hex,
    }
}

fn oklab_points_from_hexes(colors: &[String]) -> Result<Vec<OklabPointDto>, String> {
    colors
        .iter()
        .map(|hex| {
            let rgb = hex_arg_to_lin_rgb(hex)?;
            let normalized = normalize_hex_arg(hex)?;
            Ok(oklab_point_from_lin_rgb(rgb, format!("#{}", normalized)))
        })
        .collect()
}

fn oklab_points_from_linear(
    colors: &[engine_color::palette::LinearColor],
) -> Vec<OklabPointDto> {
    colors
        .iter()
        .map(|c| {
            let rgb = engine_color::LinRgb {
                r: c.r,
                g: c.g,
                b: c.b,
            };
            oklab_point_from_lin_rgb(rgb, format!("#{}", linear_to_hex(c)))
        })
        .collect()
}

/// Convert sRGB hex colors to Oklab via `linear_to_oklab` (draft + saved share this path).
#[tauri::command]
pub fn colors_to_oklab(colors: Vec<String>) -> Result<Vec<OklabPointDto>, String> {
    oklab_points_from_hexes(&colors)
}

/// Load a document palette and return Oklab points (same math as `colors_to_oklab`).
#[tauri::command]
pub fn get_palette_oklab(
    palette_id: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<OklabPointDto>, String> {
    let snapshot = state.active_session()?.document_handle.snapshot();
    let palette = snapshot
        .palettes
        .iter()
        .find(|p| p.id == palette_id)
        .ok_or_else(|| format!("Palette {} not found", palette_id))?;
    Ok(oklab_points_from_linear(&palette.colors))
}/// Import a palette from a file path (format auto-detected by extension).
#[tauri::command]
pub fn import_palette(
    doc_id: u32,
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    use engine_color::palette::{import_palette as do_import, PaletteFormat};
    use std::path::Path;

    let file_path = Path::new(&path);
    let ext = file_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let format = match ext.as_str() {
        "ase" => PaletteFormat::Ase,
        "aco" => PaletteFormat::Aco,
        "gpl" => PaletteFormat::Gpl,
        "pal" => PaletteFormat::Pal,
        "csv" => PaletteFormat::Csv,
        "json" => PaletteFormat::Json,
        _ => return Err(format!("Unsupported palette format: .{}", ext)),
    };

    // Parse the palette file (returns linear colors)
    let linear_colors = do_import(file_path, format).map_err(|e| format!("{}", e))?;

    // Derive name from filename without extension
    let name = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Imported")
        .to_string();

    // Add to document
    let mut palette_id_raw = 0u32;
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            let pid = doc.add_palette(name.clone(), linear_colors.clone());
            palette_id_raw = pid.0;
            doc.increment_generation();
        });

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == palette_id_raw)
            .ok_or_else(|| "Failed to find newly added palette".to_string())?;
        Ok(palette_to_dto(palette))
    })
}

/// Add a palette manually (from JSON color data).
#[tauri::command]
pub fn add_palette(
    req: AddPaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    use engine_color::palette::{srgb_to_linear, LinearColor};

    // Convert sRGB u8 to linear f32
    let linear_colors: Vec<LinearColor> = req
        .colors
        .iter()
        .map(|[r, g, b]| LinearColor {
            r: srgb_to_linear(*r),
            g: srgb_to_linear(*g),
            b: srgb_to_linear(*b),
        })
        .collect();

    let mut palette_id_raw = 0u32;
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            let pid = doc.add_palette(req.name.clone(), linear_colors);
            palette_id_raw = pid.0;
            doc.increment_generation();
        });

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == palette_id_raw)
            .ok_or_else(|| "Failed to find newly added palette".to_string())?;
        Ok(palette_to_dto(palette))
    })
}

/// Replace an existing document palette's name + colors (Color Lab Apply).
#[derive(Debug, Clone, Deserialize)]
pub struct ReplacePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub name: String,
    pub colors: Vec<[u8; 3]>,
}

#[tauri::command]
pub fn replace_palette(
    req: ReplacePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    use engine_color::palette::{srgb_to_linear, LinearColor};
    use engine_project::types::PaletteId;

    let trimmed = req.name.trim().to_string();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err("Name must be 1–255 characters".to_string());
    }
    if req.colors.is_empty() {
        return Err("Palette must contain at least one color".to_string());
    }

    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        if !snapshot.palettes.iter().any(|p| p.id == req.palette_id) {
            return Err(format!("Palette {} not found", req.palette_id));
        }
    }

    let linear_colors: Vec<LinearColor> = req
        .colors
        .iter()
        .map(|[r, g, b]| LinearColor {
            r: srgb_to_linear(*r),
            g: srgb_to_linear(*g),
            b: srgb_to_linear(*b),
        })
        .collect();

    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            let _ = doc.modify_palette(PaletteId::new(req.palette_id), linear_colors.clone());
            if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                palette.name = trimmed.clone();
            }
            doc.increment_generation();
        });

        invalidate_palette_changed(PaletteId::new(req.palette_id), &state);

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        Ok(palette_to_dto(palette))
    })
}

/// Generate a palette from a layer's pixels.
///
/// Heavy work runs on a blocking pool so the UI stays responsive. Large images
/// are stride-sampled before MedianCut / K-Means.
#[tauri::command]
pub async fn generate_palette(
    req: GeneratePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || generate_palette_blocking(req, &state, Some(&app_handle)))
        .await
        .map_err(|e| format!("Palette generation task failed: {}", e))?
}

fn generate_palette_blocking(
    req: GeneratePaletteRequest,
    state: &AppState,
    app: Option<&AppHandle>,
) -> Result<PaletteDto, String> {
    use engine_color::palette::generate::{PaletteGenMethod, MAX_GENERATION_SAMPLES};
    use engine_color::palette::LinearColor;
    use engine_tiles::{CacheStage, TileCoord, TileKey, HALO, TILE_SIZE};

    let doc_id = req.doc_id;
    let method = match req.method.as_str() {
        "KMeans" => PaletteGenMethod::KMeans,
        _ => PaletteGenMethod::MedianCut,
    };

    if req.target_count < 2 || req.target_count > 256 {
        return Err("target_count must be between 2 and 256".to_string());
    }

    let weights = engine_color::palette::generate::GenerateWeights {
        chroma_weight: req.chroma_weight,
        contrast_weight: req.contrast_weight,
    };
    weights
        .validated()
        .map_err(|e| e.to_string())?;

    let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
    let doc_width = snapshot.width;
    let doc_height = snapshot.height;
    drop(snapshot);

    let total_pixels = (doc_width as u64).saturating_mul(doc_height as u64).max(1);
    // Stride so we never push much more than MAX_GENERATION_SAMPLES opaque samples.
    let stride =
        ((total_pixels as usize + MAX_GENERATION_SAMPLES - 1) / MAX_GENERATION_SAMPLES).max(1);

    let cols = (doc_width + TILE_SIZE - 1) / TILE_SIZE;
    let rows = (doc_height + TILE_SIZE - 1) / TILE_SIZE;

    let mut pixels: Vec<(LinearColor, f32)> =
        Vec::with_capacity((total_pixels as usize / stride).min(MAX_GENERATION_SAMPLES + 1024));
    let mut sample_index: u64 = 0;

    for row in 0..rows {
        for col in 0..cols {
            let key = TileKey {
                doc: doc_id,
                layer: req.layer_id,
                coord: TileCoord {
                    level: 0,
                    x: col,
                    y: row,
                },
                stage: CacheStage::Raw,
            };
            if let Some(tile) = state.tile_cache.get_entry(key) {
                let tile_max_x =
                    std::cmp::min(TILE_SIZE, doc_width.saturating_sub(col * TILE_SIZE));
                let tile_max_y =
                    std::cmp::min(TILE_SIZE, doc_height.saturating_sub(row * TILE_SIZE));
                for ty in 0..tile_max_y {
                    for tx in 0..tile_max_x {
                        let take = sample_index % stride as u64 == 0;
                        sample_index += 1;
                        if !take {
                            continue;
                        }
                        let px = tx + HALO;
                        let py = ty + HALO;
                        let a = tile.at(px, py, 3);
                        if a <= 0.0 {
                            continue;
                        }
                        pixels.push((
                            LinearColor {
                                r: tile.at(px, py, 0),
                                g: tile.at(px, py, 1),
                                b: tile.at(px, py, 2),
                            },
                            a,
                        ));
                    }
                }
            }
        }
    }

    if pixels.is_empty() {
        return Err("No tile data available for this layer. Load an image first.".to_string());
    }

    let mut palette_id_raw = 0u32;
    crate::undo::with_document_undo(state, app, doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            match engine_project::palette_gen::generate_palette_from_layer_weighted(
                doc,
                engine_project::types::LayerId::new(req.layer_id),
                pixels.into_iter(),
                req.target_count,
                method,
                weights,
            ) {
                Ok(pid) => {
                    palette_id_raw = pid.0;
                    doc.increment_generation();
                }
                Err(_) => {}
            }
        });

        if palette_id_raw == 0 {
            return Err(
                "Palette generation failed. Ensure the layer has non-transparent pixels.".to_string(),
            );
        }

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == palette_id_raw)
            .ok_or_else(|| "Failed to find generated palette".to_string())?;
        Ok(palette_to_dto(palette))
    })
}

/// Remove a palette from the document.
#[tauri::command]
pub fn remove_palette(
    doc_id: u32,
    palette_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    use engine_project::types::PaletteId;

    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let mut result: Result<(), String> = Ok(());
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            match doc.remove_palette(PaletteId::new(palette_id)) {
                Ok(_) => {
                    doc.increment_generation();
                }
                Err(e) => {
                    result = Err(format!("{}", e));
                }
            }
        });
        result
    })
}

// ============================================================================
// Rename Palette Command
// ============================================================================

/// Request body for renaming a palette.
#[derive(Debug, Clone, Deserialize)]
pub struct RenamePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub name: String,
}

/// Rename a palette. Does NOT trigger invalidation since name changes
/// do not affect rendering.
#[tauri::command]
pub fn rename_palette(
    req: RenamePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    // 1. Validate name: trim, then check 1–255 chars
    let trimmed_name = req.name.trim().to_string();
    if trimmed_name.is_empty() || trimmed_name.len() > 255 {
        return Err("Name must be 1–255 characters".to_string());
    }

    // 2. Validate palette exists
    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        if !snapshot.palettes.iter().any(|p| p.id == req.palette_id) {
            return Err(format!("Palette {} not found", req.palette_id));
        }
    }

    // 3. Mutate document: update palette name (no invalidation)
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                palette.name = trimmed_name;
            }
        });

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        Ok(palette_to_dto(palette))
    })
}

/// Request body for creating a new empty palette.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatePaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub name: String,
}

/// Create a new empty palette with a given name.
#[tauri::command]
pub fn create_palette(
    req: CreatePaletteRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    // Trim and validate name: 1–255 characters after trimming
    let trimmed_name = req.name.trim().to_string();
    if trimmed_name.is_empty() || trimmed_name.len() > 255 {
        return Err("Name must be 1–255 characters".to_string());
    }

    // Mutate document: add palette with empty color list
    let mut palette_id_raw = 0u32;
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            let pid = doc.add_palette(trimmed_name.clone(), vec![]);
            palette_id_raw = pid.0;
            doc.increment_generation();
        });

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == palette_id_raw)
            .ok_or_else(|| "Failed to find newly created palette".to_string())?;
        Ok(palette_to_dto(palette))
    })
}

/// Request body for exporting a palette to a file.
#[derive(Debug, Clone, Deserialize)]
pub struct ExportPaletteRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub path: String,
    pub format: String, // "ase", "gpl", "json", "aco", "pal", "csv"
}

/// Export a palette to a file in the specified format.
#[tauri::command]
pub fn export_palette(
    req: ExportPaletteRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let doc_id = req.doc_id;
    use engine_color::palette::{export_palette as do_export, PaletteFormat};

    // 1. Validate palette exists
    let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
    let palette = snapshot
        .palettes
        .iter()
        .find(|p| p.id == req.palette_id)
        .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;

    // 2. Check palette is not empty
    if palette.colors.is_empty() {
        return Err("Palette is empty and cannot be exported".to_string());
    }

    // 3. Parse format string (case-insensitive)
    let format = match req.format.to_lowercase().as_str() {
        "ase" => PaletteFormat::Ase,
        "aco" => PaletteFormat::Aco,
        "gpl" => PaletteFormat::Gpl,
        "pal" => PaletteFormat::Pal,
        "csv" => PaletteFormat::Csv,
        "json" => PaletteFormat::Json,
        _ => return Err(format!("Unsupported export format: {}", req.format)),
    };

    // 4. Call engine_color export_palette to get bytes
    let bytes = do_export(palette, format).map_err(|e| format!("{}", e))?;

    // 5. Write bytes to file
    std::fs::write(&req.path, &bytes)
        .map_err(|e| format!("Failed to write file: {}", e))?;

    Ok(())
}

// ============================================================================
// Add Color to Palette Command
// ============================================================================

/// Request body for adding a color to an existing palette.
#[derive(Debug, Clone, Deserialize)]
pub struct AddColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub hex: String, // 6-char hex, e.g. "FF0000"
}

/// Add a color to an existing palette. Parses hex to linear, validates palette
/// exists and has fewer than 65536 colors, pushes the color, increments revision,
/// triggers invalidation cascade, and returns the updated PaletteDto.
#[tauri::command]
pub fn add_color_to_palette(
    req: AddColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    use engine_project::types::PaletteId;

    // 1. Parse hex to linear color
    let color = hex_to_linear(&req.hex)?;

    // 2. Validate palette exists and size < 65536
    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        if palette.colors.len() >= 65536 {
            return Err("Palette has reached maximum size (65536 colors)".to_string());
        }
    }

    // 3. Mutate document: push color, increment revision
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                palette.colors.push(color);
                palette.revision += 1;
            }
        });

        invalidate_palette_changed(PaletteId::new(req.palette_id), &state);

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        Ok(palette_to_dto(palette))
    })
}

// ============================================================================
// Update Palette Color Command
// ============================================================================

/// Request body for updating a single color in a palette.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub index: usize,
    pub hex: String,
}

/// Update a color at a given index within a palette.
/// Parses the hex string, validates the palette exists and index is in bounds,
/// replaces the color, increments revision, triggers invalidation cascade.
#[tauri::command]
pub fn update_palette_color(
    req: UpdateColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    use engine_project::types::PaletteId;

    // 1. Parse hex to linear color
    let color = hex_to_linear(&req.hex)?;

    // 2. Validate palette exists and index is in bounds
    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;

        let color_count = palette.colors.len();
        if req.index >= color_count {
            return Err(format!(
                "Color index {} out of bounds (palette has {} colors)",
                req.index, color_count
            ));
        }
    }

    // 3. Mutate document: replace color at index, increment revision
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                palette.colors[req.index] = color;
                palette.revision += 1;
            }
        });

        invalidate_palette_changed(PaletteId::new(req.palette_id), &state);

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        Ok(palette_to_dto(palette))
    })
}

// ============================================================================
// Remove Palette Color Command
// ============================================================================

/// Request body for removing a color from a palette by index.
#[derive(Debug, Clone, Deserialize)]
pub struct RemoveColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub index: usize,
}

/// Remove a color at a given index from a palette.
/// Validates palette exists, index is in bounds, and that removal would not
/// empty a palette that is referenced by filters (error in that case).
/// Otherwise removes the color, increments revision, triggers invalidation cascade.
#[tauri::command]
pub fn remove_palette_color(
    req: RemoveColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    use engine_project::types::PaletteId;

    // 1. Validate palette exists and index is in bounds
    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;

        let color_count = palette.colors.len();
        if req.index >= color_count {
            return Err(format!(
                "Color index {} out of bounds (palette has {} colors)",
                req.index, color_count
            ));
        }

        // 2. Check: if removal would leave 0 colors AND palette is referenced → error
        if color_count == 1 {
            let referencing_layers =
                find_layers_referencing_palette(&snapshot.root, PaletteId::new(req.palette_id));
            if !referencing_layers.is_empty() {
                return Err(
                    "Cannot remove last color from a palette referenced by filters".to_string(),
                );
            }
        }
    }

    // 3. Mutate document: remove color at index, increment revision
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                palette.colors.remove(req.index);
                palette.revision += 1;
            }
        });

        invalidate_palette_changed(PaletteId::new(req.palette_id), &state);

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        Ok(palette_to_dto(palette))
    })
}

// ============================================================================
// Reorder Palette Color Command
// ============================================================================

/// Request body for reordering a color within a palette.
#[derive(Debug, Clone, Deserialize)]
pub struct ReorderColorRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub palette_id: u32,
    pub from_index: usize,
    pub to_index: usize,
}

/// Reorder a color within a palette by moving from one index to another.
/// If from_index == to_index, this is a no-op (no revision increment, no invalidation).
#[tauri::command]
pub fn reorder_palette_color(
    req: ReorderColorRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PaletteDto, String> {
    let doc_id = req.doc_id;
    use engine_project::types::PaletteId;

    // 1. Validate palette exists and both indices are in bounds
    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;

        let color_count = palette.colors.len();
        if req.from_index >= color_count || req.to_index >= color_count {
            return Err("Index out of bounds".to_string());
        }
    }

    // 2. If from == to, return current PaletteDto unchanged (no-op)
    if req.from_index == req.to_index {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        return Ok(palette_to_dto(palette));
    }

    // 3. Mutate document: remove at from_index, insert at to_index, increment revision
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        state.require_session(doc_id)?.document_handle.mutate(|doc| {
            if let Some(palette) = doc.palettes.iter_mut().find(|p| p.id == req.palette_id) {
                let color = palette.colors.remove(req.from_index);
                palette.colors.insert(req.to_index, color);
                palette.revision += 1;
            }
        });

        invalidate_palette_changed(PaletteId::new(req.palette_id), &state);

        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == req.palette_id)
            .ok_or_else(|| format!("Palette {} not found", req.palette_id))?;
        Ok(palette_to_dto(palette))
    })
}

// ============================================================================
// Delete Palette Command (Force-Delete with Reference Clearing)
// ============================================================================

/// Response for the delete_palette command.
#[derive(Debug, Clone, Serialize)]
pub struct DeletePaletteResponse {
    pub affected_filter_ids: Vec<String>,
}

/// Force-delete a palette, clearing any filter references first.
///
/// Unlike `remove_palette` (which fails if filters reference the palette),
/// this command:
/// 1. Finds all filter references to the palette
/// 2. For DitherV2 filters: sets palette_id = None
/// 3. For PaletteQuantize filters: removes the entire filter from the layer
/// 4. Removes the palette from the document
/// 5. Evicts the palette from PaletteKdCache
/// 6. Invalidates affected layers
/// 7. Returns the list of affected filter IDs
#[tauri::command]
pub fn delete_palette(
    doc_id: u32,
    palette_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DeletePaletteResponse, String> {
    use engine_project::types::PaletteId;

    let pid = PaletteId::new(palette_id);

    // 1. Verify palette exists
    {
        let snapshot = state.require_session(doc_id)?.document_handle.snapshot();
        if !snapshot.palettes.iter().any(|p| p.id == palette_id) {
            return Err(format!("Palette {} not found", palette_id));
        }
    }

    // 2. Find all filter references and clear them, collecting affected filter IDs
    //    Also track affected layer IDs for invalidation.
    crate::undo::with_document_undo(&state, Some(&app_handle), doc_id, || {
        let mut affected_filter_ids: Vec<String> = Vec::new();
        let mut affected_layer_ids: Vec<u32> = Vec::new();

        state.require_session(doc_id)?.document_handle.mutate(|doc| {
        // Recursive helper to walk layers and clear palette references
        fn clear_palette_refs(
            nodes: &mut Vec<engine_project::layer::LayerNode>,
            palette_id: engine_project::types::PaletteId,
            affected_filter_ids: &mut Vec<String>,
            affected_layer_ids: &mut Vec<u32>,
        ) {
            for node in nodes.iter_mut() {
                match node {
                    engine_project::layer::LayerNode::Leaf(layer) => {
                        let mut layer_affected = false;
                        let mut filters_to_remove: Vec<usize> = Vec::new();

                        for (idx, filter) in layer.filters.iter_mut().enumerate() {
                            match &mut filter.params {
                                engine_project::filter::FilterParams::DitherV2(params) => {
                                    if params.palette_id == Some(palette_id) {
                                        // Clear the palette reference
                                        params.palette_id = None;
                                        affected_filter_ids.push(filter.id.to_string());
                                        layer_affected = true;
                                    }
                                }
                                engine_project::filter::FilterParams::PaletteQuantize { palette_id: pid, .. } => {
                                    if *pid == palette_id {
                                        // Mark for removal
                                        affected_filter_ids.push(filter.id.to_string());
                                        filters_to_remove.push(idx);
                                        layer_affected = true;
                                    }
                                }
                                _ => {}
                            }
                        }

                        // Remove PaletteQuantize filters (in reverse to preserve indices)
                        for idx in filters_to_remove.into_iter().rev() {
                            layer.filters.remove(idx);
                        }

                        if layer_affected {
                            affected_layer_ids.push(layer.id.0);
                        }
                    }
                    engine_project::layer::LayerNode::Group(group) => {
                        clear_palette_refs(
                            &mut group.children,
                            palette_id,
                            affected_filter_ids,
                            affected_layer_ids,
                        );
                    }
                }
            }
        }

        clear_palette_refs(&mut doc.root, pid, &mut affected_filter_ids, &mut affected_layer_ids);

        // 3. Remove the palette from the document
        doc.palettes.retain(|p| p.id != palette_id);

        // Increment document revision
        doc.increment_generation();
    });

    // 4. Evict from PaletteKdCache and PaletteLutCache
    let doc = state.require_session(doc_id)?.document_handle.snapshot().id.0;
    state.palette_cache.evict(doc, palette_id);
    state.palette_lut_cache.evict(doc, palette_id);

    // 5. Invalidate affected layers
    for layer_id in &affected_layer_ids {
        engine_tiles::invalidation::invalidate(
            &state.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc, layer: *layer_id,
            },
        );
    }

    if !affected_layer_ids.is_empty() {
            schedule_dirty_viewport_tiles(&state);
        }

        Ok(DeletePaletteResponse { affected_filter_ids })
    })
}

// ============================================================================
// Selection Commands
// ============================================================================

/// Update selection state and broadcast to all windows.
#[tauri::command]
pub fn set_selection(
    layer_id: Option<u32>,
    filter_id: Option<String>,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let mut sel = state.selection.lock().map_err(|e| e.to_string())?;
    sel.selected_layer_id = layer_id;
    sel.selected_filter_id = filter_id.clone();
    drop(sel);

    let _ = app_handle.emit_to(
        tauri::EventTarget::Any,
        "selection-changed",
        SelectionChangedPayload {
            selected_layer_id: layer_id,
            selected_filter_id: filter_id,
        },
    );

    Ok(())
}

/// Get current selection state (for initial fetch on window mount).
#[tauri::command]
pub fn get_selection(state: State<'_, Arc<AppState>>) -> Result<SelectionState, String> {
    let sel = state.selection.lock().map_err(|e| e.to_string())?;
    Ok(sel.clone())
}

/// Track O: launch auto-check is release-only (`cfg!(debug_assertions)` skip).
#[tauri::command]
pub fn is_release_build() -> bool {
    !cfg!(debug_assertions)
}

/// Industrial-gate T10: Preferences GPU preview opt-in status.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuPreviewStatus {
    /// Effective gate (`gpu_preview_enabled`).
    pub enabled: bool,
    /// Adapter + resident executor present.
    pub available: bool,
    /// `DITHER_GPU_PREVIEW` env is set (overrides Preferences for soak/CI).
    pub env_forced: bool,
}

#[tauri::command]
pub fn get_gpu_preview_status(state: State<'_, Arc<AppState>>) -> GpuPreviewStatus {
    GpuPreviewStatus {
        enabled: engine_gpu::gpu_preview_enabled(),
        available: state.gpu.is_some() && state.gpu_executor.is_some(),
        env_forced: std::env::var("DITHER_GPU_PREVIEW").is_ok(),
    }
}

/// Preferences: set Path B GPU preview authorship (UI override). Env still wins when set.
#[tauri::command]
pub fn set_gpu_preview_enabled(
    enabled: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<GpuPreviewStatus, String> {
    engine_gpu::set_gpu_preview_ui_override(Some(enabled));
    // Re-author visible Composite under the new gate (CPU or GPU).
    if let Ok(session) = state.active_session() {
        let doc = session.document_handle.snapshot().id.0;
        let viewport = state.viewport.lock().unwrap().clone();
        for coord in &viewport.visible_tiles {
            state.tile_cache.mark_dirty(engine_tiles::TileKey {
                doc,
                layer: 0,
                coord: *coord,
                stage: engine_tiles::CacheStage::Composite,
            });
        }
        schedule_dirty_viewport_tiles(&state);
    }
    Ok(get_gpu_preview_status(state))
}

#[tauri::command]
pub fn list_open_documents(state: State<'_, Arc<AppState>>) -> OpenDocumentsPayload {
    state.tab_list()
}

#[tauri::command]
pub fn set_active_document(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    state.activate(doc_id)?;
    emit_document_changed(&app_handle, "document_activated", None, Some(doc_id));
    emit_tabs_changed(Some(&app_handle), &state);
    schedule_dirty_viewport_tiles(&state);
    get_document_snapshot(state)
}

#[tauri::command]
pub fn close_document(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<OpenDocumentsPayload, String> {
    state.close_session(doc_id)?;
    emit_document_changed(&app_handle, "document_closed", None, Some(doc_id));
    emit_tabs_changed(Some(&app_handle), &state);
    Ok(state.tab_list())
}

/// Minimal AppState for unit/integration tests (commands + undo).
#[cfg(test)]
pub(crate) fn make_test_app_state() -> Arc<AppState> {
    use engine_project::Document;
    use engine_project::types::DocumentId;

    let state = AppState::empty_process(None, 512 * 1024 * 1024, true);
    state.spawn_session(Document::new(DocumentId::new(1), 800, 600));
    Arc::new(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_mode_parsing_works() {
        let bm = match "multiply".into() {
            name => match name {
                "multiply" => Some(BlendMode::Multiply),
                _ => None,
            }
        };
        assert!(bm.is_some());
    }

    #[test]
    fn test_f32_to_u8_conversion() {
        assert_eq!(f32_to_u8(0.0), 0);
        assert_eq!(f32_to_u8(1.0), 255);
        assert_eq!(f32_to_u8(0.5), 127); // 0.5 * 255 = 127.5, clamped as u8 = 127
        assert_eq!(f32_to_u8(-0.1), 0); // clamped
        assert_eq!(f32_to_u8(1.5), 255); // clamped
    }

    #[test]
    fn test_encode_rgba_to_png() {
        // Minimal 1x1 red pixel
        let buffer = vec![255u8, 0, 0, 255];
        let result = encode_rgba_to_png(&buffer, 1, 1);
        assert!(result.is_ok());
        let png_bytes = result.unwrap();
        // PNG magic bytes
        assert_eq!(&png_bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);
    }

    // ========================================================================
    // hex_to_linear tests
    // ========================================================================

    #[test]
    fn hex_to_linear_valid_uppercase() {
        let result = hex_to_linear("FF0000").unwrap();
        // FF → 255 → srgb_to_linear(255) ≈ 1.0
        assert!((result.r - 1.0).abs() < 1e-5);
        assert!((result.g - 0.0).abs() < 1e-5);
        assert!((result.b - 0.0).abs() < 1e-5);
    }

    #[test]
    fn hex_to_linear_valid_lowercase() {
        let result = hex_to_linear("00ff00").unwrap();
        assert!((result.r - 0.0).abs() < 1e-5);
        assert!((result.g - 1.0).abs() < 1e-5);
        assert!((result.b - 0.0).abs() < 1e-5);
    }

    #[test]
    fn hex_to_linear_valid_mixed_case() {
        let result = hex_to_linear("aAbBcC").unwrap();
        assert!(result.r > 0.0 && result.r < 1.0);
        assert!(result.g > 0.0 && result.g < 1.0);
        assert!(result.b > 0.0 && result.b < 1.0);
    }

    #[test]
    fn hex_to_linear_black() {
        let result = hex_to_linear("000000").unwrap();
        assert_eq!(result.r, 0.0);
        assert_eq!(result.g, 0.0);
        assert_eq!(result.b, 0.0);
    }

    #[test]
    fn hex_to_linear_white() {
        let result = hex_to_linear("FFFFFF").unwrap();
        assert!((result.r - 1.0).abs() < 1e-5);
        assert!((result.g - 1.0).abs() < 1e-5);
        assert!((result.b - 1.0).abs() < 1e-5);
    }

    #[test]
    fn hex_to_linear_err_too_short() {
        let result = hex_to_linear("FFF");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_too_long() {
        let result = hex_to_linear("FF00FF00");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_empty() {
        let result = hex_to_linear("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_with_hash_prefix() {
        let result = hex_to_linear("#FF0000");
        assert!(result.is_err());
        // 7 chars with '#', so length check fails
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_non_hex_chars() {
        let result = hex_to_linear("GGHHII");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid hex character"));
    }

    #[test]
    fn hex_to_linear_err_special_chars() {
        let result = hex_to_linear("FF$$00");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid hex character"));
    }

    // ========================================================================
    // linear_to_hex tests
    // ========================================================================

    #[test]
    fn linear_to_hex_black() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 0.0, g: 0.0, b: 0.0 };
        assert_eq!(linear_to_hex(&color), "000000");
    }

    #[test]
    fn linear_to_hex_white() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 1.0, g: 1.0, b: 1.0 };
        assert_eq!(linear_to_hex(&color), "FFFFFF");
    }

    #[test]
    fn linear_to_hex_red() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 1.0, g: 0.0, b: 0.0 };
        assert_eq!(linear_to_hex(&color), "FF0000");
    }

    #[test]
    fn linear_to_hex_green() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 0.0, g: 1.0, b: 0.0 };
        assert_eq!(linear_to_hex(&color), "00FF00");
    }

    #[test]
    fn linear_to_hex_blue() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 0.0, g: 0.0, b: 1.0 };
        assert_eq!(linear_to_hex(&color), "0000FF");
    }

    #[test]
    fn linear_to_hex_uppercase_format() {
        use engine_color::palette::LinearColor;
        // Verify output is always uppercase
        let color = LinearColor { r: 0.5, g: 0.5, b: 0.5 };
        let hex = linear_to_hex(&color);
        assert_eq!(hex.len(), 6);
        assert_eq!(hex, hex.to_uppercase());
    }

    #[test]
    fn linear_to_hex_clamps_above_one() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 1.5, g: 2.0, b: 3.0 };
        // linear_to_srgb clamps to [0, 1] before conversion
        assert_eq!(linear_to_hex(&color), "FFFFFF");
    }

    #[test]
    fn linear_to_hex_clamps_below_zero() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: -1.0, g: -0.5, b: -0.1 };
        assert_eq!(linear_to_hex(&color), "000000");
    }

    // ========================================================================
    // Round-trip tests
    // ========================================================================

    #[test]
    fn hex_round_trip_known_values() {
        // For these known hex values, converting to linear and back should be identity
        let test_cases = [
            "000000", "FFFFFF", "FF0000", "00FF00", "0000FF",
            "808080", "C0C0C0", "A0B0C0", "123456", "ABCDEF",
        ];
        for hex in &test_cases {
            let linear = hex_to_linear(hex).unwrap();
            let back = linear_to_hex(&linear);
            assert_eq!(
                &back,
                &hex.to_uppercase(),
                "Round-trip failed for input '{}'",
                hex
            );
        }
    }

    #[test]
    fn hex_round_trip_case_insensitive() {
        // Same color expressed in different cases should produce the same uppercase output
        let lower = hex_to_linear("abcdef").unwrap();
        let upper = hex_to_linear("ABCDEF").unwrap();
        let mixed = hex_to_linear("AbCdEf").unwrap();

        let hex_lower = linear_to_hex(&lower);
        let hex_upper = linear_to_hex(&upper);
        let hex_mixed = linear_to_hex(&mixed);

        assert_eq!(hex_lower, "ABCDEF");
        assert_eq!(hex_upper, "ABCDEF");
        assert_eq!(hex_mixed, "ABCDEF");
    }

    // ========================================================================
    // Track L: colors_to_oklab / get_palette_oklab
    // ========================================================================

    fn gameboy_hexes() -> Vec<String> {
        use engine_color::palette::find_preset;
        find_preset("gameboy")
            .expect("gameboy preset")
            .colors_srgb
            .iter()
            .map(|&(r, g, b)| format!("#{:02X}{:02X}{:02X}", r, g, b))
            .collect()
    }

    #[test]
    fn colors_to_oklab_gameboy_matches_linear_to_oklab() {
        use engine_color::palette::{find_preset, srgb_to_linear};
        use engine_color::{linear_to_oklab, LinRgb};

        let hexes = gameboy_hexes();
        let points = oklab_points_from_hexes(&hexes).unwrap();
        assert_eq!(points.len(), 4);

        let gb = find_preset("gameboy").unwrap();
        const EPS: f32 = 1e-5;
        for (i, &(r, g, b)) in gb.colors_srgb.iter().enumerate() {
            let expected = linear_to_oklab(LinRgb {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            });
            assert!(
                (points[i].l - expected.l).abs() < EPS,
                "L[{}]: {} vs {}",
                i,
                points[i].l,
                expected.l
            );
            assert!(
                (points[i].a - expected.a).abs() < EPS,
                "a[{}]: {} vs {}",
                i,
                points[i].a,
                expected.a
            );
            assert!(
                (points[i].b - expected.b).abs() < EPS,
                "b[{}]: {} vs {}",
                i,
                points[i].b,
                expected.b
            );
            assert_eq!(points[i].srgb_hex, hexes[i]);
        }
    }

    #[test]
    fn colors_to_oklab_empty_list() {
        let points = oklab_points_from_hexes(&[]).unwrap();
        assert!(points.is_empty());
    }

    #[test]
    fn colors_to_oklab_invalid_hex_errors() {
        let err = oklab_points_from_hexes(&["#GG0000".into()]).unwrap_err();
        assert!(err.contains("invalid") || err.contains("Hex"));
    }

    #[test]
    fn get_palette_oklab_missing_palette_errors() {
        let state = make_test_app_state();
        let snapshot = state.must_active().document_handle.snapshot();
        assert!(snapshot.palettes.iter().all(|p| p.id != 9999));
        drop(snapshot);
        let err = snapshot_palette_oklab(&state, 9999).unwrap_err();
        assert!(err.contains("Palette 9999 not found"));
    }

    #[test]
    fn get_palette_oklab_gameboy_matches_linear_to_oklab() {
        use engine_color::palette::{find_preset, srgb_to_linear, LinearColor};
        use engine_color::{linear_to_oklab, LinRgb};

        let gb = find_preset("gameboy").unwrap();
        let linear: Vec<LinearColor> = gb
            .colors_srgb
            .iter()
            .map(|&(r, g, b)| LinearColor {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            })
            .collect();

        let state = make_test_app_state();
        let mut palette_id = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let pid = doc.add_palette("Game Boy".to_string(), linear);
            palette_id = pid.0;
        });

        let points = snapshot_palette_oklab(&state, palette_id).unwrap();
        assert_eq!(points.len(), 4);
        const EPS: f32 = 1e-5;
        for (i, &(r, g, b)) in gb.colors_srgb.iter().enumerate() {
            let expected = linear_to_oklab(LinRgb {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            });
            assert!((points[i].l - expected.l).abs() < EPS);
            assert!((points[i].a - expected.a).abs() < EPS);
            assert!((points[i].b - expected.b).abs() < EPS);
        }
    }

    fn snapshot_palette_oklab(
        state: &AppState,
        palette_id: u32,
    ) -> Result<Vec<OklabPointDto>, String> {
        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == palette_id)
            .ok_or_else(|| format!("Palette {} not found", palette_id))?;
        Ok(oklab_points_from_linear(&palette.colors))
    }

    #[test]
    fn hex_round_trip_exhaustive_boundaries() {
        // Test all boundary values (00, 01, FE, FF) per channel
        let boundary_values = ["00", "01", "7F", "80", "FE", "FF"];
        for r in &boundary_values {
            for g in &boundary_values {
                for b in &boundary_values {
                    let hex = format!("{}{}{}", r, g, b);
                    let linear = hex_to_linear(&hex).unwrap();
                    let back = linear_to_hex(&linear);
                    assert_eq!(
                        back, hex,
                        "Round-trip failed for '{}'",
                        hex
                    );
                }
            }
        }
    }

    // ========================================================================
    // find_layers_referencing_palette tests
    // ========================================================================

    #[test]
    fn find_layers_referencing_palette_empty_tree() {
        use engine_project::types::PaletteId;

        let nodes: Vec<engine_project::layer::LayerNode> = vec![];
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(1));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_no_references() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{FilterInstance, FilterKind, FilterParams, DitherMode};

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::Bayer { matrix_size: 4 },
                color_depth: 4,
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(1));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_dither_v2_match() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode,
        };

        let mut layer = Layer::new(LayerId::new(5), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: Some(PaletteId::new(42)),
            ..Default::default()
            }),
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(42));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(5));
    }

    #[test]
    fn find_layers_referencing_palette_dither_v2_no_palette() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode,
        };

        let mut layer = Layer::new(LayerId::new(5), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: None, // No palette reference,
            ..Default::default()
            }),
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(42));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_palette_quantize_match() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{FilterInstance, FilterKind, FilterParams, DiffusionKernel};

        let mut layer = Layer::new(LayerId::new(10), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(7),
                diffusion: Some(DiffusionKernel::FloydSteinberg),
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(7));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(10));
    }

    #[test]
    fn find_layers_referencing_palette_wrong_palette_id() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{FilterInstance, FilterKind, FilterParams};

        let mut layer = Layer::new(LayerId::new(10), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(7),
                diffusion: None,
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        // Search for a different palette ID
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(99));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_recursive_group() {
        use engine_project::layer::{Layer, LayerGroup, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode, DiffusionKernel,
        };

        // Create a nested group structure:
        // root:
        //   - Layer 1 (no palette ref)
        //   - Group 2:
        //     - Layer 3 (DitherV2 refs palette 5)
        //     - Group 4:
        //       - Layer 5 (PaletteQuantize refs palette 5)
        //   - Layer 6 (PaletteQuantize refs palette 99)

        let layer1 = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);

        let mut layer3 = Layer::new(LayerId::new(3), LayerKind::Raster, 256, 256);
        layer3.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer8x8,
                levels: 8,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: Some(PaletteId::new(5)),
            ..Default::default()
            }),
        ));

        let mut layer5 = Layer::new(LayerId::new(5), LayerKind::Raster, 256, 256);
        layer5.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(5),
                diffusion: Some(DiffusionKernel::Atkinson),
            },
        ));

        let mut group4 = LayerGroup::new(LayerId::new(4));
        group4.children.push(LayerNode::Leaf(layer5));

        let mut group2 = LayerGroup::new(LayerId::new(2));
        group2.children.push(LayerNode::Leaf(layer3));
        group2.children.push(LayerNode::Group(group4));

        let mut layer6 = Layer::new(LayerId::new(6), LayerKind::Raster, 256, 256);
        layer6.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(99),
                diffusion: None,
            },
        ));

        let nodes = vec![
            LayerNode::Leaf(layer1),
            LayerNode::Group(group2),
            LayerNode::Leaf(layer6),
        ];

        // Search for palette 5 → should find layers 3 and 5
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(5));
        assert_eq!(result.len(), 2);
        assert!(result.contains(&LayerId::new(3)));
        assert!(result.contains(&LayerId::new(5)));

        // Search for palette 99 → should find only layer 6
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(99));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(6));
    }

    #[test]
    fn find_layers_referencing_palette_multiple_filters_on_one_layer() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode, DiffusionKernel,
        };

        // Layer with both DitherV2 and PaletteQuantize referencing same palette
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: Some(PaletteId::new(3)),
            ..Default::default()
            }),
        ));
        layer.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(3),
                diffusion: Some(DiffusionKernel::FloydSteinberg),
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        // Should only return the layer once even though two filters reference it
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(3));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(1));
    }

    // ========================================================================
    // Integration Tests: Palette CRUD Lifecycle, Invalidation Cascade,
    // Force-Delete with Filter Reference Clearing
    // ========================================================================

    // ------------------------------------------------------------------
    // Integration Test: Full Palette CRUD Lifecycle
    // ------------------------------------------------------------------

    #[test]
    fn integration_palette_crud_lifecycle() {
        let state = make_test_app_state();

        // === CREATE ===
        let name = "Test Palette".to_string();
        let trimmed_name = name.trim().to_string();
        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let pid = doc.add_palette(trimmed_name.clone(), vec![]);
            palette_id_raw = pid.0;
            doc.increment_generation();
        });

        // Verify palette was created
        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw);
        assert!(palette.is_some());
        let palette = palette.unwrap();
        assert_eq!(palette.name, "Test Palette");
        assert_eq!(palette.colors.len(), 0);
        assert_eq!(palette.revision, 1);
        drop(snapshot);

        // === RENAME ===
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.name = "Renamed Palette".to_string();
            }
        });
        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.name, "Renamed Palette");
        drop(snapshot);

        // === ADD COLORS ===
        let red = hex_to_linear("FF0000").unwrap();
        let green = hex_to_linear("00FF00").unwrap();
        let blue = hex_to_linear("0000FF").unwrap();

        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.push(red);
                p.revision += 1;
            }
        });
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.push(green);
                p.revision += 1;
            }
        });
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.push(blue);
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.colors.len(), 3);
        assert_eq!(palette.revision, 4); // initial 1 + 3 adds
        drop(snapshot);

        // === UPDATE COLOR ===
        // Change green (index 1) to yellow FF FF 00
        let yellow = hex_to_linear("FFFF00").unwrap();
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors[1] = yellow;
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.revision, 5);
        // Verify the color at index 1 changed (via hex round-trip)
        let hex_at_1 = linear_to_hex(&palette.colors[1]);
        assert_eq!(hex_at_1, "FFFF00");
        drop(snapshot);

        // === REMOVE COLOR ===
        // Remove index 0 (red)
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.remove(0);
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.colors.len(), 2);
        assert_eq!(palette.revision, 6);
        // First color should now be yellow
        assert_eq!(linear_to_hex(&palette.colors[0]), "FFFF00");
        // Second color should be blue
        assert_eq!(linear_to_hex(&palette.colors[1]), "0000FF");
        drop(snapshot);

        // === REORDER ===
        // Move index 0 (yellow) to index 1 → [blue, yellow]
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                let color = p.colors.remove(0);
                p.colors.insert(1, color);
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.colors.len(), 2);
        assert_eq!(palette.revision, 7);
        assert_eq!(linear_to_hex(&palette.colors[0]), "0000FF");
        assert_eq!(linear_to_hex(&palette.colors[1]), "FFFF00");
        drop(snapshot);

        // === DELETE ===
        state.must_active().document_handle.mutate(|doc| {
            doc.palettes.retain(|p| p.id != palette_id_raw);
            doc.increment_generation();
        });

        let snapshot = state.must_active().document_handle.snapshot();
        assert!(snapshot.palettes.iter().find(|p| p.id == palette_id_raw).is_none());
        drop(snapshot);
    }

    // ------------------------------------------------------------------
    // Integration Test: Invalidation Cascade Verification
    // Modify palette → verify tiles marked dirty for referencing layers
    // ------------------------------------------------------------------

    #[test]
    fn integration_invalidation_cascade_on_palette_modify() {
        use std::sync::atomic::Ordering;
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode,
        };
        use engine_tiles::{CacheStage, TileCoord, TileKey, PixelTile};

        let state = make_test_app_state();

        // 1. Create a palette with one color
        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let color = engine_color::palette::LinearColor {
                r: 1.0, g: 0.0, b: 0.0,
            };
            let pid = doc.add_palette("Test".to_string(), vec![color]);
            palette_id_raw = pid.0;
        });

        // 2. Add a layer with a DitherV2 filter referencing this palette
        let layer_id = 42u32;
        state.must_active().document_handle.mutate(|doc| {
            let mut layer = Layer::new(
                LayerId::new(layer_id),
                LayerKind::Raster,
                800,
                600,
            );
            layer.filters.push(FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: DitherModeV2::Bayer4x4,
                    levels: 4,
                    threshold_scale: 1.0,
                    pixel_size: 1,
                    color_mode: DitherColorMode::Rgb,
                    palette_id: Some(PaletteId::new(palette_id_raw)),
            ..Default::default()
                }),
            ));
            doc.root.push(LayerNode::Leaf(layer));
        });

        // 3. Insert clean Processed and Composite tiles for this layer
        let processed_key = TileKey {
            doc: 1,
            layer: layer_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let composite_key = TileKey {
            doc: 1,
            layer: layer_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Composite,
        };
        let tile = Arc::new(PixelTile::new());
        state.tile_cache.get_or_insert(processed_key, tile.clone());
        state.tile_cache.get_or_insert(composite_key, tile.clone());

        // Verify tiles are NOT dirty before modification
        let entry_p = state.tile_cache.entries.get(&processed_key).unwrap();
        assert!(!entry_p.dirty.load(Ordering::Relaxed));
        drop(entry_p);
        let entry_c = state.tile_cache.entries.get(&composite_key).unwrap();
        assert!(!entry_c.dirty.load(Ordering::Relaxed));
        drop(entry_c);

        // 4. Modify the palette (add a color) and trigger invalidation
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                let green = engine_color::palette::LinearColor {
                    r: 0.0, g: 1.0, b: 0.0,
                };
                p.colors.push(green);
                p.revision += 1;
            }
        });
        invalidate_palette_changed(PaletteId::new(palette_id_raw), &state);

        // 5. Verify tiles are now dirty
        let entry_p = state.tile_cache.entries.get(&processed_key).unwrap();
        assert!(
            entry_p.dirty.load(Ordering::Relaxed),
            "Processed tile should be dirty after palette modification"
        );
        drop(entry_p);
        let entry_c = state.tile_cache.entries.get(&composite_key).unwrap();
        assert!(
            entry_c.dirty.load(Ordering::Relaxed),
            "Composite tile should be dirty after palette modification"
        );
        drop(entry_c);
    }

    // ------------------------------------------------------------------
    // Integration Test: Invalidation does NOT happen for unreferenced palette
    // ------------------------------------------------------------------

    #[test]
    fn integration_no_invalidation_for_unreferenced_palette() {
        use std::sync::atomic::Ordering;
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_tiles::{CacheStage, TileCoord, TileKey, PixelTile};

        let state = make_test_app_state();

        // Create a palette (not referenced by any filter)
        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let pid = doc.add_palette("Unused".to_string(), vec![]);
            palette_id_raw = pid.0;
        });

        // Add a layer with NO filters referencing the palette
        let layer_id = 10u32;
        state.must_active().document_handle.mutate(|doc| {
            let layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, 800, 600);
            doc.root.push(LayerNode::Leaf(layer));
        });

        // Insert a clean Processed tile for this layer
        let key = TileKey {
            doc: 1,
            layer: layer_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let tile = Arc::new(PixelTile::new());
        state.tile_cache.get_or_insert(key, tile);

        // Trigger invalidation for the unreferenced palette
        invalidate_palette_changed(PaletteId::new(palette_id_raw), &state);

        // Tile should remain clean
        let entry = state.tile_cache.entries.get(&key).unwrap();
        assert!(
            !entry.dirty.load(Ordering::Relaxed),
            "Tile should NOT be dirty when palette is unreferenced"
        );
    }

    // ------------------------------------------------------------------
    // Integration Test: Force-delete palette with filter reference clearing
    // ------------------------------------------------------------------

    #[test]
    fn integration_force_delete_palette_clears_references() {
        use std::sync::atomic::Ordering;
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode, DiffusionKernel,
        };
        use engine_tiles::{CacheStage, TileCoord, TileKey, PixelTile};

        let state = make_test_app_state();

        // 1. Create a palette with colors
        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let colors = vec![
                engine_color::palette::LinearColor { r: 1.0, g: 0.0, b: 0.0 },
                engine_color::palette::LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            ];
            let pid = doc.add_palette("ToDelete".to_string(), colors);
            palette_id_raw = pid.0;
        });

        // 2. Add two layers:
        //    - Layer A with DitherV2 referencing this palette
        //    - Layer B with PaletteQuantize referencing this palette
        let layer_a_id = 100u32;
        let layer_b_id = 200u32;
        state.must_active().document_handle.mutate(|doc| {
            // Layer A: DitherV2 with palette_id
            let mut layer_a = Layer::new(
                LayerId::new(layer_a_id), LayerKind::Raster, 800, 600,
            );
            layer_a.filters.push(FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: DitherModeV2::Bayer4x4,
                    levels: 4,
                    threshold_scale: 1.0,
                    pixel_size: 1,
                    color_mode: DitherColorMode::Rgb,
                    palette_id: Some(PaletteId::new(palette_id_raw)),
            ..Default::default()
                }),
            ));
            doc.root.push(LayerNode::Leaf(layer_a));

            // Layer B: PaletteQuantize with palette_id
            let mut layer_b = Layer::new(
                LayerId::new(layer_b_id), LayerKind::Raster, 800, 600,
            );
            layer_b.filters.push(FilterInstance::new(
                FilterKind::PaletteQuantize,
                FilterParams::PaletteQuantize {
                    palette_id: PaletteId::new(palette_id_raw),
                    diffusion: Some(DiffusionKernel::FloydSteinberg),
                },
            ));
            doc.root.push(LayerNode::Leaf(layer_b));
        });

        // 3. Insert clean tiles for both layers
        let key_a = TileKey {
            doc: 1,
            layer: layer_a_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let key_b = TileKey {
            doc: 1,
            layer: layer_b_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let tile = Arc::new(PixelTile::new());
        state.tile_cache.get_or_insert(key_a, tile.clone());
        state.tile_cache.get_or_insert(key_b, tile.clone());

        // 4. Perform force-delete logic (replicating delete_palette behavior)
        let pid = PaletteId::new(palette_id_raw);
        let mut affected_filter_ids: Vec<String> = Vec::new();
        let mut affected_layer_ids: Vec<u32> = Vec::new();

        state.must_active().document_handle.mutate(|doc| {
            // Clear palette references
            for node in doc.root.iter_mut() {
                if let LayerNode::Leaf(layer) = node {
                    let mut layer_affected = false;
                    let mut filters_to_remove: Vec<usize> = Vec::new();

                    for (idx, filter) in layer.filters.iter_mut().enumerate() {
                        match &mut filter.params {
                            FilterParams::DitherV2(params) => {
                                if params.palette_id == Some(pid) {
                                    params.palette_id = None;
                                    affected_filter_ids.push(filter.id.to_string());
                                    layer_affected = true;
                                }
                            }
                            FilterParams::PaletteQuantize {
                                palette_id: ref p, ..
                            } => {
                                if *p == pid {
                                    affected_filter_ids.push(filter.id.to_string());
                                    filters_to_remove.push(idx);
                                    layer_affected = true;
                                }
                            }
                            _ => {}
                        }
                    }

                    // Remove PaletteQuantize filters in reverse
                    for idx in filters_to_remove.into_iter().rev() {
                        layer.filters.remove(idx);
                    }

                    if layer_affected {
                        affected_layer_ids.push(layer.id.0);
                    }
                }
            }

            // Remove the palette
            doc.palettes.retain(|p| p.id != palette_id_raw);
            doc.increment_generation();
        });

        // Evict from palette cache
        state.palette_cache.evict(1, palette_id_raw);
        state.palette_lut_cache.evict(1, palette_id_raw);

        // Invalidate affected layers
        for layer_id in &affected_layer_ids {
            engine_tiles::invalidation::invalidate(
                &state.tile_cache,
                engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: *layer_id,
                },
            );
        }

        // 5. Verify: palette removed
        let snapshot = state.must_active().document_handle.snapshot();
        assert!(
            snapshot.palettes.iter().find(|p| p.id == palette_id_raw).is_none(),
            "Palette should be removed from document"
        );

        // 6. Verify: DitherV2 filter on layer A has palette_id cleared to None
        let layer_a_node = snapshot.root.iter().find(|n| {
            matches!(n, LayerNode::Leaf(l) if l.id.0 == layer_a_id)
        });
        assert!(layer_a_node.is_some());
        if let Some(LayerNode::Leaf(layer_a)) = layer_a_node {
            assert_eq!(layer_a.filters.len(), 1, "DitherV2 filter should remain");
            match &layer_a.filters[0].params {
                FilterParams::DitherV2(params) => {
                    assert_eq!(
                        params.palette_id, None,
                        "DitherV2 palette_id should be cleared to None"
                    );
                }
                _ => panic!("Expected DitherV2 filter"),
            }
        }

        // 7. Verify: PaletteQuantize filter on layer B is removed
        let layer_b_node = snapshot.root.iter().find(|n| {
            matches!(n, LayerNode::Leaf(l) if l.id.0 == layer_b_id)
        });
        assert!(layer_b_node.is_some());
        if let Some(LayerNode::Leaf(layer_b)) = layer_b_node {
            assert_eq!(
                layer_b.filters.len(), 0,
                "PaletteQuantize filter should be removed"
            );
        }
        drop(snapshot);

        // 8. Verify: affected filter IDs were collected
        assert_eq!(affected_filter_ids.len(), 2);
        assert_eq!(affected_layer_ids.len(), 2);

        // 9. Verify: tiles for affected layers are dirty
        let entry_a = state.tile_cache.entries.get(&key_a).unwrap();
        assert!(
            entry_a.dirty.load(Ordering::Relaxed),
            "Layer A tiles should be dirty after force-delete"
        );
        drop(entry_a);
        let entry_b = state.tile_cache.entries.get(&key_b).unwrap();
        assert!(
            entry_b.dirty.load(Ordering::Relaxed),
            "Layer B tiles should be dirty after force-delete"
        );
        drop(entry_b);
    }

    // ========================================================================
    // Track G: create_document
    // ========================================================================

    #[test]
    fn validate_document_dimensions_rejects_zero_and_over_max() {
        assert!(validate_document_dimensions(0, 8).is_err());
        assert!(validate_document_dimensions(8, 0).is_err());
        assert!(validate_document_dimensions(MAX_DOCUMENT_DIMENSION + 1, 8).is_err());
        assert!(validate_document_dimensions(8, MAX_DOCUMENT_DIMENSION + 1).is_err());
        assert!(validate_document_dimensions(1, 1).is_ok());
        assert!(validate_document_dimensions(MAX_DOCUMENT_DIMENSION, MAX_DOCUMENT_DIMENSION).is_ok());
    }

    #[test]
    fn invalid_create_size_leaves_document_unchanged() {
        let state = make_test_app_state();
        let before = state.must_active().document_handle.snapshot();
        let before_w = before.width;
        let before_h = before.height;
        let before_len = before.root.len();
        drop(before);

        assert!(validate_document_dimensions(0, 8).is_err());
        assert!(validate_document_dimensions(8193, 8).is_err());

        let after = state.must_active().document_handle.snapshot();
        assert_eq!(after.width, before_w);
        assert_eq!(after.height, before_h);
        assert_eq!(after.root.len(), before_len);
    }

    #[test]
    fn blank_buffer_transparent_is_zeros_white_is_ones() {
        let t = blank_rgba_f32(2, 1, BlankBackground::Transparent);
        assert_eq!(t, vec![0.0; 8]);
        let w = blank_rgba_f32(1, 1, BlankBackground::White);
        assert_eq!(w, vec![1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn create_blank_document_one_leaf_project_path_none() {
        let state = make_test_app_state();
        *state.must_active().project_path.lock().unwrap() = Some(std::path::PathBuf::from("/tmp/old.dyproj"));

        let buf = blank_rgba_f32(8, 8, BlankBackground::White);
        let response = install_raster_document(&state, 8, 8, &buf, None).unwrap();
        assert_eq!(response.doc_id, 2);
        assert_eq!(response.width, 8);
        assert_eq!(response.height, 8);
        assert!(response.tile_count >= 1);

        let snap = state.must_active().document_handle.snapshot();
        assert_eq!(snap.root.len(), 1);
        match &snap.root[0] {
            engine_project::layer::LayerNode::Leaf(layer) => {
                assert_eq!(layer.id.0, 1);
                assert_eq!(layer.kind, engine_project::types::LayerKind::Raster);
                assert!(layer.filters.is_empty());
            }
            _ => panic!("expected a single raster leaf"),
        }
        assert!(state.must_active().project_path.lock().unwrap().is_none());
        assert!(
            !state.tile_cache.entries.is_empty(),
            "decompose should insert at least one Raw tile"
        );
    }

    #[test]
    fn create_document_does_not_record_recent_files() {
        let dir = tempfile::tempdir().unwrap();
        let recent_path = dir.path().join("recent_files.json");
        std::fs::write(&recent_path, "[]").unwrap();

        let state = make_test_app_state();
        let buf = blank_rgba_f32(8, 8, BlankBackground::Transparent);
        install_raster_document(&state, 8, 8, &buf, None).unwrap();

        let contents = std::fs::read_to_string(&recent_path).unwrap();
        assert_eq!(contents.trim(), "[]");
        assert!(crate::recent_files::load_recent_files(&recent_path).is_empty());
    }

    #[test]
    fn install_raster_document_clears_undo_stacks() {
        let state = make_test_app_state();
        crate::undo::with_document_undo(&state, None, state.active_id().unwrap(), || {
            state.must_active().document_handle.mutate(|doc| {
                doc.increment_generation();
            });
            Ok::<(), String>(())
        })
        .unwrap();
        assert!(state.must_active().undo_manager.lock().unwrap().state_dto().can_undo);

        let buf = blank_rgba_f32(8, 8, BlankBackground::White);
        install_raster_document(&state, 8, 8, &buf, None).unwrap();
        let dto = state.must_active().undo_manager.lock().unwrap().state_dto();
        assert!(!dto.can_undo);
        assert!(!dto.can_redo);
    }

    #[test]
    fn two_sessions_keep_separate_handles() {
        let state = make_test_app_state();
        let first = state.active_id().unwrap();
        let buf = blank_rgba_f32(8, 8, BlankBackground::White);
        let response = install_raster_document(&state, 8, 8, &buf, None).unwrap();
        assert_ne!(response.doc_id, first);
        assert!(state.session(first).is_ok());
        assert!(state.session(response.doc_id).is_ok());
        assert_eq!(state.active_id(), Some(response.doc_id));
        state.activate(first).unwrap();
        assert_eq!(state.active_id(), Some(first));
        state.close_session(first).unwrap();
        assert!(state.session(first).is_err());
        assert_eq!(state.active_id(), Some(response.doc_id));
    }

    #[test]
    fn second_session_composite_reads_own_raw_not_doc_one() {
        use engine_tiles::{CacheStage, TileCoord, TileKey};
        use std::sync::Arc;

        let state = make_test_app_state();
        // Doc 1 leftover: opaque red Raw under doc=1 (must not leak into doc=2 composite).
        {
            let mut red = engine_tiles::PixelTile::new();
            for i in 0..red.data.len() / 4 {
                red.data[i * 4] = 1.0;
                red.data[i * 4 + 3] = 1.0;
            }
            state.tile_cache.insert_fresh_gen(
                TileKey {
                    doc: 1,
                    layer: 1,
                    coord: TileCoord {
                        level: 0,
                        x: 0,
                        y: 0,
                    },
                    stage: CacheStage::Raw,
                },
                Arc::new(red),
                1,
            );
        }

        let blue = blank_rgba_f32(8, 8, BlankBackground::White);
        // Make buffer distinctly non-red: already white via blank helper.
        let installed = install_raster_document(&state, 8, 8, &blue, None).unwrap();
        assert_ne!(installed.doc_id, 1);

        let key = TileKey {
            doc: installed.doc_id,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        let tile = crate::tile_pipeline::compute_composite_tile(key, &state)
            .expect("composite for second session");
        // White blank (or near-white), not the red planted under doc=1.
        assert!(
            tile.at(engine_tiles::HALO, engine_tiles::HALO, 0) < 0.1
                || (tile.at(engine_tiles::HALO, engine_tiles::HALO, 0) - 1.0).abs() < 0.05,
            "r={}",
            tile.at(engine_tiles::HALO, engine_tiles::HALO, 0)
        );
        assert!(
            (tile.at(engine_tiles::HALO, engine_tiles::HALO, 0)
                - tile.at(engine_tiles::HALO, engine_tiles::HALO, 1))
            .abs()
                < 0.05,
            "second-doc composite must not pick up doc=1 red Raw"
        );
    }

    // ========================================================================
    // Track P3: Import Image as Layer
    // ========================================================================

    #[test]
    fn is_release_build_false_under_debug_assertions() {
        assert!(
            !is_release_build(),
            "unit tests compile with debug_assertions; launch auto-check must stay off"
        );
    }

    fn raw_pixel_at(state: &AppState, layer: u32, x: u32, y: u32) -> [f32; 4] {
        use engine_tiles::{HALO, TILE_SIZE, TileCoord};
        let key = TileKey {
            doc: state.active_id().expect("active document"),
            layer,
            coord: TileCoord {
                level: 0,
                x: x / TILE_SIZE,
                y: y / TILE_SIZE,
            },
            stage: CacheStage::Raw,
        };
        let entry = state
            .tile_cache
            .entries
            .get(&key)
            .unwrap_or_else(|| panic!("missing raw tile for layer {layer} at ({x},{y})"));
        let lx = (x % TILE_SIZE) + HALO;
        let ly = (y % TILE_SIZE) + HALO;
        [
            entry.tile.at(lx, ly, 0),
            entry.tile.at(lx, ly, 1),
            entry.tile.at(lx, ly, 2),
            entry.tile.at(lx, ly, 3),
        ]
    }

    fn solid_rgba(w: u32, h: u32, r: f32, g: f32, b: f32, a: f32) -> Vec<f32> {
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            buf.extend_from_slice(&[r, g, b, a]);
        }
        buf
    }

    #[test]
    fn install_raster_replaces_high_gen_source_tiles() {
        use engine_tiles::{TileCoord, HALO};

        let state = make_test_app_state();
        state.must_active().document_handle.mutate(|doc| {
            doc.generations.set_document_gen(40);
        });

        let leftover = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let raw00 = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let composite = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };

        let mut red = PixelTile::new();
        red.set(HALO, HALO, 0, 1.0);
        red.set(HALO, HALO, 3, 1.0);
        assert!(state
            .tile_cache
            .insert_fresh_gen(raw00, Arc::new(red), 50));
        assert!(state.tile_cache.insert_fresh_gen(
            leftover,
            Arc::new(PixelTile::new()),
            50
        ));
        assert!(state.tile_cache.insert_fresh_gen(
            composite,
            Arc::new(PixelTile::new()),
            50
        ));

        let blue = solid_rgba(8, 8, 0.0, 0.0, 1.0, 1.0);
        let installed = install_raster_document(&state, 8, 8, &blue, None).unwrap();
        let new_doc = installed.doc_id;

        assert!(
            state.tile_cache.entries.get(&leftover).is_some(),
            "previous document tiles stay until that session is closed"
        );
        let px = raw_pixel_at(&state, 1, 0, 0);
        assert!(
            (px[2] - 1.0).abs() < 1e-5 && px[0].abs() < 1e-5,
            "expected blue Image Source, got {px:?}"
        );

        let live_gen = state.must_active().document_handle
            .snapshot()
            .generations
            .current_document_gen();
        assert_eq!(live_gen, 1);
        let new_raw = TileKey {
            doc: new_doc,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let raw_entry = state.tile_cache.entries.get(&new_raw).unwrap();
        assert_eq!(raw_entry.generation, 1);
        drop(raw_entry);

        let new_composite = TileKey {
            doc: new_doc,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        assert!(state.tile_cache.insert_fresh_gen(
            new_composite,
            Arc::new(PixelTile::new()),
            live_gen
        ));
    }

    #[test]
    fn place_image_at_origin_pads_smaller_and_clips_larger() {
        let src = vec![1.0, 0.0, 0.0, 1.0];
        let padded = place_image_at_origin(&src, 1, 1, 2, 1);
        assert_eq!(padded, vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);

        let wide = vec![
            1.0, 0.0, 0.0, 1.0, // x=0
            0.0, 1.0, 0.0, 1.0, // x=1 discarded
        ];
        let clipped = place_image_at_origin(&wide, 2, 1, 1, 1);
        assert_eq!(clipped, vec![1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn import_raster_layer_requires_open_document() {
        let state = make_test_app_state();
        let src = solid_rgba(2, 2, 1.0, 0.0, 0.0, 1.0);
        let err = import_raster_layer(&state, 1, 2, 2, &src, None).unwrap_err();
        assert!(
            err.contains("No document") || err.contains("closed") || err.contains("session"),
            "{err}"
        );
    }

    #[test]
    fn import_smaller_image_leaves_transparent_remainder() {
        let state = make_test_app_state();
        let bg = blank_rgba_f32(16, 16, BlankBackground::White);
        install_raster_document(&state, 16, 16, &bg, None).unwrap();

        let src = solid_rgba(4, 4, 1.0, 0.0, 0.0, 1.0);
        let resp = import_raster_layer(&state, state.active_id().unwrap(), 4, 4, &src, None).unwrap();
        assert_eq!(resp.layer_id, 2);

        let snap = state.must_active().document_handle.snapshot();
        assert_eq!(snap.root.len(), 2);
        assert_eq!(snap.width, 16);
        assert_eq!(snap.height, 16);

        let origin = raw_pixel_at(&state, 2, 0, 0);
        assert!((origin[0] - 1.0).abs() < 1e-5 && origin[3] > 0.9);
        let remainder = raw_pixel_at(&state, 2, 8, 0);
        assert!(remainder[3].abs() < 1e-5, "outside source must stay transparent");
        let base = raw_pixel_at(&state, 1, 0, 0);
        assert!((base[0] - 1.0).abs() < 1e-5 && (base[3] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn import_larger_image_clips_to_document() {
        let state = make_test_app_state();
        let bg = blank_rgba_f32(8, 8, BlankBackground::Transparent);
        install_raster_document(&state, 8, 8, &bg, None).unwrap();

        // 12×8: unique colors at (0,0) red, (7,0) blue, (11,0) green (green must be dropped).
        let mut src = vec![0.0; 12 * 8 * 4];
        src[0..4].copy_from_slice(&[1.0, 0.0, 0.0, 1.0]);
        let i7 = 7 * 4;
        src[i7..i7 + 4].copy_from_slice(&[0.0, 0.0, 1.0, 1.0]);
        let i11 = 11 * 4;
        src[i11..i11 + 4].copy_from_slice(&[0.0, 1.0, 0.0, 1.0]);

        let resp = import_raster_layer(&state, state.active_id().unwrap(), 12, 8, &src, None).unwrap();
        let red = raw_pixel_at(&state, resp.layer_id, 0, 0);
        let blue = raw_pixel_at(&state, resp.layer_id, 7, 0);
        assert!((red[0] - 1.0).abs() < 1e-5);
        assert!((blue[2] - 1.0).abs() < 1e-5);
        let snap = state.must_active().document_handle.snapshot();
        assert_eq!(snap.width, 8);
        assert_eq!(snap.height, 8);
    }

    #[test]
    fn import_raster_layer_does_not_rewrite_existing_filter_palette_id() {
        use engine_project::filter::{
            DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
        };
        use engine_project::types::PaletteId;

        let state = make_test_app_state();
        let bg = blank_rgba_f32(8, 8, BlankBackground::White);
        install_raster_document(&state, 8, 8, &bg, None).unwrap();

        state.must_active().document_handle.mutate(|doc| {
            if let engine_project::layer::LayerNode::Leaf(layer) = &mut doc.root[0] {
                layer.filters.push(FilterInstance::new(
                    FilterKind::Dither,
                    FilterParams::DitherV2(DitherParamsV2 {
                        mode: DitherModeV2::Bayer4x4,
                        levels: 4,
                        threshold_scale: 1.0,
                        pixel_size: 1,
                        color_mode: DitherColorMode::Rgb,
                        palette_id: Some(PaletteId::new(7)),
                        ..Default::default()
                    }),
                ));
            }
        });

        let src = solid_rgba(2, 2, 0.0, 1.0, 0.0, 1.0);
        import_raster_layer(&state, state.active_id().unwrap(), 2, 2, &src, None).unwrap();

        let snap = state.must_active().document_handle.snapshot();
        match &snap.root[0] {
            engine_project::layer::LayerNode::Leaf(layer) => match &layer.filters[0].params {
                FilterParams::DitherV2(p) => {
                    assert_eq!(p.palette_id, Some(PaletteId::new(7)));
                }
                other => panic!("expected DitherV2, got {other:?}"),
            },
            _ => panic!("expected leaf"),
        }
        assert_eq!(snap.root.len(), 2);
    }

    #[test]
    fn preview_refresh_coalesces_while_pass_in_flight() {
        let state = make_test_app_state();
        assert_eq!(state.error_residuals.clear_count(), 0);

        state
            .preview_pass_inflight
            .store(1, std::sync::atomic::Ordering::Release);
        for _ in 0..4 {
            request_preview_refresh(&state, 1, true);
        }
        assert_eq!(
            state.error_residuals.clear_count(),
            0,
            "in-flight pass must stash instead of clearing residuals four times"
        );
        assert!(state.pending_preview_refresh.lock().unwrap().is_some());

        state
            .preview_pass_inflight
            .store(0, std::sync::atomic::Ordering::Release);
        on_preview_task_finished(&state);
        assert_eq!(
            state.error_residuals.clear_count(),
            1,
            "idle flush applies the latest coalesced refresh once"
        );
        assert!(state.pending_preview_refresh.lock().unwrap().is_none());
    }

    #[test]
    fn preview_refresh_runs_immediately_when_idle() {
        let state = make_test_app_state();
        request_preview_refresh(&state, 1, true);
        request_preview_refresh(&state, 1, true);
        assert_eq!(state.error_residuals.clear_count(), 2);
    }
}
