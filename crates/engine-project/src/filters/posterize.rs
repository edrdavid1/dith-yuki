//! Posterize — pointwise utility.
//!
//! Quantizes each RGB channel to `levels` steps (2..=64).
//! `out = round(in * (n-1)) / (n-1)`. Alpha unchanged.

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

#[inline]
fn quantize_channel(v: f32, levels: u32) -> f32 {
    let n = levels.max(2) as f32;
    let steps = n - 1.0;
    ((v.clamp(0.0, 1.0) * steps).round()) / steps
}

/// Apply posterize into `dst`. `src` and `dst` must not alias.
pub fn apply_posterize_into(src: &PixelTile, levels: u32, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let levels = levels.clamp(2, 64);
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            dst.set(x, y, 0, quantize_channel(src.at(x, y, 0), levels));
            dst.set(x, y, 1, quantize_channel(src.at(x, y, 1), levels));
            dst.set(x, y, 2, quantize_channel(src.at(x, y, 2), levels));
            dst.set(x, y, 3, src.at(x, y, 3));
        }
    }
}

#[inline]
pub fn apply_posterize_point_rgba(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    levels: u32,
) -> (f32, f32, f32, f32) {
    (
        quantize_channel(r, levels),
        quantize_channel(g, levels),
        quantize_channel(b, levels),
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_levels_is_black_or_white() {
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.3);
        src.set(HALO, HALO, 1, 0.7);
        src.set(HALO, HALO, 2, 0.5);
        src.set(HALO, HALO, 3, 0.9);
        let mut dst = PixelTile::new();
        apply_posterize_into(&src, 2, &mut dst);
        assert_eq!(dst.at(HALO, HALO, 0), 0.0);
        assert_eq!(dst.at(HALO, HALO, 1), 1.0);
        assert_eq!(dst.at(HALO, HALO, 2), 1.0); // 0.5 rounds up
        assert_eq!(dst.at(HALO, HALO, 3), 0.9);
    }

    #[test]
    fn three_levels_hits_mid() {
        assert!((quantize_channel(0.5, 3) - 0.5).abs() < 1e-6);
        assert!((quantize_channel(0.0, 3) - 0.0).abs() < 1e-6);
        assert!((quantize_channel(1.0, 3) - 1.0).abs() < 1e-6);
    }
}
