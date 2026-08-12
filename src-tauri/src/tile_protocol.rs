//! Tile protocol URL parser and error types.
//!
//! Parses URLs of the form:
//! `tile://doc/{doc_id}/layer/{layer_id}/stage/{stage}/l/{level}/{x}/{y}`
//!
//! Where:
//! - doc_id: u32 document identifier
//! - layer_id: u32 layer identifier, or "composite" for the final composite
//! - stage: "raw" | "processed" | "composite"
//! - level: u8 pyramid level (0 = full res)
//! - x: u32 tile column index at this level
//! - y: u32 tile row index at this level

use engine_tiles::{CacheStage, PixelTile, HALO, TILE_SIZE};
use std::fmt;

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur when parsing a tile protocol URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileProtocolError {
    /// The URL does not match the expected structure at all.
    MalformedUrl(String),
    /// A specific path segment could not be parsed or is invalid.
    InvalidSegment { segment: String, reason: String },
}

impl fmt::Display for TileProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TileProtocolError::MalformedUrl(msg) => {
                write!(f, "Malformed tile URL: {}", msg)
            }
            TileProtocolError::InvalidSegment { segment, reason } => {
                write!(f, "Invalid segment '{}': {}", segment, reason)
            }
        }
    }
}

impl std::error::Error for TileProtocolError {}

// ============================================================================
// Parsed URL Types
// ============================================================================

/// Identifies the layer target in a tile URL.
///
/// Can be either a specific layer by numeric ID, or "composite" for the
/// final composited result of all visible layers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerTarget {
    /// A specific layer identified by its u32 ID.
    Id(u32),
    /// The final composite of all visible layers.
    Composite,
}

/// A fully parsed tile protocol URL with all validated fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTileUrl {
    /// Document identifier.
    pub doc_id: u32,
    /// Layer target (specific layer or composite).
    pub layer: LayerTarget,
    /// Cache stage requested.
    pub stage: CacheStage,
    /// Pyramid level (0 = full resolution).
    pub level: u8,
    /// Tile column index at this pyramid level.
    pub x: u32,
    /// Tile row index at this pyramid level.
    pub y: u32,
}

// ============================================================================
// Parser
// ============================================================================

/// Parse a tile protocol URL into its component parts.
///
/// Expected format: `tile://doc/{doc_id}/layer/{layer_id}/stage/{stage}/l/{level}/{x}/{y}`
///
/// # Errors
///
/// Returns `TileProtocolError::MalformedUrl` if the URL structure doesn't match
/// the expected pattern (wrong number of segments, missing prefixes, etc.).
///
/// Returns `TileProtocolError::InvalidSegment` if a specific segment has the
/// right position but an unparseable value (e.g., non-numeric doc_id).
///
/// # Examples
///
/// ```ignore
/// let url = "tile://doc/1/layer/5/stage/raw/l/0/3/4";
/// let parsed = parse_tile_url(url).unwrap();
/// assert_eq!(parsed.doc_id, 1);
/// assert_eq!(parsed.layer, LayerTarget::Id(5));
/// assert_eq!(parsed.stage, CacheStage::Raw);
/// assert_eq!(parsed.level, 0);
/// assert_eq!(parsed.x, 3);
/// assert_eq!(parsed.y, 4);
/// ```
pub fn parse_tile_url(uri: &str) -> Result<ParsedTileUrl, TileProtocolError> {
    // Strip the scheme. Accept both "tile://" and "tile://localhost/" prefixes
    // (Tauri may normalize custom protocol URLs with a localhost authority).
    let path = uri
        .strip_prefix("tile://localhost/")
        .or_else(|| uri.strip_prefix("tile://"))
        .ok_or_else(|| {
            TileProtocolError::MalformedUrl(format!(
                "URL must start with 'tile://', got: {}",
                uri
            ))
        })?;

    // Split path into segments, filtering out empty segments from leading/trailing slashes
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // Expected: ["doc", doc_id, "layer", layer_id, "stage", stage, "l", level, x, y]
    if segments.len() != 10 {
        return Err(TileProtocolError::MalformedUrl(format!(
            "Expected 10 path segments (doc/{{id}}/layer/{{id}}/stage/{{stage}}/l/{{level}}/{{x}}/{{y}}), got {}",
            segments.len()
        )));
    }

    // Validate fixed segment labels
    if segments[0] != "doc" {
        return Err(TileProtocolError::MalformedUrl(format!(
            "Expected 'doc' at position 0, got '{}'",
            segments[0]
        )));
    }
    if segments[2] != "layer" {
        return Err(TileProtocolError::MalformedUrl(format!(
            "Expected 'layer' at position 2, got '{}'",
            segments[2]
        )));
    }
    if segments[4] != "stage" {
        return Err(TileProtocolError::MalformedUrl(format!(
            "Expected 'stage' at position 4, got '{}'",
            segments[4]
        )));
    }
    if segments[6] != "l" {
        return Err(TileProtocolError::MalformedUrl(format!(
            "Expected 'l' at position 6, got '{}'",
            segments[6]
        )));
    }

    // Parse doc_id
    let doc_id: u32 = segments[1].parse().map_err(|_| {
        TileProtocolError::InvalidSegment {
            segment: segments[1].to_string(),
            reason: "doc_id must be a valid u32 integer".to_string(),
        }
    })?;

    // Parse layer_id (u32 or "composite")
    let layer = if segments[3] == "composite" {
        LayerTarget::Composite
    } else {
        let id: u32 = segments[3].parse().map_err(|_| {
            TileProtocolError::InvalidSegment {
                segment: segments[3].to_string(),
                reason: "layer_id must be a valid u32 integer or 'composite'".to_string(),
            }
        })?;
        LayerTarget::Id(id)
    };

    // Parse stage
    let stage = match segments[5] {
        "raw" => CacheStage::Raw,
        "processed" => CacheStage::Processed,
        "composite" => CacheStage::Composite,
        other => {
            return Err(TileProtocolError::InvalidSegment {
                segment: other.to_string(),
                reason: "stage must be 'raw', 'processed', or 'composite'".to_string(),
            });
        }
    };

    // Parse level
    let level: u8 = segments[7].parse().map_err(|_| {
        TileProtocolError::InvalidSegment {
            segment: segments[7].to_string(),
            reason: "level must be a valid u8 integer (0-255)".to_string(),
        }
    })?;

    // Parse x
    let x: u32 = segments[8].parse().map_err(|_| {
        TileProtocolError::InvalidSegment {
            segment: segments[8].to_string(),
            reason: "x must be a valid u32 integer".to_string(),
        }
    })?;

    // Parse y
    let y: u32 = segments[9].parse().map_err(|_| {
        TileProtocolError::InvalidSegment {
            segment: segments[9].to_string(),
            reason: "y must be a valid u32 integer".to_string(),
        }
    })?;

    Ok(ParsedTileUrl {
        doc_id,
        layer,
        stage,
        level,
        x,
        y,
    })
}

// ============================================================================
// Conversion
// ============================================================================

/// Convert f32 tile main region to u8 RGBA buffer for wire transfer.
///
/// Extracts only the 256×256 main region (skipping the 2px halo on each side)
/// from a PixelTile and converts each f32 channel value to a u8 byte:
/// - Clamp the value to [0.0, 1.0]
/// - Multiply by 255.0
/// - Add 0.5 and truncate to u8 (equivalent to rounding)
///
/// Uses row-based SIMD processing for performance.
///
/// # Returns
///
/// A `Vec<u8>` of exactly 262,144 bytes (256 × 256 × 4 channels), in row-major RGBA8 order.
///
/// # Examples
///
/// ```ignore
/// let tile = PixelTile::new(); // all zeros
/// let buf = f32_tile_to_rgba8(&tile);
/// assert_eq!(buf.len(), 262_144);
/// assert!(buf.iter().all(|&b| b == 0));
/// ```
pub fn f32_tile_to_rgba8(tile: &PixelTile) -> Vec<u8> {
    use engine_project::f32_to_rgba8_row_simd;

    let pixel_count = (TILE_SIZE * TILE_SIZE) as usize;
    let mut buf = vec![0u8; pixel_count * 4];
    let size = (TILE_SIZE + 2 * HALO) as usize; // 260

    for row in 0..TILE_SIZE as usize {
        let src_start = ((HALO as usize + row) * size + HALO as usize) * 4;
        let src_end = src_start + (TILE_SIZE as usize) * 4;
        let dst_start = row * (TILE_SIZE as usize) * 4;
        let dst_end = dst_start + (TILE_SIZE as usize) * 4;
        f32_to_rgba8_row_simd(&mut buf[dst_start..dst_end], &tile.data[src_start..src_end]);
    }
    buf
}

// ============================================================================
// Tests
// ============================================================================

/// Reference implementation of `f32_tile_to_rgba8` preserved for property-based testing.
/// This is an exact copy of the current `f32_tile_to_rgba8` implementation at the time of snapshotting.
/// Used to verify that optimized versions (SIMD) produce byte-identical output.
#[cfg(test)]
pub fn reference_f32_tile_to_rgba8(tile: &PixelTile) -> Vec<u8> {
    let mut buf = Vec::with_capacity((TILE_SIZE * TILE_SIZE * 4) as usize);
    for y in HALO..(HALO + TILE_SIZE) {
        for x in HALO..(HALO + TILE_SIZE) {
            for c in 0..4u32 {
                buf.push((tile.at(x, y, c).clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_url_with_numeric_layer() {
        let url = "tile://doc/1/layer/5/stage/raw/l/0/3/4";
        let parsed = parse_tile_url(url).unwrap();
        assert_eq!(parsed.doc_id, 1);
        assert_eq!(parsed.layer, LayerTarget::Id(5));
        assert_eq!(parsed.stage, CacheStage::Raw);
        assert_eq!(parsed.level, 0);
        assert_eq!(parsed.x, 3);
        assert_eq!(parsed.y, 4);
    }

    #[test]
    fn parse_valid_url_with_composite_layer() {
        let url = "tile://doc/42/layer/composite/stage/composite/l/2/10/20";
        let parsed = parse_tile_url(url).unwrap();
        assert_eq!(parsed.doc_id, 42);
        assert_eq!(parsed.layer, LayerTarget::Composite);
        assert_eq!(parsed.stage, CacheStage::Composite);
        assert_eq!(parsed.level, 2);
        assert_eq!(parsed.x, 10);
        assert_eq!(parsed.y, 20);
    }

    #[test]
    fn parse_valid_url_processed_stage() {
        let url = "tile://doc/7/layer/3/stage/processed/l/1/0/0";
        let parsed = parse_tile_url(url).unwrap();
        assert_eq!(parsed.doc_id, 7);
        assert_eq!(parsed.layer, LayerTarget::Id(3));
        assert_eq!(parsed.stage, CacheStage::Processed);
        assert_eq!(parsed.level, 1);
        assert_eq!(parsed.x, 0);
        assert_eq!(parsed.y, 0);
    }

    #[test]
    fn parse_valid_url_with_localhost() {
        // Tauri may normalize custom protocol URLs with localhost
        let url = "tile://localhost/doc/1/layer/2/stage/raw/l/0/5/6";
        let parsed = parse_tile_url(url).unwrap();
        assert_eq!(parsed.doc_id, 1);
        assert_eq!(parsed.layer, LayerTarget::Id(2));
        assert_eq!(parsed.stage, CacheStage::Raw);
        assert_eq!(parsed.level, 0);
        assert_eq!(parsed.x, 5);
        assert_eq!(parsed.y, 6);
    }

    #[test]
    fn parse_max_values() {
        let url = "tile://doc/4294967295/layer/4294967295/stage/composite/l/255/4294967295/4294967295";
        let parsed = parse_tile_url(url).unwrap();
        assert_eq!(parsed.doc_id, u32::MAX);
        assert_eq!(parsed.layer, LayerTarget::Id(u32::MAX));
        assert_eq!(parsed.stage, CacheStage::Composite);
        assert_eq!(parsed.level, u8::MAX);
        assert_eq!(parsed.x, u32::MAX);
        assert_eq!(parsed.y, u32::MAX);
    }

    #[test]
    fn error_wrong_scheme() {
        let url = "http://doc/1/layer/2/stage/raw/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_too_few_segments() {
        let url = "tile://doc/1/layer/2/stage/raw";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_too_many_segments() {
        let url = "tile://doc/1/layer/2/stage/raw/l/0/0/0/extra";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_invalid_doc_id() {
        let url = "tile://doc/abc/layer/2/stage/raw/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(
            err,
            TileProtocolError::InvalidSegment { ref segment, .. } if segment == "abc"
        ));
    }

    #[test]
    fn error_invalid_layer_id() {
        let url = "tile://doc/1/layer/xyz/stage/raw/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(
            err,
            TileProtocolError::InvalidSegment { ref segment, .. } if segment == "xyz"
        ));
    }

    #[test]
    fn error_invalid_stage() {
        let url = "tile://doc/1/layer/2/stage/unknown/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(
            err,
            TileProtocolError::InvalidSegment { ref segment, .. } if segment == "unknown"
        ));
    }

    #[test]
    fn error_invalid_level_too_large() {
        let url = "tile://doc/1/layer/2/stage/raw/l/256/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(
            err,
            TileProtocolError::InvalidSegment { ref segment, .. } if segment == "256"
        ));
    }

    #[test]
    fn error_invalid_x_negative() {
        let url = "tile://doc/1/layer/2/stage/raw/l/0/-1/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(
            err,
            TileProtocolError::InvalidSegment { ref segment, .. } if segment == "-1"
        ));
    }

    #[test]
    fn error_invalid_y_not_a_number() {
        let url = "tile://doc/1/layer/2/stage/raw/l/0/0/foo";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(
            err,
            TileProtocolError::InvalidSegment { ref segment, .. } if segment == "foo"
        ));
    }

    #[test]
    fn error_wrong_fixed_segment_doc() {
        let url = "tile://document/1/layer/2/stage/raw/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_wrong_fixed_segment_layer() {
        let url = "tile://doc/1/layers/2/stage/raw/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_wrong_fixed_segment_stage() {
        let url = "tile://doc/1/layer/2/stages/raw/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_wrong_fixed_segment_level() {
        let url = "tile://doc/1/layer/2/stage/raw/level/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_empty_url() {
        let url = "";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::MalformedUrl(_)));
    }

    #[test]
    fn error_doc_id_overflow() {
        // u32::MAX + 1 = 4294967296
        let url = "tile://doc/4294967296/layer/2/stage/raw/l/0/0/0";
        let err = parse_tile_url(url).unwrap_err();
        assert!(matches!(err, TileProtocolError::InvalidSegment { .. }));
    }

    // ========================================================================
    // f32_tile_to_rgba8 tests
    // ========================================================================

    #[test]
    fn f32_tile_to_rgba8_returns_correct_length() {
        let tile = PixelTile::new();
        let buf = f32_tile_to_rgba8(&tile);
        assert_eq!(buf.len(), 262_144); // 256 * 256 * 4
    }

    #[test]
    fn f32_tile_to_rgba8_zero_tile_produces_all_zeros() {
        let tile = PixelTile::new();
        let buf = f32_tile_to_rgba8(&tile);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn f32_tile_to_rgba8_one_produces_255() {
        let mut tile = PixelTile::new();
        // Set all main-region pixels to 1.0 in all channels
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..4u32 {
                    tile.set(x, y, c, 1.0);
                }
            }
        }
        let buf = f32_tile_to_rgba8(&tile);
        assert!(buf.iter().all(|&b| b == 255));
    }

    #[test]
    fn f32_tile_to_rgba8_clamps_negative_to_zero() {
        let mut tile = PixelTile::new();
        tile.set(HALO, HALO, 0, -5.0);
        let buf = f32_tile_to_rgba8(&tile);
        assert_eq!(buf[0], 0); // first pixel, red channel
    }

    #[test]
    fn f32_tile_to_rgba8_clamps_above_one_to_255() {
        let mut tile = PixelTile::new();
        tile.set(HALO, HALO, 0, 2.5);
        let buf = f32_tile_to_rgba8(&tile);
        assert_eq!(buf[0], 255); // first pixel, red channel
    }

    #[test]
    fn f32_tile_to_rgba8_rounds_half() {
        let mut tile = PixelTile::new();
        // 0.5 * 255 = 127.5, + 0.5 = 128.0 → 128
        tile.set(HALO, HALO, 0, 0.5);
        let buf = f32_tile_to_rgba8(&tile);
        assert_eq!(buf[0], 128);
    }

    #[test]
    fn f32_tile_to_rgba8_skips_halo_region() {
        let mut tile = PixelTile::new();
        // Set halo pixel (0,0) to 1.0 — this should NOT appear in output
        tile.set(0, 0, 0, 1.0);
        tile.set(1, 1, 0, 1.0);
        let buf = f32_tile_to_rgba8(&tile);
        // Output should be all zeros since only halo was set
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn f32_tile_to_rgba8_correct_pixel_ordering() {
        let mut tile = PixelTile::new();
        // Set pixel at main-region (0, 0) — tile coords (HALO, HALO)
        tile.set(HALO, HALO, 0, 1.0); // R = 255
        tile.set(HALO, HALO, 1, 0.0); // G = 0
        tile.set(HALO, HALO, 2, 0.0); // B = 0
        tile.set(HALO, HALO, 3, 1.0); // A = 255

        // Set pixel at main-region (1, 0) — tile coords (HALO+1, HALO)
        tile.set(HALO + 1, HALO, 0, 0.0); // R = 0
        tile.set(HALO + 1, HALO, 1, 1.0); // G = 255
        tile.set(HALO + 1, HALO, 2, 0.0); // B = 0
        tile.set(HALO + 1, HALO, 3, 1.0); // A = 255

        let buf = f32_tile_to_rgba8(&tile);

        // First pixel (index 0): RGBA = (255, 0, 0, 255)
        assert_eq!(buf[0], 255);
        assert_eq!(buf[1], 0);
        assert_eq!(buf[2], 0);
        assert_eq!(buf[3], 255);

        // Second pixel (index 4): RGBA = (0, 255, 0, 255)
        assert_eq!(buf[4], 0);
        assert_eq!(buf[5], 255);
        assert_eq!(buf[6], 0);
        assert_eq!(buf[7], 255);
    }
}
