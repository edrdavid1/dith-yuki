//! Block downsample for mega-pixel dithering (`pixel_size > 1`).
//!
//! When `dither_alpha` is on, alpha is the arithmetic mean of the `N×N` block and
//! RGB is the **alpha-weighted** mean (straight alpha). See
//! `docs/adr/alpha-grid-align.md`.

/// Reduce an `N×N` block of straight-alpha RGBA (`f32`, channels in `[0,1]`).
///
/// `pixels` is row-major `[r,g,b,a, …]` with `n * n` pixels (`n >= 1`).
/// Returns `[block_r, block_g, block_b, block_a]`.
#[inline]
pub fn reduce_block_rgba_f32(pixels: &[[f32; 4]], n: usize) -> [f32; 4] {
    debug_assert!(n >= 1);
    debug_assert_eq!(pixels.len(), n * n);
    let count = (n * n) as f32;
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
    let block_a = sum_a / count;
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

/// Gather `ps×ps` samples from a linear RGBA `f32` buffer (row-major, 4 channels).
/// Out-of-bounds samples are skipped (not counted); if none are in-bounds, returns zeros.
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
    if pix.is_empty() {
        return [0.0, 0.0, 0.0, 0.0];
    }
    // Spec: mean over N×N. Partial edge blocks still divide by N×N (missing = a=0).
    while pix.len() < ps * ps {
        pix.push([0.0, 0.0, 0.0, 0.0]);
    }
    reduce_block_rgba_f32(&pix, ps)
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
}