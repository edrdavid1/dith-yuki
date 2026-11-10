//! Temperature / Tint — pointwise Post color filter.
//!
//! Warm/cool (temperature) and green/magenta (tint) via soft RGB channel gains.
//! Identity when both amounts are exactly `0.0`.

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;
/// Full-throw channel gain delta (±).
const STRENGTH: f32 = 0.25;

/// Apply temperature/tint into `dst`. `src` and `dst` must not alias.
///
/// `temperature` / `tint` in `[-1, 1]`: positive = warmer / greener.
pub fn apply_temperature_tint_into(
    src: &PixelTile,
    temperature: f32,
    tint: f32,
    dst: &mut PixelTile,
) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let temperature = temperature.clamp(-1.0, 1.0);
    let tint = tint.clamp(-1.0, 1.0);
    if temperature == 0.0 && tint == 0.0 {
        dst.copy_from(src);
        return;
    }
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = src.at(x, y, 0);
            let g = src.at(x, y, 1);
            let b = src.at(x, y, 2);
            let a = src.at(x, y, 3);
            let (or, og, ob, oa) = apply_temperature_tint_point_rgba(r, g, b, a, temperature, tint);
            dst.set(x, y, 0, or);
            dst.set(x, y, 1, og);
            dst.set(x, y, 2, ob);
            dst.set(x, y, 3, oa);
        }
    }
}

#[inline]
pub fn apply_temperature_tint_point_rgba(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    temperature: f32,
    tint: f32,
) -> (f32, f32, f32, f32) {
    let t = temperature;
    let m = tint;
    (
        (r * (1.0 + STRENGTH * t - 0.5 * STRENGTH * m)).clamp(0.0, 1.0),
        (g * (1.0 + STRENGTH * m)).clamp(0.0, 1.0),
        (b * (1.0 - STRENGTH * t - 0.5 * STRENGTH * m)).clamp(0.0, 1.0),
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(v: f32) -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
                t.set(x, y, 0, v);
                t.set(x, y, 1, v);
                t.set(x, y, 2, v);
                t.set(x, y, 3, 1.0);
            }
        }
        t
    }

    #[test]
    fn identity_is_noop_bitwise() {
        let src = gray(0.4);
        let mut dst = PixelTile::new();
        apply_temperature_tint_into(&src, 0.0, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn warm_boosts_red_cuts_blue() {
        let src = gray(0.5);
        let mut dst = PixelTile::new();
        apply_temperature_tint_into(&src, 1.0, 0.0, &mut dst);
        assert!(dst.at(HALO, HALO, 0) > 0.5);
        assert!((dst.at(HALO, HALO, 1) - 0.5).abs() < 1e-6);
        assert!(dst.at(HALO, HALO, 2) < 0.5);
    }

    #[test]
    fn green_tint_boosts_green() {
        let src = gray(0.5);
        let mut dst = PixelTile::new();
        apply_temperature_tint_into(&src, 0.0, 1.0, &mut dst);
        assert!(dst.at(HALO, HALO, 1) > 0.5);
        assert!(dst.at(HALO, HALO, 0) < 0.5);
        assert!(dst.at(HALO, HALO, 2) < 0.5);
    }
}
