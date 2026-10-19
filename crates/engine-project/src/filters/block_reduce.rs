//! Block-grid alpha threshold support (`alpha_grid_align_v1`).
//!
//! Reduction math lives in [`engine_tiles::block_reduce`]. This module holds the
//! routing counter for `dither.alpha_threshold_blocks`.

use std::sync::atomic::{AtomicU64, Ordering};

/// Emitted when ED/ordered applies `map_alpha` on a block-reduced alpha with `pixel_size > 1`.
#[derive(Debug, Default)]
pub struct AlphaGridAlignMetrics {
    /// Total times the block-level alpha threshold path ran (one per tile/strip apply).
    pub alpha_threshold_blocks: AtomicU64,
}

pub static ALPHA_GRID_ALIGN_METRICS: AlphaGridAlignMetrics = AlphaGridAlignMetrics {
    alpha_threshold_blocks: AtomicU64::new(0),
};

#[inline]
pub fn note_alpha_threshold_blocks(pixel_size: u8, _mode: &str) {
    let _ = pixel_size;
    ALPHA_GRID_ALIGN_METRICS
        .alpha_threshold_blocks
        .fetch_add(1, Ordering::Relaxed);
}

pub use engine_tiles::{
    reduce_block_from_rgba_f32, reduce_block_rgba_f32, reduce_block_rgba_u8,
};