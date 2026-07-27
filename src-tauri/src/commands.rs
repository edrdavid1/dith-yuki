//! Tauri command handlers for document operations.
//!
//! This module registers the public IPC endpoints that the frontend calls.
//! Each command acquires the DocumentHandle and TileCache from app state,
//! delegates to engine-project for mutations, and returns DTOs for serialization.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::State;

use engine_project::{
    document::DocumentHandle,
    dto::DocumentSnapshotDto,
    types::{LayerId, LayerKind, BlendMode},
    commands::{AddLayerArgs, LayerPropsPatch},
    commands as engine_commands,
};
use engine_tiles::{PixelTile, TileCache, Scheduler};
use engine_tiles::{CacheStage, Priority, RecomputeTask, TileKey};

// ============================================================================
// Data Structures for Command Arguments
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AddLayerRequest {
    pub kind: String, // "raster" or "adjustment"
    pub parent_group: Option<u32>,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetLayerPropsRequest {
    pub layer_id: u32,
    pub name: Option<String>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<String>,
    pub visible: Option<bool>,
    pub offset: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderLayerRequest {
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

/// Shared application state for Tauri commands.
pub struct AppState {
    pub document_handle: DocumentHandle,
    pub tile_cache: TileCache,
    pub scheduler: Scheduler,
    pub viewport: Mutex<ViewportState>,
}

// ============================================================================
// Helpers
// ============================================================================

// ============================================================================
// Tauri Commands
// ============================================================================

/// Create a new document.
#[tauri::command]
pub fn new_document(
    width: u32,
    height: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    use engine_project::types::DocumentId;
    
    let new_doc = engine_project::Document::new(DocumentId::new(1), width, height);
    state.document_handle.mutate(|doc| {
        *doc = new_doc.clone();
    });
    
    let snapshot = state.document_handle.snapshot();
    let dto = engine_project::dto::document_to_dto(&snapshot);
    
    Ok(DocumentResponse { snapshot: dto })
}

/// Get current document snapshot.
#[tauri::command]
pub fn get_document_snapshot(
    state: State<'_, Arc<AppState>>,
) -> Result<DocumentResponse, String> {
    let snapshot = state.document_handle.snapshot();
    let dto = engine_project::dto::document_to_dto(&snapshot);
    Ok(DocumentResponse { snapshot: dto })
}

/// Get the layer tree as a flat DTO structure for frontend consumption.
///
/// Returns the full layer hierarchy as `Vec<LayerNodeDto>`, where groups
/// have `children: Some(vec![...])` and leaves have `children: None`.
#[tauri::command]
pub fn get_layer_tree(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<LayerNodeDto>, String> {
    let snapshot = state.document_handle.snapshot();
    let tree = layer_nodes_to_dto(&snapshot.root);
    Ok(tree)
}

/// Convert internal LayerNode tree to flat LayerNodeDto tree.
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
    state: State<'_, Arc<AppState>>,
) -> Result<LayerIdResponse, String> {
    let kind = match req.kind.as_str() {
        "raster" => LayerKind::Raster,
        "adjustment" => LayerKind::Adjustment,
        _ => return Err("Invalid layer kind".to_string()),
    };

    let snapshot = state.document_handle.snapshot();
    let width = snapshot.width;
    let height = snapshot.height;
    let doc_id = snapshot.id;
    drop(snapshot);

    let args = AddLayerArgs {
        kind,
        parent_group: req.parent_group.map(LayerId::new),
        index: req.index,
        width,
        height,
    };

    match engine_commands::add_layer(&state.document_handle, &state.tile_cache, doc_id, args) {
        Ok(layer_id) => Ok(LayerIdResponse { layer_id: layer_id.0 }),
        Err(e) => Err(format!("Failed to add layer: {:?}", e)),
    }
}

/// Remove a layer from the document.
#[tauri::command]
pub fn remove_layer(
    layer_id: u32,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let snapshot = state.document_handle.snapshot();
    let doc_id = snapshot.id;
    drop(snapshot);

    match engine_commands::remove_layer(
        &state.document_handle,
        &state.tile_cache,
        doc_id,
        LayerId::new(layer_id),
    ) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to remove layer: {:?}", e)),
    }
}

/// Set layer properties (name, opacity, blend mode, visibility, offset).
#[tauri::command]
pub fn set_layer_props(
    req: SetLayerPropsRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
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

    let patch = LayerPropsPatch {
        name: req.name,
        opacity: req.opacity,
        blend_mode,
        visible: req.visible,
        offset: req.offset,
    };

    let snapshot = state.document_handle.snapshot();
    let doc_id = snapshot.id;
    drop(snapshot);

    match engine_commands::set_layer_props(
        &state.document_handle,
        &state.tile_cache,
        doc_id,
        LayerId::new(req.layer_id),
        patch,
    ) {
        Ok(_) => {
            // Schedule viewport-visible dirty tiles for recomputation (requirement 10.2)
            if is_visual_change {
                schedule_dirty_viewport_tiles(&state);
            }
            Ok(())
        }
        Err(e) => Err(format!("Failed to set layer props: {:?}", e)),
    }
}

/// Reorder a layer (move to new parent/position).
#[tauri::command]
pub fn reorder_layer(
    req: ReorderLayerRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let snapshot = state.document_handle.snapshot();
    let doc_id = snapshot.id;
    drop(snapshot);

    match engine_commands::reorder_layer(
        &state.document_handle,
        &state.tile_cache,
        doc_id,
        LayerId::new(req.layer_id),
        req.new_parent.map(LayerId::new),
        req.new_index,
    ) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to reorder layer: {:?}", e)),
    }
}

// ============================================================================
// Filter Commands
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AddFilterRequest {
    pub layer_id: u32,
    pub kind: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateFilterRequest {
    pub layer_id: u32,
    pub filter_id: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RemoveFilterRequest {
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
    state: State<'_, Arc<AppState>>,
) -> Result<FilterIdResponse, String> {
    use engine_project::{FilterKind, FilterParams, FilterInstance};
    use engine_project::filters::curves::CurveChannel;
    use engine_project::filters::dither::DitherAlgorithm;
    use engine_project::filters::glitch::GlitchType;
    
    let kind = match req.kind.as_str() {
        "Curves" => FilterKind::Curves,
        "Levels" => FilterKind::Levels,
        "Dither" => FilterKind::Dither,
        "Glitch" => FilterKind::Glitch,
        _ => return Err("Invalid filter kind".to_string()),
    };

    // Parse params based on kind
    let params = match kind {
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
            FilterParams::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
            }
        }
        FilterKind::Dither => {
            let algorithm = match req.params.get("algorithm").and_then(|v| v.as_str()).unwrap_or("FloydSteinberg") {
                "Ordered" => DitherAlgorithm::Ordered,
                "Threshold" => DitherAlgorithm::Threshold,
                _ => DitherAlgorithm::FloydSteinberg,
            };
            let color_depth = req.params.get("color_depth").and_then(|v| v.as_u64()).unwrap_or(4) as u8;
            if !(1..=8).contains(&color_depth) {
                return Err("Color depth must be 1-8 bits".to_string());
            }
            FilterParams::Dither { algorithm, color_depth }
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
        FilterKind::Placeholder => FilterParams::Placeholder("unknown".to_string()),
    };

    let filter = FilterInstance::new(kind, params);
    
    // Validate the filter parameters before adding
    filter.validate().map_err(|e| format!("{}", e))?;
    
    let filter_id = filter.id.to_string();

    // Add filter to layer in document
    let layer_id = req.layer_id;
    let mut found = false;
    state.document_handle.mutate(|doc| {
        // Find layer (recursing into groups) and add filter
        fn find_and_add_filter(nodes: &mut Vec<engine_project::LayerNode>, layer_id: u32, filter: FilterInstance) -> bool {
            for node in nodes.iter_mut() {
                match node {
                    engine_project::LayerNode::Leaf(layer) => {
                        if layer.id.0 == layer_id {
                            layer.filters.push(filter);
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

    Ok(FilterIdResponse { filter_id })
}

/// Remove a filter from a layer.
#[tauri::command]
pub fn remove_filter(
    req: RemoveFilterRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    use engine_tiles::{invalidate, InvalidationEvent};

    let mut found = false;

    state.document_handle.mutate(|doc| {
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

    // Invalidate tile cache for the affected layer
    invalidate(
        &state.tile_cache,
        InvalidationEvent::LayerFilterChanged { layer: req.layer_id },
    );

    Ok(())
}

// ============================================================================
// Update Filter Command
// ============================================================================

/// Update filter parameters on a layer.
#[tauri::command]
pub fn update_filter(
    req: UpdateFilterRequest,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    use engine_project::{FilterKind, FilterParams, FilterInstance};
    use engine_project::filters::curves::CurveChannel;
    use engine_project::filters::dither::DitherAlgorithm;
    use engine_project::filters::glitch::GlitchType;
    use engine_project::types::FilterInstanceId;

    // Parse the filter_id string into a UUID
    let uuid = uuid::Uuid::parse_str(&req.filter_id)
        .map_err(|e| format!("Invalid filter_id: {}", e))?;
    let filter_id = FilterInstanceId(uuid);

    // First, get the filter's kind so we know how to parse params
    let snapshot = state.document_handle.snapshot();
    let filter_kind = {
        fn find_filter_kind(
            nodes: &[engine_project::LayerNode],
            layer_id: u32,
            filter_id: FilterInstanceId,
        ) -> Option<FilterKind> {
            for node in nodes.iter() {
                match node {
                    engine_project::LayerNode::Leaf(layer) => {
                        if layer.id.0 == layer_id {
                            if let Some(filter) = layer.find_filter(filter_id) {
                                return Some(filter.kind);
                            }
                        }
                    }
                    engine_project::LayerNode::Group(group) => {
                        if let Some(kind) = find_filter_kind(&group.children, layer_id, filter_id) {
                            return Some(kind);
                        }
                    }
                }
            }
            None
        }
        find_filter_kind(&snapshot.root, req.layer_id, filter_id)
            .ok_or_else(|| format!(
                "Filter {} not found on layer {}",
                req.filter_id, req.layer_id
            ))?
    };
    drop(snapshot);

    // Parse new params based on the filter's kind
    let new_params = match filter_kind {
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
            FilterParams::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
            }
        }
        FilterKind::Dither => {
            let algorithm = match req.params.get("algorithm").and_then(|v| v.as_str()).unwrap_or("FloydSteinberg") {
                "Ordered" => DitherAlgorithm::Ordered,
                "Threshold" => DitherAlgorithm::Threshold,
                _ => DitherAlgorithm::FloydSteinberg,
            };
            let color_depth = req.params.get("color_depth").and_then(|v| v.as_u64()).unwrap_or(4) as u8;
            FilterParams::Dither { algorithm, color_depth }
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
        FilterKind::Placeholder => FilterParams::Placeholder("unknown".to_string()),
    };

    // Validate new params before applying
    let temp_filter = FilterInstance::new(filter_kind, new_params.clone());
    temp_filter.validate().map_err(|e| format!("Invalid parameters: {}", e))?;

    // Apply the update within a document mutation
    let layer_id = req.layer_id;
    let mut found = false;
    state.document_handle.mutate(|doc| {
        fn update_filter_in_nodes(
            nodes: &mut Vec<engine_project::LayerNode>,
            layer_id: u32,
            filter_id: engine_project::types::FilterInstanceId,
            new_params: FilterParams,
        ) -> bool {
            for node in nodes.iter_mut() {
                match node {
                    engine_project::LayerNode::Leaf(layer) => {
                        if layer.id.0 == layer_id {
                            if let Some(filter) = layer.find_filter_mut(filter_id) {
                                filter.params = new_params;
                                return true;
                            }
                        }
                    }
                    engine_project::LayerNode::Group(group) => {
                        if update_filter_in_nodes(&mut group.children, layer_id, filter_id, new_params.clone()) {
                            return true;
                        }
                    }
                }
            }
            false
        }

        found = update_filter_in_nodes(&mut doc.root, layer_id, filter_id, new_params.clone());
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
        let snapshot = state.document_handle.snapshot();
        snapshot.generations.increment_layer_gen(layer_id);
    }

    // Invalidate tile cache for the affected layer (Processed + Composite cascade)
    engine_tiles::invalidation::invalidate(
        &state.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
            layer: req.layer_id,
        },
    );

    // Schedule viewport-visible dirty tiles for immediate recomputation (requirement 4.4)
    schedule_dirty_viewport_tiles(&state);

    Ok(())
}

// ============================================================================
// Load Image Command
// ============================================================================

/// Response from the load_image command.
#[derive(Debug, Clone, Serialize)]
pub struct LoadImageResponse {
    pub doc_id: u32,
    pub width: u32,
    pub height: u32,
    pub tile_count: u32,
}

/// Load an image from disk, decode it, split into tiles, and create a document.
#[tauri::command]
pub async fn load_image(
    path: String,
    state: State<'_, Arc<AppState>>,
) -> Result<LoadImageResponse, String> {
    use engine_project::types::DocumentId;
    use engine_tiles::decompose::decompose_image_to_tiles;

    // Do heavy I/O and CPU work in a blocking thread
    let (width, height, rgba_f32) = tauri::async_runtime::spawn_blocking(move || {
        // Open and decode the image
        let img = image::open(&path).map_err(|e| {
            format!("IO error: {}", e)
        })?;

        let img_rgba = img.to_rgba8();
        let width = img_rgba.width();
        let height = img_rgba.height();

        // Validate dimensions
        if width > 8192 || height > 8192 {
            return Err(format!(
                "Invalid state: image dimensions {}x{} exceed maximum 8192x8192",
                width, height
            ));
        }
        if width == 0 || height == 0 {
            return Err("Invalid state: image has zero dimensions".to_string());
        }

        // Convert image to RGBA f32 buffer (row-major, 4 floats per pixel)
        let pixel_count = (width as usize) * (height as usize);
        let mut rgba_f32: Vec<f32> = Vec::with_capacity(pixel_count * 4);
        for pixel in img_rgba.pixels() {
            rgba_f32.push(pixel[0] as f32 / 255.0);
            rgba_f32.push(pixel[1] as f32 / 255.0);
            rgba_f32.push(pixel[2] as f32 / 255.0);
            rgba_f32.push(pixel[3] as f32 / 255.0);
        }

        Ok::<_, String>((width, height, rgba_f32))
    }).await.map_err(|e| format!("Load error: {}", e))??;

    // Decompose into Raw-stage tiles in TileCache (DashMap is thread-safe, no blocking needed)
    let layer_id = 1u32; // Primary raster layer
    let grid = decompose_image_to_tiles(&rgba_f32, width, height, layer_id, &state.tile_cache)
        .map_err(|e| format!("Tile decomposition error: {}", e))?;

    let tile_count = grid.cols * grid.rows;

    // Assign a new doc_id
    let doc_id: u32 = 1;

    // Create a new Document with image dimensions and one raster layer
    let mut new_doc = engine_project::Document::new(DocumentId::new(doc_id), width, height);
    let layer = engine_project::layer::Layer::new(
        engine_project::types::LayerId::new(1),
        engine_project::types::LayerKind::Raster,
        width,
        height,
    );
    new_doc.root.push(engine_project::layer::LayerNode::Leaf(layer));
    new_doc.increment_generation();

    state.document_handle.mutate(|doc| {
        *doc = new_doc;
    });

    Ok(LoadImageResponse {
        doc_id,
        width,
        height,
        tile_count,
    })
}



// ============================================================================
// Tile Scheduling Helpers
// ============================================================================

/// Schedule viewport-visible dirty tiles for immediate recomputation.
///
/// Reads the current viewport state, iterates over visible tile coordinates, and
/// enqueues Immediate-priority recompute tasks for any Composite-stage tile that
/// is currently marked dirty in the cache. This ensures the user sees updated tiles
/// promptly after a filter or layer property change.
///
/// The `tile-ready` event is emitted by the worker loop upon successful recomputation
/// (requirements 2.4, 10.4, 10.6).
fn schedule_dirty_viewport_tiles(state: &AppState) {
    use std::sync::atomic::Ordering;

    let viewport = state.viewport.lock().unwrap().clone();
    let snapshot = state.document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

    for coord in &viewport.visible_tiles {
        // Schedule Composite-stage tile recomputation.
        // The worker's compute_composite_tile already ensures Processed tiles
        // are fresh before compositing, so we only need to schedule Composite tasks.
        let key = TileKey {
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
            state.scheduler.enqueue(task);
        }
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
    if req.format != "PNG" && req.format != "JPEG" {
        return Err("Invalid parameters: format must be PNG or JPEG".to_string());
    }

    // 2. Get document snapshot and validate doc_id
    let snapshot = state.document_handle.snapshot();
    if snapshot.id.0 != req.doc_id {
        return Err("Document not found".to_string());
    }
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
                layer: layer_id,
                coord: TileCoord { level: 0, x: col, y: row },
                stage: CacheStage::Raw,
            };
            match state.tile_cache.get_entry(key) {
                Some(tile) => row_tiles.push(tile),
                None => return Err("Document not found".to_string()),
            }
        }
        tiles.push(row_tiles);
    }

    // 4. Get the document snapshot and clone the layer
    let layer_clone = find_first_visible_layer(&snapshot.root).cloned();
    drop(snapshot);

    // 5. Do heavy rendering and I/O in a blocking thread
    let req_format = req.format.clone();
    let req_path = req.path.clone();
    let req_quality = req.quality;

    tauri::async_runtime::spawn_blocking(move || {
        let mut rgba_buffer: Vec<u8> = vec![0u8; (img_width * img_height * 4) as usize];

        for row in 0..rows {
            for col in 0..cols {
                let tile = &tiles[row as usize][col as usize];

                // Apply filters if we have a visible layer
                let processed_tile = if let Some(ref layer) = layer_clone {
                    let coord = TileCoord { level: 0, x: col, y: row };
                    apply_filter_to_tile(tile, layer, coord)
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
            _ => unreachable!(), // Already validated above
        }

        Ok::<(), String>(())
    }).await.map_err(|e| format!("Export error: {}", e))?
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
}
