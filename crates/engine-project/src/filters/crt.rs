//! CRT scanline (+ optional RGB triad mask) filter.
//!
//! Scanline phase keys off global `Y_g` via `GlobalCoordSigned` — never
//! `tile_y * TILE_SIZE + local_y` inline.
//!
//! Alpha is preserved unchanged.

use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

/// Tile full size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

/// Scanline gain for a global Y and CRT params.
///
/// Dark rows are the first floor of the period (or half when period is even).
pub(crate) fn scanline_gain(y_g: i32, period: u8, strength: f32) -> f32 {
    let p = period as i32;
    let line = (y_g as i64).rem_euclid(p as i64) as i32;
    let dark_rows = (p / 2).max(1);
    if line < dark_rows {
        1.0 - strength
    } else {
        1.0
    }
}

/// RGB triad mask multipliers for a global X.
///
/// Channel `i` (0=R,1=G,2=B) is attenuated when `X_g % 3 != i`.
pub(crate) fn rgb_mask_gain(x_g: i32, channel: usize, mask_strength: f32) -> f32 {
    if mask_strength <= 0.0 {
        return 1.0;
    }
    let col = (x_g as i64).rem_euclid(3) as usize;
    if col == channel {
        1.0
    } else {
        1.0 - mask_strength
    }
}

/// Apply CRT scanlines (+ optional RGB mask) to a tile.
pub fn apply_crt(
    tile: &PixelTile,
    coord: TileCoord,
    period: u8,
    strength: f32,
    mask_strength: f32,
) -> PixelTile {
    let mut result = PixelTile::new();
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
            let gain = scanline_gain(g.y, period, strength);
            for c in 0..3usize {
                let mask = rgb_mask_gain(g.x, c, mask_strength);
                let v = tile.at(x, y, c as u32) * gain * mask;
                result.set(x, y, c as u32, v.clamp(0.0, 1.0));
            }
            result.set(x, y, 3, tile.at(x, y, 3));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanline_gain_fixed() {
        assert!((scanline_gain(0, 2, 1.0) - 0.0).abs() < 1e-6);
        assert!((scanline_gain(1, 2, 1.0) - 1.0).abs() < 1e-6);
        assert!((scanline_gain(-1, 2, 0.5) - 1.0).abs() < 1e-6); // rem_euclid(-1,2)=1
    }

    #[test]
    fn crt_seamless_horizontal_boundary() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                tile.set(x, y, 0, 0.8);
                tile.set(x, y, 1, 0.8);
                tile.set(x, y, 2, 0.8);
                tile.set(x, y, 3, 1.0);
            }
        }
        let top = apply_crt(&tile, TileCoord { level: 0, x: 0, y: 0 }, 2, 0.5, 0.0);
        let bottom = apply_crt(&tile, TileCoord { level: 0, x: 0, y: 1 }, 2, 0.5, 0.0);

        // Last core row of tile y=0 is global Y=255; first core of y=1 is Y=256.
        let last_core_y = HALO + TILE_SIZE - 1;
        let first_core_y = HALO;
        let g_top = GlobalCoordSigned::from_local_with_halo(
            TileCoord { level: 0, x: 0, y: 0 },
            HALO,
            last_core_y,
            HALO,
        );
        let g_bot = GlobalCoordSigned::from_local_with_halo(
            TileCoord { level: 0, x: 0, y: 1 },
            HALO,
            first_core_y,
            HALO,
        );
        assert_eq!(g_top.y + 1, g_bot.y);

        let v_top = top.at(HALO, last_core_y, 0);
        let v_bot = bottom.at(HALO, first_core_y, 0);
        let expected_top = 0.8 * scanline_gain(g_top.y, 2, 0.5);
        let expected_bot = 0.8 * scanline_gain(g_bot.y, 2, 0.5);
        assert!((v_top - expected_top).abs() < 1e-5);
        assert!((v_bot - expected_bot).abs() < 1e-5);
        // Adjacent global rows with period=2 must alternate (phase continuous).
        assert!((v_top - v_bot).abs() > 0.1);
    }
}
