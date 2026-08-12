//! Optional GPU compute path for per-tile pattern filters.
//!
//! Pixel I/O is **RGBA32 float** (locked Track D). Pattern shaders receive
//! `tile_offset` so indexing matches CPU `GlobalCoord`. Error Diffusion is
//! never GpuEligible.
//!
//! Force CPU: `DITHER_FORCE_CPU=1`. Prefer GPU (when available): `DITHER_GPU=1`.

mod bayer;
mod context;
mod crt;
mod dispatch;
mod halftone;
mod prefer;

pub use bayer::{apply_bayer_gpu, BayerGpuParams, BayerMatrixSize};
pub use context::GpuContext;
pub use crt::{apply_crt_gpu, CrtGpuParams};
pub use dispatch::{
    core_pixel_count, dispatch_rgba32, map_read_with_timeout, TileUniforms, CORE_SIZE,
    FLOATS_PER_TILE, MAP_TIMEOUT_DEFAULT, WORKGROUP_SIZE,
};
pub use halftone::{apply_halftone_gpu, HalftoneGpuParams};
pub use prefer::{force_cpu, prefer_gpu, gpu_filters_enabled};

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
