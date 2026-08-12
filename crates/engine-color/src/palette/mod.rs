//! Palette entity management: import, export, generation, and format parsing.
//!
//! This module provides:
//! - `LinearColor` and `Palette` types for representing palettes in linear RGB space
//! - sRGB ↔ linear conversion functions with proper gamma curves
//! - `PaletteError` for all palette-related error conditions
//! - `import_palette` and `export_palette` top-level API functions
//! - Submodules for format parsing/export and palette generation

pub mod formats;
pub mod generate;
pub mod presets;

pub use presets::{find_preset, PalettePreset, BUILTIN_PRESETS};

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

/// A single color in linear RGB space (matching PixelTile representation).
/// Each channel is nominally in [0.0, 1.0].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LinearColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// Unique identifier for a palette within a Document.
pub type PaletteId = u32;

/// A named, ordered palette stored in the Document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    /// Unique identifier within the document.
    pub id: PaletteId,
    /// Display name (1–255 chars).
    pub name: String,
    /// Ordered list of colors in linear RGB (1–65536 entries).
    pub colors: Vec<LinearColor>,
    /// Incremented on any color modification.
    pub revision: u64,
}

/// Supported palette file formats for import/export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteFormat {
    /// Adobe Swatch Exchange
    Ase,
    /// Adobe Color
    Aco,
    /// GIMP Palette
    Gpl,
    /// Microsoft RIFF Palette
    Pal,
    /// Comma-separated values
    Csv,
    /// JSON array of {r, g, b}
    Json,
}

/// Errors that can occur during palette operations.
#[derive(Debug, Error)]
pub enum PaletteError {
    #[error("palette is empty (0 colors)")]
    Empty,

    #[error("palette exceeds maximum size (65536 colors)")]
    TooLarge,

    #[error("parse error in {format} at {location}: {reason}")]
    ParseError {
        format: String,
        location: String,
        reason: String,
    },

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("sandbox error: {0}")]
    Sandbox(#[from] engine_io::sandbox::SandboxError),

    #[error("palette not found: {0}")]
    NotFound(PaletteId),

    #[error("generation failed: {0}")]
    GenerationFailed(String),
}

/// Convert an sRGB gamma-encoded u8 value to linear f32.
///
/// Uses the standard sRGB transfer function:
/// - If normalized <= 0.04045: linear = normalized / 12.92
/// - Otherwise: linear = ((normalized + 0.055) / 1.055)^2.4
pub fn srgb_to_linear(value: u8) -> f32 {
    let normalized = value as f32 / 255.0;
    if normalized <= 0.04045 {
        normalized / 12.92
    } else {
        ((normalized + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert a linear f32 value to sRGB gamma-encoded u8.
///
/// Uses the standard sRGB transfer function:
/// - Clamps input to [0.0, 1.0]
/// - If value <= 0.0031308: result = value * 12.92
/// - Otherwise: result = 1.055 * value^(1/2.4) - 0.055
/// - Returns (result * 255.0 + 0.5) as u8 (rounded)
pub fn linear_to_srgb(value: f32) -> u8 {
    let clamped = value.clamp(0.0, 1.0);
    let result = if clamped <= 0.0031308 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    (result * 255.0 + 0.5) as u8
}

/// Get the allowed file extensions for a given palette format.
fn allowed_extensions(format: PaletteFormat) -> &'static [&'static str] {
    match format {
        PaletteFormat::Ase => &["ase"],
        PaletteFormat::Aco => &["aco"],
        PaletteFormat::Gpl => &["gpl"],
        PaletteFormat::Pal => &["pal"],
        PaletteFormat::Csv => &["csv"],
        PaletteFormat::Json => &["json"],
    }
}

/// Parse a palette file into linear RGB colors.
///
/// Steps:
/// 1. Validate path via sandbox (allowed extensions based on format)
/// 2. Read file bytes
/// 3. Parse via format dispatcher
/// 4. Convert sRGB u8 to linear f32 via `srgb_to_linear`
pub fn import_palette(
    path: &Path,
    format: PaletteFormat,
) -> Result<Vec<LinearColor>, PaletteError> {
    // 1. Validate path via sandbox
    let canonical = engine_io::sandbox::resolve_user_path(
        path.to_str().unwrap_or(""),
        allowed_extensions(format),
    )?;

    // 2. Read file bytes
    let data = std::fs::read(&canonical).map_err(|e| PaletteError::ParseError {
        format: format!("{:?}", format),
        location: canonical.display().to_string(),
        reason: format!("I/O error: {}", e),
    })?;

    // 3. Parse via format dispatcher
    let srgb_colors = formats::parse_format(&data, format)?;

    // 4. Convert sRGB u8 to linear f32
    let linear_colors = srgb_colors
        .into_iter()
        .map(|(r, g, b)| LinearColor {
            r: srgb_to_linear(r),
            g: srgb_to_linear(g),
            b: srgb_to_linear(b),
        })
        .collect();

    Ok(linear_colors)
}

/// Export a palette to the given format as bytes.
///
/// Steps:
/// 1. Validate palette non-empty
/// 2. Call format-specific exporter (converts LinearColor → sRGB u8 internally)
pub fn export_palette(
    palette: &Palette,
    format: PaletteFormat,
) -> Result<Vec<u8>, PaletteError> {
    // 1. Validate palette non-empty
    if palette.colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    // 2. Call format-specific exporter
    formats::export_format(&palette.colors, format, Some(&palette.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn srgb_to_linear_black() {
        assert_eq!(srgb_to_linear(0), 0.0);
    }

    #[test]
    fn srgb_to_linear_white() {
        let result = srgb_to_linear(255);
        assert!((result - 1.0).abs() < 1e-5, "expected ~1.0, got {}", result);
    }

    #[test]
    fn srgb_to_linear_mid_gray() {
        // sRGB 128 should be approximately 0.2158 in linear space
        let result = srgb_to_linear(128);
        assert!(
            (result - 0.2158).abs() < 0.01,
            "expected ~0.2158, got {}",
            result
        );
    }

    #[test]
    fn linear_to_srgb_black() {
        assert_eq!(linear_to_srgb(0.0), 0);
    }

    #[test]
    fn linear_to_srgb_white() {
        assert_eq!(linear_to_srgb(1.0), 255);
    }

    #[test]
    fn linear_to_srgb_clamps_negative() {
        assert_eq!(linear_to_srgb(-0.5), 0);
    }

    #[test]
    fn linear_to_srgb_clamps_above_one() {
        assert_eq!(linear_to_srgb(1.5), 255);
    }

    #[test]
    fn round_trip_all_u8_values() {
        // For every u8 value, converting to linear and back should give the same value
        for i in 0..=255u8 {
            let linear = srgb_to_linear(i);
            let back = linear_to_srgb(linear);
            assert_eq!(
                back, i,
                "round-trip failed for sRGB {}: linear={}, back={}",
                i, linear, back
            );
        }
    }

    #[test]
    fn linear_to_srgb_monotonic() {
        // Output should be monotonically non-decreasing
        let mut prev = 0u8;
        for i in 0..=1000u32 {
            let linear = i as f32 / 1000.0;
            let srgb = linear_to_srgb(linear);
            assert!(
                srgb >= prev,
                "not monotonic at linear={}: got {} after {}",
                linear,
                srgb,
                prev
            );
            prev = srgb;
        }
    }

    #[test]
    fn srgb_to_linear_monotonic() {
        // Output should be monotonically non-decreasing
        let mut prev = 0.0f32;
        for i in 0..=255u8 {
            let linear = srgb_to_linear(i);
            assert!(
                linear >= prev,
                "not monotonic at sRGB {}: got {} after {}",
                i,
                linear,
                prev
            );
            prev = linear;
        }
    }

    #[test]
    fn export_palette_basic() {
        let palette = Palette {
            id: 1,
            name: "Test Palette".to_string(),
            colors: vec![
                LinearColor { r: 1.0, g: 0.0, b: 0.0 },
                LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            ],
            revision: 1,
        };

        // Test each format
        for format in [
            PaletteFormat::Gpl,
            PaletteFormat::Json,
            PaletteFormat::Csv,
            PaletteFormat::Ase,
            PaletteFormat::Aco,
            PaletteFormat::Pal,
        ] {
            let result = export_palette(&palette, format);
            assert!(result.is_ok(), "export failed for {:?}: {:?}", format, result.err());
            assert!(!result.unwrap().is_empty());
        }
    }

    #[test]
    fn export_palette_empty_errors() {
        let palette = Palette {
            id: 1,
            name: "Empty".to_string(),
            colors: vec![],
            revision: 1,
        };

        let result = export_palette(&palette, PaletteFormat::Gpl);
        assert!(matches!(result, Err(PaletteError::Empty)));
    }
}
