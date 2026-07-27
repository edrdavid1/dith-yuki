//! Pixel tile storage and access.
//!
//! This module defines `PixelTile`, the core container for RGBA pixel data within the tile engine.
//! For architecture details, see `tile-engine-architecture.md` §2.1 (Pixel Tile).
//!
//! # Overview
//!
//! A `PixelTile` stores a (256 + 2×HALO)² array of RGBA pixels as 32-bit floats.
//! The halo region (2 pixels on each side) enables correct filter processing at tile boundaries.
//!
//! # Memory Layout
//!
//! Pixels are stored in row-major order (left to right, top to bottom):
//! - Size: (TILE_SIZE + 2×HALO)² = 260² = 67,600 pixels
//! - Channels per pixel: 4 (R, G, B, A)
//! - Total elements: 67,600 × 4 = 270,400 f32 values
//! - Total bytes: 270,400 × 4 = 1,081,600 bytes (~1.03 MB per tile)
//!
//! For a pixel at tile coordinate (x, y) with channel c:
//! - Linear index: `(y * size + x) * 4 + c`
//! - where `size = TILE_SIZE + 2*HALO = 260`

use crate::{HALO, TILE_SIZE};

/// Container for RGBA pixel data in a single tile.
///
/// Stores pixels as 32-bit floats in row-major order.
/// The tile includes a halo region (2 pixels on each side) to enable
/// correct filter application at boundaries.
///
/// # Examples
///
/// ```ignore
/// let tile = PixelTile::new();
/// let red_channel_value = tile.at(128, 128, 0);  // Red channel at center
/// 
/// let mut tile = PixelTile::new();
/// tile.set(10, 10, 0, 1.0);  // Set red to 1.0 at (10, 10)
/// ```
pub struct PixelTile {
    /// Flat array of RGBA pixel data in row-major order.
    /// Size: (TILE_SIZE + 2*HALO)² × 4 = 260² × 4 = 270,400 f32 values
    pub data: Box<[f32]>,
}

impl PixelTile {
    /// Create a new, zero-initialized pixel tile.
    ///
    /// Allocates (TILE_SIZE + 2×HALO)² × 4 f32 values on the heap.
    /// All pixels are initialized to 0.0.
    ///
    /// # Returns
    ///
    /// A new PixelTile with 260² × 4 = 270,400 zero-initialized f32 elements.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let tile = PixelTile::new();
    /// assert_eq!(tile.data.len(), 270_400);  // (260)² × 4
    /// ```
    pub fn new() -> Self {
        let size = (TILE_SIZE + 2 * HALO) as usize;
        Self {
            data: vec![0.0; size * size * 4].into_boxed_slice(),
        }
    }

    /// Retrieve the value of a single channel at a tile coordinate.
    ///
    /// # Arguments
    ///
    /// - `x`: Horizontal coordinate in tile space (0..260, including halo)
    /// - `y`: Vertical coordinate in tile space (0..260, including halo)
    /// - `channel`: Channel index (0=Red, 1=Green, 2=Blue, 3=Alpha)
    ///
    /// # Returns
    ///
    /// The f32 value at this coordinate and channel.
    ///
    /// # Notes
    ///
    /// The coordinate system includes the halo region:
    /// - Halo left/top: (0..HALO, 0..HALO)
    /// - Main tile: (HALO..(HALO+TILE_SIZE), HALO..(HALO+TILE_SIZE))
    /// - Halo right/bottom: ((HALO+TILE_SIZE)..260, (HALO+TILE_SIZE)..260)
    ///
    /// # Panics
    ///
    /// Panics if the calculated index is out of bounds (shouldn't happen with valid x, y, channel).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let tile = PixelTile::new();
    /// let val = tile.at(128, 128, 0);  // Get red channel at (128, 128)
    /// ```
    pub fn at(&self, x: u32, y: u32, channel: u32) -> f32 {
        let size = TILE_SIZE + 2 * HALO;
        let idx = ((y * size + x) * 4 + channel) as usize;
        self.data[idx]
    }

    /// Set the value of a single channel at a tile coordinate.
    ///
    /// # Arguments
    ///
    /// - `x`: Horizontal coordinate in tile space (0..260, including halo)
    /// - `y`: Vertical coordinate in tile space (0..260, including halo)
    /// - `channel`: Channel index (0=Red, 1=Green, 2=Blue, 3=Alpha)
    /// - `value`: The f32 value to store
    ///
    /// # Notes
    ///
    /// The coordinate system includes the halo region (see `at()` for details).
    /// Both main tile and halo regions are readable and writable.
    ///
    /// # Panics
    ///
    /// Panics if the calculated index is out of bounds.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let mut tile = PixelTile::new();
    /// tile.set(128, 128, 0, 1.0);  // Set red channel at (128, 128) to 1.0
    /// assert_eq!(tile.at(128, 128, 0), 1.0);
    /// ```
    pub fn set(&mut self, x: u32, y: u32, channel: u32, value: f32) {
        let size = TILE_SIZE + 2 * HALO;
        let idx = ((y * size + x) * 4 + channel) as usize;
        self.data[idx] = value;
    }
}

impl Default for PixelTile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_tile_new_allocates_correct_size() {
        let tile = PixelTile::new();
        let expected_size = (TILE_SIZE + 2 * HALO) as usize;
        let total_elements = expected_size * expected_size * 4;
        assert_eq!(tile.data.len(), total_elements);
        assert_eq!(tile.data.len(), 260 * 260 * 4);
        assert_eq!(tile.data.len(), 270_400);
    }

    #[test]
    fn pixel_tile_new_initializes_to_zero() {
        let tile = PixelTile::new();
        for &val in tile.data.iter() {
            assert_eq!(val, 0.0);
        }
    }

    #[test]
    fn at_and_set_round_trip() {
        let mut tile = PixelTile::new();

        // Set a value
        tile.set(10, 20, 0, 0.5);
        // Get it back
        assert_eq!(tile.at(10, 20, 0), 0.5);

        // Set another value
        tile.set(100, 100, 3, 1.0);
        assert_eq!(tile.at(100, 100, 3), 1.0);

        // Verify first value unchanged
        assert_eq!(tile.at(10, 20, 0), 0.5);
    }

    #[test]
    fn halo_region_is_accessible() {
        let mut tile = PixelTile::new();
        let size = (TILE_SIZE + 2 * HALO) as u32;

        // Test halo left/top (0..2, 0..2)
        tile.set(0, 0, 0, 0.1);
        assert_eq!(tile.at(0, 0, 0), 0.1);

        tile.set(1, 1, 1, 0.2);
        assert_eq!(tile.at(1, 1, 1), 0.2);

        // Test halo right/bottom (258..260, 258..260)
        tile.set(size - 1, size - 1, 2, 0.3);
        assert_eq!(tile.at(size - 1, size - 1, 2), 0.3);

        tile.set(size - 2, size - 2, 3, 0.4);
        assert_eq!(tile.at(size - 2, size - 2, 3), 0.4);
    }

    #[test]
    fn main_tile_region_is_accessible() {
        let mut tile = PixelTile::new();

        // Main tile region: (HALO..(HALO+TILE_SIZE), HALO..(HALO+TILE_SIZE))
        // Which is: (2..258, 2..258)
        tile.set(HALO, HALO, 0, 0.7);
        assert_eq!(tile.at(HALO, HALO, 0), 0.7);

        tile.set(HALO + TILE_SIZE - 1, HALO + TILE_SIZE - 1, 3, 0.9);
        assert_eq!(tile.at(HALO + TILE_SIZE - 1, HALO + TILE_SIZE - 1, 3), 0.9);
    }

    #[test]
    fn channels_are_independent() {
        let mut tile = PixelTile::new();

        // Set all 4 channels at same coordinate
        tile.set(50, 60, 0, 0.1); // Red
        tile.set(50, 60, 1, 0.2); // Green
        tile.set(50, 60, 2, 0.3); // Blue
        tile.set(50, 60, 3, 0.4); // Alpha

        assert_eq!(tile.at(50, 60, 0), 0.1);
        assert_eq!(tile.at(50, 60, 1), 0.2);
        assert_eq!(tile.at(50, 60, 2), 0.3);
        assert_eq!(tile.at(50, 60, 3), 0.4);
    }

    #[test]
    fn default_creates_zero_tile() {
        let tile = PixelTile::default();
        assert_eq!(tile.data.len(), 270_400);
        assert_eq!(tile.at(100, 100, 0), 0.0);
    }
}
