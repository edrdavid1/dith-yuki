//! ASCII / text-art output stage.
//!
//! Converts an RGBA image into a character grid and renders
//! or exports it. Design: `.local-doc/ASCII_DITHER_system_spec.md`.
//!
//! This crate is independent of `engine-project`: it takes pixels and settings
//! and returns data, so it can be tested and reused without the app.

#![forbid(unsafe_code)]

pub mod analyze;
pub mod atlas;
pub mod color;
pub mod convert;
pub mod descriptor;
pub mod dither;
pub mod export;
pub mod font;
pub mod grid;
pub mod matching;
pub mod render;
pub mod symbols;

pub use analyze::{analyse_cell, analyse_image, ink_from_rgba, mean_cell_alpha, sample_cell_ink};
pub use atlas::{AtlasCache, AtlasError, AtlasGlyph, AtlasKey, AtlasOptions, FontSize, GlyphAtlas};
pub use color::{
    mean_cell_srgb8, mean_cell_srgb8_alpha, quantize_pair, quantize_rgb, resolve_rgb,
    xterm256_colour, Ansi16Palette, ColorTarget, VGA_16, WINDOWS10_16, XTERM_16,
};
pub use convert::{convert, convert_mono, MatchMode, ConvertOptions};
pub use descriptor::{ShapeContrastDesc, ShapeDesc, ToneDesc, mean_coverage};
pub use dither::CellDither;
pub use export::{
    parse_ansi_indices, to_ansi, to_html, to_json, to_png, to_png_scaled, to_svg, to_txt,
    AnsiDepth, AnsiOptions, HtmlOptions, SvgOptions, TXT_EXPORT_MAX_COLS,
};
pub use font::{BundledFont, CellMetrics, FontError, FontFace, FontId};
pub use grid::{AsciiGrid, Cell, CellColor, GridColorMode};
pub use matching::{
    MatcherTables, TwoColorMatch, EdgeField, EdgeOrient, EdgeOverlay, match_mask_two_color,
    match_shape, match_shape_contrast, match_tone, orient_glyph, sample_cell_rgb,
};
pub use render::{render_rgba, render_rgba8};
pub use symbols::{GlyphSource, Procedural, Symbol, SymbolSet};

/// FNV-1a 64-bit. Stable across runs and platforms (unlike `DefaultHasher`),
/// so it is safe for persisted identifiers.
pub(crate) fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    #[test]
    fn fnv_reference_vectors() {
        assert_eq!(super::fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(super::fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(super::fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }
}
