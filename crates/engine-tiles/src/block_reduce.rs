//! Block downsample for mega-pixel dithering (`pixel_size > 1`).
//!
//! When `dither_alpha` is on, alpha is the arithmetic mean of the block and
//! RGB is the **alpha-weighted** mean (straight alpha). A full `N×N` block
//! divides by `N×N`. A gather that crosses the document edge divides by the
//! in-bounds sample count only. See `docs/adr/alpha-grid-align.md`.

/// Reduce an `N×N` block of straight-alpha RGBA (`f32`, channels in `[0,1]`).
///
/// `pixels` is row-major `[r,g,b,a, …]` with `n * n` pixels (`n >= 1`).
/// Returns `[block_r, block_g, block_b, block_a]`.
#[inline]
pub fn reduce_block_rgba_f32(pixels: &[[f32; 4]], n: usize) -> [f32; 4] {
    debug_assert!(n >= 1);
    debug_assert_eq!(pixels.len(), n * n);
    // `count == n * n` keeps this path bit-identical to dividing by `n * n`.
    reduce_straight_mean_f32(pixels, n * n)
}

/// Straight-alpha mean over `count` samples.
///
/// Alpha is the arithmetic mean. RGB is the alpha-weighted mean. `count == 0`
/// returns zeros (no division). A positive `count` with `sum_a == 0` returns
/// transparent black, matching [`reduce_block_rgba_f32`].
#[inline]
fn reduce_straight_mean_f32(pixels: &[[f32; 4]], count: usize) -> [f32; 4] {
    if count == 0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    debug_assert_eq!(pixels.len(), count);
    let count_f = count as f32;
    let mut sum_a = 0.0f32;
    let mut sum_r = 0.0f32;
    let mut sum_g = 0.0f32;
    let mut sum_b = 0.0f32;
    for p in pixels {
        let a = p[3];
        sum_a += a;
        // Straight alpha: weight RGB by a. Premultiplied input would use p[c] alone
        // in the numerator — PixelTile / document raw are straight (see Adjust
        // unpremultiply before leaving blur).
        sum_r += p[0] * a;
        sum_g += p[1] * a;
        sum_b += p[2] * a;
    }
    let block_a = sum_a / count_f;
    if sum_a > 0.0 {
        [sum_r / sum_a, sum_g / sum_a, sum_b / sum_a, block_a]
    } else {
        [0.0, 0.0, 0.0, block_a]
    }
}

/// Same reduction for display-referred `u8` RGBA (GPU block lattice).
#[inline]
pub fn reduce_block_rgba_u8(pixels: &[[u8; 4]], n: usize) -> [u8; 4] {
    debug_assert!(n >= 1);
    debug_assert_eq!(pixels.len(), n * n);
    let count = (n * n) as f32;
    let mut sum_a = 0.0f32;
    let mut sum_r = 0.0f32;
    let mut sum_g = 0.0f32;
    let mut sum_b = 0.0f32;
    for p in pixels {
        let a = p[3] as f32;
        sum_a += a;
        sum_r += p[0] as f32 * a;
        sum_g += p[1] as f32 * a;
        sum_b += p[2] as f32 * a;
    }
    let block_a = (sum_a / count).round().clamp(0.0, 255.0) as u8;
    if sum_a > 0.0 {
        [
            (sum_r / sum_a).round().clamp(0.0, 255.0) as u8,
            (sum_g / sum_a).round().clamp(0.0, 255.0) as u8,
            (sum_b / sum_a).round().clamp(0.0, 255.0) as u8,
            block_a,
        ]
    } else {
        [0, 0, 0, 0]
    }
}

/// Gather the in-bounds samples of a `ps×ps` block from a linear RGBA `f32`
/// buffer (row-major, 4 channels) and reduce them with the same straight-alpha
/// mean as [`reduce_block_rgba_f32`].
///
/// The mean is `sum(in-bounds pixels) / count(in-bounds pixels)`. Samples past
/// `width`/`height`, or past the end of `rgba`, contribute to no channel,
/// including alpha. A block that lies fully inside the buffer has `ps×ps`
/// samples, so the result is bit-identical to [`reduce_block_rgba_f32`] on
/// those pixels. A block with no in-bounds samples returns `[0, 0, 0, 0]`.
pub fn reduce_block_from_rgba_f32(
    rgba: &[f32],
    width: u32,
    height: u32,
    origin_x: u32,
    origin_y: u32,
    ps: u32,
) -> [f32; 4] {
    let ps = ps.max(1) as usize;
    let w = width as usize;
    let h = height as usize;
    let mut pix = Vec::with_capacity(ps * ps);
    for dy in 0..ps {
        let y = origin_y as usize + dy;
        if y >= h {
            continue;
        }
        for dx in 0..ps {
            let x = origin_x as usize + dx;
            if x >= w {
                continue;
            }
            let i = (y * w + x) * 4;
            if i + 3 >= rgba.len() {
                continue;
            }
            pix.push([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]);
        }
    }
    // Full blocks take the `n * n` divisor path. Partial blocks divide by the
    // number of samples actually inside the document.
    if pix.len() == ps * ps {
        reduce_block_rgba_f32(&pix, ps)
    } else {
        reduce_straight_mean_f32(&pix, pix.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_alpha_is_mean() {
        // 4×4: alternating opaque/transparent rows → mean 0.5
        let mut pix = Vec::with_capacity(16);
        for row in 0..4 {
            let a = if row % 2 == 0 { 1.0 } else { 0.0 };
            for _ in 0..4 {
                pix.push([1.0, 0.0, 0.0, a]);
            }
        }
        let r = reduce_block_rgba_f32(&pix, 4);
        assert!((r[3] - 0.5).abs() < 1e-6, "block_alpha={}", r[3]);
    }

    #[test]
    fn block_rgb_weighted_by_alpha() {
        let mut pix = vec![[0.0, 0.0, 0.0, 0.0]; 16];
        pix[0] = [1.0, 0.0, 0.0, 1.0]; // one opaque red
        let r = reduce_block_rgba_f32(&pix, 4);
        assert!((r[0] - 1.0).abs() < 1e-6, "r={}", r[0]);
        assert!((r[1]).abs() < 1e-6);
        assert!((r[2]).abs() < 1e-6);
        assert!((r[3] - 1.0 / 16.0).abs() < 1e-6);
    }

    #[test]
    fn block_rgb_all_transparent() {
        let pix = [[0.0f32, 0.0, 0.0, 0.0]; 16];
        let r = reduce_block_rgba_f32(&pix, 4);
        assert_eq!(r, [0.0, 0.0, 0.0, 0.0]);
        assert!(r[0].is_finite() && r[1].is_finite() && r[2].is_finite());
    }

    #[test]
    fn premultiplied_input_not_double_multiplied() {
        // Document path is straight alpha. If someone passed premultiplied
        // half-red (r=0.5,a=0.5), straight weighting would wrongly yield r=0.5
        // after /sum_a only if we did NOT multiply again — verify our contract:
        // straight red (1,0,0,0.5) → weighted r = 1.0.
        let pix = [
            [1.0, 0.0, 0.0, 0.5],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0],
        ];
        let r = reduce_block_rgba_f32(&pix, 2);
        assert!(
            (r[0] - 1.0).abs() < 1e-6,
            "straight alpha must not treat buffer as premul; got r={}",
            r[0]
        );
    }

    fn fill_rgba(width: u32, height: u32, color: [f32; 4]) -> Vec<f32> {
        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            rgba.extend_from_slice(&color);
        }
        rgba
    }

    /// Dyadic channels so the mean of identical samples is bit-exact.
    const SOLID: [f32; 4] = [0.25, 0.5, 0.75, 1.0];

    #[test]
    fn in_bounds_block_matches_full_reducer() {
        // 8×8, pixel_size 4. Block (4, 4) sits fully inside, including a
        // transparent sample that must still count toward the divisor.
        let w = 8u32;
        let h = 8u32;
        let ps = 4u32;
        let mut rgba = vec![0.0f32; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                rgba[i] = (x as f32) / 8.0;
                rgba[i + 1] = (y as f32) / 8.0;
                rgba[i + 2] = 0.5;
                rgba[i + 3] = if (x + y) % 3 == 0 { 0.0 } else { 1.0 };
            }
        }
        let origin_x = 4u32;
        let origin_y = 4u32;
        let mut expected_pix = Vec::with_capacity((ps * ps) as usize);
        for dy in 0..ps {
            for dx in 0..ps {
                let x = origin_x + dx;
                let y = origin_y + dy;
                let i = ((y * w + x) * 4) as usize;
                expected_pix.push([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]);
            }
        }
        let expected = reduce_block_rgba_f32(&expected_pix, ps as usize);
        let got = reduce_block_from_rgba_f32(&rgba, w, h, origin_x, origin_y, ps);
        assert_eq!(got, expected);
    }

    #[test]
    fn right_edge_partial_block_keeps_source_color() {
        // width 6, ps 4 → last column is 2 px, not padded out to 4.
        let rgba = fill_rgba(6, 4, SOLID);
        let got = reduce_block_from_rgba_f32(&rgba, 6, 4, 4, 0, 4);
        assert_eq!(got, SOLID);
    }

    #[test]
    fn bottom_edge_partial_block_keeps_source_color() {
        // height 6, ps 4 → last row is 2 px.
        let rgba = fill_rgba(4, 6, SOLID);
        let got = reduce_block_from_rgba_f32(&rgba, 4, 6, 0, 4, 4);
        assert_eq!(got, SOLID);
    }

    #[test]
    fn corner_partial_block_keeps_source_color() {
        // 6×6, ps 4 → corner block is 2×2.
        let rgba = fill_rgba(6, 6, SOLID);
        let got = reduce_block_from_rgba_f32(&rgba, 6, 6, 4, 4, 4);
        assert_eq!(got, SOLID);
    }

    #[test]
    fn block_fully_outside_stays_zero() {
        let rgba = fill_rgba(4, 4, SOLID);
        let got = reduce_block_from_rgba_f32(&rgba, 4, 4, 4, 4, 4);
        assert_eq!(got, [0.0, 0.0, 0.0, 0.0]);
        assert!(got.iter().all(|c| c.is_finite()));
    }

    #[test]
    fn canvas_100x80_pixel_size_8_edge_column_matches_source() {
        // 100 = 12*8 + 4, 80 = 10*8. The last column is 4 px wide; every
        // painted pixel, including that column, must keep the source color.
        const W: u32 = 100;
        const H: u32 = 80;
        const PS: u32 = 8;
        let rgba = fill_rgba(W, H, SOLID);
        for y in 0..H {
            for x in 0..W {
                let ox = (x / PS) * PS;
                let oy = (y / PS) * PS;
                let got = reduce_block_from_rgba_f32(&rgba, W, H, ox, oy, PS);
                assert_eq!(got, SOLID, "pixel ({x}, {y}) block ({ox}, {oy})");
            }
        }
    }
}