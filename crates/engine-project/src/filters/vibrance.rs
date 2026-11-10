//! Vibrance — pointwise Post color filter.
//!
//! Positive amounts boost low-saturation colors more; negative amounts
//! desaturate high-saturation colors more. Identity at `amount == 0.0`.

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

#[inline]
fn luma709(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Apply vibrance into `dst`. `src` and `dst` must not alias.
///
/// `amount` in `[-1, 1]`.
pub fn apply_vibrance_into(src: &PixelTile, amount: f32, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let amount = amount.clamp(-1.0, 1.0);
    if amount == 0.0 {
        dst.copy_from(src);
        return;
    }
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = src.at(x, y, 0);
            let g = src.at(x, y, 1);
            let b = src.at(x, y, 2);
            let a = src.at(x, y, 3);
            let (or, og, ob, oa) = apply_vibrance_point_rgba(r, g, b, a, amount);
            dst.set(x, y, 0, or);
            dst.set(x, y, 1, og);
            dst.set(x, y, 2, ob);
            dst.set(x, y, 3, oa);
        }
    }
}

#[inline]
pub fn apply_vibrance_point_rgba(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    amount: f32,
) -> (f32, f32, f32, f32) {
    let maxc = r.max(g).max(b);
    let minc = r.min(g).min(b);
    let sat = if maxc > 1e-6 {
        (maxc - minc) / maxc
    } else {
        0.0
    };
    // Positive: protect already-vivid; negative: pull vivid toward gray first.
    let weight = if amount >= 0.0 { 1.0 - sat } else { sat };
    let scale = 1.0 + amount * weight;
    let y = luma709(r, g, b);
    (
        (y + (r - y) * scale).clamp(0.0, 1.0),
        (y + (g - y) * scale).clamp(0.0, 1.0),
        (y + (b - y) * scale).clamp(0.0, 1.0),
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
        let src = solid(0.4, 0.3, 0.2);
        let mut dst = PixelTile::new();
        apply_vibrance_into(&src, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    fn rel_sat(r: f32, g: f32, b: f32) -> f32 {
        let maxc = r.max(g).max(b);
        let minc = r.min(g).min(b);
        if maxc > 1e-6 {
            (maxc - minc) / maxc
        } else {
            0.0
        }
    }

    #[test]
    fn positive_raises_pastel_sat_more_than_vivid() {
        let pastel = solid(0.55, 0.50, 0.48);
        let vivid = solid(0.9, 0.1, 0.1);
        let mut out_p = PixelTile::new();
        let mut out_v = PixelTile::new();
        apply_vibrance_into(&pastel, 1.0, &mut out_p);
        apply_vibrance_into(&vivid, 1.0, &mut out_v);
        let dp = rel_sat(
            out_p.at(HALO, HALO, 0),
            out_p.at(HALO, HALO, 1),
            out_p.at(HALO, HALO, 2),
        ) - rel_sat(0.55, 0.50, 0.48);
        let dv = rel_sat(
            out_v.at(HALO, HALO, 0),
            out_v.at(HALO, HALO, 1),
            out_v.at(HALO, HALO, 2),
        ) - rel_sat(0.9, 0.1, 0.1);
        assert!(dp > dv);
        assert!(dp > 1e-4);
    }

    #[test]
    fn gray_stays_gray() {
        let src = solid(0.4, 0.4, 0.4);
        let mut dst = PixelTile::new();
        apply_vibrance_into(&src, 1.0, &mut dst);
        assert!((dst.at(HALO, HALO, 0) - 0.4).abs() < 1e-5);
        assert!((dst.at(HALO, HALO, 1) - 0.4).abs() < 1e-5);
        assert!((dst.at(HALO, HALO, 2) - 0.4).abs() < 1e-5);
    }
}
