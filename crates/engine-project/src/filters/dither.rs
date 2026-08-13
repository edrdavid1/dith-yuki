//! Dither filter implementation.
//!
//! Color reduction with Bayer (ordered), ThresholdMap, and ErrorDiffusion algorithms.
//! Supports seamless tiling via global pixel coordinates.

use crate::error::EngineError;
use crate::filter::{DiffusionKernel, DitherMode};
use engine_color::threshold_map::ThresholdMapCache;
use engine_tiles::{GlobalCoord, PixelTile, TileCoord};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Tile full size including halo (260x260).
const TILE_FULL_SIZE: u32 = 260;

/// 2x2 Bayer matrix normalized to [0, 1) range.
const BAYER_2X2: [[f32; 2]; 2] = [
    [0.0 / 4.0, 2.0 / 4.0],
    [3.0 / 4.0, 1.0 / 4.0],
];

/// 4x4 Bayer matrix normalized to [0, 1) range.
#[rustfmt::skip]
const BAYER_4X4: [[f32; 4]; 4] = [
    [ 0.0/16.0,  8.0/16.0,  2.0/16.0, 10.0/16.0],
    [12.0/16.0,  4.0/16.0, 14.0/16.0,  6.0/16.0],
    [ 3.0/16.0, 11.0/16.0,  1.0/16.0,  9.0/16.0],
    [15.0/16.0,  7.0/16.0, 13.0/16.0,  5.0/16.0],
];

/// 8x8 Bayer matrix normalized to [0, 1) range.
#[rustfmt::skip]
const BAYER_8X8: [[f32; 8]; 8] = [
    [ 0.0/64.0, 32.0/64.0,  8.0/64.0, 40.0/64.0,  2.0/64.0, 34.0/64.0, 10.0/64.0, 42.0/64.0],
    [48.0/64.0, 16.0/64.0, 56.0/64.0, 24.0/64.0, 50.0/64.0, 18.0/64.0, 58.0/64.0, 26.0/64.0],
    [12.0/64.0, 44.0/64.0,  4.0/64.0, 36.0/64.0, 14.0/64.0, 46.0/64.0,  6.0/64.0, 38.0/64.0],
    [60.0/64.0, 28.0/64.0, 52.0/64.0, 20.0/64.0, 62.0/64.0, 30.0/64.0, 54.0/64.0, 22.0/64.0],
    [ 3.0/64.0, 35.0/64.0, 11.0/64.0, 43.0/64.0,  1.0/64.0, 33.0/64.0,  9.0/64.0, 41.0/64.0],
    [51.0/64.0, 19.0/64.0, 59.0/64.0, 27.0/64.0, 49.0/64.0, 17.0/64.0, 57.0/64.0, 25.0/64.0],
    [15.0/64.0, 47.0/64.0,  7.0/64.0, 39.0/64.0, 13.0/64.0, 45.0/64.0,  5.0/64.0, 37.0/64.0],
    [63.0/64.0, 31.0/64.0, 55.0/64.0, 23.0/64.0, 61.0/64.0, 29.0/64.0, 53.0/64.0, 21.0/64.0],
];

/// Dithering algorithm selection (legacy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DitherAlgorithm {
    FloydSteinberg,
    Ordered,
    Threshold,
}

/// Dither filter for color reduction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DitherFilter {
    pub algorithm: DitherAlgorithm,
    pub color_depth: u8,
}

impl DitherFilter {
    /// Create a new dither filter (legacy constructor).
    pub fn new(algorithm: DitherAlgorithm, color_depth: u8) -> Result<Self, EngineError> {
        if !(1..=8).contains(&color_depth) {
            return Err(EngineError::InvalidFilterParams {
                reason: "Color depth must be 1-8 bits".to_string(),
            });
        }
        Ok(DitherFilter { algorithm, color_depth })
    }

    /// Quantize a pixel value to the target color depth (round to nearest).
    #[allow(dead_code)]
    pub(crate) fn quantize(&self, value: f32) -> f32 {
        let levels = ((1u32 << self.color_depth) - 1) as f32;
        (value * levels).round().clamp(0.0, levels) / levels
    }

    /// Legacy apply method for backward compatibility.
    pub fn apply_to_tile(&self, tile: &PixelTile, coord: TileCoord) -> Result<PixelTile, EngineError> {
        let cache = ThresholdMapCache::new();
        let mode = match self.algorithm {
            DitherAlgorithm::FloydSteinberg => DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::FloydSteinberg,
            },
            DitherAlgorithm::Ordered => DitherMode::Bayer { matrix_size: 4 },
            DitherAlgorithm::Threshold => DitherMode::Bayer { matrix_size: 2 },
        };
        Self::apply(tile, coord, &mode, self.color_depth, &cache)
    }

    /// Apply dither to a tile using the expanded mode system.
    ///
    /// Uses global pixel coordinates for seamless tiling across tile boundaries.
    /// Alpha channel is always preserved unmodified.
    pub fn apply(
        tile: &PixelTile,
        coord: TileCoord,
        mode: &DitherMode,
        color_depth: u8,
        threshold_cache: &ThresholdMapCache,
    ) -> Result<PixelTile, EngineError> {
        if !(1..=8).contains(&color_depth) {
            return Err(EngineError::InvalidFilterParams {
                reason: "Color depth must be 1-8 bits".to_string(),
            });
        }
        match mode {
            DitherMode::Bayer { matrix_size } => Self::apply_bayer(tile, coord, *matrix_size, color_depth),
            DitherMode::ThresholdMap { path } => Self::apply_threshold_map(tile, coord, path, color_depth, threshold_cache),
            DitherMode::ErrorDiffusion { kernel } => Self::apply_error_diffusion(tile, *kernel, color_depth),
        }
    }

    fn apply_bayer(tile: &PixelTile, coord: TileCoord, matrix_size: u8, color_depth: u8) -> Result<PixelTile, EngineError> {
        if !matches!(matrix_size, 2 | 4 | 8) {
            return Err(EngineError::InvalidFilterParams {
                reason: "Bayer matrix_size must be 2, 4, or 8".to_string(),
            });
        }
        let levels = ((1u32 << color_depth) - 1) as f32;
        let n = matrix_size as u32;
        let mut result = PixelTile::new();
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                let g = GlobalCoord::from_tile_pixel(coord, x, y);
                let (gx, gy) = g.pattern_cell(n);
                let threshold = match matrix_size {
                    2 => BAYER_2X2[gy as usize][gx as usize],
                    4 => BAYER_4X4[gy as usize][gx as usize],
                    8 => BAYER_8X8[gy as usize][gx as usize],
                    _ => unreachable!(),
                };
                let threshold_offset = threshold - 0.5;
                for c in 0..3u32 {
                    let pixel = tile.at(x, y, c);
                    let quantized = ((pixel * levels + threshold_offset).round()).clamp(0.0, levels) / levels;
                    result.set(x, y, c, quantized);
                }
                result.set(x, y, 3, tile.at(x, y, 3));
            }
        }
        Ok(result)
    }

    fn apply_threshold_map(tile: &PixelTile, coord: TileCoord, path: &str, color_depth: u8, threshold_cache: &ThresholdMapCache) -> Result<PixelTile, EngineError> {
        let map = threshold_cache.get_or_load(Path::new(path)).map_err(|e| EngineError::IoError {
            reason: format!("Failed to load threshold map: {}", e),
        })?;
        let levels = ((1u32 << color_depth) - 1) as f32;
        let mut result = PixelTile::new();
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                let g = GlobalCoord::from_tile_pixel(coord, x, y);
                let threshold = map.sample(g.x, g.y);
                let threshold_offset = threshold - 0.5;
                for c in 0..3u32 {
                    let pixel = tile.at(x, y, c);
                    let quantized = ((pixel * levels + threshold_offset).round()).clamp(0.0, levels) / levels;
                    result.set(x, y, c, quantized);
                }
                result.set(x, y, 3, tile.at(x, y, 3));
            }
        }
        Ok(result)
    }

    fn apply_error_diffusion(tile: &PixelTile, kernel: DiffusionKernel, color_depth: u8) -> Result<PixelTile, EngineError> {
        let levels = ((1u32 << color_depth) - 1) as f32;
        let size = TILE_FULL_SIZE as usize;
        let mut buffer = vec![0.0f32; size * size * 3];
        for y in 0..size {
            for x in 0..size {
                for c in 0..3 {
                    buffer[(y * size + x) * 3 + c] = tile.at(x as u32, y as u32, c as u32);
                }
            }
        }
        let mut result = PixelTile::new();
        for y in 0..size {
            for x in 0..size {
                for c in 0..3 {
                    let idx = (y * size + x) * 3 + c;
                    let pixel = buffer[idx];
                    let quantized = (pixel * levels).round().clamp(0.0, levels) / levels;
                    result.set(x as u32, y as u32, c as u32, quantized);
                    let error = pixel - quantized;
                    distribute_error(&mut buffer, x, y, c, size, error, kernel);
                }
                result.set(x as u32, y as u32, 3, tile.at(x as u32, y as u32, 3));
            }
        }
        Ok(result)
    }
}

fn distribute_error(buffer: &mut [f32], x: usize, y: usize, c: usize, size: usize, error: f32, kernel: DiffusionKernel) {
    apply_offsets(buffer, x, y, c, size, error, kernel.offsets());
}

#[inline]
fn apply_offsets(buffer: &mut [f32], x: usize, y: usize, c: usize, size: usize, error: f32, offsets: &[(i32, i32, f32)]) {
    for &(dx, dy, weight) in offsets {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx >= 0 && nx < size as i32 && ny >= 0 && ny < size as i32 {
            let nidx = (ny as usize * size + nx as usize) * 3 + c;
            buffer[nidx] += error * weight;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_uniform_tile(r: f32, g: f32, b: f32, a: f32) -> PixelTile {
        let mut tile = PixelTile::new();
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                tile.set(x, y, 0, r);
                tile.set(x, y, 1, g);
                tile.set(x, y, 2, b);
                tile.set(x, y, 3, a);
            }
        }
        tile
    }

    fn tc(x: u32, y: u32) -> TileCoord {
        TileCoord { level: 0, x, y }
    }

    fn is_valid_level(v: f32, levels: f32) -> bool {
        (v * levels - (v * levels).round()).abs() < 1e-4
    }

    #[test]
    fn bayer_2x2_produces_valid_levels() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 2 };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        let levels = 15.0f32;
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn bayer_4x4_produces_valid_levels() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 2, &cache).unwrap();
        let levels = 3.0f32;
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn bayer_8x8_produces_valid_levels() {
        let tile = make_uniform_tile(0.25, 0.75, 0.5, 0.8);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 8 };
        let result = DitherFilter::apply(&tile, tc(3, 7), &mode, 3, &cache).unwrap();
        let levels = 7.0f32;
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn bayer_invalid_matrix_size_errors() {
        let tile = PixelTile::new();
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 3 };
        assert!(DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).is_err());
    }

    #[test]
    fn bayer_preserves_alpha() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 0.42);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                assert_eq!(result.at(x, y, 3), 0.42);
            }
        }
    }

    #[test]
    fn floyd_steinberg_produces_valid_levels() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        let levels = 15.0f32;
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn atkinson_produces_valid_levels() {
        let tile = make_uniform_tile(0.6, 0.4, 0.2, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Atkinson };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 3, &cache).unwrap();
        let levels = 7.0f32;
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn jjn_produces_valid_levels() {
        let tile = make_uniform_tile(0.1, 0.9, 0.5, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::ErrorDiffusion { kernel: DiffusionKernel::JarvisJudiceNinke };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 2, &cache).unwrap();
        let levels = 3.0f32;
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn stucki_produces_valid_levels() {
        let tile = make_uniform_tile(0.33, 0.67, 0.5, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Stucki };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        let levels = 15.0f32;
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn error_diffusion_preserves_alpha() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 0.77);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                assert_eq!(result.at(x, y, 3), 0.77);
            }
        }
    }

    #[test]
    fn color_depth_0_errors() {
        let tile = PixelTile::new();
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        assert!(DitherFilter::apply(&tile, tc(0, 0), &mode, 0, &cache).is_err());
    }

    #[test]
    fn color_depth_9_errors() {
        let tile = PixelTile::new();
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        assert!(DitherFilter::apply(&tile, tc(0, 0), &mode, 9, &cache).is_err());
    }

    #[test]
    fn color_depth_1_to_8_valid() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        for depth in 1..=8 {
            assert!(DitherFilter::apply(&tile, tc(0, 0), &mode, depth, &cache).is_ok());
        }
    }

    #[test]
    fn bayer_is_deterministic() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        let r1 = DitherFilter::apply(&tile, tc(5, 10), &mode, 4, &cache).unwrap();
        let r2 = DitherFilter::apply(&tile, tc(5, 10), &mode, 4, &cache).unwrap();
        assert_eq!(r1.data, r2.data);
    }

    #[test]
    fn error_diffusion_is_deterministic() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg };
        let r1 = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        let r2 = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        assert_eq!(r1.data, r2.data);
    }

    #[test]
    fn bayer_seamless_adjacent_tiles() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        let left = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        let right = DitherFilter::apply(&tile, tc(1, 0), &mode, 4, &cache).unwrap();
        let levels = 15.0f32;
        for y in 0..4u32 {
            assert!(is_valid_level(left.at(259, y, 0), levels));
            assert!(is_valid_level(right.at(0, y, 0), levels));
        }
    }

    #[test]
    fn black_tile_stays_black() {
        let tile = make_uniform_tile(0.0, 0.0, 0.0, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert_eq!(result.at(x, y, c), 0.0);
                }
            }
        }
    }

    #[test]
    fn white_tile_stays_white() {
        let tile = make_uniform_tile(1.0, 1.0, 1.0, 1.0);
        let cache = ThresholdMapCache::new();
        let mode = DitherMode::Bayer { matrix_size: 4 };
        let result = DitherFilter::apply(&tile, tc(0, 0), &mode, 4, &cache).unwrap();
        for y in 0u32..TILE_FULL_SIZE {
            for x in 0u32..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    assert_eq!(result.at(x, y, c), 1.0);
                }
            }
        }
    }

    #[test]
    fn legacy_api_compatibility() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let dither = DitherFilter::new(DitherAlgorithm::FloydSteinberg, 4).unwrap();
        assert!(dither.apply_to_tile(&tile, tc(0, 0)).is_ok());
        let dither = DitherFilter::new(DitherAlgorithm::Ordered, 4).unwrap();
        assert!(dither.apply_to_tile(&tile, tc(0, 0)).is_ok());
        let dither = DitherFilter::new(DitherAlgorithm::Threshold, 4).unwrap();
        assert!(dither.apply_to_tile(&tile, tc(0, 0)).is_ok());
    }

    #[test]
    fn legacy_color_depth_validation() {
        assert!(DitherFilter::new(DitherAlgorithm::Ordered, 0).is_err());
        assert!(DitherFilter::new(DitherAlgorithm::Ordered, 9).is_err());
        assert!(DitherFilter::new(DitherAlgorithm::Ordered, 4).is_ok());
    }
}
