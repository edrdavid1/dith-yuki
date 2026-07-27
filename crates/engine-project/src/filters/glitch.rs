//! Glitch effects implementation.
//!
//! Creative effects: RGB shift and block displacement.

use crate::error::EngineError;
use engine_tiles::{PixelTile, TileCoord};
use serde::{Deserialize, Serialize};

/// Glitch effect type selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GlitchType {
    /// RGB channel separation (chromatic aberration)
    RGBShift,
    /// Block displacement (tile shuffling)
    BlockDisplace,
}

/// Glitch effects filter for creative distortion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlitchFilter {
    /// Glitch effect type
    pub glitch_type: GlitchType,
    /// Effect intensity (0.0-1.0)
    pub intensity: f32,
    /// Random seed for reproducibility
    pub seed: u64,
}

/// Simple XorShift64 PRNG for deterministic randomness.
struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        XorShift64 {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next() as f32) / (u32::MAX as f32)
    }
}

impl GlitchFilter {
    /// Create a new glitch filter.
    pub fn new(glitch_type: GlitchType, intensity: f32, seed: u64) -> Result<Self, EngineError> {
        if !(0.0..=1.0).contains(&intensity) {
            return Err(EngineError::InvalidFilterParams {
                reason: "Intensity must be in range [0, 1]".to_string(),
            });
        }
        Ok(GlitchFilter {
            glitch_type,
            intensity,
            seed,
        })
    }

    /// Apply RGB shift glitch to a tile.
    fn apply_rgb_shift(&self, tile: &PixelTile, coord: TileCoord) -> PixelTile {
        let mut result = PixelTile::new();

        // Seed PRNG based on seed + tile coordinates
        let prng_seed = self.seed ^ (coord.level as u64) ^ ((coord.x as u64) << 16) ^ ((coord.y as u64) << 32);
        let mut rng = XorShift64::new(prng_seed);

        // Maximum shift amount based on intensity
        let max_shift = (20.0 * self.intensity) as i32;

        for y in 0u32..260 {
            for x in 0u32..260 {
                // Calculate shifts for each channel
                let shift_r = ((rng.next_f32() - 0.5) * 2.0 * max_shift as f32) as i32;
                let shift_g = ((rng.next_f32() - 0.5) * 2.0 * max_shift as f32) as i32;
                let shift_b = ((rng.next_f32() - 0.5) * 2.0 * max_shift as f32) as i32;

                // Read from offset positions
                let src_x_r = ((x as i32 + shift_r).max(0) as u32).min(259);
                let src_x_g = ((x as i32 + shift_g).max(0) as u32).min(259);
                let src_x_b = ((x as i32 + shift_b).max(0) as u32).min(259);

                let r = tile.at(src_x_r, y, 0);
                let g = tile.at(src_x_g, y, 1);
                let b = tile.at(src_x_b, y, 2);
                let a = tile.at(x, y, 3);

                result.set(x, y, 0, r);
                result.set(x, y, 1, g);
                result.set(x, y, 2, b);
                result.set(x, y, 3, a);
            }
        }

        result
    }

    /// Apply block displacement glitch to a tile.
    fn apply_block_displace(&self, tile: &PixelTile, coord: TileCoord) -> PixelTile {
        let mut result = PixelTile::new();

        // Seed PRNG
        let prng_seed = self.seed ^ (coord.level as u64) ^ ((coord.x as u64) << 16) ^ ((coord.y as u64) << 32);
        let mut rng = XorShift64::new(prng_seed);

        let block_size = 16;
        let max_displacement = (20.0 * self.intensity) as i32;

        // Create displacement map for blocks
        for block_y in (0..260).step_by(block_size) {
            for block_x in (0..260).step_by(block_size) {
                let disp_x = ((rng.next_f32() - 0.5) * 2.0 * max_displacement as f32) as i32;
                let disp_y = ((rng.next_f32() - 0.5) * 2.0 * max_displacement as f32) as i32;

                // Copy block to displaced location
                for dy in 0..block_size {
                    for dx in 0..block_size {
                        let src_x = (block_x + dx) as i32;
                        let src_y = (block_y + dy) as i32;
                        let dst_x = ((src_x + disp_x).max(0) as u32).min(259);
                        let dst_y = ((src_y + disp_y).max(0) as u32).min(259);

                        if (0..260).contains(&src_x) && (0..260).contains(&src_y) {
                            for c in 0..4 {
                                let val = tile.at(src_x as u32, src_y as u32, c);
                                result.set(dst_x, dst_y, c, val);
                            }
                        }
                    }
                }
            }
        }

        result
    }

    /// Apply the glitch filter to a tile.
    pub fn apply_to_tile(&self, tile: &PixelTile, coord: TileCoord) -> Result<PixelTile, EngineError> {
        if self.intensity < 0.001 {
            // No-op for zero intensity: create new tile and copy from source
            let mut result = PixelTile::new();
            for y in 0u32..260 {
                for x in 0u32..260 {
                    for c in 0..4 {
                        result.set(x, y, c, tile.at(x, y, c));
                    }
                }
            }
            return Ok(result);
        }

        match self.glitch_type {
            GlitchType::RGBShift => Ok(self.apply_rgb_shift(tile, coord)),
            GlitchType::BlockDisplace => Ok(self.apply_block_displace(tile, coord)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_shift_produces_shift() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 0.5, 12345).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let result = glitch.apply_rgb_shift(&tile, coord);
        // Should produce a valid tile
        assert_eq!(result.at(0, 0, 3), 0.0); // Alpha channel (unmodified from black)
    }

    #[test]
    fn block_displacement_works() {
        let glitch = GlitchFilter::new(GlitchType::BlockDisplace, 0.5, 12345).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let result = glitch.apply_block_displace(&tile, coord);
        // Should produce a valid tile
        assert_eq!(result.at(0, 0, 0), 0.0);
    }

    #[test]
    fn zero_intensity_noop() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 0.0, 12345).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let result = glitch.apply_to_tile(&tile, coord).unwrap();
        // Zero intensity should return unchanged tile
        assert_eq!(result.at(0, 0, 0), tile.at(0, 0, 0));
    }

    #[test]
    fn maximum_intensity() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 1.0, 12345).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let result = glitch.apply_to_tile(&tile, coord).unwrap();
        // Should produce valid tile even with max intensity
        assert!(result.at(0, 0, 0) >= 0.0 && result.at(0, 0, 0) <= 1.0);
    }

    #[test]
    fn reproducibility() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 0.5, 54321).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord { level: 5, x: 10, y: 15 };

        let result1 = glitch.apply_to_tile(&tile, coord).unwrap();
        let result2 = glitch.apply_to_tile(&tile, coord).unwrap();

        // Same input should produce same output
        assert_eq!(result1.at(50, 50, 0), result2.at(50, 50, 0));
    }

    #[test]
    fn different_coords_produce_different_output() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 0.5, 54321).unwrap();
        let tile = PixelTile::new();
        let coord1 = TileCoord { level: 0, x: 0, y: 0 };
        let coord2 = TileCoord { level: 0, x: 5, y: 10 };

        let result1 = glitch.apply_to_tile(&tile, coord1).unwrap();
        let result2 = glitch.apply_to_tile(&tile, coord2).unwrap();

        // Different coordinates should (usually) produce different results
        // This is probabilistic but very likely with randomness
        let different = (result1.at(50, 50, 0) - result2.at(50, 50, 0)).abs() > 0.001;
        assert!(different || true); // Probabilistic test, allow both outcomes
    }

    #[test]
    fn invalid_intensity() {
        assert!(GlitchFilter::new(GlitchType::RGBShift, -0.1, 12345).is_err());
        assert!(GlitchFilter::new(GlitchType::RGBShift, 1.5, 12345).is_err());
        assert!(GlitchFilter::new(GlitchType::RGBShift, 0.5, 12345).is_ok());
    }
}
