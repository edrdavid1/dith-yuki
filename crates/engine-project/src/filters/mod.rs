//! Filter algorithms module.
//!
//! This module implements actual pixel-transforming filters:
//! - Curves: Tone adjustment via spline interpolation
//! - Levels: Histogram adjustment + gamma correction
//! - Dither: Color reduction (Bayer, ThresholdMap, ErrorDiffusion)
//! - PaletteQuantize: Oklab-based palette quantization
//! - Glitch: Creative effects (RGB shift, block displacement)
//! - Adjust: Contrast / brightness / saturation / blur / sharpness / noise

pub mod adjust;
pub mod context;
pub mod crosshatch;
pub mod crt;
pub mod curves;
pub mod dither;
pub mod dither_diffusion;
pub mod dither_ordered;
pub mod dither_residuals;
pub mod full_document;
pub mod hilbert;
pub mod line_screen;
pub mod voronoi_stipple;
pub mod ostromoukhov_table;
pub mod riemersma;
pub mod void_and_cluster;
pub mod zhou_fang_table;
pub mod glitch;
pub mod glow;
pub mod gpu_bridge;
pub mod gpu_graph;
pub mod levels;
pub mod palette_quantize;

pub mod apply;

// Re-export main API
pub use apply::apply_filter_to_tile;
pub use apply::apply_filter_to_tile_with_caches;
pub use apply::apply_filter_to_tile_with_park;
pub use apply::apply_filter_to_tile_with_residuals;
pub use curves::{CurveChannel, CurvesFilter};
pub use dither::{DitherAlgorithm, DitherFilter};
pub use full_document::{
    ensure_full_document, layer_has_full_document_filter, publish_processed_tiles,
    slice_processed_tile, FullDocumentCache, FullDocumentResult,
};
pub use glitch::{GlitchFilter, GlitchType};
pub use levels::LevelsFilter;
pub use palette_quantize::PaletteQuantizeFilter;

// Re-export filter types from filter.rs for convenience
pub use crate::filter::{DiffusionKernel, DitherMode};

// Re-export error residuals types
pub use dither_residuals::{ErrorResiduals, ErrorResidualsStore, CORNER_PATCH};

// Re-export error diffusion engine
pub use dither_diffusion::{apply_error_diffusion, apply_error_diffusion_with_cache};

// Re-export FilterContext
pub use context::FilterContext;
