//! CMYK8 TIFF writer with embedded ICC, PPI, and strip streaming.

use std::fs::File;
use std::io::{BufWriter, Seek, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;
use tiff::encoder::colortype::CMYK8;
use tiff::encoder::{Compression, Rational, TiffEncoder};
use tiff::tags::{ResolutionUnit, Tag};

#[derive(Debug, Error)]
pub enum CmykTiffError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TIFF error: {0}")]
    Tiff(String),
    #[error("invalid dimensions")]
    BadDimensions,
    #[error("CMYK buffer length mismatch")]
    BufferLength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmykTiffCompression {
    None,
    Lzw,
}

/// Write a CMYK8 TIFF (strips) with ICC profile and resolution tags.
///
/// Writes to `path` via a sibling temp file, then renames on success.
pub fn write_cmyk_tiff_atomic(
    path: &Path,
    width: u32,
    height: u32,
    cmyk: &[u8],
    icc: &[u8],
    ppi: f64,
    compression: CmykTiffCompression,
) -> Result<(), CmykTiffError> {
    if width == 0 || height == 0 {
        return Err(CmykTiffError::BadDimensions);
    }
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(CmykTiffError::BadDimensions)?;
    if cmyk.len() != expected {
        return Err(CmykTiffError::BufferLength);
    }

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("export.tif");
    let tmp = unique_temp_path(parent, file_name)?;

    let result = (|| {
        let file = File::create(&tmp)?;
        let mut writer = BufWriter::new(file);
        write_cmyk_tiff(&mut writer, width, height, cmyk, icc, ppi, compression)?;
        writer.flush()?;
        Ok::<(), CmykTiffError>(())
    })();

    match result {
        Ok(()) => {
            if let Err(e) = std::fs::rename(&tmp, path) {
                std::fs::copy(&tmp, path).map_err(|_| e)?;
                let _ = std::fs::remove_file(&tmp);
            }
            Ok(())
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

pub fn write_cmyk_tiff<W: Write + Seek>(
    writer: &mut W,
    width: u32,
    height: u32,
    cmyk: &[u8],
    icc: &[u8],
    ppi: f64,
    compression: CmykTiffCompression,
) -> Result<(), CmykTiffError> {
    let compression = match compression {
        CmykTiffCompression::None => Compression::Uncompressed,
        CmykTiffCompression::Lzw => Compression::Lzw,
    };

    let mut encoder = TiffEncoder::new(writer)
        .map_err(|e| CmykTiffError::Tiff(e.to_string()))?
        .with_compression(compression);

    let mut image = encoder
        .new_image::<CMYK8>(width, height)
        .map_err(|e| CmykTiffError::Tiff(e.to_string()))?;

    // PPI as rational (thousandths for determinism).
    let ppi_milli = (ppi * 1000.0).round().clamp(1.0, f64::from(u32::MAX)) as u32;
    image.resolution(
        ResolutionUnit::Inch,
        Rational {
            n: ppi_milli,
            d: 1000,
        },
    );

    if !icc.is_empty() {
        image
            .encoder()
            .write_tag(Tag::IccProfile, icc)
            .map_err(|e| CmykTiffError::Tiff(e.to_string()))?;
    }

    // Prefer moderate strip height for streaming-friendly files.
    let rows = ((1_000_000u64) / (u64::from(width) * 4).max(1)).clamp(1, u64::from(height)) as u32;
    image
        .rows_per_strip(rows)
        .map_err(|e| CmykTiffError::Tiff(e.to_string()))?;

    image
        .write_data(cmyk)
        .map_err(|e| CmykTiffError::Tiff(e.to_string()))?;

    Ok(())
}

fn unique_temp_path(parent: &Path, file_name: &str) -> Result<PathBuf, CmykTiffError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    Ok(parent.join(format!(".{file_name}.dither-print-{nanos}.tmp")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn writes_cmyk_header_tags() {
        let w = 4u32;
        let h = 2u32;
        let mut cmyk = vec![0u8; (w * h * 4) as usize];
        cmyk[0] = 10;
        cmyk[3] = 255;
        let icc = b"fake-icc-bytes-for-tag";
        let mut buf = Cursor::new(Vec::new());
        write_cmyk_tiff(&mut buf, w, h, &cmyk, icc, 300.0, CmykTiffCompression::None).unwrap();
        let bytes = buf.into_inner();
        assert!(bytes.len() > 100);
        // Little-endian TIFF magic
        assert_eq!(&bytes[0..2], b"II");
        assert!(bytes.windows(icc.len()).any(|w| w == icc));
    }

    #[test]
    fn atomic_roundtrip_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.tif");
        let cmyk = vec![0u8, 0, 0, 255, 0, 0, 0, 0];
        write_cmyk_tiff_atomic(&path, 2, 1, &cmyk, b"icc", 72.0, CmykTiffCompression::Lzw).unwrap();
        assert!(path.is_file());
        let data = std::fs::read(&path).unwrap();
        assert!(data.windows(3).any(|w| w == b"icc"));
    }
}
