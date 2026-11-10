//! Exposure (EV stops) — pointwise Post tone filter.
//!
//! `out_rgb = clamp(in_rgb * 2^stops, 0, 1)`; alpha unchanged.
//! Identity at `stops == 0.0` (exact).

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

/// Apply exposure into `dst`. `src` and `dst` must not alias.
pub fn apply_exposure_into(src: &PixelTile, stops: f32, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let stops = stops.clamp(-5.0, 5.0);
    if stops == 0.0 {
        dst.copy_from(src);
        return;
    }
    let gain = 2f32.powf(stops);
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            dst.set(x, y, 0, (src.at(x, y, 0) * gain).clamp(0.0, 1.0));
            dst.set(x, y, 1, (src.at(x, y, 1) * gain).clamp(0.0, 1.0));
            dst.set(x, y, 2, (src.at(x, y, 2) * gain).clamp(0.0, 1.0));
            dst.set(x, y, 3, src.at(x, y, 3));
        }
    }
}

#[inline]
pub fn apply_exposure_point_rgba(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    gain: f32,
) -> (f32, f32, f32, f32) {
    (
        (r * gain).clamp(0.0, 1.0),
        (g * gain).clamp(0.0, 1.0),
        (b * gain).clamp(0.0, 1.0),
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_gradient() -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
                let v = x as f32 / TILE_FULL as f32;
                t.set(x, y, 0, v);
                t.set(x, y, 1, v * 0.5);
                t.set(x, y, 2, 1.0 - v);
                t.set(x, y, 3, 0.75);
            }
        }
        t
    }

    #[test]
    fn identity_is_noop_bitwise() {
        let src = fill_gradient();
        let mut dst = PixelTile::new();
        apply_exposure_into(&src, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn plus_one_stop_doubles_mid_gray() {
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.25);
        src.set(HALO, HALO, 1, 0.25);
        src.set(HALO, HALO, 2, 0.25);
        src.set(HALO, HALO, 3, 1.0);
        let mut dst = PixelTile::new();
        apply_exposure_into(&src, 1.0, &mut dst);
        assert!((dst.at(HALO, HALO, 0) - 0.5).abs() < 1e-6);
        assert!((dst.at(HALO, HALO, 1) - 0.5).abs() < 1e-6);
        assert!((dst.at(HALO, HALO, 2) - 0.5).abs() < 1e-6);
        assert_eq!(dst.at(HALO, HALO, 3), 1.0);
    }

    #[test]
    fn clamps_to_unit_interval() {
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.8);
        src.set(HALO, HALO, 1, 0.8);
        src.set(HALO, HALO, 2, 0.8);
        src.set(HALO, HALO, 3, 1.0);
        let mut dst = PixelTile::new();
        apply_exposure_into(&src, 2.0, &mut dst);
        assert_eq!(dst.at(HALO, HALO, 0), 1.0);
    }
}
