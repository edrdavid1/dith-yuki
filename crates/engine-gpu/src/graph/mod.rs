//! Compute graph IR (Path B D3) — no wgpu in this module.

mod compile;
mod types;

pub use compile::{compile_graph, GraphCompileError};
pub use types::{
    BayerPassParams, ComputeGraph, CpuCheckpointKind, CrtPassParams, GpuPass, GpuPipelineKey,
    GraphLayerFilter, GraphNode, HalftonePassParams, PaletteGuidedPassParams,
    PaletteMixedPassParams, PaletteQuantizePassParams,
};
