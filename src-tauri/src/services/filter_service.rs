use std::sync::Arc;
use tauri::AppHandle;

use engine_project::filter::{DitherMode, DiffusionKernel};
use engine_project::filters::curves::CurveChannel;
use engine_project::filters::glitch::GlitchType;
use engine_project::types::FilterInstanceId;
use engine_project::{FilterInstance, FilterKind, FilterParams};
use serde::{Deserialize, Serialize};

use crate::commands::{
    emit_document_changed, layer_needs_dither_cache_reset, request_preview_refresh,
    schedule_dirty_viewport_tiles, AppState,
};
use crate::services::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct AddFilterRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub kind: String,
    pub params: serde_json::Value,
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

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderFilterRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub filter_id: String,
    pub new_index: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterIdResponse {
    pub filter_id: String,
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
) -> FilterParams {
    FilterParams::Adjust {
        contrast: json_f32_adjust(params, "contrast", contrast),
        brightness: json_f32_adjust(params, "brightness", brightness),
        saturation: json_f32_adjust(params, "saturation", saturation),
        blur: json_f32_adjust(params, "blur", blur),
        sharpness: json_f32_adjust(params, "sharpness", sharpness),
        noise: json_f32_adjust(params, "noise", noise),
    }
}

pub struct FilterService {
    state: Arc<AppState>,
}

impl FilterService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub fn add_filter(
        &self,
        app_handle: &AppHandle,
        req: AddFilterRequest,
    ) -> Result<FilterIdResponse, AppError> {
        let doc_id = req.doc_id;

        let kind = match req.kind.as_str() {
            "Curves" => FilterKind::Curves,
            "Levels" => FilterKind::Levels,
            "Dither" => FilterKind::Dither,
            "DitherV2" => FilterKind::Dither,
            "PaletteQuantize" => FilterKind::PaletteQuantize,
            "Glitch" => FilterKind::Glitch,
            "Glow" => FilterKind::Glow,
            "Crt" => FilterKind::Crt,
            "Adjust" => FilterKind::Adjust,
            _ => return Err(AppError::InvalidOperation("Invalid filter kind".to_string())),
        };

        let params = match req.kind.as_str() {
            "DitherV2" => {
                let dither_params: engine_project::filter::DitherParamsV2 =
                    serde_json::from_value(req.params.clone())
                        .map_err(|e| AppError::InvalidOperation(format!("Invalid DitherV2 params: {}", e)))?;
                dither_params.validate().map_err(|e| AppError::InvalidOperation(format!("{}", e)))?;
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
                        return Err(AppError::InvalidOperation("Color depth must be 1-8 bits".to_string()));
                    }
                    FilterParams::Dither { mode, color_depth }
                }
                FilterKind::PaletteQuantize => {
                    let palette_id = req
                        .params
                        .get("palette_id")
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| AppError::InvalidOperation("palette_id is required for PaletteQuantize".to_string()))? as u32;
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
            },
        };

        let filter = FilterInstance::new(kind, params);
        filter.validate().map_err(|e| AppError::InvalidOperation(format!("{}", e)))?;

        let filter_id = filter.id.to_string();

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let layer_id = req.layer_id;
            let mut found = false;

            if matches!(&filter.params, FilterParams::DitherV2(_)) {
                self.state
                    .tiles
                    .error_residuals
                    .evict_layer(doc_id, engine_project::types::LayerId::new(layer_id));
                self.state.tiles.block_representatives.clear_dithered();
            }

            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
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

            {
                let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
                snapshot.generations.increment_layer_gen(layer_id);
            }

            let doc = self.state.require_session(doc_id)?.document_handle.snapshot().id.0;
            engine_tiles::invalidation::invalidate(
                &self.state.tiles.tile_cache,
                engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc, layer: layer_id },
            );

            schedule_dirty_viewport_tiles(&self.state);
            emit_document_changed(app_handle, "filter_added", Some(layer_id), Some(doc_id));

            Ok(FilterIdResponse { filter_id })
        })
        .map_err(AppError::Generic)
    }

    pub fn remove_filter(
        &self,
        app_handle: &AppHandle,
        req: RemoveFilterRequest,
    ) -> Result<(), AppError> {
        let doc_id = req.doc_id;
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let mut found = false;

            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
                fn find_and_remove_filter(nodes: &mut Vec<engine_project::LayerNode>, layer_id: u32, filter_id: &str) -> bool {
                    for node in nodes.iter_mut() {
                        match node {
                            engine_project::LayerNode::Leaf(layer) => {
                                if layer.id.0 == layer_id {
                                    if let Some(idx) = layer.filters.iter().position(|f| f.id.to_string() == filter_id) {
                                        layer.filters.remove(idx);
                                        return true;
                                    }
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
                return Err(format!("Filter '{}' not found on layer {}", req.filter_id, req.layer_id));
            }

            request_preview_refresh(
                &self.state,
                req.layer_id,
                layer_needs_dither_cache_reset(&self.state.require_session(doc_id)?.document_handle.snapshot().root, req.layer_id),
            );

            emit_document_changed(app_handle, "filter_removed", Some(req.layer_id), Some(doc_id));
            Ok(())
        })
        .map_err(AppError::Generic)
    }

    pub fn reorder_filter(
        &self,
        app_handle: &AppHandle,
        req: ReorderFilterRequest,
    ) -> Result<(), AppError> {
        let doc_id = req.doc_id;
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let mut success = false;

            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
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
                return Err(format!("Filter '{}' not found on layer {}", req.filter_id, req.layer_id));
            }

            request_preview_refresh(
                &self.state,
                req.layer_id,
                layer_needs_dither_cache_reset(&self.state.require_session(doc_id)?.document_handle.snapshot().root, req.layer_id),
            );

            emit_document_changed(app_handle, "filter_reordered", Some(req.layer_id), Some(doc_id));
            Ok(())
        })
        .map_err(AppError::Generic)
    }

    pub fn update_filter(
        &self,
        app_handle: &AppHandle,
        req: UpdateFilterRequest,
    ) -> Result<(), AppError> {
        let doc_id = req.doc_id;

        let uuid = uuid::Uuid::parse_str(&req.filter_id)
            .map_err(|e| AppError::InvalidOperation(format!("Invalid filter_id: {}", e)))?;
        let filter_id = FilterInstanceId(uuid);

        let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
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
                .ok_or_else(|| AppError::InvalidOperation(format!("Filter {} not found on layer {}", req.filter_id, req.layer_id)))?;
            (kind, is_dither_v2, params)
        };
        drop(snapshot);

        let params_empty = req.params.as_object().map(|o| o.is_empty()).unwrap_or(false);

        let new_params = if params_empty {
            existing_params
        } else if is_dither_v2 || (filter_kind == FilterKind::Dither && req.params.get("levels").is_some()) {
            let dither_params: engine_project::filter::DitherParamsV2 =
                serde_json::from_value(req.params.clone())
                    .map_err(|e| AppError::InvalidOperation(format!("Invalid DitherV2 params: {}", e)))?;
            dither_params.validate().map_err(|e| AppError::InvalidOperation(format!("{}", e)))?;
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
            }
        };

        let mut temp_filter = FilterInstance::new(filter_kind, new_params.clone());
        if let Some(opacity) = req.opacity {
            temp_filter.opacity = opacity;
        }
        let parsed_blend = if let Some(ref name) = req.blend_mode {
            let mode = engine_project::BlendMode::from_name(name).ok_or_else(|| {
                AppError::InvalidOperation(format!("Invalid or reserved blend mode: {}", name))
            })?;
            temp_filter.blend_mode = mode;
            Some(mode)
        } else {
            None
        };
        temp_filter.validate().map_err(|e| AppError::InvalidOperation(format!("Invalid parameters: {}", e)))?;

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let layer_id = req.layer_id;
            let mut found = false;
            self.state.require_session(doc_id)?.document_handle.mutate(|doc| {
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
                                        filter.requires_full_row =
                                            engine_project::FilterInstance::params_require_full_row(&new_params);
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
                return Err(format!("Filter {} not found on layer {} during update", req.filter_id, req.layer_id));
            }

            {
                let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
                snapshot.generations.increment_layer_gen(layer_id);
            }

            request_preview_refresh(
                &self.state,
                layer_id,
                layer_needs_dither_cache_reset(&self.state.require_session(doc_id)?.document_handle.snapshot().root, layer_id),
            );

            emit_document_changed(app_handle, "filter_updated", Some(layer_id), Some(doc_id));
            Ok(())
        })
        .map_err(AppError::Generic)
    }
}
