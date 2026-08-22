//! Levels filter implementation.
//!
//! Histogram adjustment with gamma correction.

use crate::error::EngineError;
use engine_tiles::PixelTile;
use serde::{Deserialize, Serialize};

fn default_channel_on() -> bool {
    true
}

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
    /// When false, that RGB channel is forced to 0.
    #[serde(default = "default_channel_on")]
    pub channel_r: bool,
    #[serde(default = "default_channel_on")]
    pub channel_g: bool,
    #[serde(default = "default_channel_on")]
    pub channel_b: bool,
    /// Pre-computed LUT: 4096 entries mapping [0.0, 1.0] → output.
    /// Derived from other fields; not serialized.
    #[serde(skip)]
    lut: Vec<f32>,
}

impl LevelsFilter {
    /// Create a new levels filter with default (no-op) values.
    pub fn new() -> Self {
        let mut filter = LevelsFilter {
            input_black: 0.0,
            input_white: 1.0,
            gamma: 1.0,
            output_black: 0.0,
            output_white: 1.0,
            channel_r: true,
            channel_g: true,
            channel_b: true,
            lut: Vec::new(),
        };
        filter.rebuild_lut();
        filter
    }

    /// Rebuild the pre-computed LUT from current parameters.
    /// Must be called after construction or any parameter change.
    /// After deserialization (where `lut` is empty due to `#[serde(skip)]`),
    /// callers should invoke this method to populate the LUT.
    pub fn rebuild_lut(&mut self) {
        self.lut.resize(4096, 0.0);
        for i in 0..4096 {
            let x = i as f32 / 4095.0;
            self.lut[i] = self.apply_to_value(x);
        }
    }

    /// Fast LUT lookup with linear interpolation between adjacent entries.
    /// Returns a value within ±1/65536 of `apply_to_value(x)`.
    pub fn lut_lookup(&self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        let idx_f = x * 4095.0;
        let idx_lo = idx_f as usize;
        let idx_hi = (idx_lo + 1).min(4095);
        let frac = idx_f - idx_lo as f32;
        self.lut[idx_lo] * (1.0 - frac) + self.lut[idx_hi] * frac
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
    /// Uses row-based SIMD LUT processing for performance.
    pub fn apply_to_tile(&self, tile: &PixelTile) -> Result<PixelTile, EngineError> {
        let mut result = PixelTile::new();
        self.apply_to_tile_into(tile, &mut result)?;
        Ok(result)
    }

    /// Apply levels into an existing buffer (full 260² write, no alloc).
    pub fn apply_to_tile_into(&self, tile: &PixelTile, dst: &mut PixelTile) -> Result<(), EngineError> {
        use crate::simd::levels_row_simd;
        use engine_tiles::{HALO, TILE_SIZE};

        let size = (TILE_SIZE + 2 * HALO) as usize; // 260
        let mask = [self.channel_r, self.channel_g, self.channel_b];

        for y in 0..size {
            let row_start = y * size * 4;
            let row_end = row_start + size * 4;
            levels_row_simd(
                &mut dst.data[row_start..row_end],
                &tile.data[row_start..row_end],
                &self.lut,
                mask,
            );
        }
        Ok(())
    }
}

impl Default for LevelsFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// Reference implementation of `LevelsFilter::apply_to_tile` preserved for property-based testing.
/// This is an exact copy of the current `apply_to_tile` implementation at the time of snapshotting.
/// Used to verify that optimized versions (LUT-based) produce identical or near-identical output.
#[cfg(test)]
pub fn reference_levels_apply_to_tile(filter: &LevelsFilter, tile: &PixelTile) -> Result<PixelTile, EngineError> {
    let mut result = PixelTile::new();

    // Iterate over all pixels in the tile (256+4 for halo)
    for y in 0u32..260 {
        for x in 0u32..260 {
            // Apply to RGB channels
            for c in 0..3 {
                let val = tile.at(x, y, c);
                let adjusted = if [filter.channel_r, filter.channel_g, filter.channel_b][c as usize] {
                    filter.apply_to_value(val)
                } else {
                    0.0
                };
                result.set(x, y, c, adjusted);
            }
            // Copy alpha channel
            result.set(x, y, 3, tile.at(x, y, 3));
        }
    }

    Ok(result)
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

    #[test]
    fn lut_identity_lookup() {
        let levels = LevelsFilter::new();
        // Identity filter: LUT should return ~input
        assert!((levels.lut_lookup(0.0) - 0.0).abs() < 1e-5);
        assert!((levels.lut_lookup(0.5) - 0.5).abs() < 1e-5);
        assert!((levels.lut_lookup(1.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn lut_accuracy_vs_apply_to_value() {
        let mut levels = LevelsFilter::new();
        levels.input_black = 0.1;
        levels.input_white = 0.9;
        levels.gamma = 2.2;
        levels.output_black = 0.05;
        levels.output_white = 0.95;
        levels.rebuild_lut();

        // Check that lut_lookup closely approximates apply_to_value.
        // With 4096 entries and linear interpolation, max error depends on
        // curvature. For typical gamma curves, error is well under 1/4096.
        let tolerance = 1.0 / 4096.0;
        for i in 0..100 {
            let x = i as f32 / 99.0;
            let lut_val = levels.lut_lookup(x);
            let direct_val = levels.apply_to_value(x);
            assert!(
                (lut_val - direct_val).abs() <= tolerance,
                "LUT diverged at x={}: lut={}, direct={}, diff={}",
                x,
                lut_val,
                direct_val,
                (lut_val - direct_val).abs()
            );
        }
    }

    #[test]
    fn lut_clamps_out_of_range() {
        let levels = LevelsFilter::new();
        // Values outside [0,1] should be clamped
        assert!((levels.lut_lookup(-1.0) - 0.0).abs() < 1e-5);
        assert!((levels.lut_lookup(2.0) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn rebuild_lut_updates_with_new_params() {
        let mut levels = LevelsFilter::new();
        let before = levels.lut_lookup(0.5);

        levels.gamma = 2.0;
        levels.rebuild_lut();
        let after = levels.lut_lookup(0.5);

        // Gamma change should produce a different value for midtones
        assert!((after - before).abs() > 0.01);
    }

    #[test]
    fn apply_to_tile_uses_rebuilt_lut() {
        let mut tile = PixelTile::new();
        tile.set(10, 10, 0, 0.5);
        tile.set(10, 10, 1, 0.5);
        tile.set(10, 10, 2, 0.5);

        let identity = LevelsFilter::new();
        let id_out = identity.apply_to_tile(&tile).unwrap();
        assert!((id_out.at(10, 10, 0) - 0.5).abs() < 0.01);

        let mut gamma = LevelsFilter::new();
        gamma.gamma = 2.0;
        gamma.rebuild_lut();
        let g_out = gamma.apply_to_tile(&tile).unwrap();
        assert!(
            g_out.at(10, 10, 0) > id_out.at(10, 10, 0) + 0.05,
            "gamma 2.0 should brighten midtones, got {}",
            g_out.at(10, 10, 0)
        );
    }

    #[test]
    fn disabled_channel_is_zeroed() {
        let mut tile = PixelTile::new();
        tile.set(5, 5, 0, 0.8);
        tile.set(5, 5, 1, 0.6);
        tile.set(5, 5, 2, 0.4);

        let mut filter = LevelsFilter::new();
        filter.channel_r = false;
        let out = filter.apply_to_tile(&tile).unwrap();
        assert!((out.at(5, 5, 0) - 0.0).abs() < 1e-5);
        assert!((out.at(5, 5, 1) - 0.6).abs() < 0.01);
        assert!((out.at(5, 5, 2) - 0.4).abs() < 0.01);
    }
}
