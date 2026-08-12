//! Filter algorithms module.
//!
//! This module implements actual pixel-transforming filters:
//! - Curves: Tone adjustment via spline interpolation
//! - Levels: Histogram adjustment + gamma correction
//! - Dither: Color reduction (Bayer, ThresholdMap, ErrorDiffusion)
//! - PaletteQuantize: Oklab-based palette quantization
//! - Glitch: Creative effects (RGB shift, block displacement)

pub mod curves;
pub mod crt;
pub mod dither;
pub mod dither_diffusion;
pub mod dither_ordered;
pub mod dither_residuals;
pub mod glitch;
pub mod glow;
pub mod gpu_bridge;
pub mod levels;
pub mod palette_quantize;

pub mod apply;

// Re-export main API
pub use apply::apply_filter_to_tile;
pub use apply::apply_filter_to_tile_with_residuals;
pub use apply::apply_filter_to_tile_with_caches;
pub use curves::{CurveChannel, CurvesFilter};
pub use dither::{DitherAlgorithm, DitherFilter};
pub use glitch::{GlitchFilter, GlitchType};
pub use levels::LevelsFilter;
pub use palette_quantize::PaletteQuantizeFilter;

// Re-export filter types from filter.rs for convenience
pub use crate::filter::{DiffusionKernel, DitherMode};

// Re-export error residuals types
pub use dither_residuals::{ErrorResiduals, ErrorResidualsStore, CORNER_PATCH};

// Re-export error diffusion engine
pub use dither_diffusion::{apply_error_diffusion, apply_error_diffusion_with_cache};
