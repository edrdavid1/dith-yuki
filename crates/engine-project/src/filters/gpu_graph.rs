//! FilterStack → [`engine_gpu::graph::ComputeGraph`] bridge.

use engine_gpu::{
    compile_graph, BayerPassParams, ComputeGraph, CpuCheckpointKind, CrtPassParams,
    GraphCompileError, GraphLayerFilter, GpuPipelineKey, HalftonePassParams,
};

use crate::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterParams,
};

/// Map one layer's enabled filters to graph layer specs (order preserved).
pub fn layer_to_graph_specs(filters: &[FilterInstance]) -> Vec<GraphLayerFilter> {
    filters
        .iter()
        .filter(|f| f.enabled)
        .map(|f| filter_to_spec(f))
        .collect()
}

pub fn compile_layer_graph(filters: &[FilterInstance]) -> Result<ComputeGraph, GraphCompileError> {
    let specs = layer_to_graph_specs(filters);
    compile_graph(&specs)
}

fn filter_to_spec(filter: &FilterInstance) -> GraphLayerFilter {
    match &filter.params {
        FilterParams::DitherV2(p) => dither_v2_spec(p),
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

fn dither_v2_spec(params: &DitherParamsV2) -> GraphLayerFilter {
    if params.mode.is_error_diffusion() {
        return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::ErrorDiffusion);
    }
    if params.pixel_size > 1 {
        return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::BlockGranularity);
    }
    if params.palette_id.is_some() || params.palette_dither_mode.is_guided() {
        return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither);
    }

    match params.mode {
        DitherModeV2::Bayer2x2 | DitherModeV2::Bayer4x4 | DitherModeV2::Bayer8x8 => {
            let pipeline = match params.mode {
                DitherModeV2::Bayer2x2 => GpuPipelineKey::Bayer2,
                DitherModeV2::Bayer8x8 => GpuPipelineKey::Bayer8,
                _ => GpuPipelineKey::Bayer4,
            };
            let color_mode = match params.color_mode {
                crate::filter::DitherColorMode::Rgb => 0u32,
                crate::filter::DitherColorMode::Grayscale => 1u32,
            } + if params.dither_alpha { 2 } else { 0 };
            GraphLayerFilter::Bayer(BayerPassParams {
                pipeline,
                levels: params.levels,
                threshold_scale: params.threshold_scale,
                color_mode,
                threshold_bias: params.threshold_bias,
                pattern_angle: params.pattern_angle,
            })
        }
        DitherModeV2::CmykHalftone => {
            if params.threshold_bias != 0.0 {
                return GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::IneligibleDither);
            }
            let grayscale = matches!(
                params.color_mode,
                crate::filter::DitherColorMode::Grayscale
            );
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
}
