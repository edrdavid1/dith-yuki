//! Adobe Swatch Exchange (.ase) parser and exporter.
//!
//! ASE is a binary big-endian format with a file header followed by color blocks.
//! File structure:
//! - 4 bytes: "ASEF" magic
//! - 2 bytes: major version (u16 BE)
//! - 2 bytes: minor version (u16 BE)
//! - 4 bytes: number of blocks (u32 BE)
//! - blocks: each with 2-byte type, 4-byte length, then payload

use super::parse_error;
use crate::palette::{linear_to_srgb, LinearColor, PaletteError};

const MAGIC: &[u8; 4] = b"ASEF";
const COLOR_ENTRY: u16 = 0x0001;
#[allow(dead_code)]
const GROUP_START: u16 = 0xC001;
#[allow(dead_code)]
const GROUP_END: u16 = 0xC002;

/// Parse ASE format bytes into sRGB color triples.
pub fn parse(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    if data.len() < 12 {
        return Err(parse_error("ASE", "byte 0", "file too short for ASE header"));
    }

    // Check magic
    if &data[0..4] != MAGIC {
        return Err(parse_error("ASE", "byte 0", "invalid magic bytes, expected 'ASEF'"));
    }

    let _major = u16::from_be_bytes([data[4], data[5]]);
    let _minor = u16::from_be_bytes([data[6], data[7]]);
    let num_blocks = u32::from_be_bytes([data[8], data[9], data[10], data[11]]) as usize;

    let mut colors = Vec::new();
    let mut offset = 12;

    for _ in 0..num_blocks {
        if offset + 6 > data.len() {
            break;
        }

        let block_type = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let block_length = u32::from_be_bytes([
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
        ]) as usize;

        offset += 6;

        if block_type == COLOR_ENTRY {
            if offset + block_length > data.len() {
                return Err(parse_error(
                    "ASE",
                    &format!("byte {}", offset),
                    "color block extends past end of file",
                ));
            }

            // Skip name (null-terminated UTF-16BE string)
            let block_data = &data[offset..offset + block_length];
            if let Some(color) = parse_ase_color_block(block_data) {
                colors.push(color);
            }
        }

        offset += block_length;
    }

    Ok(colors)
}

fn parse_ase_color_block(block: &[u8]) -> Option<(u8, u8, u8)> {
    // Block layout: name_length (u16 BE) + name (UTF-16BE with null term) + color model (4 bytes) + color values + color type (u16)
    if block.len() < 2 {
        return None;
    }

    let name_len = u16::from_be_bytes([block[0], block[1]]) as usize;
    // name_len is in UTF-16 code units (2 bytes each)
    let name_bytes = name_len * 2;
    let color_offset = 2 + name_bytes;

    if block.len() < color_offset + 4 {
        return None;
    }

    let model = &block[color_offset..color_offset + 4];

    match model {
        b"RGB " => {
            if block.len() < color_offset + 4 + 12 {
                return None;
            }
            let r_bytes = &block[color_offset + 4..color_offset + 8];
            let g_bytes = &block[color_offset + 8..color_offset + 12];
            let b_bytes = &block[color_offset + 12..color_offset + 16];

            let r = f32::from_be_bytes([r_bytes[0], r_bytes[1], r_bytes[2], r_bytes[3]]);
            let g = f32::from_be_bytes([g_bytes[0], g_bytes[1], g_bytes[2], g_bytes[3]]);
            let b = f32::from_be_bytes([b_bytes[0], b_bytes[1], b_bytes[2], b_bytes[3]]);

            // ASE stores RGB as 0.0–1.0 floats; convert to u8
            Some((
                (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
                (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
                (b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ))
        }
        _ => None, // Skip CMYK, LAB, Gray — only extract RGB entries
    }
}

/// Export linear colors to ASE format bytes.
pub fn export(colors: &[LinearColor], _name: Option<&str>) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let mut output = Vec::new();

    // Magic
    output.extend_from_slice(MAGIC);
    // Version 1.0
    output.extend_from_slice(&1u16.to_be_bytes());
    output.extend_from_slice(&0u16.to_be_bytes());
    // Number of blocks
    output.extend_from_slice(&(colors.len() as u32).to_be_bytes());

    for (i, color) in colors.iter().enumerate() {
        let r = linear_to_srgb(color.r) as f32 / 255.0;
        let g = linear_to_srgb(color.g) as f32 / 255.0;
        let b = linear_to_srgb(color.b) as f32 / 255.0;

        // Build color entry block
        let mut block = Vec::new();

        // Name: short name as UTF-16BE null-terminated
        let name_str = format!("color_{}", i);
        let name_utf16: Vec<u16> = name_str.encode_utf16().chain(std::iter::once(0)).collect();
        block.extend_from_slice(&(name_utf16.len() as u16).to_be_bytes());
        for ch in &name_utf16 {
            block.extend_from_slice(&ch.to_be_bytes());
        }

        // Color model: RGB
        block.extend_from_slice(b"RGB ");
        // Color values as f32 BE
        block.extend_from_slice(&r.to_be_bytes());
        block.extend_from_slice(&g.to_be_bytes());
        block.extend_from_slice(&b.to_be_bytes());
        // Color type: 0 = Global
        block.extend_from_slice(&0u16.to_be_bytes());

        // Write block header
        output.extend_from_slice(&COLOR_ENTRY.to_be_bytes());
        output.extend_from_slice(&(block.len() as u32).to_be_bytes());
        output.extend_from_slice(&block);
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
    fn parse_invalid_magic_errors() {
        let data = b"NOT_ASEF0000";
        assert!(parse(data).is_err());
    }

    #[test]
    fn export_basic() {
        let colors = vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
        ];
        let result = export(&colors, None).unwrap();
        // Verify magic
        assert_eq!(&result[0..4], b"ASEF");
        // Verify block count
        let count = u32::from_be_bytes([result[8], result[9], result[10], result[11]]);
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
        // Check that values are close (within 1 of direct sRGB conversion)
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
