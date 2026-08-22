//! Optional GPU compute path for per-tile pattern filters (Path B Resident Executor).
//!
//! Pixel I/O is **RGBA32 float** (locked Track D). Pattern shaders receive
//! `tile_offset` so indexing matches CPU `GlobalCoord`. Error Diffusion is
//! never GpuEligible.
//!
//! Force CPU: `DITHER_FORCE_CPU=1`. Enable GPU preview: runtime `DITHER_GPU_PREVIEW=1` or Preferences UI toggle.

mod bayer;
mod composite;
mod context;
mod crt;
mod dispatch;
pub mod ed_prototype;
mod executor;
mod graph;
mod halftone;
mod palette_guided;
mod palette_quantize;
mod prefer;
pub mod resident;

pub use bayer::{apply_bayer_gpu, BayerGpuParams, BayerMatrixSize};
pub use composite::{
    CompositePassParams, GpuCompositeFrameJob, GpuCompositeLayerOp, GpuCompositeTileWork,
};
pub use context::GpuContext;
pub use crt::{apply_crt_gpu, CrtGpuParams};
pub use dispatch::{
    core_pixel_count, dispatch_rgba32, map_read_with_timeout, TileUniforms, CORE_SIZE,
    FLOATS_PER_TILE, MAP_TIMEOUT_DEFAULT, WORKGROUP_SIZE,
};
pub use executor::{GpuExecutor, GpuFrameJob, GpuTileWork};
pub use graph::{
    compile_graph, BayerPassParams, ComputeGraph, CpuCheckpointKind, CrtPassParams,
    GraphCompileError, GraphLayerFilter, GraphNode, GpuPass, GpuPipelineKey, HalftonePassParams,
    PaletteGuidedPassParams, PaletteMixedPassParams, PaletteQuantizePassParams,
};
pub use halftone::{apply_halftone_gpu, HalftoneGpuParams};
pub use palette_guided::{palette_guided_params, palette_mixed_params_from_palette};
pub use palette_quantize::palette_quantize_params_from_lut;
pub use prefer::{
    force_cpu, gpu_filters_enabled, gpu_preview_enabled, gpu_resident_enabled, prefer_gpu,
    set_gpu_preview_ui_override,
};
pub use resident::{
    GpuTileCache, ResidentBayerPipelines, ResidentCompositePipelines, ResidentCrtPipelines,
    ResidentHalftonePipelines, ResidentPaletteGuidedPipelines, ResidentPalettePipelines,
};

/// Errors from a GPU tile dispatch (caller falls back to CPU).
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("GPU map_async timed out or failed")]
    MapTimeout,
    #[error("GPU pipeline not available: {0}")]
    Pipeline(&'static str),
    #[error("GPU buffer / device error: {0}")]
    Device(String),
}
