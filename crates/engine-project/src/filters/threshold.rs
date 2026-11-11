//! Threshold — pointwise utility.
//!
//! Luminance (Rec.709) ≥ `threshold` → white; else black. Alpha unchanged.
//! Always transforms when enabled (no param identity).

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

#[inline]
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Apply hard threshold into `dst`. `src` and `dst` must not alias.
pub fn apply_threshold_into(src: &PixelTile, threshold: f32, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let threshold = threshold.clamp(0.0, 1.0);
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = src.at(x, y, 0);
            let g = src.at(x, y, 1);
            let b = src.at(x, y, 2);
            let v = if luminance(r, g, b) >= threshold {
                1.0
            } else {
                0.0
            };
            dst.set(x, y, 0, v);
            dst.set(x, y, 1, v);
            dst.set(x, y, 2, v);
            dst.set(x, y, 3, src.at(x, y, 3));
        }
    }
}

#[inline]
pub fn apply_threshold_point_rgba(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    threshold: f32,
) -> (f32, f32, f32, f32) {
    let v = if luminance(r, g, b) >= threshold {
        1.0
    } else {
        0.0
    };
    (v, v, v, a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mid_gray_splits_at_half() {
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.4);
        src.set(HALO, HALO, 1, 0.4);
        src.set(HALO, HALO, 2, 0.4);
        src.set(HALO, HALO, 3, 1.0);
        let mut dst = PixelTile::new();
        apply_threshold_into(&src, 0.5, &mut dst);
        assert_eq!(dst.at(HALO, HALO, 0), 0.0);

        src.set(HALO, HALO, 0, 0.6);
        src.set(HALO, HALO, 1, 0.6);
        src.set(HALO, HALO, 2, 0.6);
        apply_threshold_into(&src, 0.5, &mut dst);
        assert_eq!(dst.at(HALO, HALO, 0), 1.0);
        assert_eq!(dst.at(HALO, HALO, 3), 1.0);
    }
}
