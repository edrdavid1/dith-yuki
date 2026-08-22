//! Filter stack → compute graph (pure).

use super::types::{
    hash_graph_nodes, ComputeGraph, GraphLayerFilter, GraphNode, GpuPass, GpuPipelineKey,
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphCompileError {
    #[error("filter stack produced no graph nodes")]
    Empty,
}

fn empty_pass(pipeline: GpuPipelineKey) -> GpuPass {
    GpuPass {
        pipeline,
        bayer: None,
        halftone: None,
        crt: None,
        palette_quantize: None,
        palette_guided: None,
        palette_mixed: None,
    }
}

pub fn compile_graph(filters: &[GraphLayerFilter]) -> Result<ComputeGraph, GraphCompileError> {
    let mut nodes = Vec::new();
    for f in filters {
        match f {
            GraphLayerFilter::Skip => {}
            GraphLayerFilter::Bayer(p) => {
                let mut pass = empty_pass(p.pipeline);
                pass.bayer = Some(*p);
                nodes.push(GraphNode::Gpu(pass));
            }
            GraphLayerFilter::Halftone(p) => {
                let mut pass = empty_pass(GpuPipelineKey::Halftone);
                pass.halftone = Some(*p);
                nodes.push(GraphNode::Gpu(pass));
            }
            GraphLayerFilter::Crt(p) => {
                let mut pass = empty_pass(GpuPipelineKey::Crt);
                pass.crt = Some(*p);
                nodes.push(GraphNode::Gpu(pass));
            }
            GraphLayerFilter::PaletteQuantize(p) => {
                let mut pass = empty_pass(GpuPipelineKey::PaletteQuantize);
                pass.palette_quantize = Some(p.clone());
                nodes.push(GraphNode::Gpu(pass));
            }
            GraphLayerFilter::PaletteGuided(p) => {
                let mut pass = empty_pass(GpuPipelineKey::PaletteGuided);
                pass.palette_guided = Some(*p);
                nodes.push(GraphNode::Gpu(pass));
            }
            GraphLayerFilter::PaletteMixed(p) => {
                let mut pass = empty_pass(GpuPipelineKey::PaletteMixed);
                pass.palette_mixed = Some(p.clone());
                nodes.push(GraphNode::Gpu(pass));
            }
            GraphLayerFilter::CpuCheckpoint(kind) => {
                nodes.push(GraphNode::CpuCheckpoint(*kind));
            }
        }
    }
    if nodes.is_empty() {
        return Err(GraphCompileError::Empty);
    }
    Ok(ComputeGraph {
        content_hash: hash_graph_nodes(&nodes),
        nodes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{BayerPassParams, CpuCheckpointKind, CrtPassParams, HalftonePassParams};

    fn bayer4_filter() -> GraphLayerFilter {
        GraphLayerFilter::Bayer(BayerPassParams {
            pipeline: GpuPipelineKey::Bayer4,
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            threshold_bias: 0.0,
            pattern_angle: 0.0,
        })
    }

    #[test]
    fn single_bayer4_compiles() {
        let g = compile_graph(&[bayer4_filter()]).unwrap();
        assert!(g.gpu_only_bayer4());
    }

    #[test]
    fn empty_stack_errors() {
        assert!(matches!(
            compile_graph(&[]),
            Err(GraphCompileError::Empty)
        ));
    }

    #[test]
    fn checkpoint_preserves_order() {
        let g = compile_graph(&[
            bayer4_filter(),
            GraphLayerFilter::CpuCheckpoint(CpuCheckpointKind::ErrorDiffusion),
            GraphLayerFilter::Crt(CrtPassParams {
                period: 2,
                strength: 0.5,
                mask_strength: 0.0,
            }),
        ])
        .unwrap();
        assert_eq!(g.nodes.len(), 3);
        assert!(!g.is_gpu_only());
    }

    #[test]
    fn halftone_compiles() {
        let g = compile_graph(&[GraphLayerFilter::Halftone(HalftonePassParams {
            cell_size: 4,
            threshold_scale: 1.0,
            dither_alpha: false,
            grayscale: false,
        })])
        .unwrap();
        assert!(g.is_gpu_only());
        assert!(matches!(
            g.nodes.first(),
            Some(GraphNode::Gpu(GpuPass {
                pipeline: GpuPipelineKey::Halftone,
                ..
            }))
        ));
    }

    #[test]
    fn guided_compiles() {
        let g = compile_graph(&[GraphLayerFilter::PaletteGuided(
            crate::graph::PaletteGuidedPassParams {
                matrix: GpuPipelineKey::Bayer4,
                channel_levels: 4,
                threshold_scale: 1.0,
                color_mode: 0,
                threshold_bias: 0.0,
                pattern_angle: 0.0,
                ranges: [[0.0, 1.0]; 3],
            },
        )])
        .unwrap();
        assert!(matches!(
            g.nodes.first(),
            Some(GraphNode::Gpu(GpuPass {
                pipeline: GpuPipelineKey::PaletteGuided,
                ..
            }))
        ));
    }
}
