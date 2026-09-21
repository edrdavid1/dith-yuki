//! Image codec support and video decoding infrastructure.
//!
//! This module provides encoding/decoding for image formats (PNG, JPEG, WebP)
//! and video decoding via FFmpeg bindings.
//!
//! Currently provides sandbox path validation utilities for secure file access
//! and SVG vectorization export.

pub mod atomic_write;
pub mod sandbox;
pub mod svg_export;

pub use atomic_write::atomic_write;
pub use svg_export::{
    escape_xml, raster_to_svg, write_svg_file, SvgAlgorithm, SvgExportError, SvgExportOptions,
};

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}
