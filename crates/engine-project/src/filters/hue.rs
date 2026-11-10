//! Hue rotate — pointwise Post color filter.
//!
//! Rotates hue in HSL by `degrees` (−180 .. 180). Identity at `degrees == 0.0`.

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

#[inline]
fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let l = (maxc + minc) * 0.5;
    if (maxc - minc).abs() < 1e-8 {
        return (0.0, 0.0, l);
    }
    let d = maxc - minc;
    let s = if l > 0.5 {
        d / (2.0 - maxc - minc)
    } else {
        d / (maxc + minc)
    };
    let h = if (maxc - r).abs() < 1e-8 {
        let mut v = (g - b) / d;
        if g < b {
            v += 6.0;
        }
        v
    } else if (maxc - g).abs() < 1e-8 {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

#[inline]
fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 0.5 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

#[inline]
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s.abs() < 1e-8 {
        return (l, l, l);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    (
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    )
}

/// Apply hue rotate into `dst`. `src` and `dst` must not alias.
pub fn apply_hue_into(src: &PixelTile, degrees: f32, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let degrees = degrees.clamp(-180.0, 180.0);
    if degrees == 0.0 {
        dst.copy_from(src);
        return;
    }
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = src.at(x, y, 0);
            let g = src.at(x, y, 1);
            let b = src.at(x, y, 2);
            let a = src.at(x, y, 3);
            let (or, og, ob, oa) = apply_hue_point_rgba(r, g, b, a, degrees);
            dst.set(x, y, 0, or);
            dst.set(x, y, 1, og);
            dst.set(x, y, 2, ob);
            dst.set(x, y, 3, oa);
        }
    }
}

#[inline]
pub fn apply_hue_point_rgba(r: f32, g: f32, b: f32, a: f32, degrees: f32) -> (f32, f32, f32, f32) {
    let (h, s, l) = rgb_to_hsl(r, g, b);
    let mut nh = h + degrees / 360.0;
    nh -= nh.floor();
    let (or, og, ob) = hsl_to_rgb(nh, s, l);
    (
        or.clamp(0.0, 1.0),
        og.clamp(0.0, 1.0),
        ob.clamp(0.0, 1.0),
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(r: f32, g: f32, b: f32) -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
                t.set(x, y, 0, r);
                t.set(x, y, 1, g);
                t.set(x, y, 2, b);
                t.set(x, y, 3, 1.0);
            }
        }
        t
    }

    #[test]
    fn identity_is_noop_bitwise() {
        let src = solid(0.8, 0.2, 0.1);
        let mut dst = PixelTile::new();
        apply_hue_into(&src, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn gray_stays_gray() {
        let src = solid(0.4, 0.4, 0.4);
        let mut dst = PixelTile::new();
        apply_hue_into(&src, 90.0, &mut dst);
        assert!((dst.at(HALO, HALO, 0) - 0.4).abs() < 1e-5);
        assert!((dst.at(HALO, HALO, 1) - 0.4).abs() < 1e-5);
        assert!((dst.at(HALO, HALO, 2) - 0.4).abs() < 1e-5);
    }

    #[test]
    fn one_eighty_swaps_redish_toward_cyan() {
        // Pure-ish red → cyan-ish after 180°.
        let src = solid(0.9, 0.1, 0.1);
        let mut dst = PixelTile::new();
        apply_hue_into(&src, 180.0, &mut dst);
        let r = dst.at(HALO, HALO, 0);
        let g = dst.at(HALO, HALO, 1);
        let b = dst.at(HALO, HALO, 2);
        assert!(g > r && b > r);
    }
}
