//! Indexed PNG (PNG8 / colortype 3) encoder for palette-constrained game assets.

use std::collections::HashMap;
use std::io::Cursor;

use thiserror::Error;

/// Errors from indexed PNG encoding.
#[derive(Debug, Error)]
pub enum IndexedPngError {
    #[error("palette is empty")]
    EmptyPalette,
    #[error("palette has {0} colors; indexed PNG supports at most 256")]
    TooManyColors(usize),
    #[error(
        "pixel at ({x}, {y}) color #{r:02X}{g:02X}{b:02X} is not in the palette \
         (indexed export requires Strict / Mixed / Simple output; Guided mode is not supported)"
    )]
    ColorNotInPalette { x: u32, y: u32, r: u8, g: u8, b: u8 },
    #[error(
        "pixel at ({x}, {y}) has soft alpha ({alpha}); enable Pixelate Alpha or \
         threshold alpha before indexed export"
    )]
    SoftAlpha { x: u32, y: u32, alpha: u8 },
    #[error("PNG encode error: {0}")]
    Encode(String),
    #[error("image dimensions do not match buffer length")]
    DimensionMismatch,
}

/// Encode RGBA8 pixels as an indexed PNG using an exact palette match.
///
/// - Opaque pixels (`a == 255`) must match a palette RGB exactly.
/// - Fully transparent pixels (`a == 0`) use a `tRNS` transparent index
///   (RGB normalized to 0); they do not need to match palette colors.
/// - Soft alpha (`0 < a < 255`) is rejected.
/// - More than 256 palette colors is an error (no silent truncation).
pub fn encode_indexed_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    palette_srgb: &[(u8, u8, u8)],
) -> Result<Vec<u8>, IndexedPngError> {
    if palette_srgb.is_empty() {
        return Err(IndexedPngError::EmptyPalette);
    }
    if palette_srgb.len() > 256 {
        return Err(IndexedPngError::TooManyColors(palette_srgb.len()));
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .unwrap_or(usize::MAX);
    if rgba.len() != expected {
        return Err(IndexedPngError::DimensionMismatch);
    }

    let mut color_to_index: HashMap<(u8, u8, u8), u8> = HashMap::with_capacity(palette_srgb.len());
    let mut plte: Vec<u8> = Vec::with_capacity(palette_srgb.len() * 3);
    for (i, &(r, g, b)) in palette_srgb.iter().enumerate() {
        color_to_index.entry((r, g, b)).or_insert(i as u8);
        plte.push(r);
        plte.push(g);
        plte.push(b);
    }

    // Transparent index: reuse palette index 0 via tRNS, or append if room.
    let needs_transparency = rgba.chunks_exact(4).any(|p| p[3] == 0);
    let (transparent_index, trns): (Option<u8>, Option<Vec<u8>>) = if needs_transparency {
        // Prefer a dedicated transparent slot so opaque uses of color 0 stay opaque.
        if plte.len() / 3 < 256 {
            let idx = (plte.len() / 3) as u8;
            plte.extend_from_slice(&[0, 0, 0]);
            let mut t = vec![255u8; idx as usize];
            t.push(0);
            (Some(idx), Some(t))
        } else {
            // Full 256-color palette: mark index 0 transparent when used for a=0 only.
            (Some(0), Some(vec![0u8]))
        }
    } else {
        (None, None)
    };

    let mut indices = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let i = ((y * width + x) * 4) as usize;
            let r = rgba[i];
            let g = rgba[i + 1];
            let b = rgba[i + 2];
            let a = rgba[i + 3];
            if a == 0 {
                indices.push(transparent_index.unwrap_or(0));
                continue;
            }
            if a != 255 {
                return Err(IndexedPngError::SoftAlpha { x, y, alpha: a });
            }
            match color_to_index.get(&(r, g, b)) {
                Some(&idx) => indices.push(idx),
                None => {
                    return Err(IndexedPngError::ColorNotInPalette { x, y, r, g, b });
                }
            }
        }
    }

    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut out), width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(plte);
        if let Some(t) = trns {
            encoder.set_trns(t);
        }
        let mut writer = encoder
            .write_header()
            .map_err(|e| IndexedPngError::Encode(e.to_string()))?;
        writer
            .write_image_data(&indices)
            .map_err(|e| IndexedPngError::Encode(e.to_string()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_indices() {
        let palette = [(255, 0, 0), (0, 255, 0), (0, 0, 255)];
        let rgba = [
            255, 0, 0, 255, //
            0, 255, 0, 255, //
            0, 0, 255, 255, //
            0, 0, 0, 0, // transparent
        ];
        let png = encode_indexed_png(&rgba, 2, 2, &palette).unwrap();
        let decoder = png::Decoder::new(Cursor::new(&png));
        let mut reader = decoder.read_info().unwrap();
        assert_eq!(reader.info().color_type, png::ColorType::Indexed);
        let mut buf = vec![0; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut buf).unwrap();
        assert_eq!(&buf[..frame.buffer_size()], &[0, 1, 2, 3]);
    }

    #[test]
    fn rejects_257_colors() {
        let palette: Vec<(u8, u8, u8)> = (0..257).map(|i| (i as u8, 0, 0)).collect();
        let err = encode_indexed_png(&[0, 0, 0, 255], 1, 1, &palette).unwrap_err();
        assert!(matches!(err, IndexedPngError::TooManyColors(257)));
    }

    #[test]
    fn rejects_out_of_palette() {
        let palette = [(0, 0, 0)];
        let err = encode_indexed_png(&[1, 2, 3, 255], 1, 1, &palette).unwrap_err();
        assert!(matches!(err, IndexedPngError::ColorNotInPalette { .. }));
    }

    #[test]
    fn rejects_soft_alpha() {
        let palette = [(0, 0, 0)];
        let err = encode_indexed_png(&[0, 0, 0, 128], 1, 1, &palette).unwrap_err();
        assert!(matches!(err, IndexedPngError::SoftAlpha { .. }));
    }
}
