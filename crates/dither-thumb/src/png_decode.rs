//! PNG decode with IHDR checks before allocation (preview SPEC §4.1 steps 8–9).

use crate::{ThumbBitmap, ThumbError};
use dither_zip_safe::limits::ThumbLimits;
use std::io::Cursor;

pub fn decode_thumbnail_png(bytes: &[u8], limits: &ThumbLimits) -> Result<ThumbBitmap, ThumbError> {
    // Peek IHDR before full decode.
    if bytes.len() < 24 || &bytes[0..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(ThumbError::Corrupt);
    }
    if &bytes[12..16] != b"IHDR" {
        return Err(ThumbError::Corrupt);
    }
    let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    let bit_depth = bytes[24];
    let color_type = bytes[25];
    if width == 0 || height == 0 || width > limits.max_png_side || height > limits.max_png_side {
        return Err(ThumbError::Limit);
    }
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or(ThumbError::Limit)?;
    if pixels > limits.max_png_pixels {
        return Err(ThumbError::Limit);
    }
    // Allowed: RGBA8, RGB8, Gray8, GrayA8, Indexed8.
    let ok = matches!(
        (color_type, bit_depth),
        (6, 8) | (2, 8) | (0, 8) | (4, 8) | (3, 8)
    );
    if !ok {
        return Err(ThumbError::Unsupported);
    }

    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_limits(png::Limits {
        bytes: limits.max_thumbnail_uncompressed as usize,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().map_err(|_| ThumbError::Corrupt)?;
    let info = reader.info();
    let w = info.width;
    let h = info.height;
    if w > limits.max_png_side || h > limits.max_png_side {
        return Err(ThumbError::Limit);
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    // Row-by-row with deadline checks would go here; for now one frame.
    let frame = reader.next_frame(&mut buf).map_err(|_| ThumbError::Corrupt)?;
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf[..frame.buffer_size()].to_vec(),
        other => {
            // EXPAND|ALPHA should yield RGBA; treat anything else as corrupt.
            let _ = other;
            return Err(ThumbError::Corrupt);
        }
    };
    if rgba.len() != (w as usize) * (h as usize) * 4 {
        return Err(ThumbError::Corrupt);
    }
    Ok(ThumbBitmap {
        width: w,
        height: h,
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_huge_ihdr() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(&13u32.to_be_bytes());
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&65535u32.to_be_bytes());
        png.extend_from_slice(&65535u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        png.extend_from_slice(&[0, 0, 0, 0]); // fake CRC
        let err = decode_thumbnail_png(&png, &ThumbLimits::default()).unwrap_err();
        assert_eq!(err, ThumbError::Limit);
    }
}
