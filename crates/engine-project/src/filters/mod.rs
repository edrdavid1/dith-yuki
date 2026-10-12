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
pub mod ascii_job;
pub mod context;
pub mod crosshatch;
pub mod crt;
pub mod curves;
pub mod dither;
pub mod dither_diffusion;
pub mod dither_ordered;
pub mod dither_residuals;
pub mod ed_wrap_padding;
pub mod full_document;
pub mod glitch;
pub mod glow;
pub mod gpu_bridge;
pub mod gpu_graph;
pub mod hilbert;
pub mod levels;
pub mod line_screen;
pub mod ostromoukhov_table;
pub mod palette_quantize;
pub mod random_dot_stipple;
pub mod riemersma;
pub mod void_and_cluster;
pub mod voronoi_stipple;
pub mod zhou_fang_table;

pub mod apply;

// Re-export main API
pub use apply::apply_filter_to_tile;
pub use apply::apply_filter_to_tile_with_caches;
pub use apply::apply_filter_to_tile_with_park;
pub use apply::apply_filter_to_tile_with_residuals;
pub use apply::{apply_filter_stack_tile_row_strip, layer_needs_ed_strip};
pub use ascii_job::{
    apply_ascii_rgba, export_ascii_bytes, prepare_layer_for_txt_export, run_ascii_job,
    AsciiExportFormat, AsciiJobResult,
};
pub use curves::{CurveChannel, CurvesFilter};
pub use dither::{DitherAlgorithm, DitherFilter};
pub use full_document::{
    compute_ascii_job, ensure_full_document, ensure_full_document_ex,
    ensure_full_document_ex_timed, filter_uses_ed_wrap_padding, last_fd_render_stages,
    layer_has_full_document_filter, layer_has_full_document_filter_ex, publish_processed_tiles,
    slice_processed_tile, EnsureTrace, FdRenderStages, FullDocumentCache, FullDocumentResult,
};
pub use glitch::{GlitchFilter, GlitchType};
pub use levels::LevelsFilter;
pub use palette_quantize::PaletteQuantizeFilter;

// Re-export filter types from filter.rs for convenience
pub use crate::filter::{DiffusionKernel, DitherMode};

// Re-export error residuals types
pub use dither_residuals::{ErrorResiduals, ErrorResidualsStore, CORNER_PATCH};

// Re-export error diffusion engine
pub use dither_diffusion::{
    apply_error_diffusion, apply_error_diffusion_tile_row_strip, apply_error_diffusion_with_cache,
};
pub use ed_wrap_padding::{
    apply_ed_buffer_no_wrap, apply_ed_with_wrap_padding, apply_ed_with_wrap_padding_simple,
    edge_margin, wrap_pad_rgba_f32, wrap_padding_fits_limits, EdWrapError, MAX_MARGIN_PX,
    MAX_PADDED_PIXELS, WRAP_PADDING_HINT,
};

// Re-export FilterContext
pub use context::FilterContext;
