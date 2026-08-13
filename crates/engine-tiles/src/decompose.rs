//! Image decomposition into tiles.
//!
//! Decomposes a full image buffer into Raw-stage PixelTile entries at pyramid level 0.
//! Tiles the image left-to-right, top-to-bottom in 256×256 blocks.
//! Edge tiles are zero-filled for regions beyond image bounds.
//! The 2px halo region is populated from adjacent pixel data.

use crate::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};
use std::sync::Arc;

/// Result of decomposing an image into a grid of tiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileGrid {
    /// Number of tile columns.
    pub cols: u32,
    /// Number of tile rows.
    pub rows: u32,
}

/// Errors that can occur during tile operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileError {
    /// The provided buffer size does not match width × height × 4.
    InvalidBufferSize {
        expected: usize,
        actual: usize,
    },
    /// Image dimensions are zero.
    ZeroDimensions,
}

impl std::fmt::Display for TileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TileError::InvalidBufferSize { expected, actual } => {
                write!(
                    f,
                    "Invalid buffer size: expected {} f32 elements, got {}",
                    expected, actual
                )
            }
            TileError::ZeroDimensions => {
                write!(f, "Image dimensions must be non-zero")
            }
        }
    }
}

impl std::error::Error for TileError {}

/// Decompose an image buffer into Raw tiles at pyramid level 0.
///
/// Tiles the image left-to-right, top-to-bottom in 256×256 blocks.
/// Edge tiles are zero-filled for regions beyond image bounds.
/// The 2px halo region is populated from adjacent pixel data.
///
/// # Arguments
///
/// - `rgba_f32`: RGBA f32 pixel buffer, row-major (4 floats per pixel)
/// - `width`: Image width in pixels
/// - `height`: Image height in pixels
/// - `layer_id`: The layer ID to assign to cached tile keys
/// - `cache`: The tile cache to store tiles into
///
/// # Returns
///
/// A `TileGrid` indicating the number of columns and rows of tiles produced.
///
/// # Errors
///
/// Returns `TileError::InvalidBufferSize` if `rgba_f32.len() != width * height * 4`.
/// Returns `TileError::ZeroDimensions` if width or height is zero.
pub fn decompose_image_to_tiles(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    layer_id: u32,
    cache: &TileCache,
) -> Result<TileGrid, TileError> {
    if width == 0 || height == 0 {
        return Err(TileError::ZeroDimensions);
    }

    let expected_len = (width as usize) * (height as usize) * 4;
    if rgba_f32.len() != expected_len {
        return Err(TileError::InvalidBufferSize {
            expected: expected_len,
            actual: rgba_f32.len(),
        });
    }

    let cols = (width + TILE_SIZE - 1) / TILE_SIZE;
    let rows = (height + TILE_SIZE - 1) / TILE_SIZE;

    for row in 0..rows {
        for col in 0..cols {
            let tile = extract_tile(rgba_f32, width, height, col, row);
            let key = TileKey {
                layer: layer_id,
                coord: TileCoord {
                    level: 0,
                    x: col,
                    y: row,
                },
                stage: CacheStage::Raw,
            };
            // Always overwrite: reload/open must not keep stale Raw from a prior document.
            cache.insert_fresh(key, Arc::new(tile));
        }
    }

    Ok(TileGrid { cols, rows })
}

/// Extract a single 256×256 tile (with 2px halo) from the image buffer.
///
/// The tile's main region spans pixels at:
///   image x: [tile_col * 256, tile_col * 256 + 255]
///   image y: [tile_row * 256, tile_row * 256 + 255]
///
/// The halo extends 2 pixels beyond the main region on each side.
/// Pixels outside image bounds are zero-filled (transparent black).
fn extract_tile(
    buffer: &[f32],
    img_width: u32,
    img_height: u32,
    tile_col: u32,
    tile_row: u32,
) -> PixelTile {
    let mut tile = PixelTile::new();
    let tile_stride = TILE_SIZE + 2 * HALO; // 260

    // The top-left corner of the main region in image coordinates
    let origin_x = (tile_col * TILE_SIZE) as i64;
    let origin_y = (tile_row * TILE_SIZE) as i64;

    // We fill the entire tile storage (260×260) including halo.
    // Halo starts at -HALO offset from the main region origin.
    for ty in 0..tile_stride {
        for tx in 0..tile_stride {
            // Image coordinates for this tile pixel
            // The halo offset means tile pixel (0,0) maps to image (origin_x - HALO, origin_y - HALO)
            let img_x = origin_x + (tx as i64) - (HALO as i64);
            let img_y = origin_y + (ty as i64) - (HALO as i64);

            // Check bounds — out-of-bounds pixels stay zero (transparent black)
            if img_x >= 0
                && img_x < (img_width as i64)
                && img_y >= 0
                && img_y < (img_height as i64)
            {
                let src_idx = ((img_y as usize) * (img_width as usize) + (img_x as usize)) * 4;
                for c in 0..4u32 {
                    tile.set(tx, ty, c, buffer[src_idx + c as usize]);
                }
            }
            // else: remains zero-initialized (transparent black)
        }
    }

    tile
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_single_tile_image() {
        // A 256×256 image produces exactly 1 tile
        let width = 256u32;
        let height = 256u32;
        let buffer = vec![0.5f32; (width * height * 4) as usize];
        let cache = TileCache::new(100_000_000);

        let grid = decompose_image_to_tiles(&buffer, width, height, 0, &cache).unwrap();

        assert_eq!(grid.cols, 1);
        assert_eq!(grid.rows, 1);
        assert_eq!(cache.entry_count(), 1);

        // Verify main region pixel values
        let key = TileKey {
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let tile = cache.get_entry(key).unwrap();
        // Main region pixel at (HALO, HALO) should be 0.5
        assert_eq!(tile.at(HALO, HALO, 0), 0.5);
        assert_eq!(tile.at(HALO, HALO, 3), 0.5);
        // Halo pixel at (0, 0) should be zero (out of image bounds)
        assert_eq!(tile.at(0, 0, 0), 0.0);
    }

    #[test]
    fn decompose_edge_tile_zero_filled() {
        // A 300×300 image produces 2×2 tile grid
        let width = 300u32;
        let height = 300u32;
        let buffer = vec![1.0f32; (width * height * 4) as usize];
        let cache = TileCache::new(100_000_000);

        let grid = decompose_image_to_tiles(&buffer, width, height, 1, &cache).unwrap();

        assert_eq!(grid.cols, 2);
        assert_eq!(grid.rows, 2);
        assert_eq!(cache.entry_count(), 4);

        // Check the bottom-right edge tile (col=1, row=1)
        // Its main region starts at image (256, 256).
        // Only pixels (256..300, 256..300) = 44×44 pixels are in-bounds.
        let key = TileKey {
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 1,
            },
            stage: CacheStage::Raw,
        };
        let tile = cache.get_entry(key).unwrap();

        // Pixel at main region (0, 0) → image (256, 256) → should be 1.0
        assert_eq!(tile.at(HALO, HALO, 0), 1.0);
        // Pixel at main region (43, 43) → image (299, 299) → should be 1.0
        assert_eq!(tile.at(HALO + 43, HALO + 43, 0), 1.0);
        // Pixel at main region (44, 44) → image (300, 300) → out of bounds → 0.0
        assert_eq!(tile.at(HALO + 44, HALO + 44, 0), 0.0);
    }

    #[test]
    fn decompose_halo_populated_from_adjacent_pixels() {
        // Create a 512×512 image with known pattern
        let width = 512u32;
        let height = 512u32;
        let mut buffer = vec![0.0f32; (width * height * 4) as usize];

        // Set pixel at (255, 128) to red — this is in tile (0,0) main region
        // but also in tile (1,0) halo (left halo, 1px into halo)
        let idx = ((128 * width + 255) * 4) as usize;
        buffer[idx] = 0.8; // R
        buffer[idx + 1] = 0.0; // G
        buffer[idx + 2] = 0.0; // B
        buffer[idx + 3] = 1.0; // A

        let cache = TileCache::new(100_000_000);
        let grid = decompose_image_to_tiles(&buffer, width, height, 0, &cache).unwrap();

        assert_eq!(grid.cols, 2);
        assert_eq!(grid.rows, 2);

        // In tile (1, 0), the main region starts at image x=256.
        // The halo extends 2px to the left, so tile pixel x=0 maps to image x=254,
        // tile pixel x=1 maps to image x=255.
        // The pixel we set is at image (255, 128).
        // In tile (1, 0): tx = 1 (halo), ty = HALO + 128 (main row 128)
        let key = TileKey {
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let tile = cache.get_entry(key).unwrap();
        // tile pixel (1, HALO + 128) should have our red pixel
        assert_eq!(tile.at(1, HALO + 128, 0), 0.8);
        assert_eq!(tile.at(1, HALO + 128, 3), 1.0);
    }

    #[test]
    fn decompose_invalid_buffer_size_returns_error() {
        let buffer = vec![0.0f32; 100]; // wrong size for any valid image
        let cache = TileCache::new(100_000_000);

        let result = decompose_image_to_tiles(&buffer, 256, 256, 0, &cache);
        assert!(result.is_err());
        match result.unwrap_err() {
            TileError::InvalidBufferSize { expected, actual } => {
                assert_eq!(expected, 256 * 256 * 4);
                assert_eq!(actual, 100);
            }
            _ => panic!("Expected InvalidBufferSize error"),
        }
    }

    #[test]
    fn decompose_zero_dimensions_returns_error() {
        let buffer: Vec<f32> = vec![];
        let cache = TileCache::new(100_000_000);

        let result = decompose_image_to_tiles(&buffer, 0, 256, 0, &cache);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), TileError::ZeroDimensions);
    }

    #[test]
    fn decompose_exact_multiple_of_tile_size() {
        // 512×512 = exactly 2×2 tiles, no edge padding needed
        let width = 512u32;
        let height = 512u32;
        let buffer = vec![0.25f32; (width * height * 4) as usize];
        let cache = TileCache::new(100_000_000);

        let grid = decompose_image_to_tiles(&buffer, width, height, 0, &cache).unwrap();

        assert_eq!(grid.cols, 2);
        assert_eq!(grid.rows, 2);
        assert_eq!(cache.entry_count(), 4);

        // All main region pixels should be 0.25
        let key = TileKey {
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 1,
            },
            stage: CacheStage::Raw,
        };
        let tile = cache.get_entry(key).unwrap();
        assert_eq!(tile.at(HALO + 128, HALO + 128, 0), 0.25);
    }

    #[test]
    fn decompose_1x1_image() {
        // Smallest valid image: 1×1 pixel
        let buffer = vec![0.9f32, 0.8, 0.7, 1.0];
        let cache = TileCache::new(100_000_000);

        let grid = decompose_image_to_tiles(&buffer, 1, 1, 5, &cache).unwrap();

        assert_eq!(grid.cols, 1);
        assert_eq!(grid.rows, 1);

        let key = TileKey {
            layer: 5,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let tile = cache.get_entry(key).unwrap();
        // The single pixel should be at main region origin (HALO, HALO)
        assert_eq!(tile.at(HALO, HALO, 0), 0.9);
        assert_eq!(tile.at(HALO, HALO, 1), 0.8);
        assert_eq!(tile.at(HALO, HALO, 2), 0.7);
        assert_eq!(tile.at(HALO, HALO, 3), 1.0);
        // Adjacent pixels should be zero
        assert_eq!(tile.at(HALO + 1, HALO, 0), 0.0);
    }
}
