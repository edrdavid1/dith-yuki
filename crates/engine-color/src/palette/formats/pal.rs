//! Microsoft RIFF Palette (.pal) parser and exporter.
//!
//! PAL is a RIFF container format:
//! - 4 bytes: "RIFF" magic
//! - 4 bytes: file size - 8 (u32 LE)
//! - 4 bytes: "PAL " form type
//! - 4 bytes: "data" chunk ID
//! - 4 bytes: chunk size (u32 LE)
//! - 2 bytes: version (always 0x0300)
//! - 2 bytes: number of entries (u16 LE)
//! - entries: 4 bytes each (R, G, B, flags)

use super::parse_error;
use crate::palette::{linear_to_srgb, LinearColor, PaletteError};

/// Parse Microsoft RIFF Palette format bytes into sRGB color triples.
pub fn parse(data: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    if data.len() < 24 {
        return Err(parse_error("PAL", "byte 0", "file too short for RIFF PAL header"));
    }

    // Check RIFF magic
    if &data[0..4] != b"RIFF" {
        return Err(parse_error("PAL", "byte 0", "expected 'RIFF' magic"));
    }

    // Check PAL form type
    if &data[8..12] != b"PAL " {
        return Err(parse_error("PAL", "byte 8", "expected 'PAL ' form type"));
    }

    // Find data chunk
    let mut offset = 12;
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;

        if chunk_id == b"data" {
            if offset + 8 + chunk_size > data.len() {
                return Err(parse_error(
                    "PAL",
                    &format!("byte {}", offset),
                    "data chunk extends past end of file",
                ));
            }

            let chunk_data = &data[offset + 8..offset + 8 + chunk_size];
            return parse_pal_data_chunk(chunk_data);
        }

        // Skip to next chunk (pad to even size)
        offset += 8 + chunk_size;
        if chunk_size % 2 != 0 {
            offset += 1;
        }
    }

    Err(parse_error("PAL", "end of file", "no 'data' chunk found"))
}

fn parse_pal_data_chunk(chunk: &[u8]) -> Result<Vec<(u8, u8, u8)>, PaletteError> {
    if chunk.len() < 4 {
        return Err(parse_error("PAL", "data chunk", "data chunk too short"));
    }

    let _version = u16::from_le_bytes([chunk[0], chunk[1]]);
    let count = u16::from_le_bytes([chunk[2], chunk[3]]) as usize;

    let mut colors = Vec::with_capacity(count);
    let mut offset = 4;

    for i in 0..count {
        if offset + 4 > chunk.len() {
            return Err(parse_error(
                "PAL",
                &format!("entry {}", i),
                "entry extends past end of data chunk",
            ));
        }

        let r = chunk[offset];
        let g = chunk[offset + 1];
        let b = chunk[offset + 2];
        // chunk[offset + 3] is flags byte (ignored)

        colors.push((r, g, b));
        offset += 4;
    }

    Ok(colors)
}

/// Export linear colors to Microsoft RIFF Palette format bytes.
pub fn export(colors: &[LinearColor], _name: Option<&str>) -> Result<Vec<u8>, PaletteError> {
    if colors.is_empty() {
        return Err(PaletteError::Empty);
    }

    let num_entries = colors.len();
    // data chunk: 2 (version) + 2 (count) + 4 * num_entries
    let data_chunk_size = 4 + 4 * num_entries;
    // RIFF file size: 4 (PAL ) + 4 (data) + 4 (chunk size) + data_chunk_size
    let riff_size = 4 + 4 + 4 + data_chunk_size;

    let mut output = Vec::with_capacity(8 + riff_size);

    // RIFF header
    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&(riff_size as u32).to_le_bytes());
    output.extend_from_slice(b"PAL ");

    // data chunk header
    output.extend_from_slice(b"data");
    output.extend_from_slice(&(data_chunk_size as u32).to_le_bytes());

    // data chunk content
    output.extend_from_slice(&0x0300u16.to_le_bytes()); // version
    output.extend_from_slice(&(num_entries as u16).to_le_bytes()); // count

    for color in colors {
        let r = linear_to_srgb(color.r);
        let g = linear_to_srgb(color.g);
        let b = linear_to_srgb(color.b);
        output.push(r);
        output.push(g);
        output.push(b);
        output.push(0x00); // flags
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
        let data = b"NOT_RIFF_FORMATTED_DATA_12345678";
        assert!(parse(data).is_err());
    }

    #[test]
    fn export_basic() {
        let colors = vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
        ];
        let result = export(&colors, None).unwrap();
        assert_eq!(&result[0..4], b"RIFF");
        assert_eq!(&result[8..12], b"PAL ");
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
            LinearColor { r: 0.5, g: 0.5, b: 0.5 },
        ];
        let exported = export(&colors, None).unwrap();
        let parsed = parse(&exported).unwrap();
        assert_eq!(parsed.len(), 4);
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
