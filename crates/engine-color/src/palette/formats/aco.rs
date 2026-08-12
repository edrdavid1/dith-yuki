//! Adobe Color (.aco) parser and exporter.
//!
//! ACO is a binary format with version 1 and optional version 2 sections.
//! Version 1: header (2-byte version, 2-byte count) + color entries (10 bytes each).
//! Each color entry: 2-byte color space ID + 4 × u16 color values.
//! Color space 0 = RGB (values are 0-65535, top byte is significant).

use super::parse_error;
use crate::palette::{linear_to_srgb, LinearColor, PaletteError};

const COLOR_SPACE_RGB: u16 = 0;

/// Parse ACO format bytes into sRGB color triples.
pub fn parse(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    if data.len() < 4 {
        return Err(parse_error("ACO", "byte 0", "file too short for ACO header"));
    }

    let version = u16::from_be_bytes([data[0], data[1]]);
    let count = u16::from_be_bytes([data[2], data[3]]) as usize;

    if version != 1 && version != 2 {
        return Err(parse_error(
            "ACO",
            "byte 0",
            &format!("unsupported ACO version: {}", version),
        ));
    }

    let mut colors = Vec::new();
    let mut offset = 4;

    for i in 0..count {
        if offset + 10 > data.len() {
            return Err(parse_error(
                "ACO",
                &format!("byte {}", offset),
                &format!("color entry {} extends past end of file", i),
            ));
        }

        let color_space = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let w = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        let x = u16::from_be_bytes([data[offset + 4], data[offset + 5]]);
        let y = u16::from_be_bytes([data[offset + 6], data[offset + 7]]);
        let _z = u16::from_be_bytes([data[offset + 8], data[offset + 9]]);

        offset += 10;

        if color_space == COLOR_SPACE_RGB {
            // ACO RGB uses u16 values where the high byte is the actual 8-bit value
            let r = (w >> 8) as u8;
            let g = (x >> 8) as u8;
            let b = (y >> 8) as u8;
            colors.push((r, g, b));
        }
        // Skip non-RGB color spaces (CMYK, HSB, etc.)

        // Version 2 has a name string after each color entry
        if version == 2 {
            if offset + 2 > data.len() {
                break;
            }
            // Skip past version 2 name: 4-byte padding + length (u32) + UTF-16 name
            // Actually in v2, after the 10-byte color: 2 bytes padding + 4 bytes name_len + name_len*2 bytes
            // Simplified: skip variable-length name
            let name_len_offset = offset;
            if name_len_offset + 4 <= data.len() {
                // Skip 2 bytes (padding/zero) then get name length
                let name_len = u32::from_be_bytes([
                    data[name_len_offset],
                    data[name_len_offset + 1],
                    data[name_len_offset + 2],
                    data[name_len_offset + 3],
                ]) as usize;
                offset += 4 + name_len * 2;
            }
        }
    }

    Ok(colors)
}

/// Export linear colors to ACO version 1 format bytes.
pub fn export(colors: &[LinearColor], _name: Option<&str>) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let mut output = Vec::new();

    // Version 1 header
    output.extend_from_slice(&1u16.to_be_bytes());
    // Color count
    output.extend_from_slice(&(colors.len() as u16).to_be_bytes());

    for color in colors {
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);

        // Color space: RGB (0)
        output.extend_from_slice(&COLOR_SPACE_RGB.to_be_bytes());
        // RGB values as u16 (high byte is the actual value, low byte = 0)
        output.extend_from_slice(&((r as u16) << 8).to_be_bytes());
        output.extend_from_slice(&((g as u16) << 8).to_be_bytes());
        output.extend_from_slice(&((b as u16) << 8).to_be_bytes());
        // Fourth value (unused for RGB)
        output.extend_from_slice(&0u16.to_be_bytes());
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_data_errors() {
        assert!(parse(&[]).is_err());
    }

    #[test]
    fn parse_invalid_version_errors() {
        let mut data = vec![0u8; 4];
        data[0] = 0;
        data[1] = 3; // version 3 - unsupported
        data[2] = 0;
        data[3] = 0; // 0 colors
        assert!(parse(&data).is_err());
    }

    #[test]
    fn export_basic() {
        let colors = vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
        ];
        let result = export(&colors, None).unwrap();
        // Check version
        let version = u16::from_be_bytes([result[0], result[1]]);
        assert_eq!(version, 1);
        // Check count
        let count = u16::from_be_bytes([result[2], result[3]]);
        assert_eq!(count, 2);
    }

    #[test]
    fn export_empty_errors() {
        let colors: Vec<LinearColor> = vec![];
        assert!(export(&colors, None).is_err());
    }

    #[test]
    fn round_trip() {
        let colors = vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            LinearColor { r: 0.0, g: 0.0, b: 1.0 },
        ];
        let exported = export(&colors, None).unwrap();
        let parsed = parse(&exported).unwrap();
        assert_eq!(parsed.len(), 3);
        for (i, color) in colors.iter().enumerate() {
            let (pr, pg, pb) = parsed[i];
            let er = linear_to_srgb(color.r);
            let eg = linear_to_srgb(color.g);
            let eb = linear_to_srgb(color.b);
            assert!((pr as i16 - er as i16).unsigned_abs() <= 1);
            assert!((pg as i16 - eg as i16).unsigned_abs() <= 1);
            assert!((pb as i16 - eb as i16).unsigned_abs() <= 1);
        }
    }
}
