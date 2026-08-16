//! Tile pyramid structure and downsampling.
//!
//! This module provides lazy pyramid generation for the tile engine.
//! For architecture details, see `tile-engine-architecture.md` §2.2 (Pyramid Downsampling).
//!
//! # Overview
//!
//! The pyramid system enables efficient processing of large images by generating coarser
//! levels on demand. Each level is a 1:2 box-filtered downsample of the previous level:
//! - Level 0: Full resolution (256 × 256 main pixels + halo)
//! - Level 1: 1:2 downsampled (128 × 128 main pixels + halo)
//! - Level 2: 1:4 downsampled (64 × 64 main pixels + halo)
//! - etc.
//!
//! Coarser levels enable fast preview rendering and instant user feedback at zoom-out levels.
//!
//! # Downsampling Algorithm
//!
//! The `downsample_tile` function implements a simple 1:2 box filter:
//! - For each output pixel (x, y), average its 2×2 neighborhood from the input
//! - Applied per-channel (RGBA)
//! - Deterministic and reproducible

use crate::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};
use std::sync::Arc;

/// Maximum pyramid level for a document: `floor(log2(max_dim / TILE_SIZE))`.
///
/// Level 0 is full resolution. At level L each tile covers `TILE_SIZE * 2^L`
/// document pixels. Returns 0 when the image fits in a single tile.
pub fn max_pyramid_level(doc_width: u32, doc_height: u32) -> u8 {
    let max_dim = doc_width.max(doc_height);
    if max_dim <= TILE_SIZE {
        return 0;
    }
    let ratio = max_dim as f64 / TILE_SIZE as f64;
    ratio.log2().floor().max(0.0) as u8
}

/// Tile grid size (columns, rows) at a pyramid level.
pub fn tile_grid_at_level(doc_width: u32, doc_height: u32, level: u8) -> (u32, u32) {
    let scale = 1u32 << level;
    let tile_px = TILE_SIZE * scale;
    let cols = (doc_width + tile_px - 1) / tile_px;
    let rows = (doc_height + tile_px - 1) / tile_px;
    (cols, rows)
}

/// Build Raw-stage pyramid levels 1..=max from already-cached level-0 Raw tiles.
///
/// Used by tests and optional callers. Production preview does **not** filter
/// these tiles — zoom-out display downsamples Composite L0 instead.
pub fn build_raw_pyramid(layer_id: u32, width: u32, height: u32, cache: &TileCache) {
    let max_level = max_pyramid_level(width, height);
    for level in 1..=max_level {
        let (cols, rows) = tile_grid_at_level(width, height, level);
        for y in 0..rows {
            for x in 0..cols {
                let coord = TileCoord { level, x, y };
                if let Some(tile) = generate_pyramid_tile(level, coord, layer_id, CacheStage::Raw, cache)
                {
                    cache.insert_fresh(
                        TileKey {
                            layer: layer_id,
                            coord,
                            stage: CacheStage::Raw,
                        },
                        Arc::new(tile),
                    );
                }
            }
        }
    }
}

/// Downsample a tile by 1:2 using box filtering.
///
/// Creates a new child tile with half the resolution of the parent.
/// Each output pixel is the average of a 2×2 neighborhood from the parent tile.
///
/// # Algorithm
///
/// For each output pixel (x, y) in the child tile main region (0..TILE_SIZE/2),
/// we compute a box-filtered average of 4 pixels from the parent:
/// - Parent main region ranges from HALO to HALO+TILE_SIZE-1 per dimension (256 pixels)
/// - For child output (x, y), we sample parent at 2×2: (2x, 2y), (2x+1, 2y), (2x, 2y+1), (2x+1, 2y+1)
/// - All 4 RGBA channels are processed identically
/// - Average: (p00 + p10 + p01 + p11) × 0.25
/// - Result is placed at (x + HALO, y + HALO, c) in the child tile
///
/// # Output Size
///
/// - Main tile region: (TILE_SIZE/2) × (TILE_SIZE/2) pixels (128 × 128)
/// - With halo: (TILE_SIZE + 2×HALO)² = 260² = 67,600 pixels (same as input for memory efficiency)
/// - Total: 67,600 × 4 channels = 270,400 f32 values
/// - Total bytes: 1,081,600 bytes (~1.03 MB)
///
/// # Boundary Handling
///
/// The parent tile's main region (HALO..HALO+TILE_SIZE, HALO..HALO+TILE_SIZE) is assumed to
/// contain properly populated neighbor data. The downsampling only accesses coordinates
/// within (HALO..HALO+TILE_SIZE-1) of the parent, which is always safe.
///
/// # Arguments
///
/// * `parent` - Reference to the parent tile at the finer resolution
///
/// # Returns
///
/// A new `PixelTile` containing the downsampled result at half resolution.
///
/// # Examples
///
/// ```ignore
/// let parent = PixelTile::new();
/// // ... populate parent with pixels ...
/// let child = downsample_tile(&parent);
/// // child now contains 1:2 box-filtered downsample
/// ```
pub fn downsample_tile(parent: &PixelTile) -> PixelTile {
    let mut child = PixelTile::new();

    // Output resolution is half of input: TILE_SIZE/2 × TILE_SIZE/2
    let output_size = TILE_SIZE / 2;

    // Process the main tile region: output_size × output_size output pixels
    for y in 0..output_size {
        for x in 0..output_size {
            for c in 0..4 {
                // Parent coordinates for 2×2 neighborhood
                // Since both parent and child use the same absolute coordinate system (with halo),
                // we read from parent's main region (HALO..HALO+TILE_SIZE)
                let p00 = parent.at(2 * x + HALO, 2 * y + HALO, c);
                let p10 = parent.at(2 * x + 1 + HALO, 2 * y + HALO, c);
                let p01 = parent.at(2 * x + HALO, 2 * y + 1 + HALO, c);
                let p11 = parent.at(2 * x + 1 + HALO, 2 * y + 1 + HALO, c);

                // Box filter: average of 2×2 neighborhood
                let avg = (p00 + p10 + p01 + p11) * 0.25;

                // Write to child at halo-offset position
                child.set(x + HALO, y + HALO, c, avg);
            }
        }
    }

    child
}

/// Generate a pyramid tile at level N by downsampling 4 child tiles at level N-1.
///
/// Each pixel in the output tile is the average of a 2×2 block of pixels from
/// the corresponding region in the child tiles at level N-1.
///
/// A tile at `(level, x, y)` is generated from 4 children at level N-1:
/// - Top-left:     `(level-1, 2x,   2y)`
/// - Top-right:    `(level-1, 2x+1, 2y)`
/// - Bottom-left:  `(level-1, 2x,   2y+1)`
/// - Bottom-right: `(level-1, 2x+1, 2y+1)`
///
/// Returns `None` if `level == 0` (level 0 tiles are source tiles) or if any
/// required child tile is missing from the cache.
pub fn generate_pyramid_tile(
    level: u8,
    coord: TileCoord,
    layer: u32,
    stage: CacheStage,
    cache: &TileCache,
) -> Option<PixelTile> {
    if level == 0 {
        // Level 0 tiles are source tiles, not generated
        return None;
    }

    // A tile at (level, x, y) is generated from 4 tiles at (level-1, 2x, 2y), etc.
    let child_level = level - 1;
    let children = [
        TileCoord { level: child_level, x: coord.x * 2,     y: coord.y * 2 },
        TileCoord { level: child_level, x: coord.x * 2 + 1, y: coord.y * 2 },
        TileCoord { level: child_level, x: coord.x * 2,     y: coord.y * 2 + 1 },
        TileCoord { level: child_level, x: coord.x * 2 + 1, y: coord.y * 2 + 1 },
    ];

    // Fetch all 4 child tiles from cache
    let child_tiles: Vec<_> = children.iter().map(|c| {
        let key = TileKey { layer, coord: *c, stage };
        cache.get_entry(key)
    }).collect();

    // At least one child must be present to produce a meaningful result.
    // If no children are cached yet, return None so the caller can fall through
    // to computing level 0 composites first.
    let children_found = child_tiles.iter().filter(|t| t.is_some()).count();
    if children_found == 0 {
        return None;
    }

    let mut result = PixelTile::new();

    // For each pixel in the output tile (main region only: HALO..HALO+TILE_SIZE)
    for out_y in HALO..(HALO + TILE_SIZE) {
        for out_x in HALO..(HALO + TILE_SIZE) {
            // This output pixel corresponds to a 2x2 block in the source
            let src_local_x = (out_x - HALO) * 2;
            let src_local_y = (out_y - HALO) * 2;

            // Average 2x2 block from the appropriate child tiles per channel
            for c in 0..4u32 {
                let mut sum = 0.0f32;
                let mut count = 0u32;

                for dy in 0..2u32 {
                    for dx in 0..2u32 {
                        let px = src_local_x + dx;
                        let py = src_local_y + dy;
                        // Which child tile? (0=TL, 1=TR, 2=BL, 3=BR)
                        let child_idx = (py / TILE_SIZE) * 2 + (px / TILE_SIZE);
                        let local_x = (px % TILE_SIZE) + HALO;
                        let local_y = (py % TILE_SIZE) + HALO;

                        if let Some(ref tile) = child_tiles[child_idx as usize] {
                            sum += tile.at(local_x, local_y, c);
                            count += 1;
                        }
                    }
                }

                if count > 0 {
                    result.set(out_x, out_y, c, sum / count as f32);
                }
            }
        }
    }

    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsample_output_size_is_correct() {
        let parent = PixelTile::new();
        let child = downsample_tile(&parent);

        // Output should be same size as any PixelTile: (260)² × 4 elements
        // (same total size because we store 128×128 logical pixels in a 260×260 allocation)
        let expected_size = (TILE_SIZE + 2 * HALO) as usize;
        let total_elements = expected_size * expected_size * 4;
        assert_eq!(child.data.len(), 270_400);
        assert_eq!(child.data.len(), total_elements);
    }

    #[test]
    fn downsample_uniform_color() {
        // Create parent with uniform color
        let mut parent = PixelTile::new();
        let color = 0.8f32;

        // Fill parent main region with uniform color
        // Main region is (HALO..HALO+TILE_SIZE, HALO..HALO+TILE_SIZE)
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..4 {
                    parent.set(x, y, c, color);
                }
            }
        }

        let child = downsample_tile(&parent);

        // Child main region is (HALO..HALO+TILE_SIZE/2, HALO..HALO+TILE_SIZE/2)
        // and should have uniform color
        let output_size = TILE_SIZE / 2;
        for y in HALO..(HALO + output_size) {
            for x in HALO..(HALO + output_size) {
                for c in 0..4 {
                    let val = child.at(x, y, c);
                    // Average of same values is the same value
                    assert!((val - color).abs() < 1e-6, "Expected {}, got {}", color, val);
                }
            }
        }
    }

    #[test]
    fn downsample_known_pattern() {
        let mut parent = PixelTile::new();

        // Create a simple pattern: 2×2 blocks of alternating values
        // Fill parent main region with pattern
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                let val = if ((x - HALO) + (y - HALO)) % 2 == 0 { 1.0 } else { 2.0 };
                for c in 0..4 {
                    parent.set(x, y, c, val);
                }
            }
        }

        let child = downsample_tile(&parent);

        // After downsampling, each 2×2 block of [1.0, 2.0; 2.0, 1.0] becomes (1+2+2+1)/4 = 1.5
        let output_size = TILE_SIZE / 2;
        for y in HALO..(HALO + output_size) {
            for x in HALO..(HALO + output_size) {
                for c in 0..4 {
                    let val = child.at(x, y, c);
                    assert!((val - 1.5).abs() < 1e-6, "Expected 1.5, got {}", val);
                }
            }
        }
    }

    #[test]
    fn downsample_preserves_channel_independence() {
        let mut parent = PixelTile::new();

        // Create a pattern with different values per channel
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                parent.set(x, y, 0, 0.2); // Red
                parent.set(x, y, 1, 0.4); // Green
                parent.set(x, y, 2, 0.6); // Blue
                parent.set(x, y, 3, 0.8); // Alpha
            }
        }

        let child = downsample_tile(&parent);

        // Each channel should maintain its own value
        let output_size = TILE_SIZE / 2;
        for y in HALO..(HALO + output_size) {
            for x in HALO..(HALO + output_size) {
                assert!((child.at(x, y, 0) - 0.2).abs() < 1e-6);
                assert!((child.at(x, y, 1) - 0.4).abs() < 1e-6);
                assert!((child.at(x, y, 2) - 0.6).abs() < 1e-6);
                assert!((child.at(x, y, 3) - 0.8).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn downsample_specific_values() {
        let mut parent = PixelTile::new();

        // Set specific 2×2 blocks in parent and verify downsampling
        // Block at (HALO, HALO) in parent should downsample to (HALO, HALO) in child
        parent.set(HALO, HALO, 0, 0.0);     // p00
        parent.set(HALO + 1, HALO, 0, 1.0);     // p10
        parent.set(HALO, HALO + 1, 0, 2.0);     // p01
        parent.set(HALO + 1, HALO + 1, 0, 3.0); // p11

        let child = downsample_tile(&parent);

        // Child at (HALO, HALO) should be average of above: (0 + 1 + 2 + 3) / 4 = 1.5
        assert!((child.at(HALO, HALO, 0) - 1.5).abs() < 1e-6);
    }

    // --- generate_pyramid_tile tests ---

    #[test]
    fn generate_pyramid_tile_returns_none_for_level_0() {
        let cache = TileCache::new(100_000_000);
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let result = generate_pyramid_tile(0, coord, 0, CacheStage::Composite, &cache);
        assert!(result.is_none());
    }

    #[test]
    fn generate_pyramid_tile_returns_some_when_all_children_present() {
        use std::sync::Arc;

        let cache = TileCache::new(100_000_000);
        let child_level = 0u8;

        // Insert 4 child tiles with uniform color into cache
        let color = 0.5f32;
        for cy in 0..2u32 {
            for cx in 0..2u32 {
                let mut tile = PixelTile::new();
                for y in HALO..(HALO + TILE_SIZE) {
                    for x in HALO..(HALO + TILE_SIZE) {
                        for c in 0..4 {
                            tile.set(x, y, c, color);
                        }
                    }
                }
                let key = TileKey {
                    layer: 0,
                    coord: TileCoord { level: child_level, x: cx, y: cy },
                    stage: CacheStage::Composite,
                };
                cache.insert_fresh(key, Arc::new(tile));
            }
        }

        // Generate pyramid tile at level 1, coord (0,0)
        let coord = TileCoord { level: 1, x: 0, y: 0 };
        let result = generate_pyramid_tile(1, coord, 0, CacheStage::Composite, &cache);
        assert!(result.is_some());

        let tile = result.unwrap();
        // Since all children are uniform, output should also be uniform
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..4 {
                    let val = tile.at(x, y, c);
                    assert!(
                        (val - color).abs() < 1e-6,
                        "At ({}, {}, {}): expected {}, got {}",
                        x, y, c, color, val
                    );
                }
            }
        }
    }

    #[test]
    fn generate_pyramid_tile_handles_missing_children_gracefully() {
        use std::sync::Arc;

        let cache = TileCache::new(100_000_000);

        // Insert only 2 of 4 child tiles (top-left and top-right)
        let color = 0.8f32;
        for cx in 0..2u32 {
            let mut tile = PixelTile::new();
            for y in HALO..(HALO + TILE_SIZE) {
                for x in HALO..(HALO + TILE_SIZE) {
                    for c in 0..4 {
                        tile.set(x, y, c, color);
                    }
                }
            }
            let key = TileKey {
                layer: 0,
                coord: TileCoord { level: 0, x: cx, y: 0 },
                stage: CacheStage::Composite,
            };
            cache.insert_fresh(key, Arc::new(tile));
        }

        // Generate pyramid tile — missing bottom children
        let coord = TileCoord { level: 1, x: 0, y: 0 };
        let result = generate_pyramid_tile(1, coord, 0, CacheStage::Composite, &cache);

        // Should still return Some since we handle missing children gracefully
        // (averaging only present children)
        assert!(result.is_some());
    }

    #[test]
    fn generate_pyramid_tile_averages_correctly_across_children() {
        use std::sync::Arc;

        let cache = TileCache::new(100_000_000);

        // Create 4 child tiles with distinct uniform values
        let values = [0.1f32, 0.2, 0.3, 0.4];
        let coords = [
            TileCoord { level: 0, x: 0, y: 0 }, // TL
            TileCoord { level: 0, x: 1, y: 0 }, // TR
            TileCoord { level: 0, x: 0, y: 1 }, // BL
            TileCoord { level: 0, x: 1, y: 1 }, // BR
        ];

        for (i, coord) in coords.iter().enumerate() {
            let mut tile = PixelTile::new();
            for y in HALO..(HALO + TILE_SIZE) {
                for x in HALO..(HALO + TILE_SIZE) {
                    for c in 0..4 {
                        tile.set(x, y, c, values[i]);
                    }
                }
            }
            let key = TileKey { layer: 0, coord: *coord, stage: CacheStage::Composite };
            cache.insert_fresh(key, Arc::new(tile));
        }

        // Generate level 1 tile at (0,0)
        let coord = TileCoord { level: 1, x: 0, y: 0 };
        let result = generate_pyramid_tile(1, coord, 0, CacheStage::Composite, &cache);
        assert!(result.is_some());
        let tile = result.unwrap();

        // The first output pixel at (HALO, HALO) reads from the top-left child's first 2x2 block.
        // Since TL is uniform 0.1, the average of 4 identical values is still 0.1.
        let val = tile.at(HALO, HALO, 0);
        assert!((val - 0.1).abs() < 1e-6, "Expected 0.1, got {}", val);

        // Pixel at (HALO + 128, HALO) should read from the top-right child (uniform 0.2)
        let val = tile.at(HALO + 128, HALO, 0);
        assert!((val - 0.2).abs() < 1e-6, "Expected 0.2, got {}", val);

        // Pixel at (HALO, HALO + 128) should read from the bottom-left child (uniform 0.3)
        let val = tile.at(HALO, HALO + 128, 0);
        assert!((val - 0.3).abs() < 1e-6, "Expected 0.3, got {}", val);

        // Pixel at (HALO + 128, HALO + 128) should read from bottom-right (uniform 0.4)
        let val = tile.at(HALO + 128, HALO + 128, 0);
        assert!((val - 0.4).abs() < 1e-6, "Expected 0.4, got {}", val);
    }

    #[test]
    fn max_pyramid_level_matches_document_size() {
        assert_eq!(max_pyramid_level(256, 256), 0);
        assert_eq!(max_pyramid_level(300, 300), 0);
        assert_eq!(max_pyramid_level(512, 512), 1);
        assert_eq!(max_pyramid_level(1024, 1024), 2);
        assert_eq!(max_pyramid_level(3000, 3000), 3);
        assert_eq!(max_pyramid_level(8192, 8192), 5);
    }

    #[test]
    fn tile_grid_at_level_3000() {
        assert_eq!(tile_grid_at_level(3000, 3000, 0), (12, 12));
        assert_eq!(tile_grid_at_level(3000, 3000, 1), (6, 6));
        assert_eq!(tile_grid_at_level(3000, 3000, 2), (3, 3));
        assert_eq!(tile_grid_at_level(3000, 3000, 3), (2, 2));
    }

    #[test]
    fn build_raw_pyramid_inserts_coarser_levels() {
        use crate::decompose::decompose_image_to_tiles;

        let width = 512u32;
        let height = 512u32;
        let buffer = vec![0.4f32; (width * height * 4) as usize];
        let cache = TileCache::new(100_000_000);
        decompose_image_to_tiles(&buffer, width, height, 7, &cache).unwrap();
        crate::pyramid::build_raw_pyramid(7, width, height, &cache);

        let l1 = cache.get_entry(TileKey {
            layer: 7,
            coord: TileCoord { level: 1, x: 0, y: 0 },
            stage: CacheStage::Raw,
        });
        assert!(l1.is_some(), "level-1 raw must exist after decompose");
        let tile = l1.unwrap();
        assert!((tile.at(HALO, HALO, 0) - 0.4).abs() < 1e-5);
        assert_eq!(cache.entry_count(), 5);
    }
}

