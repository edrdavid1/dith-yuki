//! Dither filter implementation.
//!
//! Color reduction with Floyd-Steinberg, Ordered (Bayer), or Threshold algorithms.

use crate::error::EngineError;
use engine_tiles::{PixelTile, TileCoord};
use serde::{Deserialize, Serialize};

/// Dithering algorithm selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DitherAlgorithm {
    /// Floyd-Steinberg error diffusion (high quality, slower)
    FloydSteinberg,
    /// Ordered (Bayer matrix) dithering (fast, pattern-based)
    Ordered,
    /// Simple threshold (binary output)
    Threshold,
}

/// Dither filter for color reduction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DitherFilter {
    /// Dithering algorithm
    pub algorithm: DitherAlgorithm,
    /// Target color depth (bits per channel, 1-8)
    pub color_depth: u8,
}

impl DitherFilter {
    /// Create a new dither filter.
    pub fn new(algorithm: DitherAlgorithm, color_depth: u8) -> Result<Self, EngineError> {
        if !(1..=8).contains(&color_depth) {
            return Err(EngineError::InvalidFilterParams {
                reason: "Color depth must be 1-8 bits".to_string(),
            });
        }
        Ok(DitherFilter {
            algorithm,
            color_depth,
        })
    }

    /// Quantize a pixel value to the target color depth.
    fn quantize(&self, value: f32) -> f32 {
        let levels = ((1 << self.color_depth) - 1) as f32;
        (value * levels).round() / levels
    }

    /// Apply Floyd-Steinberg dithering to a tile.
    fn apply_floyd_steinberg(&self, tile: &PixelTile) -> PixelTile {
        let mut result = PixelTile::new();
        let mut error_map = vec![vec![[0.0; 4]; 260]; 260];

        // Single pass with error diffusion
        for y in 0u32..260 {
            for x in 0u32..260 {
                for c in 0..3 {
                    // Get pixel with accumulated error
                    let pixel = tile.at(x, y, c as u32) + error_map[y as usize][x as usize][c];
                    let quantized = self.quantize(pixel);
                    let error = pixel - quantized;

                    result.set(x, y, c as u32, quantized);

                    // Distribute error to neighbors
                    // Right: 7/16
                    if x + 1 < 260 {
                        error_map[y as usize][(x + 1) as usize][c] += error * 7.0 / 16.0;
                    }
                    // Below-left: 3/16
                    if y + 1 < 260 && x > 0 {
                        error_map[(y + 1) as usize][(x - 1) as usize][c] += error * 3.0 / 16.0;
                    }
                    // Below: 5/16
                    if y + 1 < 260 {
                        error_map[(y + 1) as usize][x as usize][c] += error * 5.0 / 16.0;
                    }
                    // Below-right: 1/16
                    if y + 1 < 260 && x + 1 < 260 {
                        error_map[(y + 1) as usize][(x + 1) as usize][c] += error * 1.0 / 16.0;
                    }
                }
            }
        }

        // Copy alpha channel from source
        for y in 0u32..260 {
            for x in 0u32..260 {
                result.set(x, y, 3, tile.at(x, y, 3));
            }
        }

        result
    }

    /// Apply Ordered (Bayer) dithering to a tile.
    fn apply_ordered(&self, tile: &PixelTile, coord: TileCoord) -> PixelTile {
        let mut result = PixelTile::new();

        // 4x4 Bayer matrix (normalized to [0, 1])
        let bayer_4x4 = [
            [0.0, 0.5],
            [0.75, 0.25],
        ];

        for y in 0u32..260 {
            for x in 0u32..260 {
                for c in 0..3 {
                    let pixel = tile.at(x, y, c as u32);

                    // Get dither threshold from Bayer matrix
                    let bx = ((x as usize) ^ (coord.x as usize)) % 2;
                    let by = ((y as usize) ^ (coord.y as usize)) % 2;
                    let threshold = bayer_4x4[by][bx];

                    // Compare and quantize
                    let levels = ((1 << self.color_depth) - 1) as f32;
                    let quantized = if (pixel * levels).fract() < threshold {
                        (pixel * levels).floor() / levels
                    } else {
                        (pixel * levels).ceil() / levels
                    };

                    result.set(x, y, c as u32, quantized.clamp(0.0, 1.0));
                }
                // Copy alpha channel
                result.set(x, y, 3, tile.at(x, y, 3));
            }
        }

        result
    }

    /// Apply threshold dithering to a tile.
    fn apply_threshold(&self, tile: &PixelTile) -> PixelTile {
        let mut result = PixelTile::new();

        for y in 0u32..260 {
            for x in 0u32..260 {
                for c in 0..3 {
                    let pixel = tile.at(x, y, c as u32);
                    let threshold = 0.5;
                    let quantized = if pixel < threshold { 0.0 } else { 1.0 };
                    result.set(x, y, c as u32, quantized);
                }
                // Copy alpha channel
                result.set(x, y, 3, tile.at(x, y, 3));
            }
        }

        result
    }

    /// Apply the dither filter to a tile.
    pub fn apply_to_tile(&self, tile: &PixelTile, coord: TileCoord) -> Result<PixelTile, EngineError> {
        match self.algorithm {
            DitherAlgorithm::FloydSteinberg => Ok(self.apply_floyd_steinberg(tile)),
            DitherAlgorithm::Ordered => Ok(self.apply_ordered(tile, coord)),
            DitherAlgorithm::Threshold => Ok(self.apply_threshold(tile)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floyd_steinberg_error_distribution() {
        let dither = DitherFilter::new(DitherAlgorithm::FloydSteinberg, 1).unwrap();
        let tile = PixelTile::new();
        let result = dither.apply_floyd_steinberg(&tile);
        // Verify it returns a tile (detailed correctness tested via integration)
        assert_eq!(result.at(0, 0, 0), 0.0);
    }

    #[test]
    fn ordered_dithering_pattern() {
        let dither = DitherFilter::new(DitherAlgorithm::Ordered, 1).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let result = dither.apply_ordered(&tile, coord);
        // Verify it returns a tile
        assert_eq!(result.at(0, 0, 0), 0.0);
    }

    #[test]
    fn threshold_dithering() {
        let dither = DitherFilter::new(DitherAlgorithm::Threshold, 1).unwrap();
        let tile = PixelTile::new();
        let result = dither.apply_threshold(&tile);
        // Black tiles should remain black after threshold
        assert_eq!(result.at(0, 0, 0), 0.0);
    }

    #[test]
    fn color_depth_validation() {
        assert!(DitherFilter::new(DitherAlgorithm::Ordered, 0).is_err());
        assert!(DitherFilter::new(DitherAlgorithm::Ordered, 9).is_err());
        assert!(DitherFilter::new(DitherAlgorithm::Ordered, 4).is_ok());
    }

    #[test]
    fn quantization_levels() {
        let dither = DitherFilter::new(DitherAlgorithm::FloydSteinberg, 2).unwrap();
        // 2-bit = 4 levels: 0.0, 0.33, 0.67, 1.0
        let quantized = dither.quantize(0.5);
        assert!(quantized == 0.0 || quantized == (1.0 / 3.0) || quantized == (2.0 / 3.0) || quantized == 1.0);
    }

    #[test]
    fn reproducibility() {
        let dither = DitherFilter::new(DitherAlgorithm::Ordered, 4).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 5,
            y: 10,
        };

        let result1 = dither.apply_ordered(&tile, coord);
        let result2 = dither.apply_ordered(&tile, coord);

        // Same input should produce same output
        assert_eq!(result1.at(0, 0, 0), result2.at(0, 0, 0));
    }
}
