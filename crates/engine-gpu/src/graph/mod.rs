//! Compute graph IR (Path B D3) — no wgpu in this module.

mod compile;
mod types;

pub use compile::{compile_graph, GraphCompileError};
pub use types::{
    BayerPassParams, ComputeGraph, CpuCheckpointKind, CrtPassParams, GraphNode, GpuPass,
    GpuPipelineKey, GraphLayerFilter, HalftonePassParams, PaletteGuidedPassParams,
    PaletteMixedPassParams, PaletteQuantizePassParams,
};
