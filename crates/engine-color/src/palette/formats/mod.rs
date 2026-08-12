//! Palette format parsers and exporters.
//!
//! Supported formats: ASE, ACO, GPL, PAL, CSV, JSON.
//! Each parser returns `Vec<(u8, u8, u8)>` (sRGB) or a descriptive error.
//! Each exporter takes `&[LinearColor]` and produces format bytes.

pub mod aco;
pub mod ase;
pub mod csv_json;
pub mod gpl;
pub mod pal;

use super::{LinearColor, PaletteError, PaletteFormat};

/// Maximum number of palette entries allowed.
const MAX_ENTRIES: usize = 65536;

/// Parse raw bytes into sRGB color triples using the specified format.
pub fn parse_format(data: &[u8], format: PaletteFormat) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    let colors = match format {
        PaletteFormat::Ase => ase::parse(data)?,
        PaletteFormat::Aco => aco::parse(data)?,
        PaletteFormat::Gpl => gpl::parse(data)?,
        PaletteFormat::Pal => pal::parse(data)?,
        PaletteFormat::Csv => csv_json::parse_csv(data)?,
        PaletteFormat::Json => csv_json::parse_json(data)?,
    };

    validate_count(&colors, format)?;
    Ok(colors)
}

/// Export linear colors to the specified format as bytes.
///
/// The `name` parameter is used by GPL format for the palette name header.
pub fn export_format(
    colors: &[LinearColor],
    format: PaletteFormat,
    name: Option<&str>,
) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    match format {
        PaletteFormat::Ase => ase::export(colors, name),
        PaletteFormat::Aco => aco::export(colors, name),
        PaletteFormat::Gpl => gpl::export(colors, name),
        PaletteFormat::Pal => pal::export(colors, name),
        PaletteFormat::Csv => csv_json::export_csv(colors),
        PaletteFormat::Json => csv_json::export_json(colors),
    }
}

/// Validate that the parsed color count is within [1, 65536].
fn validate_count(colors: &[(u8, u8, u8)], _format: PaletteFormat) -> Result<(), PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }
    if colors.len() > MAX_ENTRIES {
        return Err(PaletteError::TooLarge);
    }
    Ok(())
}

/// Helper to create a ParseError with format name and location info.
pub(crate) fn parse_error(format: &str, location: &str, reason: &str) -> PaletteError {
    PaletteError::ParseError {
        format: format.to_string(),
        location: location.to_string(),
        reason: reason.to_string(),
    }
}
