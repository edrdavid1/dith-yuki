use std::sync::Arc;
use tauri::AppHandle;

use engine_project::filter::{DiffusionKernel, DitherMode};
use engine_project::filters::curves::CurveChannel;
use engine_project::filters::glitch::GlitchParams as GlitchFilterParams;
use engine_project::types::FilterInstanceId;
use engine_project::{
    filter_kind_for_algorithm_id, FilterInstance, FilterKind, FilterParams, PlaceholderParams,
};
use serde::{Deserialize, Serialize};

use crate::commands::{
    emit_document_changed, layer_needs_dither_cache_reset, request_preview_refresh_at,
    schedule_dirty_viewport_tiles, AppState,
};
use crate::services::AppError;

fn layer_filters(
    nodes: &[engine_project::LayerNode],
    layer_id: u32,
) -> Option<Vec<engine_project::FilterInstance>> {
    for node in nodes {
        match node {
            engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                return Some(layer.filters.clone());
            }
            engine_project::LayerNode::Group(group) => {
                if let Some(f) = layer_filters(&group.children, layer_id) {
                    return Some(f);
                }
            }
            _ => {}
        }
    }
    None
}

/// P2: when Post-cache is on and classify says Post, skip residual clears and
/// preserve Dithered tiles. Otherwise keep today's clear rule.
fn post_cache_refresh_policy(
    old: &[engine_project::FilterInstance],
    new: &[engine_project::FilterInstance],
    base_clear: bool,
) -> (bool, bool) {
    if !engine_project::post_cache_enabled() {
        return (base_clear, false);
    }
    match engine_project::classify_change(old, new) {
        engine_project::ChangeClass::Post => (false, true),
        _ => (base_clear, false),
    }
}

/// Task 2: bump post_gen / dither_gen (or neither) after a stack mutate.
/// Returns whether `layer_gen` should advance (dither-path edits).
fn apply_stack_gens_after_mutate(
    doc: &mut engine_project::Document,
    layer_id: u32,
    old: &[engine_project::FilterInstance],
    new: &[engine_project::FilterInstance],
) -> bool {
    let bump = doc.apply_stack_edit_gens(engine_project::types::LayerId::new(layer_id), old, new);
    match bump {
        engine_project::StackEditBump::None => false,
        engine_project::StackEditBump::Post => !engine_project::post_cache_enabled(),
        engine_project::StackEditBump::Dither => true,
    }
}

fn layer_raw_rev(nodes: &[engine_project::LayerNode], layer_id: u32) -> Option<u64> {
    for node in nodes {
        match node {
            engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                return Some(layer.raw_rev);
            }
            engine_project::LayerNode::Group(group) => {
                if let Some(v) = layer_raw_rev(&group.children, layer_id) {
                    return Some(v);
                }
            }
            _ => {}
        }
    }
    None
}

fn note_prefix_externals(
    snapshot: &engine_project::Document,
    layer_id: u32,
    filters: &[engine_project::FilterInstance],
) -> engine_project::PrefixExternals {
    engine_project::prefix_externals_for_layer(
        filters,
        &snapshot.palettes,
        layer_raw_rev(&snapshot.root, layer_id).unwrap_or(0),
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct AddFilterRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub kind: String,
    pub params: serde_json::Value,
    #[serde(default)]
    pub algorithm_id: Option<String>,
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
    /// Optional gesture id for `DITHER_PROFILE=1` (ignored when profiling is off).
    #[serde(default, alias = "profileRunId")]
    pub profile_run_id: Option<u64>,
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
    exposure: f32,
    shadows: f32,
    highlights: f32,
    temperature: f32,
    tint: f32,
    vibrance: f32,
    hue: f32,
    clarity: f32,
) -> FilterParams {
    FilterParams::Adjust {
        contrast: json_f32_adjust(params, "contrast", contrast),
        brightness: json_f32_adjust(params, "brightness", brightness),
        saturation: json_f32_adjust(params, "saturation", saturation),
        blur: json_f32_adjust(params, "blur", blur),
        sharpness: json_f32_adjust(params, "sharpness", sharpness),
        noise: json_f32_adjust(params, "noise", noise),
        exposure: json_f32_adjust(params, "exposure", exposure),
        shadows: json_f32_adjust(params, "shadows", shadows),
        highlights: json_f32_adjust(params, "highlights", highlights),
        temperature: json_f32_adjust(params, "temperature", temperature),
        tint: json_f32_adjust(params, "tint", tint),
        vibrance: json_f32_adjust(params, "vibrance", vibrance),
        hue: json_f32_adjust(params, "hue", hue),
        clarity: json_f32_adjust(params, "clarity", clarity),
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
            "Exposure" => FilterKind::Exposure,
            "HighlightsShadows" => FilterKind::HighlightsShadows,
            "TemperatureTint" => FilterKind::TemperatureTint,
            "Vibrance" => FilterKind::Vibrance,
            "Hue" => FilterKind::Hue,
            "Clarity" => FilterKind::Clarity,
            "Ascii" => FilterKind::Ascii,
            other => filter_kind_for_algorithm_id(other).ok_or_else(|| {
                AppError::InvalidOperation(format!("Invalid filter kind: {other}"))
            })?,
        };

        let params = match req.kind.as_str() {
            "DitherV2" => {
                let dither_params: engine_project::filter::DitherParamsV2 =
                    serde_json::from_value(req.params.clone()).map_err(|e| {
                        AppError::InvalidOperation(format!("Invalid DitherV2 params: {}", e))
                    })?;
                dither_params
                    .validate()
                    .map_err(|e| AppError::InvalidOperation(format!("{}", e)))?;
                FilterParams::DitherV2(dither_params)
            }
            algo if filter_kind_for_algorithm_id(algo) == Some(FilterKind::Dither) => {
                let mut dither_params: engine_project::filter::DitherParamsV2 =
                    serde_json::from_value(req.params.clone()).unwrap_or_default();
                if let Ok(mode) =
                    serde_json::from_value(serde_json::Value::String(algo.to_string()))
                {
                    dither_params.mode = mode;
                }
                dither_params
                    .validate()
                    .map_err(|e| AppError::InvalidOperation(format!("{}", e)))?;
                FilterParams::DitherV2(dither_params)
            }
            _ => match kind {
                FilterKind::Curves => {
                    let channel = match req
                        .params
                        .get("channel")
                        .and_then(|v| v.as_str())
                        .unwrap_or("All")
                    {
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
                        FilterParams::Curves {
                            curve: curve_vec,
                            channel,
                        }
                    } else {
                        FilterParams::Curves {
                            curve: vec![(0.0, 0.0), (1.0, 1.0)],
                            channel,
                        }
                    }
                }
                FilterKind::Levels => {
                    let input_black = req
                        .params
                        .get("input_black")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let input_white = req
                        .params
                        .get("input_white")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let gamma = req
                        .params
                        .get("gamma")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let output_black = req
                        .params
                        .get("output_black")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let output_white = req
                        .params
                        .get("output_white")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let channel_r = req
                        .params
                        .get("channel_r")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let channel_g = req
                        .params
                        .get("channel_g")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let channel_b = req
                        .params
                        .get("channel_b")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
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
                    let mode = match req
                        .params
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ErrorDiffusion")
                    {
                        "Bayer" => {
                            let matrix_size = req
                                .params
                                .get("matrix_size")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(4) as u8;
                            DitherMode::Bayer { matrix_size }
                        }
                        "ThresholdMap" => {
                            let path = req
                                .params
                                .get("path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            DitherMode::ThresholdMap { path }
                        }
                        _ => {
                            let name = req
                                .params
                                .get("kernel")
                                .and_then(|v| v.as_str())
                                .unwrap_or("FloydSteinberg");
                            let kernel = DiffusionKernel::from_ui_name(name)
                                .unwrap_or(DiffusionKernel::FloydSteinberg);
                            DitherMode::ErrorDiffusion { kernel }
                        }
                    };
                    let color_depth = req
                        .params
                        .get("color_depth")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(4) as u8;
                    if !(1..=8).contains(&color_depth) {
                        return Err(AppError::InvalidOperation(
                            "Color depth must be 1-8 bits".to_string(),
                        ));
                    }
                    FilterParams::Dither { mode, color_depth }
                }
                FilterKind::PaletteQuantize => {
                    let palette_id = req
                        .params
                        .get("palette_id")
                        .and_then(|v| v.as_u64())
                        .ok_or_else(|| {
                            AppError::InvalidOperation(
                                "palette_id is required for PaletteQuantize".to_string(),
                            )
                        })? as u32;
                    let diffusion = req
                        .params
                        .get("diffusion")
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            DiffusionKernel::from_ui_name(s)
                                .unwrap_or(DiffusionKernel::FloydSteinberg)
                        });
                    let metric = req
                        .params
                        .get("metric")
                        .and_then(|v| v.as_str())
                        .unwrap_or("oklab")
                        .to_string();
                    FilterParams::PaletteQuantize {
                        palette_id: engine_project::PaletteId::new(palette_id),
                        diffusion,
                        metric,
                    }
                }
                FilterKind::Glitch => {
                    let p: GlitchFilterParams =
                        serde_json::from_value(req.params.clone()).unwrap_or_default();
                    FilterParams::Glitch(p)
                }
                FilterKind::Glow => {
                    let radius = req
                        .params
                        .get("radius")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(2.0) as f32;
                    let intensity = req
                        .params
                        .get("intensity")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let threshold = req
                        .params
                        .get("threshold")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Glow {
                        radius,
                        intensity,
                        threshold,
                    }
                }
                FilterKind::Crt => {
                    let period = req
                        .params
                        .get("period")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(2) as u8;
                    let strength = req
                        .params
                        .get("strength")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5) as f32;
                    let mask_strength = req
                        .params
                        .get("mask_strength")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Crt {
                        period,
                        strength,
                        mask_strength,
                    }
                }
                FilterKind::Adjust => parse_adjust_params(
                    &req.params,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                    0.0,
                ),
                FilterKind::Exposure => {
                    let stops = req
                        .params
                        .get("stops")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Exposure { stops }
                }
                FilterKind::HighlightsShadows => {
                    let shadows = req
                        .params
                        .get("shadows")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let highlights = req
                        .params
                        .get("highlights")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::HighlightsShadows {
                        shadows,
                        highlights,
                    }
                }
                FilterKind::TemperatureTint => {
                    let temperature = req
                        .params
                        .get("temperature")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let tint = req
                        .params
                        .get("tint")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::TemperatureTint { temperature, tint }
                }
                FilterKind::Vibrance => {
                    let amount = req
                        .params
                        .get("amount")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Vibrance { amount }
                }
                FilterKind::Hue => {
                    let degrees = req
                        .params
                        .get("degrees")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Hue { degrees }
                }
                FilterKind::Clarity => {
                    let amount = req
                        .params
                        .get("amount")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Clarity { amount }
                }
                FilterKind::Ascii => {
                    let p: engine_project::filter::AsciiParams =
                        serde_json::from_value(req.params.clone()).map_err(|e| {
                            AppError::InvalidOperation(format!("Invalid Ascii params: {e}"))
                        })?;
                    p.validate()
                        .map_err(|e| AppError::InvalidOperation(format!("{e}")))?;
                    FilterParams::Ascii(p)
                }
                FilterKind::Placeholder => FilterParams::Placeholder(PlaceholderParams {
                    label: "unknown".to_string(),
                    raw_params: None,
                }),
            },
        };

        let mut filter = FilterInstance::new(kind, params);
        let resolved_id = req
            .algorithm_id
            .clone()
            .or_else(|| {
                engine_project::algorithms::builtin_registry()
                    .get_by_str(&req.kind)
                    .map(|_| req.kind.clone())
            })
            .or_else(|| {
                // FilterKind aliases (Ascii, Crt, …) → registry AlgorithmId.
                engine_project::algorithm_id_for_params(&filter.params).map(str::to_string)
            });
        if let Some(id) = resolved_id.as_deref() {
            let algo = engine_project::algorithms::builtin_registry()
                .get_by_str(id)
                .ok_or_else(|| AppError::InvalidOperation(format!("Unknown algorithm: {id}")))?;
            filter.algorithm_id = Some(id.to_string());
            filter.schema_version = Some(algo.schema_version());
        }
        filter
            .validate()
            .map_err(|e| AppError::InvalidOperation(format!("{}", e)))?;

        let filter_id = filter.id.to_string();

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let layer_id = req.layer_id;
            let mut found = false;
            let old_filters = layer_filters(
                &self
                    .state
                    .require_session(doc_id)?
                    .document_handle
                    .snapshot()
                    .root,
                layer_id,
            )
            .unwrap_or_default();

            if matches!(
                &filter.params,
                FilterParams::DitherV2(_) | FilterParams::Ascii(_)
            ) {
                self.state
                    .tiles
                    .error_residuals
                    .evict_layer(doc_id, engine_project::types::LayerId::new(layer_id));
                self.state.tiles.block_representatives.clear_dithered();
                self.state.tiles.clear_strip_gates();
                self.state
                    .tiles
                    .full_document
                    .invalidate_layer(doc_id, layer_id);
            }

            self.state
                .require_session(doc_id)?
                .document_handle
                .mutate(|doc| {
                    fn find_and_add_filter(
                        nodes: &mut [engine_project::LayerNode],
                        layer_id: u32,
                        filter: FilterInstance,
                    ) -> bool {
                        for node in nodes.iter_mut() {
                            match node {
                                engine_project::LayerNode::Leaf(layer) => {
                                    if layer.id.0 == layer_id {
                                        layer.add_filter_instance(filter);
                                        return true;
                                    }
                                }
                                engine_project::LayerNode::Group(group) => {
                                    if find_and_add_filter(
                                        &mut group.children,
                                        layer_id,
                                        filter.clone(),
                                    ) {
                                        return true;
                                    }
                                }
                            }
                        }
                        false
                    }

                    found = find_and_add_filter(&mut doc.root, layer_id, filter);
                    if found {
                        let new_filters = layer_filters(&doc.root, layer_id).unwrap_or_default();
                        let bump_layer = apply_stack_gens_after_mutate(
                            doc,
                            layer_id,
                            &old_filters,
                            &new_filters,
                        );
                        if bump_layer {
                            doc.generations.increment_layer_gen(layer_id);
                        }
                    }
                });

            if !found {
                return Err(format!("Layer {} not found", layer_id));
            }

            let snapshot = self
                .state
                .require_session(doc_id)?
                .document_handle
                .snapshot();
            let doc = snapshot.id.0;
            let new_filters = layer_filters(&snapshot.root, layer_id).unwrap_or_default();
            let changed_index = new_filters
                .iter()
                .position(|f| f.id.to_string() == filter_id);
            let (clear, preserve_dithered) =
                post_cache_refresh_policy(&old_filters, &new_filters, true);
            engine_project::note_stack_change(
                layer_id,
                &old_filters,
                &new_filters,
                changed_index,
                clear,
                &note_prefix_externals(&snapshot, layer_id, &new_filters),
            );
            engine_tiles::invalidation::invalidate(
                &self.state.tiles.tile_cache,
                engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
                    doc,
                    layer: layer_id,
                    changed_index,
                    preserve_dithered,
                },
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
            let old_filters = layer_filters(
                &self
                    .state
                    .require_session(doc_id)?
                    .document_handle
                    .snapshot()
                    .root,
                req.layer_id,
            )
            .unwrap_or_default();
            let changed_index = old_filters
                .iter()
                .position(|f| f.id.to_string() == req.filter_id);

            self.state
                .require_session(doc_id)?
                .document_handle
                .mutate(|doc| {
                    fn find_and_remove_filter(
                        nodes: &mut [engine_project::LayerNode],
                        layer_id: u32,
                        filter_id: &str,
                    ) -> bool {
                        for node in nodes.iter_mut() {
                            match node {
                                engine_project::LayerNode::Leaf(layer) => {
                                    if layer.id.0 == layer_id {
                                        if let Some(idx) = layer
                                            .filters
                                            .iter()
                                            .position(|f| f.id.to_string() == filter_id)
                                        {
                                            layer.filters.remove(idx);
                                            return true;
                                        }
                                        return false;
                                    }
                                }
                                engine_project::LayerNode::Group(group) => {
                                    if find_and_remove_filter(
                                        &mut group.children,
                                        layer_id,
                                        filter_id,
                                    ) {
                                        return true;
                                    }
                                }
                            }
                        }
                        false
                    }

                    found = find_and_remove_filter(&mut doc.root, req.layer_id, &req.filter_id);
                    if found {
                        let new_filters =
                            layer_filters(&doc.root, req.layer_id).unwrap_or_default();
                        let bump_layer = apply_stack_gens_after_mutate(
                            doc,
                            req.layer_id,
                            &old_filters,
                            &new_filters,
                        );
                        if bump_layer {
                            doc.generations.increment_layer_gen(req.layer_id);
                        }
                    }
                });

            if !found {
                return Err(format!(
                    "Filter '{}' not found on layer {}",
                    req.filter_id, req.layer_id
                ));
            }

            let snapshot = self
                .state
                .require_session(doc_id)?
                .document_handle
                .snapshot();
            let base_clear = layer_needs_dither_cache_reset(&snapshot.root, req.layer_id);
            let new_filters = layer_filters(&snapshot.root, req.layer_id).unwrap_or_default();
            let (clear, preserve_dithered) =
                post_cache_refresh_policy(&old_filters, &new_filters, base_clear);
            engine_project::note_stack_change(
                req.layer_id,
                &old_filters,
                &new_filters,
                changed_index,
                clear,
                &note_prefix_externals(&snapshot, req.layer_id, &new_filters),
            );
            request_preview_refresh_at(
                &self.state,
                req.layer_id,
                clear,
                changed_index,
                preserve_dithered,
            );

            emit_document_changed(
                app_handle,
                "filter_removed",
                Some(req.layer_id),
                Some(doc_id),
            );
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
            let old_filters = layer_filters(
                &self
                    .state
                    .require_session(doc_id)?
                    .document_handle
                    .snapshot()
                    .root,
                req.layer_id,
            )
            .unwrap_or_default();
            let from_index = old_filters
                .iter()
                .position(|f| f.id.to_string() == req.filter_id);

            self.state
                .require_session(doc_id)?
                .document_handle
                .mutate(|doc| {
                    fn find_and_reorder(
                        nodes: &mut [engine_project::LayerNode],
                        layer_id: u32,
                        filter_id: &str,
                        new_index: usize,
                    ) -> bool {
                        for node in nodes.iter_mut() {
                            match node {
                                engine_project::LayerNode::Leaf(layer) => {
                                    if layer.id.0 == layer_id {
                                        let current_idx = layer
                                            .filters
                                            .iter()
                                            .position(|f| f.id.to_string() == filter_id);
                                        if let Some(idx) = current_idx {
                                            let clamped_new =
                                                new_index.min(layer.filters.len() - 1);
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
                                    if find_and_reorder(
                                        &mut group.children,
                                        layer_id,
                                        filter_id,
                                        new_index,
                                    ) {
                                        return true;
                                    }
                                }
                            }
                        }
                        false
                    }

                    success = find_and_reorder(
                        &mut doc.root,
                        req.layer_id,
                        &req.filter_id,
                        req.new_index,
                    );
                    if success {
                        let new_filters =
                            layer_filters(&doc.root, req.layer_id).unwrap_or_default();
                        let bump_layer = apply_stack_gens_after_mutate(
                            doc,
                            req.layer_id,
                            &old_filters,
                            &new_filters,
                        );
                        if bump_layer {
                            doc.generations.increment_layer_gen(req.layer_id);
                        }
                    }
                });

            if !success {
                return Err(format!(
                    "Filter '{}' not found on layer {}",
                    req.filter_id, req.layer_id
                ));
            }

            let snapshot = self
                .state
                .require_session(doc_id)?
                .document_handle
                .snapshot();
            let base_clear = layer_needs_dither_cache_reset(&snapshot.root, req.layer_id);
            let new_filters = layer_filters(&snapshot.root, req.layer_id).unwrap_or_default();
            let changed_index = match from_index {
                Some(from) => Some(from.min(req.new_index)),
                None => None,
            };
            let (clear, preserve_dithered) =
                post_cache_refresh_policy(&old_filters, &new_filters, base_clear);
            engine_project::note_stack_change(
                req.layer_id,
                &old_filters,
                &new_filters,
                changed_index,
                clear,
                &note_prefix_externals(&snapshot, req.layer_id, &new_filters),
            );
            request_preview_refresh_at(
                &self.state,
                req.layer_id,
                clear,
                changed_index,
                preserve_dithered,
            );

            emit_document_changed(
                app_handle,
                "filter_reordered",
                Some(req.layer_id),
                Some(doc_id),
            );
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
        if engine_project::dither_profile::enabled() {
            if let Some(run_id) = req.profile_run_id {
                let gen = self
                    .state
                    .session(doc_id)
                    .ok()
                    .map(|s| {
                        s.document_handle
                            .snapshot()
                            .generations
                            .document_gen
                            .load(std::sync::atomic::Ordering::Acquire)
                    })
                    .unwrap_or(0);
                engine_project::dither_profile::begin_run(run_id, gen);
            }
            engine_project::dither_profile::mark("ipc_received");
        }

        let uuid = uuid::Uuid::parse_str(&req.filter_id)
            .map_err(|e| AppError::InvalidOperation(format!("Invalid filter_id: {}", e)))?;
        let filter_id = FilterInstanceId(uuid);

        let snapshot = self
            .state
            .require_session(doc_id)?
            .document_handle
            .snapshot();
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
                                    let is_dither_v2 =
                                        matches!(&filter.params, FilterParams::DitherV2(_));
                                    return Some((
                                        filter.kind,
                                        is_dither_v2,
                                        filter.params.clone(),
                                    ));
                                }
                            }
                        }
                        engine_project::LayerNode::Group(group) => {
                            if let Some(result) =
                                find_filter_kind(&group.children, layer_id, filter_id)
                            {
                                return Some(result);
                            }
                        }
                    }
                }
                None
            }
            let (kind, is_dither_v2, params) =
                find_filter_kind(&snapshot.root, req.layer_id, filter_id).ok_or_else(|| {
                    AppError::InvalidOperation(format!(
                        "Filter {} not found on layer {}",
                        req.filter_id, req.layer_id
                    ))
                })?;
            (kind, is_dither_v2, params)
        };
        drop(snapshot);

        let params_empty = req
            .params
            .as_object()
            .map(|o| o.is_empty())
            .unwrap_or(false);

        let new_params = if params_empty {
            existing_params
        } else if is_dither_v2
            || (filter_kind == FilterKind::Dither && req.params.get("levels").is_some())
        {
            let dither_params: engine_project::filter::DitherParamsV2 =
                serde_json::from_value(req.params.clone()).map_err(|e| {
                    AppError::InvalidOperation(format!("Invalid DitherV2 params: {}", e))
                })?;
            dither_params
                .validate()
                .map_err(|e| AppError::InvalidOperation(format!("{}", e)))?;
            // Diagnostic for GPU-family pixel_size wiring (integration checklist).
            if matches!(
                dither_params.mode,
                engine_project::filter::DitherModeV2::BlockedFs
                    | engine_project::filter::DitherModeV2::BlueNoise
            ) {
                log::debug!(
                    target: "gpu_dither",
                    "update_filter doc={} layer={} mode={:?} pixel_size={}",
                    doc_id,
                    req.layer_id,
                    dither_params.mode,
                    dither_params.pixel_size
                );
            }
            FilterParams::DitherV2(dither_params)
        } else {
            match filter_kind {
                FilterKind::Curves => {
                    let channel = match req
                        .params
                        .get("channel")
                        .and_then(|v| v.as_str())
                        .unwrap_or("All")
                    {
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
                        FilterParams::Curves {
                            curve: curve_vec,
                            channel,
                        }
                    } else {
                        FilterParams::Curves {
                            curve: vec![(0.0, 0.0), (1.0, 1.0)],
                            channel,
                        }
                    }
                }
                FilterKind::Levels => {
                    let input_black = req
                        .params
                        .get("input_black")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let input_white = req
                        .params
                        .get("input_white")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let gamma = req
                        .params
                        .get("gamma")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let output_black = req
                        .params
                        .get("output_black")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    let output_white = req
                        .params
                        .get("output_white")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let channel_r = req
                        .params
                        .get("channel_r")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let channel_g = req
                        .params
                        .get("channel_g")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
                    let channel_b = req
                        .params
                        .get("channel_b")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);
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
                    let mode = match req
                        .params
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("ErrorDiffusion")
                    {
                        "Bayer" => {
                            let matrix_size = req
                                .params
                                .get("matrix_size")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(4) as u8;
                            DitherMode::Bayer { matrix_size }
                        }
                        "ThresholdMap" => {
                            let path = req
                                .params
                                .get("path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            DitherMode::ThresholdMap { path }
                        }
                        _ => {
                            let name = req
                                .params
                                .get("kernel")
                                .and_then(|v| v.as_str())
                                .unwrap_or("FloydSteinberg");
                            let kernel = DiffusionKernel::from_ui_name(name)
                                .unwrap_or(DiffusionKernel::FloydSteinberg);
                            DitherMode::ErrorDiffusion { kernel }
                        }
                    };
                    let color_depth = req
                        .params
                        .get("color_depth")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(4) as u8;
                    FilterParams::Dither { mode, color_depth }
                }
                FilterKind::PaletteQuantize => {
                    let palette_id = req
                        .params
                        .get("palette_id")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    let diffusion = req
                        .params
                        .get("diffusion")
                        .and_then(|v| v.as_str())
                        .map(|s| {
                            DiffusionKernel::from_ui_name(s)
                                .unwrap_or(DiffusionKernel::FloydSteinberg)
                        });
                    let metric = req
                        .params
                        .get("metric")
                        .and_then(|v| v.as_str())
                        .unwrap_or("oklab")
                        .to_string();
                    FilterParams::PaletteQuantize {
                        palette_id: engine_project::PaletteId::new(palette_id),
                        diffusion,
                        metric,
                    }
                }
                FilterKind::Glitch => {
                    let p: GlitchFilterParams =
                        serde_json::from_value(req.params.clone()).unwrap_or_default();
                    FilterParams::Glitch(p)
                }
                FilterKind::Glow => {
                    let radius = req
                        .params
                        .get("radius")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(2.0) as f32;
                    let intensity = req
                        .params
                        .get("intensity")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(1.0) as f32;
                    let threshold = req
                        .params
                        .get("threshold")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Glow {
                        radius,
                        intensity,
                        threshold,
                    }
                }
                FilterKind::Crt => {
                    let period = req
                        .params
                        .get("period")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(2) as u8;
                    let strength = req
                        .params
                        .get("strength")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.5) as f32;
                    let mask_strength = req
                        .params
                        .get("mask_strength")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0) as f32;
                    FilterParams::Crt {
                        period,
                        strength,
                        mask_strength,
                    }
                }
                FilterKind::Adjust => {
                    let (
                        ec,
                        eb,
                        es,
                        ebl,
                        esh,
                        en,
                        eex,
                        eshads,
                        ehi,
                        etemp,
                        etint,
                        evib,
                        ehue,
                        eclar,
                    ) = match existing_params {
                        FilterParams::Adjust {
                            contrast,
                            brightness,
                            saturation,
                            blur,
                            sharpness,
                            noise,
                            exposure,
                            shadows,
                            highlights,
                            temperature,
                            tint,
                            vibrance,
                            hue,
                            clarity,
                        } => (
                            contrast,
                            brightness,
                            saturation,
                            blur,
                            sharpness,
                            noise,
                            exposure,
                            shadows,
                            highlights,
                            temperature,
                            tint,
                            vibrance,
                            hue,
                            clarity,
                        ),
                        _ => (
                            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                        ),
                    };
                    parse_adjust_params(
                        &req.params,
                        ec,
                        eb,
                        es,
                        ebl,
                        esh,
                        en,
                        eex,
                        eshads,
                        ehi,
                        etemp,
                        etint,
                        evib,
                        ehue,
                        eclar,
                    )
                }
                FilterKind::Exposure => {
                    let existing = match existing_params {
                        FilterParams::Exposure { stops } => stops,
                        _ => 0.0,
                    };
                    let stops = req
                        .params
                        .get("stops")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(existing);
                    FilterParams::Exposure { stops }
                }
                FilterKind::HighlightsShadows => {
                    let (es, eh) = match existing_params {
                        FilterParams::HighlightsShadows {
                            shadows,
                            highlights,
                        } => (shadows, highlights),
                        _ => (0.0, 0.0),
                    };
                    let shadows = req
                        .params
                        .get("shadows")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(es);
                    let highlights = req
                        .params
                        .get("highlights")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(eh);
                    FilterParams::HighlightsShadows {
                        shadows,
                        highlights,
                    }
                }
                FilterKind::TemperatureTint => {
                    let (et, ei) = match existing_params {
                        FilterParams::TemperatureTint { temperature, tint } => (temperature, tint),
                        _ => (0.0, 0.0),
                    };
                    let temperature = req
                        .params
                        .get("temperature")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(et);
                    let tint = req
                        .params
                        .get("tint")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(ei);
                    FilterParams::TemperatureTint { temperature, tint }
                }
                FilterKind::Vibrance => {
                    let existing = match existing_params {
                        FilterParams::Vibrance { amount } => amount,
                        _ => 0.0,
                    };
                    let amount = req
                        .params
                        .get("amount")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(existing);
                    FilterParams::Vibrance { amount }
                }
                FilterKind::Hue => {
                    let existing = match existing_params {
                        FilterParams::Hue { degrees } => degrees,
                        _ => 0.0,
                    };
                    let degrees = req
                        .params
                        .get("degrees")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(existing);
                    FilterParams::Hue { degrees }
                }
                FilterKind::Clarity => {
                    let existing = match existing_params {
                        FilterParams::Clarity { amount } => amount,
                        _ => 0.0,
                    };
                    let amount = req
                        .params
                        .get("amount")
                        .and_then(|v| v.as_f64())
                        .map(|v| v as f32)
                        .unwrap_or(existing);
                    FilterParams::Clarity { amount }
                }
                FilterKind::Ascii => {
                    let p: engine_project::filter::AsciiParams =
                        serde_json::from_value(req.params.clone()).map_err(|e| {
                            AppError::InvalidOperation(format!("Invalid Ascii params: {e}"))
                        })?;
                    p.validate()
                        .map_err(|e| AppError::InvalidOperation(format!("{e}")))?;
                    FilterParams::Ascii(p)
                }
                FilterKind::Placeholder => FilterParams::Placeholder(PlaceholderParams {
                    label: "unknown".to_string(),
                    raw_params: None,
                }),
            }
        };

        let mut temp_filter = FilterInstance::new(filter_kind, new_params.clone());
        // Strict/Mixed + palette: Fade would break the exact-swatch contract.
        // Always store opacity 1.0 (mode switch or explicit opacity patch).
        let opacity_to_apply = if matches!(
            &new_params,
            FilterParams::DitherV2(p) if p.locks_filter_opacity()
        ) {
            Some(1.0)
        } else {
            req.opacity
        };
        if let Some(opacity) = opacity_to_apply {
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
        temp_filter
            .validate()
            .map_err(|e| AppError::InvalidOperation(format!("Invalid parameters: {}", e)))?;

        // Identical update (same params/opacity/blend/enabled) must not bump
        // document_gen or invalidate FullDocumentCache — that was re-running
        // GPU dither with unchanged params_hash (slider settle / queued flush).
        {
            fn find_filter(
                nodes: &[engine_project::LayerNode],
                layer_id: u32,
                filter_id: FilterInstanceId,
            ) -> Option<&engine_project::FilterInstance> {
                for node in nodes.iter() {
                    match node {
                        engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                            return layer.find_filter(filter_id);
                        }
                        engine_project::LayerNode::Group(group) => {
                            if let Some(f) = find_filter(&group.children, layer_id, filter_id) {
                                return Some(f);
                            }
                        }
                        _ => {}
                    }
                }
                None
            }
            let snapshot = self
                .state
                .require_session(doc_id)?
                .document_handle
                .snapshot();
            if let Some(cur) = find_filter(&snapshot.root, req.layer_id, filter_id) {
                let params_same = serde_json::to_value(&cur.params).ok()
                    == serde_json::to_value(&new_params).ok();
                let opacity_same = match opacity_to_apply {
                    Some(o) => (cur.opacity - o).abs() < 1e-6,
                    None => true,
                };
                let blend_same = match parsed_blend {
                    Some(m) => cur.blend_mode == m,
                    None => true,
                };
                let enabled_same = match req.enabled {
                    Some(e) => cur.enabled == e,
                    None => true,
                };
                if params_same && opacity_same && blend_same && enabled_same {
                    log::info!(
                        target: "gpu_dither",
                        "update_filter no-op doc={} layer={} filter={} (params unchanged)",
                        doc_id,
                        req.layer_id,
                        req.filter_id
                    );
                    if crate::tile_gen_trace::enabled() {
                        let before =
                            serde_json::to_value(&cur.params).unwrap_or(serde_json::Value::Null);
                        let hash = crate::tile_gen_trace::params_hash(&before);
                        let doc_gen = snapshot
                            .generations
                            .document_gen
                            .load(std::sync::atomic::Ordering::Acquire);
                        crate::tile_gen_trace::emit(
                            "update_filter_in",
                            Some(req.layer_id),
                            Some(doc_gen),
                            Some(snapshot.generations.get_layer_gen(req.layer_id)),
                            None,
                            Some(doc_gen),
                            serde_json::json!({
                                "params_hash_before": format!("{hash:x}"),
                                "params_hash_after": format!("{hash:x}"),
                                "noop": true,
                            }),
                        );
                    }
                    return Ok(());
                }
            }
        }

        if crate::tile_gen_trace::enabled() {
            let snap = self
                .state
                .require_session(doc_id)?
                .document_handle
                .snapshot();
            let doc_gen = snap
                .generations
                .document_gen
                .load(std::sync::atomic::Ordering::Acquire);
            let before_hash = {
                fn find_filter(
                    nodes: &[engine_project::LayerNode],
                    layer_id: u32,
                    filter_id: FilterInstanceId,
                ) -> Option<&engine_project::FilterInstance> {
                    for node in nodes.iter() {
                        match node {
                            engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                                return layer.find_filter(filter_id);
                            }
                            engine_project::LayerNode::Group(group) => {
                                if let Some(f) = find_filter(&group.children, layer_id, filter_id) {
                                    return Some(f);
                                }
                            }
                            _ => {}
                        }
                    }
                    None
                }
                find_filter(&snap.root, req.layer_id, filter_id)
                    .and_then(|f| serde_json::to_value(&f.params).ok())
                    .map(|v| crate::tile_gen_trace::params_hash(&v))
                    .unwrap_or(0)
            };
            let after_hash = crate::tile_gen_trace::params_hash(
                &serde_json::to_value(&new_params).unwrap_or(serde_json::Value::Null),
            );
            crate::tile_gen_trace::emit(
                "update_filter_in",
                Some(req.layer_id),
                Some(doc_gen),
                Some(snap.generations.get_layer_gen(req.layer_id)),
                None,
                Some(doc_gen),
                serde_json::json!({
                    "params_hash_before": format!("{before_hash:x}"),
                    "params_hash_after": format!("{after_hash:x}"),
                    "noop": false,
                }),
            );
        }

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let layer_id = req.layer_id;
            let mut found = false;
            let gen_before = self
                .state
                .require_session(doc_id)?
                .document_handle
                .snapshot()
                .generations
                .document_gen
                .load(std::sync::atomic::Ordering::Acquire);
            let old_filters = layer_filters(
                &self
                    .state
                    .require_session(doc_id)?
                    .document_handle
                    .snapshot()
                    .root,
                layer_id,
            )
            .unwrap_or_default();
            let changed_index = engine_project::post_filter_index(&old_filters, filter_id);
            self.state
                .require_session(doc_id)?
                .document_handle
                .mutate(|doc| {
                    fn update_filter_in_nodes(
                        nodes: &mut [engine_project::LayerNode],
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
                                            // Keep algorithm_id aligned with DitherV2.mode so
                                            // registry dispatch and requires_full_row stay consistent.
                                            if let FilterParams::DitherV2(ref p) = new_params {
                                                filter.algorithm_id =
                                                    p.mode.algorithm_id().map(str::to_string);
                                                filter.schema_version = filter
                                                .algorithm_id
                                                .as_deref()
                                                .and_then(|id| {
                                                    engine_project::algorithms::builtin_registry()
                                                        .get_by_str(id)
                                                        .map(|a| a.schema_version())
                                                });
                                            }
                                            filter.requires_full_row =
                                            engine_project::FilterInstance::params_require_full_row(
                                                &new_params,
                                            );
                                            filter.requires_full_column =
                                            engine_project::FilterInstance::params_require_full_column(
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
                        opacity_to_apply,
                        parsed_blend,
                        req.enabled,
                    );
                    if found {
                        let new_filters = layer_filters(&doc.root, layer_id).unwrap_or_default();
                        let bump_layer = apply_stack_gens_after_mutate(
                            doc,
                            layer_id,
                            &old_filters,
                            &new_filters,
                        );
                        if bump_layer {
                            doc.generations.increment_layer_gen(layer_id);
                        }
                    }
                });

            if !found {
                return Err(format!(
                    "Filter {} not found on layer {} during update",
                    req.filter_id, req.layer_id
                ));
            }

            if crate::tile_gen_trace::enabled() {
                let snapshot = self
                    .state
                    .require_session(doc_id)?
                    .document_handle
                    .snapshot();
                let gen_after = snapshot
                    .generations
                    .document_gen
                    .load(std::sync::atomic::Ordering::Acquire);
                crate::tile_gen_trace::emit(
                    "gen_bumped",
                    Some(layer_id),
                    Some(gen_after),
                    Some(snapshot.generations.get_layer_gen(layer_id)),
                    None,
                    Some(gen_after),
                    serde_json::json!({
                        "doc_gen_before": gen_before,
                        "doc_gen_after": gen_after,
                        "post_gen": snapshot.generations.get_post_gen(layer_id),
                        "dither_gen": snapshot.generations.get_dither_gen(layer_id),
                    }),
                );
            }

            let snapshot = self
                .state
                .require_session(doc_id)?
                .document_handle
                .snapshot();
            let base_clear = layer_needs_dither_cache_reset(&snapshot.root, layer_id);
            let new_filters = layer_filters(&snapshot.root, layer_id).unwrap_or_default();
            let material_unchanged =
                engine_project::material_stack_unchanged(&old_filters, &new_filters);
            let (clear, preserve_dithered) =
                post_cache_refresh_policy(&old_filters, &new_filters, base_clear);
            engine_project::note_stack_change(
                layer_id,
                &old_filters,
                &new_filters,
                changed_index,
                clear && !material_unchanged,
                &note_prefix_externals(&snapshot, layer_id, &new_filters),
            );
            // Task 2: neutral→neutral bumps neither gen and skips preview invalidate.
            if !material_unchanged {
                request_preview_refresh_at(
                    &self.state,
                    layer_id,
                    clear,
                    changed_index,
                    preserve_dithered,
                );
            }

            let live_gen = snapshot
                .generations
                .document_gen
                .load(std::sync::atomic::Ordering::Acquire);
            if engine_project::dither_profile::enabled() {
                engine_project::dither_profile::set_generation(live_gen);
                engine_project::dither_profile::mark("invalidate_done");
            }

            crate::commands::emit_document_changed_with_gen(
                app_handle,
                "filter_updated",
                Some(layer_id),
                Some(doc_id),
                Some(live_gen),
            );
            Ok(())
        })
        .map_err(AppError::Generic)
    }
}
