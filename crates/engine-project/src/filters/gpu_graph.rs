//! FilterStack → [`engine_gpu::graph::ComputeGraph`] bridge.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_guided::default_channel_levels;
use engine_color::palette_lut::{PaletteLutCache, DEFAULT_LUT_SIZE};
use engine_gpu::{
    compile_graph, palette_guided_params, palette_mixed_params_from_palette,
    palette_quantize_params_from_lut, BayerPassParams, ComputeGraph, CpuCheckpointKind,
    CrtPassParams, GraphCompileError, GraphLayerFilter, GpuPipelineKey, HalftonePassParams,
};

use crate::document::Document;
use crate::filter::{
    DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterParams, PaletteDitherMode,
};

/// Session palettes + LUT cache so Strict/Guided/Mixed/PaletteQuantize can be GPU nodes.
pub struct PaletteGraphCtx<'a> {
    pub document: &'a Document,
    pub lut_cache: &'a PaletteLutCache,
    pub kd_cache: &'a PaletteKdCache,
}

/// Map one layer's enabled filters to graph layer specs (order preserved).
pub fn layer_to_graph_specs(filters: &[FilterInstance]) -> Vec<GraphLayerFilter> {
    layer_to_graph_specs_with_palettes(filters, None)
}

pub fn layer_to_graph_specs_with_palettes(
    filters: &[FilterInstance],
    palettes: Option<&PaletteGraphCtx<'_>>,
) -> Vec<GraphLayerFilter> {
    filters
        .iter()
        .filter(|f| f.enabled)
        .map(|f| filter_to_spec(f, palettes))
        .collect()
}

pub fn compile_layer_graph(filters: &[FilterInstance]) -> Result<ComputeGraph, GraphCompileError> {
    compile_layer_graph_with_palettes(filters, None)
}

pub fn compile_layer_graph_with_palettes(
    filters: &[FilterInstance],
    palettes: Option<&PaletteGraphCtx<'_>>,
) -> Result<ComputeGraph, GraphCompileError> {
    compile_graph(&layer_to_graph_specs_with_palettes(filters, palettes))
}

fn filter_to_spec(filter: &FilterInstance, palettes: Option<&PaletteGraphCtx<'_>>) -> GraphLayerFilter {
    match &filter.params {
        FilterParams::DitherV2(p) => dither_v2_spec(p, palettes),
        FilterParams::PaletteQuantize {
            palette_id,
            diffusion,
        } => {
            if diffusion.is_some() {
                return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::ErrorDiffusion);
            }
            match try_palette_quantize_spec(*palette_id, palettes) {
                Some(spec) => spec,
                None => GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::UnsupportedFilter),
            }
        }
        FilterParams::Crt {
            period,
            strength,
            mask_strength,
        } => GraphLayerFilter::Crt(CrtPassParams {
            period: *period,
            strength: *strength,
            mask_strength: *mask_strength,
        }),
        FilterParams::Adjust { blur, .. } if *blur > 0.0 => {
            GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::AdjustBlur)
        }
        _ => GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::UnsupportedFilter),
    }
}

fn try_palette_quantize_spec(
    palette_id: crate::types::PaletteId,
    palettes: Option<&PaletteGraphCtx<'_>>,
) -> Option<GraphLayerFilter> {
    let ctx = palettes?;
    let palette = ctx.document.get_palette(palette_id)?;
    let lut = ctx
        .lut_cache
        .get_or_build(ctx.document.id.0, palette, ctx.kd_cache, DEFAULT_LUT_SIZE)
        .ok()?;
    Some(GraphLayerFilter::PaletteQuantize(
        palette_quantize_params_from_lut(lut.as_ref(), palette),
    ))
}

fn bayer_pipeline(mode: &DitherModeV2) -> Option<GpuPipelineKey> {
    match mode {
        DitherModeV2::Bayer2x2 => Some(GpuPipelineKey::Bayer2),
        DitherModeV2::Bayer4x4 => Some(GpuPipelineKey::Bayer4),
        DitherModeV2::Bayer8x8 => Some(GpuPipelineKey::Bayer8),
        _ => None,
    }
}

fn dither_color_mode(params: &DitherParamsV2) -> u32 {
    let base = match params.color_mode {
        DitherColorMode::Rgb => 0u32,
        DitherColorMode::Grayscale => 1u32,
    };
    base + if params.dither_alpha { 2 } else { 0 }
}

fn try_guided_or_mixed(
    params: &DitherParamsV2,
    palettes: Option<&PaletteGraphCtx<'_>>,
) -> Option<GraphLayerFilter> {
    let matrix = bayer_pipeline(&params.mode)?;
    let ctx = palettes?;
    let pid = params.palette_id?;
    let palette = ctx.document.get_palette(pid)?;
    let levels = match params.palette_dither_mode {
        PaletteDitherMode::Guided { channel_levels }
        | PaletteDitherMode::Mixed { channel_levels } => {
            channel_levels.unwrap_or_else(|| default_channel_levels(palette))
        }
        _ => return None,
    };
    let ranges = ctx.lut_cache.channel_ranges(ctx.document.id.0, palette);
    let guided = palette_guided_params(
        matrix,
        levels,
        params.threshold_scale,
        dither_color_mode(params),
        params.threshold_bias,
        params.pattern_angle,
        ranges,
    );
    match params.palette_dither_mode {
        PaletteDitherMode::Guided { .. } => Some(GraphLayerFilter::PaletteGuided(guided)),
        PaletteDitherMode::Mixed { .. } => Some(GraphLayerFilter::PaletteMixed(
            palette_mixed_params_from_palette(guided, palette),
        )),
        _ => None,
    }
}

fn dither_v2_spec(
    params: &DitherParamsV2,
    palettes: Option<&PaletteGraphCtx<'_>>,
) -> GraphLayerFilter {
    if params.mode.is_error_diffusion() {
        return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::ErrorDiffusion);
    }
    if params.pixel_size > 1 {
        return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::BlockGranularity);
    }
    if matches!(
        params.palette_dither_mode,
        PaletteDitherMode::Guided { .. } | PaletteDitherMode::Mixed { .. }
    ) {
        if let Some(spec) = try_guided_or_mixed(params, palettes) {
            return spec;
        }
        return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither);
    }
    if params.palette_id.is_some() {
        // Strict / Simple: GPU shaders are LUT-snap or Guided, not OrderedPalettePicker.
        return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither);
    }

    match params.mode {
        DitherModeV2::Bayer2x2 | DitherModeV2::Bayer4x4 | DitherModeV2::Bayer8x8 => {
            GraphLayerFilter::Bayer(BayerPassParams {
                pipeline: bayer_pipeline(&params.mode).unwrap(),
                levels: params.levels,
                threshold_scale: params.threshold_scale,
                color_mode: dither_color_mode(params),
                threshold_bias: params.threshold_bias,
                pattern_angle: params.pattern_angle,
            })
        }
        DitherModeV2::CmykHalftone => {
            if params.threshold_bias != 0.0 {
                return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither);
            }
            let grayscale = matches!(params.color_mode, DitherColorMode::Grayscale);
            GraphLayerFilter::Halftone(HalftonePassParams {
                cell_size: params.halftone_cell_size,
                threshold_scale: params.threshold_scale,
                dither_alpha: params.dither_alpha,
                grayscale,
            })
        }
        DitherModeV2::CustomPng { .. } | DitherModeV2::Wave => {
            GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither)
        }
        _ => GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::filter::{DitherModeV2, FilterKind};

    fn bayer_layer() -> FilterInstance {
        FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                ..Default::default()
            }),
        )
    }

    #[test]
    fn compile_bayer4_graph() {
        let g = compile_layer_graph(&[bayer_layer()]).unwrap();
        assert!(g.gpu_only_bayer4());
    }

    #[test]
    fn compile_bayer2_and_bayer8_graph() {
        let bayer2 = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer2x2,
                levels: 4,
                ..Default::default()
            }),
        );
        let g2 = compile_layer_graph(&[bayer2]).unwrap();
        assert!(g2.is_gpu_only());
        assert!(matches!(
            g2.nodes.first(),
            Some(engine_gpu::GraphNode::Gpu(engine_gpu::GpuPass {
                pipeline: engine_gpu::GpuPipelineKey::Bayer2,
                ..
            }))
        ));

        let bayer8 = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer8x8,
                levels: 4,
                ..Default::default()
            }),
        );
        let g8 = compile_layer_graph(&[bayer8]).unwrap();
        assert!(g8.is_gpu_only());
        assert!(matches!(
            g8.nodes.first(),
            Some(engine_gpu::GraphNode::Gpu(engine_gpu::GpuPass {
                pipeline: engine_gpu::GpuPipelineKey::Bayer8,
                ..
            }))
        ));
    }

    #[test]
    fn ed_becomes_checkpoint() {
        let mut f = bayer_layer();
        f.params = FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            ..Default::default()
        });
        let specs = layer_to_graph_specs(&[f]);
        assert!(matches!(
            specs[0],
            GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::ErrorDiffusion)
        ));
    }

    #[test]
    fn bayer_bias_compiles_gpu_node() {
        let f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                threshold_bias: 0.15,
                pattern_angle: 30.0,
                ..Default::default()
            }),
        );
        let g = compile_layer_graph(&[f]).unwrap();
        assert!(g.is_gpu_only());
    }

    #[test]
    fn pixel_size_block_checkpoint() {
        let f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                pixel_size: 2,
                ..Default::default()
            }),
        );
        let specs = layer_to_graph_specs(&[f]);
        assert!(matches!(
            specs[0],
            GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::BlockGranularity)
        ));
    }

    #[test]
    fn halftone_rgb_and_gray_compile() {
        let rgb = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::CmykHalftone,
                halftone_cell_size: 8,
                ..Default::default()
            }),
        );
        let g = compile_layer_graph(&[rgb]).unwrap();
        assert!(g.is_gpu_only());

        let gray = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::CmykHalftone,
                color_mode: crate::filter::DitherColorMode::Grayscale,
                halftone_cell_size: 8,
                ..Default::default()
            }),
        );
        let g = compile_layer_graph(&[gray]).unwrap();
        match &g.nodes[0] {
            engine_gpu::GraphNode::Gpu(pass) => {
                assert_eq!(pass.pipeline, engine_gpu::GpuPipelineKey::Halftone);
                assert!(pass.halftone.unwrap().grayscale);
            }
            _ => panic!("expected Gpu Halftone"),
        }
    }

    #[test]
    fn crt_compiles_gpu_node() {
        let f = FilterInstance::new(
            FilterKind::Crt,
            FilterParams::Crt {
                period: 2,
                strength: 0.5,
                mask_strength: 0.25,
            },
        );
        let g = compile_layer_graph(&[f]).unwrap();
        assert!(g.is_gpu_only());
        assert!(matches!(
            g.nodes.first(),
            Some(engine_gpu::GraphNode::Gpu(engine_gpu::GpuPass {
                pipeline: engine_gpu::GpuPipelineKey::Crt,
                ..
            }))
        ));
    }

    fn bw_doc() -> Document {
        let mut doc = Document::new(crate::types::DocumentId::new(1), 256, 256);
        doc.palettes.push(engine_color::palette::Palette {
            id: 1,
            name: "bw".into(),
            colors: vec![
                engine_color::palette::LinearColor {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                },
                engine_color::palette::LinearColor {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                },
            ],
            revision: 1,
        });
        doc
    }

    #[test]
    fn guided_without_palette_ctx_is_checkpoint() {
        let f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                palette_id: Some(crate::types::PaletteId::new(1)),
                palette_dither_mode: crate::filter::PaletteDitherMode::Guided {
                    channel_levels: Some(4),
                },
                ..Default::default()
            }),
        );
        let specs = layer_to_graph_specs(&[f]);
        assert!(matches!(
            specs[0],
            GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither)
        ));
    }

    #[test]
    fn guided_with_session_lut_is_gpu_only() {
        let doc = bw_doc();
        let lut = engine_color::palette_lut::PaletteLutCache::new();
        let kd = engine_color::palette_cache::PaletteKdCache::new();
        let ctx = PaletteGraphCtx {
            document: &doc,
            lut_cache: &lut,
            kd_cache: &kd,
        };
        let f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                palette_id: Some(crate::types::PaletteId::new(1)),
                palette_dither_mode: crate::filter::PaletteDitherMode::Guided {
                    channel_levels: Some(4),
                },
                ..Default::default()
            }),
        );
        let g = compile_layer_graph_with_palettes(&[f], Some(&ctx)).unwrap();
        assert!(g.is_gpu_only());
        assert!(matches!(
            g.nodes.first(),
            Some(engine_gpu::GraphNode::Gpu(engine_gpu::GpuPass {
                pipeline: engine_gpu::GpuPipelineKey::PaletteGuided,
                ..
            }))
        ));
    }

    #[test]
    fn palette_quantize_filter_with_lut_is_gpu_only() {
        let doc = bw_doc();
        let lut = engine_color::palette_lut::PaletteLutCache::new();
        let kd = engine_color::palette_cache::PaletteKdCache::new();
        let ctx = PaletteGraphCtx {
            document: &doc,
            lut_cache: &lut,
            kd_cache: &kd,
        };
        let f = FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: crate::types::PaletteId::new(1),
                diffusion: None,
            },
        );
        let g = compile_layer_graph_with_palettes(&[f], Some(&ctx)).unwrap();
        assert!(g.is_gpu_only());
        assert!(matches!(
            g.nodes.first(),
            Some(engine_gpu::GraphNode::Gpu(engine_gpu::GpuPass {
                pipeline: engine_gpu::GpuPipelineKey::PaletteQuantize,
                ..
            }))
        ));
    }

    #[test]
    fn strict_palette_dither_stays_checkpoint() {
        let doc = bw_doc();
        let lut = engine_color::palette_lut::PaletteLutCache::new();
        let kd = engine_color::palette_cache::PaletteKdCache::new();
        let ctx = PaletteGraphCtx {
            document: &doc,
            lut_cache: &lut,
            kd_cache: &kd,
        };
        let f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                palette_id: Some(crate::types::PaletteId::new(1)),
                palette_dither_mode: crate::filter::PaletteDitherMode::Strict,
                ..Default::default()
            }),
        );
        let g = compile_layer_graph_with_palettes(&[f], Some(&ctx)).unwrap();
        assert!(!g.is_gpu_only());
    }
}
