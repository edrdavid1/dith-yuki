//! Threshold-map embedding: content hash, zip entry paths, synthetic materialize.
//!
//! Content hash = hex(BLAKE3(png_bytes)[0..16]) → 32 lowercase hex chars.
//! Zip entry = `assets/threshold_maps/{hash}.png`.
//! JSON / callers store basename `{hash}.png` only.
//!
//! Runtime materialize path is **content-addressed** (no project/import uuid):
//! `{app_data}/dither-yuki/asset-cache/threshold-maps/{content_hash}.png`
//! Same bytes from any `.dyproj` / `.dyuki` share one file (natural dedup).
//!
//! Synthetic paths are an **internal** path class — this module does **not**
//! call `engine_io::sandbox::resolve_user_path`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Zip directory prefix for embedded threshold maps (trailing slash omitted in joins).
pub const THRESHOLD_MAPS_PREFIX: &str = "assets/threshold_maps";

/// Errors from asset hashing / materialization.
#[derive(Debug, Error)]
pub enum AssetsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unable to determine app data directory")]
    NoAppDataDir,

    #[error("invalid content-hash basename: {0}")]
    InvalidBasename(String),
}

/// BLAKE3 of PNG bytes; filename stem = first 16 digest bytes as lowercase hex.
pub fn content_hash(png_bytes: &[u8]) -> String {
    let hash = blake3::hash(png_bytes);
    let bytes = hash.as_bytes();
    hex_encode(&bytes[..16])
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Basename stored in JSON: `{content_hash}.png`.
pub fn threshold_map_basename(png_bytes: &[u8]) -> String {
    format!("{}.png", content_hash(png_bytes))
}

/// Full zip entry path for an embedded threshold map.
pub fn threshold_map_zip_entry(basename_or_hash: &str) -> String {
    let name = if basename_or_hash.ends_with(".png") {
        basename_or_hash.to_string()
    } else {
        format!("{basename_or_hash}.png")
    };
    format!("{THRESHOLD_MAPS_PREFIX}/{name}")
}

/// App-data root for the shared content-addressed asset cache:
/// `{data_dir}/dither-yuki/asset-cache/`.
pub fn asset_cache_root() -> Result<PathBuf, AssetsError> {
    let data = dirs::data_dir().ok_or(AssetsError::NoAppDataDir)?;
    Ok(data.join("dither-yuki").join("asset-cache"))
}

/// Directory for materialized threshold-map PNGs.
pub fn threshold_maps_cache_dir() -> Result<PathBuf, AssetsError> {
    Ok(asset_cache_root()?.join("threshold-maps"))
}

/// Write PNG bytes to
/// `asset-cache/threshold-maps/{content_hash}.png` and return that path.
///
/// If the file already exists (same hash from another project/pattern), it is
/// left untouched — natural cross-document deduplication.
///
/// Does **not** sandbox-validate the path (internal asset class).
pub fn materialize_threshold_map(png_bytes: &[u8]) -> Result<PathBuf, AssetsError> {
    let hash = content_hash(png_bytes);
    materialize_threshold_map_with_hash(&hash, png_bytes)
}

/// Same as [`materialize_threshold_map`], when the caller already has the hash
/// (must match `content_hash(png_bytes)`).
pub fn materialize_threshold_map_with_hash(
    hash: &str,
    png_bytes: &[u8],
) -> Result<PathBuf, AssetsError> {
    let dir = threshold_maps_cache_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{hash}.png"));
    if !path.exists() {
        let mut f = fs::File::create(&path)?;
        f.write_all(png_bytes)?;
    }
    Ok(path)
}

/// Parse `{hash}.png` basename; returns the 32-char hash stem.
pub fn parse_threshold_basename(basename: &str) -> Result<&str, AssetsError> {
    let path = Path::new(basename);
    if path.extension().and_then(|e| e.to_str()) != Some("png") {
        return Err(AssetsError::InvalidBasename(basename.to_string()));
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AssetsError::InvalidBasename(basename.to_string()))?;
    if stem.len() != 32 || !stem.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(AssetsError::InvalidBasename(basename.to_string()));
    }
    Ok(stem)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_color::threshold_map::ThresholdMapCache;

    fn grayscale_png_2x2() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, 2, 2);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 64, 128, 255]).unwrap();
        }
        buf
    }

    #[test]
    fn content_hash_stable_and_32_hex() {
        let png = grayscale_png_2x2();
        let h1 = content_hash(&png);
        let h2 = content_hash(&png);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 32);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(threshold_map_basename(&png), format!("{h1}.png"));
        assert_eq!(
            threshold_map_zip_entry(&threshold_map_basename(&png)),
            format!("assets/threshold_maps/{h1}.png")
        );
    }

    #[test]
    fn same_bytes_same_hash_name() {
        let a = grayscale_png_2x2();
        let b = grayscale_png_2x2();
        assert_eq!(content_hash(&a), content_hash(&b));
    }

    #[test]
    fn materialize_dedups_across_calls() {
        let png = grayscale_png_2x2();
        let path1 = materialize_threshold_map(&png).expect("materialize");
        let path2 = materialize_threshold_map(&png).expect("materialize again");
        assert_eq!(path1, path2);
        assert!(path1.to_string_lossy().contains("asset-cache"));
        assert!(path1.to_string_lossy().contains("threshold-maps"));
        assert_eq!(
            path1.file_name().and_then(|n| n.to_str()),
            Some(threshold_map_basename(&png).as_str())
        );

        let cache = ThresholdMapCache::new();
        let map = cache
            .get_or_load(&path1)
            .expect("ThresholdMapCache loads content-addressed path");
        assert_eq!(map.width, 2);
        assert_eq!(map.height, 2);
    }
}
