//! Glitch effects implementation.
//!
//! RGB Shift and Block Displace. Shift field is keyed by
//! [`GlobalCoordSigned`] + seed (never `TileCoord` alone). v1 offsets are
//! capped to [`HALO`] so source samples stay inside the destination tile buffer
//! at internal seams.

use crate::error::EngineError;
use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use serde::{Deserialize, Serialize};

/// Tile full size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

/// Block Displace grid in global pixels.
const BLOCK_SIZE: u32 = 16;

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

/// PRNG key: `seed XOR f(global_x, global_y, level)` — not tile indices.
#[inline]
pub(crate) fn mix_prng_key(seed: u64, gx: i32, gy: i32, level: u8) -> u64 {
    seed ^ (gx as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (gy as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (level as u64)
}

/// Map a unit random in `[0, 1]` to a signed pixel offset in `[-HALO, HALO]`,
/// scaled by intensity in `[0, 1]`.
#[inline]
pub(crate) fn signed_offset(unit: f32, intensity: f32) -> i32 {
    let max = HALO as f32 * intensity.clamp(0.0, 1.0);
    let s = ((unit - 0.5) * 2.0 * max).round() as i32;
    s.clamp(-(HALO as i32), HALO as i32)
}

/// One mix → three X-only channel shifts (RGB Shift).
#[inline]
pub(crate) fn rgb_channel_shifts(
    seed: u64,
    gx: i32,
    gy: i32,
    level: u8,
    intensity: f32,
) -> (i32, i32, i32) {
    let mut rng = XorShift64::new(mix_prng_key(seed, gx, gy, level));
    (
        signed_offset(rng.next_f32(), intensity),
        signed_offset(rng.next_f32(), intensity),
        signed_offset(rng.next_f32(), intensity),
    )
}

/// Displacement for a global block origin (Block Displace).
#[inline]
pub(crate) fn block_displacement(
    seed: u64,
    block_gx: i32,
    block_gy: i32,
    level: u8,
    intensity: f32,
) -> (i32, i32) {
    let mut rng = XorShift64::new(mix_prng_key(seed, block_gx, block_gy, level));
    (
        signed_offset(rng.next_f32(), intensity),
        signed_offset(rng.next_f32(), intensity),
    )
}

fn sample_global(tile: &PixelTile, coord: TileCoord, gx: i32, gy: i32, channel: u32) -> f32 {
    let (lx, ly) = GlobalCoordSigned { x: gx, y: gy }.to_local_with_halo(coord, HALO);
    let max = TILE_FULL_SIZE as i32 - 1;
    tile.at(lx.clamp(0, max) as u32, ly.clamp(0, max) as u32, channel)
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
    fn apply_rgb_shift_into(&self, tile: &PixelTile, coord: TileCoord, dst: &mut PixelTile) {
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
                let (sr, sg, sb) =
                    rgb_channel_shifts(self.seed, g.x, g.y, coord.level, self.intensity);

                dst.set(x, y, 0, sample_global(tile, coord, g.x + sr, g.y, 0));
                dst.set(x, y, 1, sample_global(tile, coord, g.x + sg, g.y, 1));
                dst.set(x, y, 2, sample_global(tile, coord, g.x + sb, g.y, 2));
                dst.set(x, y, 3, tile.at(x, y, 3));
            }
        }
    }

    /// Apply block displacement glitch to a tile.
    ///
    /// Dest starts as a copy of `pre` so unwritten pixels (holes) keep the
    /// source. Each source pixel is pushed by the displacement of its
    /// **global** block origin.
    fn apply_block_displace_into(&self, tile: &PixelTile, coord: TileCoord, dst: &mut PixelTile) {
        dst.copy_from(tile);

        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
                let block = g.aligned(BLOCK_SIZE);
                let (dx, dy) =
                    block_displacement(self.seed, block.x, block.y, coord.level, self.intensity);
                let dest = GlobalCoordSigned {
                    x: g.x + dx,
                    y: g.y + dy,
                };
                let (dlx, dly) = dest.to_local_with_halo(coord, HALO);
                if dlx >= 0
                    && dly >= 0
                    && dlx < TILE_FULL_SIZE as i32
                    && dly < TILE_FULL_SIZE as i32
                {
                    for c in 0..4 {
                        dst.set(dlx as u32, dly as u32, c, tile.at(x, y, c));
                    }
                }
            }
        }
    }

    /// Apply the glitch filter to a tile.
    pub fn apply_to_tile(&self, tile: &PixelTile, coord: TileCoord) -> Result<PixelTile, EngineError> {
        let mut out = PixelTile::new();
        self.apply_to_tile_into(tile, coord, &mut out)?;
        Ok(out)
    }

    /// Glitch into an existing buffer (full 260² write, no tile alloc).
    pub fn apply_to_tile_into(
        &self,
        tile: &PixelTile,
        coord: TileCoord,
        dst: &mut PixelTile,
    ) -> Result<(), EngineError> {
        if self.intensity < 0.001 {
            dst.copy_from(tile);
            return Ok(());
        }

        match self.glitch_type {
            GlitchType::RGBShift => self.apply_rgb_shift_into(tile, coord, dst),
            GlitchType::BlockDisplace => self.apply_block_displace_into(tile, coord, dst),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile_coord(x: u32, y: u32) -> TileCoord {
        TileCoord { level: 0, x, y }
    }

    /// Unique-per-global-pixel source so seam compares are meaningful.
    fn fill_global_pattern(coord: TileCoord) -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
                t.set(x, y, 0, ((g.x + 10_000).rem_euclid(251) as f32) / 250.0);
                t.set(x, y, 1, ((g.y + 10_000).rem_euclid(251) as f32) / 250.0);
                t.set(
                    x,
                    y,
                    2,
                    ((g.x.wrapping_add(g.y) + 10_000).rem_euclid(251) as f32) / 250.0,
                );
                t.set(x, y, 3, 1.0);
            }
        }
        t
    }

    fn sample_pattern(gx: i32, gy: i32, channel: u32) -> f32 {
        match channel {
            0 => ((gx + 10_000).rem_euclid(251) as f32) / 250.0,
            1 => ((gy + 10_000).rem_euclid(251) as f32) / 250.0,
            2 => ((gx.wrapping_add(gy) + 10_000).rem_euclid(251) as f32) / 250.0,
            _ => 1.0,
        }
    }

    #[test]
    fn rgb_shift_produces_shift() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 0.5, 12345).unwrap();
        let tile = PixelTile::new();
        let coord = tile_coord(0, 0);
        let result = glitch.apply_to_tile(&tile, coord).unwrap();
        assert_eq!(result.at(0, 0, 3), 0.0);
    }

    #[test]
    fn block_displacement_works() {
        let glitch = GlitchFilter::new(GlitchType::BlockDisplace, 0.5, 12345).unwrap();
        let tile = PixelTile::new();
        let coord = tile_coord(0, 0);
        let result = glitch.apply_to_tile(&tile, coord).unwrap();
        assert_eq!(result.at(0, 0, 0), 0.0);
    }

    #[test]
    fn zero_intensity_noop() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 0.0, 12345).unwrap();
        let coord = tile_coord(0, 0);
        let tile = fill_global_pattern(coord);
        let result = glitch.apply_to_tile(&tile, coord).unwrap();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..4 {
                    assert_eq!(result.at(x, y, c), tile.at(x, y, c));
                }
            }
        }
    }

    #[test]
    fn maximum_intensity() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 1.0, 12345).unwrap();
        let tile = PixelTile::new();
        let coord = tile_coord(0, 0);
        let result = glitch.apply_to_tile(&tile, coord).unwrap();
        assert!(result.at(0, 0, 0) >= 0.0 && result.at(0, 0, 0) <= 1.0);
    }

    #[test]
    fn reproducibility() {
        let glitch = GlitchFilter::new(GlitchType::RGBShift, 0.5, 54321).unwrap();
        let tile = PixelTile::new();
        let coord = TileCoord {
            level: 5,
            x: 10,
            y: 15,
        };

        let result1 = glitch.apply_to_tile(&tile, coord).unwrap();
        let result2 = glitch.apply_to_tile(&tile, coord).unwrap();

        assert_eq!(result1.at(50, 50, 0), result2.at(50, 50, 0));
        for y in HALO..(HALO + 8) {
            for x in HALO..(HALO + 8) {
                for c in 0..4 {
                    assert_eq!(result1.at(x, y, c), result2.at(x, y, c));
                }
            }
        }
    }

    #[test]
    fn invalid_intensity() {
        assert!(GlitchFilter::new(GlitchType::RGBShift, -0.1, 12345).is_err());
        assert!(GlitchFilter::new(GlitchType::RGBShift, 1.5, 12345).is_err());
        assert!(GlitchFilter::new(GlitchType::RGBShift, 0.5, 12345).is_ok());
    }

    #[test]
    fn offsets_capped_to_halo() {
        for intensity in [0.0, 0.25, 0.5, 1.0] {
            for gx in -4..=260 {
                for gy in [0, 16, 255, 256] {
                    let (r, g, b) = rgb_channel_shifts(99, gx, gy, 0, intensity);
                    let cap = HALO as i32;
                    assert!(r.abs() <= cap && g.abs() <= cap && b.abs() <= cap);
                    let (dx, dy) = block_displacement(99, gx.div_euclid(16) * 16, gy, 0, intensity);
                    assert!(dx.abs() <= cap && dy.abs() <= cap);
                }
            }
        }
    }

    #[test]
    fn prng_key_is_global_not_tile() {
        // Same global dest from two tiles must mix identically (no TileCoord in key).
        let left = tile_coord(0, 0);
        let right = tile_coord(1, 0);
        let g_left_last = GlobalCoordSigned::from_local_with_halo(
            left,
            HALO + TILE_SIZE - 1,
            HALO + 10,
            HALO,
        );
        let g_right_first =
            GlobalCoordSigned::from_local_with_halo(right, HALO, HALO + 10, HALO);
        assert_eq!(g_left_last.x + 1, g_right_first.x);

        let seed = 42u64;
        let k256 = mix_prng_key(seed, g_right_first.x, g_right_first.y, 0);
        let also = mix_prng_key(seed, 256, g_right_first.y, 0);
        assert_eq!(k256, also);
        assert_ne!(
            mix_prng_key(seed, g_left_last.x, g_left_last.y, 0),
            k256,
            "adjacent globals must not share a key"
        );
    }

    #[test]
    fn rgb_shift_2x2_seam_matches_global_formula() {
        let seed = 777u64;
        let intensity = 1.0;
        let glitch = GlitchFilter::new(GlitchType::RGBShift, intensity, seed).unwrap();
        let left_c = tile_coord(0, 0);
        let right_c = tile_coord(1, 0);
        let left = glitch
            .apply_to_tile(&fill_global_pattern(left_c), left_c)
            .unwrap();
        let right = glitch
            .apply_to_tile(&fill_global_pattern(right_c), right_c)
            .unwrap();

        let last_core_x = HALO + TILE_SIZE - 1;
        for y in HALO..(HALO + 32) {
            let g_l = GlobalCoordSigned::from_local_with_halo(left_c, last_core_x, y, HALO);
            let g_r = GlobalCoordSigned::from_local_with_halo(right_c, HALO, y, HALO);
            assert_eq!(g_l.x + 1, g_r.x);

            let (sr, sg, sb) = rgb_channel_shifts(seed, g_l.x, g_l.y, 0, intensity);
            assert!((left.at(last_core_x, y, 0) - sample_pattern(g_l.x + sr, g_l.y, 0)).abs() < 1e-6);
            assert!((left.at(last_core_x, y, 1) - sample_pattern(g_l.x + sg, g_l.y, 1)).abs() < 1e-6);
            assert!((left.at(last_core_x, y, 2) - sample_pattern(g_l.x + sb, g_l.y, 2)).abs() < 1e-6);

            let (sr, sg, sb) = rgb_channel_shifts(seed, g_r.x, g_r.y, 0, intensity);
            assert!((right.at(HALO, y, 0) - sample_pattern(g_r.x + sr, g_r.y, 0)).abs() < 1e-6);
            assert!((right.at(HALO, y, 1) - sample_pattern(g_r.x + sg, g_r.y, 1)).abs() < 1e-6);
            assert!((right.at(HALO, y, 2) - sample_pattern(g_r.x + sb, g_r.y, 2)).abs() < 1e-6);
        }
    }

    fn expected_block_at(dest_gx: i32, dest_gy: i32, seed: u64, intensity: f32) -> [f32; 4] {
        let mut out = [
            sample_pattern(dest_gx, dest_gy, 0),
            sample_pattern(dest_gx, dest_gy, 1),
            sample_pattern(dest_gx, dest_gy, 2),
            1.0,
        ];
        let cap = HALO as i32;
        // Same scan order as apply (y then x); only dest±HALO can land on dest.
        for sy in (dest_gy - cap)..=(dest_gy + cap) {
            for sx in (dest_gx - cap)..=(dest_gx + cap) {
                let block = GlobalCoordSigned { x: sx, y: sy }.aligned(BLOCK_SIZE);
                let (dx, dy) = block_displacement(seed, block.x, block.y, 0, intensity);
                if sx + dx == dest_gx && sy + dy == dest_gy {
                    out = [
                        sample_pattern(sx, sy, 0),
                        sample_pattern(sx, sy, 1),
                        sample_pattern(sx, sy, 2),
                        1.0,
                    ];
                }
            }
        }
        out
    }

    #[test]
    fn block_displace_2x2_seam_straddling_x256() {
        let seed = 1234u64;
        let intensity = 1.0;
        let glitch = GlitchFilter::new(GlitchType::BlockDisplace, intensity, seed).unwrap();
        let left_c = tile_coord(0, 0);
        let right_c = tile_coord(1, 0);

        // Block origins 240 (covers 240..255) and 256 (covers 256..271) meet at x=256.
        let (d240_a, d240_b) = (
            block_displacement(seed, 240, 0, 0, intensity),
            block_displacement(seed, 240, 0, 0, intensity),
        );
        assert_eq!(d240_a, d240_b);
        let d256 = block_displacement(seed, 256, 0, 0, intensity);
        assert_eq!(d256, block_displacement(seed, 256, 0, 0, intensity));

        let left = glitch
            .apply_to_tile(&fill_global_pattern(left_c), left_c)
            .unwrap();
        let right = glitch
            .apply_to_tile(&fill_global_pattern(right_c), right_c)
            .unwrap();

        let last_core_x = HALO + TILE_SIZE - 1;
        for y in HALO..(HALO + 32) {
            let g_l = GlobalCoordSigned::from_local_with_halo(left_c, last_core_x, y, HALO);
            let g_r = GlobalCoordSigned::from_local_with_halo(right_c, HALO, y, HALO);
            assert_eq!(g_l.x, 255);
            assert_eq!(g_r.x, 256);

            let exp_l = expected_block_at(g_l.x, g_l.y, seed, intensity);
            let exp_r = expected_block_at(g_r.x, g_r.y, seed, intensity);
            for c in 0..3 {
                assert!(
                    (left.at(last_core_x, y, c) - exp_l[c as usize]).abs() < 1e-6,
                    "left core x=255 y_local={y} c={c}"
                );
                assert!(
                    (right.at(HALO, y, c) - exp_r[c as usize]).abs() < 1e-6,
                    "right core x=256 y_local={y} c={c}"
                );
            }
        }
    }
}
