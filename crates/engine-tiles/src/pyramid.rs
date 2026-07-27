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

use crate::{PixelTile, HALO, TILE_SIZE};

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
}

