//! Highlights / Shadows — pointwise Post tone filter.
//!
//! Soft luminance masks (Rec.709) lift or crush darks and brights independently.
//! Identity when both amounts are exactly `0.0`.

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn luma709(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Apply highlights/shadows into `dst`. `src` and `dst` must not alias.
///
/// `shadows` / `highlights` in `[-1, 1]`: positive lifts that region.
pub fn apply_highlights_shadows_into(
    src: &PixelTile,
    shadows: f32,
    highlights: f32,
    dst: &mut PixelTile,
) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let shadows = shadows.clamp(-1.0, 1.0);
    let highlights = highlights.clamp(-1.0, 1.0);
    if shadows == 0.0 && highlights == 0.0 {
        dst.copy_from(src);
        return;
    }
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = src.at(x, y, 0);
            let g = src.at(x, y, 1);
            let b = src.at(x, y, 2);
            let a = src.at(x, y, 3);
            let (or, og, ob, oa) =
                apply_highlights_shadows_point_rgba(r, g, b, a, shadows, highlights);
            dst.set(x, y, 0, or);
            dst.set(x, y, 1, og);
            dst.set(x, y, 2, ob);
            dst.set(x, y, 3, oa);
        }
    }
}

#[inline]
pub fn apply_highlights_shadows_point_rgba(
    r: f32,
    g: f32,
    b: f32,
    a: f32,
    shadows: f32,
    highlights: f32,
) -> (f32, f32, f32, f32) {
    let y = luma709(r, g, b);
    let shadow_m = 1.0 - smoothstep(0.0, 0.5, y);
    let highlight_m = smoothstep(0.5, 1.0, y);
    let gain = (1.0 + shadows * shadow_m + highlights * highlight_m).max(0.0);
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
                t.set(x, y, 1, v);
                t.set(x, y, 2, v);
                t.set(x, y, 3, 1.0);
            }
        }
        t
    }

    #[test]
    fn identity_is_noop_bitwise() {
        let src = fill_gradient();
        let mut dst = PixelTile::new();
        apply_highlights_shadows_into(&src, 0.0, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn positive_shadows_lifts_dark_more_than_bright() {
        let mut src = PixelTile::new();
        // dark
        src.set(HALO, HALO, 0, 0.1);
        src.set(HALO, HALO, 1, 0.1);
        src.set(HALO, HALO, 2, 0.1);
        src.set(HALO, HALO, 3, 1.0);
        // bright
        src.set(HALO + 1, HALO, 0, 0.9);
        src.set(HALO + 1, HALO, 1, 0.9);
        src.set(HALO + 1, HALO, 2, 0.9);
        src.set(HALO + 1, HALO, 3, 1.0);
        let mut dst = PixelTile::new();
        apply_highlights_shadows_into(&src, 0.5, 0.0, &mut dst);
        let dark_delta = dst.at(HALO, HALO, 0) - 0.1;
        let bright_delta = dst.at(HALO + 1, HALO, 0) - 0.9;
        assert!(dark_delta > bright_delta + 0.01);
        assert!(dark_delta > 0.0);
    }

    #[test]
    fn positive_highlights_lifts_bright_more_than_dark() {
        let mut src = PixelTile::new();
        src.set(HALO, HALO, 0, 0.1);
        src.set(HALO, HALO, 1, 0.1);
        src.set(HALO, HALO, 2, 0.1);
        src.set(HALO, HALO, 3, 1.0);
        src.set(HALO + 1, HALO, 0, 0.9);
        src.set(HALO + 1, HALO, 1, 0.9);
        src.set(HALO + 1, HALO, 2, 0.9);
        src.set(HALO + 1, HALO, 3, 1.0);
        let mut dst = PixelTile::new();
        apply_highlights_shadows_into(&src, 0.0, 0.5, &mut dst);
        let dark_delta = dst.at(HALO, HALO, 0) - 0.1;
        let bright_delta = dst.at(HALO + 1, HALO, 0) - 0.9;
        assert!(bright_delta > dark_delta + 0.01);
        assert!(bright_delta > 0.0);
    }
}
