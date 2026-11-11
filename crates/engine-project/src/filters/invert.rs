//! Invert — pointwise utility. `out_rgb = 1 - in_rgb`; alpha unchanged.

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

/// Apply invert into `dst`. `src` and `dst` must not alias.
pub fn apply_invert_into(src: &PixelTile, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            dst.set(x, y, 0, 1.0 - src.at(x, y, 0));
            dst.set(x, y, 1, 1.0 - src.at(x, y, 1));
            dst.set(x, y, 2, 1.0 - src.at(x, y, 2));
            dst.set(x, y, 3, src.at(x, y, 3));
        }
    }
}

#[inline]
pub fn apply_invert_point_rgba(r: f32, g: f32, b: f32, a: f32) -> (f32, f32, f32, f32) {
    (1.0 - r, 1.0 - g, 1.0 - b, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverts_rgb_preserves_alpha() {
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.25);
        src.set(HALO, HALO, 1, 0.5);
        src.set(HALO, HALO, 2, 0.75);
        src.set(HALO, HALO, 3, 0.8);
        let mut dst = PixelTile::new();
        apply_invert_into(&src, &mut dst);
        assert!((dst.at(HALO, HALO, 0) - 0.75).abs() < 1e-6);
        assert!((dst.at(HALO, HALO, 1) - 0.5).abs() < 1e-6);
        assert!((dst.at(HALO, HALO, 2) - 0.25).abs() < 1e-6);
        assert_eq!(dst.at(HALO, HALO, 3), 0.8);
    }

    #[test]
    fn double_invert_is_identity() {
        let mut src = PixelTile::new();
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
                let v = (x + y) as f32 / (2 * TILE_FULL) as f32;
                src.set(x, y, 0, v);
                src.set(x, y, 1, 1.0 - v);
                src.set(x, y, 2, 0.3);
                src.set(x, y, 3, 0.9);
            }
        }
        let mut mid = PixelTile::new();
        let mut dst = PixelTile::new();
        apply_invert_into(&src, &mut mid);
        apply_invert_into(&mid, &mut dst);
        for i in 0..src.data.len() {
            assert!((dst.data[i] - src.data[i]).abs() < 1e-6);
        }
    }
}
