//! Levels filter implementation.
//!
//! Histogram adjustment with gamma correction.

use crate::error::EngineError;
use engine_tiles::PixelTile;
use serde::{Deserialize, Serialize};

/// Levels filter for histogram adjustment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelsFilter {
    /// Input black point (default 0.0)
    pub input_black: f32,
    /// Input white point (default 1.0)
    pub input_white: f32,
    /// Gamma correction (default 1.0)
    pub gamma: f32,
    /// Output black point (default 0.0)
    pub output_black: f32,
    /// Output white point (default 1.0)
    pub output_white: f32,
}

impl LevelsFilter {
    /// Create a new levels filter with default (no-op) values.
    pub fn new() -> Self {
        LevelsFilter {
            input_black: 0.0,
            input_white: 1.0,
            gamma: 1.0,
            output_black: 0.0,
            output_white: 1.0,
        }
    }

    /// Apply levels transformation to a single pixel value.
    pub fn apply_to_value(&self, pixel: f32) -> f32 {
        // 1. Remap input [input_black, input_white] → [0, 1]
        let input_range = self.input_white - self.input_black;
        if input_range.abs() < 0.001 {
            return self.output_black; // Degenerate case
        }

        let remapped = (pixel - self.input_black) / input_range;
        let remapped = remapped.clamp(0.0, 1.0);

        // 2. Apply gamma correction
        let gamma_corrected = if self.gamma.abs() < 0.001 {
            remapped
        } else {
            remapped.powf(1.0 / self.gamma)
        };

        // 3. Remap output [0, 1] → [output_black, output_white]
        let output = self.output_black + gamma_corrected * (self.output_white - self.output_black);
        output.clamp(0.0, 1.0)
    }

    /// Apply the levels filter to a tile.
    pub fn apply_to_tile(&self, tile: &PixelTile) -> Result<PixelTile, EngineError> {
        let mut result = PixelTile::new();

        // Iterate over all pixels in the tile (256+4 for halo)
        for y in 0u32..260 {
            for x in 0u32..260 {
                // Apply to RGB channels
                for c in 0..3 {
                    let val = tile.at(x, y, c);
                    let adjusted = self.apply_to_value(val);
                    result.set(x, y, c, adjusted);
                }
                // Copy alpha channel
                result.set(x, y, 3, tile.at(x, y, 3));
            }
        }

        Ok(result)
    }
}

impl Default for LevelsFilter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_levels() {
        let levels = LevelsFilter::new();
        assert!((levels.apply_to_value(0.0) - 0.0).abs() < 0.001);
        assert!((levels.apply_to_value(0.5) - 0.5).abs() < 0.001);
        assert!((levels.apply_to_value(1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn input_remapping() {
        let mut levels = LevelsFilter::new();
        levels.input_black = 0.2;
        levels.input_white = 0.8;

        // 0.2 should map to 0.0
        assert!((levels.apply_to_value(0.2) - 0.0).abs() < 0.01);
        // 0.5 should map to 0.5 (middle)
        assert!((levels.apply_to_value(0.5) - 0.5).abs() < 0.01);
        // 0.8 should map to 1.0
        assert!((levels.apply_to_value(0.8) - 1.0).abs() < 0.01);
    }

    #[test]
    fn gamma_brightening() {
        let mut levels = LevelsFilter::new();
        levels.gamma = 2.0; // Brighten

        // Gamma 2.0 should brighten mid-tones
        assert!(levels.apply_to_value(0.5) > 0.5);
    }

    #[test]
    fn gamma_darkening() {
        let mut levels = LevelsFilter::new();
        levels.gamma = 0.5; // Darken

        // Gamma 0.5 should darken mid-tones
        assert!(levels.apply_to_value(0.5) < 0.5);
    }

    #[test]
    fn output_remapping() {
        let mut levels = LevelsFilter::new();
        levels.output_black = 0.1;
        levels.output_white = 0.9;

        // 0.0 should map to 0.1
        assert!((levels.apply_to_value(0.0) - 0.1).abs() < 0.01);
        // 1.0 should map to 0.9
        assert!((levels.apply_to_value(1.0) - 0.9).abs() < 0.01);
    }

    #[test]
    fn clamping() {
        let levels = LevelsFilter::new();
        assert!(levels.apply_to_value(-0.5) >= 0.0);
        assert!(levels.apply_to_value(1.5) <= 1.0);
    }
}
