//! Extract `thumbnail.png` from Dither archives for OS preview providers.
//!
//! Algorithm: preview SPEC §4.1. `#![forbid(unsafe_code)]`.

#![forbid(unsafe_code)]

mod png_decode;
mod zip_cd;

use dither_zip_safe::io::{read_exact_at, IoError, MemoryReadAt, ReadAt};
use dither_zip_safe::limits::ThumbLimits;
use dither_zip_safe::{is_allowlisted_entry, validate_entry_syntax};
use thiserror::Error;

pub use png_decode::decode_thumbnail_png;
pub use zip_cd::{find_entry, parse_central_directory, ZipEntryMeta};

/// Which document kind the caller expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbKind {
    Project = 1,
    Pattern = 2,
}

impl ThumbKind {
    pub fn mimes(self) -> &'static [&'static [u8]] {
        match self {
            ThumbKind::Project => &[
                b"application/vnd.dither.project+zip",
                b"application/x-dither-project",
            ],
            ThumbKind::Pattern => &[
                b"application/vnd.dither.pattern+zip",
                b"application/x-dither-pattern",
            ],
        }
    }

    pub fn max_archive(self, limits: &ThumbLimits) -> u64 {
        match self {
            ThumbKind::Project => limits.max_archive_dyproj,
            ThumbKind::Pattern => limits.max_archive_dyuki,
        }
    }

    pub fn max_entries(self, limits: &ThumbLimits) -> u32 {
        match self {
            ThumbKind::Project => limits.max_entries_dyproj,
            ThumbKind::Pattern => limits.max_entries_dyuki,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ThumbError {
    #[error("not available")]
    NotAvailable,
    #[error("unsupported")]
    Unsupported,
    #[error("limit exceeded")]
    Limit,
    #[error("corrupt")]
    Corrupt,
    #[error("timeout")]
    Timeout,
    #[error("bad argument")]
    BadArg,
    #[error("internal")]
    Internal,
}

impl From<IoError> for ThumbError {
    fn from(e: IoError) -> Self {
        match e {
            IoError::Timeout | IoError::TooManyOps => ThumbError::Timeout,
            IoError::OutOfRange { .. } | IoError::ShortRead { .. } => ThumbError::Corrupt,
            IoError::Io(_) => ThumbError::Corrupt,
        }
    }
}

/// Decoded bitmap returned to the FFI layer (RGBA8, non-premultiplied unless requested).
#[derive(Debug, Clone)]
pub struct ThumbBitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Extract a preview bitmap from a random-access archive.
pub fn extract(
    io: &mut dyn ReadAt,
    kind: ThumbKind,
    max_side: u32,
    premultiply: bool,
    limits: &ThumbLimits,
) -> Result<ThumbBitmap, ThumbError> {
    if max_side == 0 {
        return Err(ThumbError::BadArg);
    }
    let size = io.size();
    if size == 0 || size > kind.max_archive(limits) {
        return Err(ThumbError::Limit);
    }

    let entries = parse_central_directory(io, limits, kind.max_entries(limits))?;
    validate_cd(&entries)?;

    let thumb_meta = find_entry(&entries, "thumbnail.png").ok_or(ThumbError::NotAvailable)?;
    let mime_meta = find_entry(&entries, "mimetype");

    if let Some(mime) = mime_meta {
        let mime_bytes = read_entry_bytes(io, mime, limits.max_mimetype, limits)?;
        if mime.method != 0 {
            return Err(ThumbError::NotAvailable);
        }
        if !kind.mimes().iter().any(|m| *m == mime_bytes.as_slice()) {
            return Err(ThumbError::NotAvailable);
        }
    } else {
        // Old files without mimetype: treat as not our preview target.
        return Err(ThumbError::NotAvailable);
    }

    if thumb_meta.method != 0 && thumb_meta.method != 8 {
        return Err(ThumbError::Unsupported);
    }
    if thumb_meta.compressed_size > limits.max_thumbnail_compressed {
        return Err(ThumbError::Limit);
    }

    let compressed = read_entry_bytes(io, thumb_meta, limits.max_thumbnail_compressed, limits)?;
    let raw = inflate_entry(thumb_meta, &compressed, limits.max_thumbnail_uncompressed)?;

    let mut bitmap = decode_thumbnail_png(&raw, limits)?;
    if max_side < bitmap.width.max(bitmap.height) {
        bitmap = resize_box(&bitmap, max_side)?;
    }
    if premultiply {
        premultiply_inplace(&mut bitmap.rgba);
    }
    Ok(bitmap)
}

/// Convenience for whole-file buffers (unit tests).
pub fn extract_from_bytes(
    bytes: &[u8],
    kind: ThumbKind,
    max_side: u32,
    premultiply: bool,
) -> Result<ThumbBitmap, ThumbError> {
    let limits = ThumbLimits::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(limits.deadline_ms);
    let mut io = MemoryReadAt::with_budget(bytes, limits.max_read_at_ops, Some(deadline));
    extract(&mut io, kind, max_side, premultiply, &limits)
}

fn validate_cd(entries: &[ZipEntryMeta]) -> Result<(), ThumbError> {
    let mut seen = std::collections::HashSet::new();
    for e in entries {
        if e.encrypted {
            return Err(ThumbError::Corrupt);
        }
        validate_entry_syntax(&e.name).map_err(|_| ThumbError::Corrupt)?;
        // Traversal / unsafe names already rejected; unknown names OK if safe.
        let _ = is_allowlisted_entry(&e.name);
        let lower = e.name.to_ascii_lowercase();
        if !seen.insert(lower) {
            return Err(ThumbError::Corrupt);
        }
    }
    Ok(())
}

fn read_entry_bytes(
    io: &mut dyn ReadAt,
    meta: &ZipEntryMeta,
    budget: u64,
    _limits: &ThumbLimits,
) -> Result<Vec<u8>, ThumbError> {
    if meta.compressed_size > budget {
        return Err(ThumbError::Limit);
    }
    // Local file header is at meta.local_header_offset; skip name/extra.
    let mut hdr = [0u8; 30];
    read_exact_at(io, meta.local_header_offset, &mut hdr)?;
    if &hdr[0..4] != b"PK\x03\x04" {
        return Err(ThumbError::Corrupt);
    }
    let name_len = u16::from_le_bytes([hdr[26], hdr[27]]) as u64;
    let extra_len = u16::from_le_bytes([hdr[28], hdr[29]]) as u64;
    let data_off = meta
        .local_header_offset
        .checked_add(30)
        .and_then(|o| o.checked_add(name_len))
        .and_then(|o| o.checked_add(extra_len))
        .ok_or(ThumbError::Corrupt)?;
    let mut buf = vec![0u8; meta.compressed_size as usize];
    read_exact_at(io, data_off, &mut buf)?;
    Ok(buf)
}

fn inflate_entry(
    meta: &ZipEntryMeta,
    compressed: &[u8],
    max_out: u64,
) -> Result<Vec<u8>, ThumbError> {
    match meta.method {
        0 => {
            if compressed.len() as u64 > max_out {
                return Err(ThumbError::Limit);
            }
            // CRC optional check
            let crc = crc32_ieee(compressed);
            if crc != meta.crc32 {
                return Err(ThumbError::Corrupt);
            }
            Ok(compressed.to_vec())
        }
        8 => {
            use flate2::read::DeflateDecoder;
            use std::io::Read;
            let mut dec = DeflateDecoder::new(compressed);
            let mut out = Vec::new();
            let mut tmp = [0u8; 8192];
            loop {
                let n = dec.read(&mut tmp).map_err(|_| ThumbError::Corrupt)?;
                if n == 0 {
                    break;
                }
                if (out.len() + n) as u64 > max_out {
                    return Err(ThumbError::Limit);
                }
                out.extend_from_slice(&tmp[..n]);
            }
            if out.len() as u64 != meta.uncompressed_size && meta.uncompressed_size != 0 {
                // Trust actual bytes; still verify CRC.
            }
            if crc32_ieee(&out) != meta.crc32 {
                return Err(ThumbError::Corrupt);
            }
            Ok(out)
        }
        _ => Err(ThumbError::Unsupported),
    }
}

fn crc32_ieee(data: &[u8]) -> u32 {
    // IEEE CRC-32 (ZIP).
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (!(crc & 1)).wrapping_add(1); // 0 or 0xFFFFFFFF
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn resize_box(src: &ThumbBitmap, max_side: u32) -> Result<ThumbBitmap, ThumbError> {
    let long = src.width.max(src.height);
    if long <= max_side {
        return Ok(src.clone());
    }
    let scale = max_side as f64 / long as f64;
    let tw = ((src.width as f64) * scale).round().max(1.0) as u32;
    let th = ((src.height as f64) * scale).round().max(1.0) as u32;
    let mut out = vec![0u8; (tw as usize) * (th as usize) * 4];
    // Area average (box) downsample.
    for y in 0..th {
        for x in 0..tw {
            let x0 = (x as u64 * src.width as u64) / tw as u64;
            let x1 = ((x as u64 + 1) * src.width as u64) / tw as u64;
            let y0 = (y as u64 * src.height as u64) / th as u64;
            let y1 = ((y as u64 + 1) * src.height as u64) / th as u64;
            let mut acc = [0u64; 4];
            let mut n = 0u64;
            for sy in y0..y1.max(y0 + 1) {
                for sx in x0..x1.max(x0 + 1) {
                    let i = ((sy as u32 * src.width + sx as u32) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += src.rgba[i + c] as u64;
                    }
                    n += 1;
                }
            }
            let di = ((y * tw + x) * 4) as usize;
            for c in 0..4 {
                out[di + c] = (acc[c] / n.max(1)) as u8;
            }
        }
    }
    Ok(ThumbBitmap {
        width: tw,
        height: th,
        rgba: out,
    })
}

fn premultiply_inplace(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u16;
        px[0] = ((px[0] as u16 * a) / 255) as u8;
        px[1] = ((px[1] as u16 * a) / 255) as u8;
        px[2] = ((px[2] as u16 * a) / 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn png_rgba(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut buf, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wtr = enc.write_header().unwrap();
            wtr.write_image_data(rgba).unwrap();
        }
        buf
    }

    fn make_archive(kind: ThumbKind, with_thumb: bool, with_mime: bool) -> Vec<u8> {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            if with_mime {
                let mime = kind.mimes()[0];
                zip.start_file("mimetype", stored).unwrap();
                zip.write_all(mime).unwrap();
            }
            zip.start_file(
                "manifest.json",
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(br#"{"format_version":1}"#).unwrap();
            if with_thumb {
                let rgba = vec![
                    10u8, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
                ];
                let png = png_rgba(2, 2, &rgba);
                zip.start_file("thumbnail.png", stored).unwrap();
                zip.write_all(&png).unwrap();
            }
            zip.finish().unwrap();
        }
        cursor.into_inner()
    }

    #[test]
    fn extracts_project_thumb() {
        let bytes = make_archive(ThumbKind::Project, true, true);
        let bmp = extract_from_bytes(&bytes, ThumbKind::Project, 1024, false).unwrap();
        assert_eq!((bmp.width, bmp.height), (2, 2));
        assert_eq!(bmp.rgba.len(), 16);
    }

    #[test]
    fn missing_thumb_is_not_available() {
        let bytes = make_archive(ThumbKind::Project, false, true);
        let err = extract_from_bytes(&bytes, ThumbKind::Project, 1024, false).unwrap_err();
        assert_eq!(err, ThumbError::NotAvailable);
    }

    #[test]
    fn wrong_mime_is_not_available() {
        let bytes = make_archive(ThumbKind::Pattern, true, true);
        let err = extract_from_bytes(&bytes, ThumbKind::Project, 1024, false).unwrap_err();
        assert_eq!(err, ThumbError::NotAvailable);
    }

    #[test]
    fn resize_honors_max_side() {
        let mut rgba = vec![0u8; 64 * 64 * 4];
        for (i, c) in rgba.chunks_exact_mut(4).enumerate() {
            c[0] = (i % 255) as u8;
            c[3] = 255;
        }
        let png = png_rgba(64, 64, &rgba);
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut zip = ZipWriter::new(&mut cursor);
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zip.start_file("mimetype", stored).unwrap();
            zip.write_all(ThumbKind::Project.mimes()[0]).unwrap();
            zip.start_file("thumbnail.png", stored).unwrap();
            zip.write_all(&png).unwrap();
            zip.finish().unwrap();
        }
        let bytes = cursor.into_inner();
        let bmp = extract_from_bytes(&bytes, ThumbKind::Project, 16, false).unwrap();
        assert!(bmp.width.max(bmp.height) <= 16);
    }
}
