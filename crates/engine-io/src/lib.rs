//! Image codec support and video decoding infrastructure.
//!
//! This module provides encoding/decoding for image formats (PNG, JPEG, WebP)
//! and video decoding via FFmpeg bindings.
//!
//! Currently provides sandbox path validation utilities for secure file access
//! and SVG vectorization export.

pub mod sandbox;
pub mod svg_export;

pub use svg_export::{
    raster_to_svg, write_svg_file, SvgAlgorithm, SvgExportError, SvgExportOptions,
};

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}
