//! Filter algorithms module.
//!
//! This module implements actual pixel-transforming filters:
//! - Curves: Tone adjustment via spline interpolation
//! - Levels: Histogram adjustment + gamma correction
//! - Dither: Color reduction (Floyd-Steinberg, Ordered)
//! - Glitch: Creative effects (RGB shift, block displacement)

pub mod curves;
pub mod dither;
pub mod glitch;
pub mod levels;

pub mod apply;

// Re-export main API
pub use apply::apply_filter_to_tile;
pub use curves::CurvesFilter;
pub use dither::{DitherAlgorithm, DitherFilter};
pub use glitch::GlitchFilter;
pub use levels::LevelsFilter;
