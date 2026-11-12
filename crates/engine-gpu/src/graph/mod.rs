//! Compute graph IR (Path B D3) — no wgpu in this module.

mod compile;
mod types;

pub use compile::{compile_graph, GraphCompileError};
pub use types::{
    AdjustPointwisePassParams, BayerPassParams, BlurNoisePassParams, ComputeGraph,
    CpuCheckpointKind, CrosshatchPassParams, CrtPassParams, CubeLutPassParams, CurvesPassParams,
    DetailPassParams, GlowPassParams, GpuPass, GpuPipelineKey, GraphLayerFilter, GraphNode,
    HalftonePassParams, LevelsPassParams, LineScreenPassParams, PaletteGuidedPassParams,
    PaletteMixedPassParams, PaletteQuantizePassParams, PointwiseOp, PointwisePassParams,
    VoidAndClusterPassParams, WavePassParams,
};
