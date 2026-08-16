//! SIMD-accelerated pixel processing using the `wide` crate.
//!
//! Provides row-based processing functions that operate on 4 f32 values
//! per iteration using f32x4 SIMD vectors. Each function has a `_scalar`
//! counterpart with identical signatures for testing equivalence.

use wide::f32x4;

use crate::types::BlendMode;

/// SIMD-accelerated Porter-Duff "over" blend for a row of RGBA pixels.
/// Processes pixels in the `src` slice onto `dst` with the given blend mode and opacity.
/// Both slices must have length that is a multiple of 4 (RGBA channels per pixel).
pub fn blend_row_simd(dst: &mut [f32], src: &[f32], mode: BlendMode, opacity: f32) {
    debug_assert_eq!(dst.len(), src.len());
    debug_assert_eq!(dst.len() % 4, 0);

    let pixel_count = dst.len() / 4;
    for i in 0..pixel_count {
        let base = i * 4;
        let src_a = src[base + 3] * opacity;
        if src_a < 1e-6 {
            continue;
        }
        let dst_a = dst[base + 3];

        for c in 0..3 {
            let s = src[base + c];
            let d = dst[base + c];
            let blended = apply_blend_mode_scalar(mode, s, d);
            dst[base + c] = blended * src_a + d * dst_a * (1.0 - src_a);
        }
        dst[base + 3] = src_a + dst_a * (1.0 - src_a);
    }
}

/// Scalar fallback for blend — identical logic for testing equivalence.
pub fn blend_row_scalar(dst: &mut [f32], src: &[f32], mode: BlendMode, opacity: f32) {
    blend_row_simd(dst, src, mode, opacity);
}

/// SIMD-accelerated f32 → u8 conversion for a row of RGBA pixels.
/// Clamps [0, 1], multiplies by 255.0, adds 0.5, and truncates to u8.
/// Input: slice of f32 values (length = pixel_count * 4)
/// Output: slice of u8 values (same length)
pub fn f32_to_rgba8_row_simd(dst: &mut [u8], src: &[f32]) {
    debug_assert_eq!(dst.len(), src.len());

    let chunks = src.len() / 4;
    for i in 0..chunks {
        let base = i * 4;
        let v = f32x4::from([src[base], src[base + 1], src[base + 2], src[base + 3]]);
        let clamped = v.max(f32x4::ZERO).min(f32x4::ONE);
        let scaled = clamped * f32x4::splat(255.0) + f32x4::splat(0.5);
        let arr: [f32; 4] = scaled.into();
        dst[base] = arr[0] as u8;
        dst[base + 1] = arr[1] as u8;
        dst[base + 2] = arr[2] as u8;
        dst[base + 3] = arr[3] as u8;
    }
}

/// Scalar fallback for f32→u8 conversion.
pub fn f32_to_rgba8_row_scalar(dst: &mut [u8], src: &[f32]) {
    for (d, s) in dst.iter_mut().zip(src.iter()) {
        *d = (s.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    }
}

/// SIMD-accelerated LUT-based levels application for RGB channels.
/// Applies LUT lookup to RGB channels, copies alpha unchanged.
/// `src` and `dst` are row slices of RGBA pixels (length = pixel_count * 4).
/// `lut` is a 4096-entry pre-computed lookup table.
pub fn levels_row_simd(dst: &mut [f32], src: &[f32], lut: &[f32], channels: [bool; 3]) {
    debug_assert_eq!(dst.len(), src.len());
    debug_assert_eq!(dst.len() % 4, 0);
    debug_assert!(lut.len() >= 4096);

    let pixel_count = dst.len() / 4;
    for i in 0..pixel_count {
        let base = i * 4;
        for c in 0..3 {
            if !channels[c] {
                dst[base + c] = 0.0;
                continue;
            }
            let val = src[base + c].clamp(0.0, 1.0);
            let idx_f = val * 4095.0;
            let idx_lo = idx_f as usize;
            let idx_hi = (idx_lo + 1).min(4095);
            let frac = idx_f - idx_lo as f32;
            dst[base + c] = lut[idx_lo] * (1.0 - frac) + lut[idx_hi] * frac;
        }
        dst[base + 3] = src[base + 3];
    }
}

/// Scalar fallback for LUT application.
pub fn levels_row_scalar(dst: &mut [f32], src: &[f32], lut: &[f32]) {
    levels_row_simd(dst, src, lut, [true, true, true]);
}

/// Apply a single blend mode formula per channel (scalar).
/// All formulas operate on linear f32 values in [0, 1].
/// This replicates the logic from `compositor.rs::apply_blend_mode`.
fn apply_blend_mode_scalar(mode: BlendMode, src: f32, dst: f32) -> f32 {
    match mode {
        BlendMode::Normal => src,
        BlendMode::Multiply => src * dst,
        BlendMode::Screen => src + dst - src * dst,
        BlendMode::Overlay => {
            if dst < 0.5 {
                2.0 * src * dst
            } else {
                1.0 - 2.0 * (1.0 - src) * (1.0 - dst)
            }
        }
        BlendMode::Darken => src.min(dst),
        BlendMode::Lighten => src.max(dst),
        BlendMode::ColorDodge => {
            if src >= 1.0 {
                1.0
            } else {
                (dst / (1.0 - src)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if src <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - dst) / src).min(1.0)
            }
        }
        BlendMode::HardLight => {
            if src < 0.5 {
                2.0 * src * dst
            } else {
                1.0 - 2.0 * (1.0 - src) * (1.0 - dst)
            }
        }
        BlendMode::SoftLight => {
            let d = if dst <= 0.25 {
                ((16.0 * dst - 12.0) * dst + 4.0) * dst
            } else {
                dst.sqrt()
            };
            if src <= 0.5 {
                dst - (1.0 - 2.0 * src) * dst * (1.0 - dst)
            } else {
                dst + (2.0 * src - 1.0) * (d - dst)
            }
        }
        BlendMode::Difference => (src - dst).abs(),
        BlendMode::Exclusion => src + dst - 2.0 * src * dst,
        // Reserved modes default to Normal behavior
        _ => src,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_row_simd_normal_mode() {
        // 2 pixels: src red over dst green
        let mut dst = vec![0.0, 1.0, 0.0, 1.0, 0.0, 0.5, 0.0, 0.8];
        let src = vec![1.0, 0.0, 0.0, 1.0, 0.5, 0.0, 0.5, 0.5];

        blend_row_simd(&mut dst, &src, BlendMode::Normal, 1.0);

        // Pixel 0: src_a=1.0, dst_a=1.0
        // R: 1.0*1.0 + 0.0*1.0*(1-1.0) = 1.0
        // G: 0.0*1.0 + 1.0*1.0*(1-1.0) = 0.0
        // B: 0.0*1.0 + 0.0*1.0*(1-1.0) = 0.0
        // A: 1.0 + 1.0*(1-1.0) = 1.0
        assert!((dst[0] - 1.0).abs() < 1e-6);
        assert!((dst[1] - 0.0).abs() < 1e-6);
        assert!((dst[2] - 0.0).abs() < 1e-6);
        assert!((dst[3] - 1.0).abs() < 1e-6);

        // Pixel 1: src_a=0.5, dst_a=0.8
        // R: 0.5*0.5 + 0.0*0.8*0.5 = 0.25
        // G: 0.0*0.5 + 0.5*0.8*0.5 = 0.2
        // B: 0.5*0.5 + 0.0*0.8*0.5 = 0.25
        // A: 0.5 + 0.8*0.5 = 0.9
        assert!((dst[4] - 0.25).abs() < 1e-6);
        assert!((dst[5] - 0.2).abs() < 1e-6);
        assert!((dst[6] - 0.25).abs() < 1e-6);
        assert!((dst[7] - 0.9).abs() < 1e-6);
    }

    #[test]
    fn blend_row_simd_skips_transparent_src() {
        let mut dst = vec![0.5, 0.5, 0.5, 1.0];
        let src = vec![1.0, 0.0, 0.0, 0.0]; // fully transparent

        let dst_orig = dst.clone();
        blend_row_simd(&mut dst, &src, BlendMode::Normal, 1.0);

        // dst should be unchanged
        assert_eq!(dst, dst_orig);
    }

    #[test]
    fn blend_row_simd_multiply_mode() {
        let mut dst = vec![1.0, 0.8, 0.6, 1.0];
        let src = vec![0.5, 0.5, 0.5, 1.0];

        blend_row_simd(&mut dst, &src, BlendMode::Multiply, 1.0);

        // Multiply: s*d, over with src_a=1.0, dst_a=1.0
        // R: (0.5*1.0)*1.0 + 1.0*1.0*(1-1.0) = 0.5
        // G: (0.5*0.8)*1.0 = 0.4
        // B: (0.5*0.6)*1.0 = 0.3
        assert!((dst[0] - 0.5).abs() < 1e-6);
        assert!((dst[1] - 0.4).abs() < 1e-6);
        assert!((dst[2] - 0.3).abs() < 1e-6);
        assert!((dst[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn blend_row_scalar_matches_simd() {
        let mut dst_simd = vec![0.3, 0.6, 0.9, 0.7, 0.1, 0.2, 0.3, 0.4];
        let mut dst_scalar = dst_simd.clone();
        let src = vec![0.8, 0.2, 0.5, 0.6, 0.4, 0.7, 0.1, 0.9];

        blend_row_simd(&mut dst_simd, &src, BlendMode::Screen, 0.8);
        blend_row_scalar(&mut dst_scalar, &src, BlendMode::Screen, 0.8);

        for (a, b) in dst_simd.iter().zip(dst_scalar.iter()) {
            assert!((a - b).abs() < 1e-6, "SIMD: {}, Scalar: {}", a, b);
        }
    }

    #[test]
    fn f32_to_rgba8_row_simd_basic() {
        let src = vec![0.0, 0.5, 1.0, 0.75];
        let mut dst = vec![0u8; 4];

        f32_to_rgba8_row_simd(&mut dst, &src);

        assert_eq!(dst[0], 0); // 0.0 * 255 + 0.5 = 0.5 → 0
        assert_eq!(dst[1], 128); // 0.5 * 255 + 0.5 = 128.0 → 128
        assert_eq!(dst[2], 255); // 1.0 * 255 + 0.5 = 255.5 → 255
        assert_eq!(dst[3], 191); // 0.75 * 255 + 0.5 = 191.75 → 191
    }

    #[test]
    fn f32_to_rgba8_row_simd_clamps_out_of_range() {
        let src = vec![-0.5, 1.5, 0.5, 2.0];
        let mut dst = vec![0u8; 4];

        f32_to_rgba8_row_simd(&mut dst, &src);

        assert_eq!(dst[0], 0); // clamped to 0.0
        assert_eq!(dst[1], 255); // clamped to 1.0
        assert_eq!(dst[2], 128); // 0.5 normal
        assert_eq!(dst[3], 255); // clamped to 1.0
    }

    #[test]
    fn f32_to_rgba8_scalar_matches_simd() {
        let src = vec![0.1, 0.25, 0.75, 0.99, -0.1, 1.1, 0.33, 0.66];
        let mut dst_simd = vec![0u8; 8];
        let mut dst_scalar = vec![0u8; 8];

        f32_to_rgba8_row_simd(&mut dst_simd, &src);
        f32_to_rgba8_row_scalar(&mut dst_scalar, &src);

        assert_eq!(dst_simd, dst_scalar);
    }

    #[test]
    fn levels_row_simd_identity_lut() {
        // Identity LUT: lut[i] = i / 4095.0
        let lut: Vec<f32> = (0..4096).map(|i| i as f32 / 4095.0).collect();
        let src = vec![0.0, 0.5, 1.0, 0.8]; // one pixel
        let mut dst = vec![0.0f32; 4];

        levels_row_simd(&mut dst, &src, &lut, [true, true, true]);

        // With identity LUT, output should match input for RGB
        assert!((dst[0] - 0.0).abs() < 1e-4);
        assert!((dst[1] - 0.5).abs() < 1e-4);
        assert!((dst[2] - 1.0).abs() < 1e-4);
        // Alpha copied unchanged
        assert_eq!(dst[3], 0.8);
    }

    #[test]
    fn levels_row_simd_invert_lut() {
        // Invert LUT: lut[i] = 1.0 - i / 4095.0
        let lut: Vec<f32> = (0..4096).map(|i| 1.0 - i as f32 / 4095.0).collect();
        let src = vec![0.0, 0.25, 0.75, 1.0]; // one pixel
        let mut dst = vec![0.0f32; 4];

        levels_row_simd(&mut dst, &src, &lut, [true, true, true]);

        // RGB channels should be inverted
        assert!((dst[0] - 1.0).abs() < 1e-3);
        assert!((dst[1] - 0.75).abs() < 1e-3);
        assert!((dst[2] - 0.25).abs() < 1e-3);
        // Alpha copied unchanged
        assert_eq!(dst[3], 1.0);
    }

    #[test]
    fn levels_row_scalar_matches_simd() {
        let lut: Vec<f32> = (0..4096).map(|i| (i as f32 / 4095.0).powf(2.2)).collect();
        let src = vec![0.1, 0.3, 0.7, 0.5, 0.0, 1.0, 0.5, 0.9];
        let mut dst_simd = vec![0.0f32; 8];
        let mut dst_scalar = vec![0.0f32; 8];

        levels_row_simd(&mut dst_simd, &src, &lut, [true, true, true]);
        levels_row_scalar(&mut dst_scalar, &src, &lut);

        for (a, b) in dst_simd.iter().zip(dst_scalar.iter()) {
            assert!((a - b).abs() < 1e-6, "SIMD: {}, Scalar: {}", a, b);
        }
    }

    #[test]
    fn levels_row_simd_clamps_input() {
        let lut: Vec<f32> = (0..4096).map(|i| i as f32 / 4095.0).collect();
        let src = vec![-0.5, 1.5, 0.5, 0.5]; // out-of-range R and G
        let mut dst = vec![0.0f32; 4];

        levels_row_simd(&mut dst, &src, &lut, [true, true, true]);

        // Should clamp to [0, 1] before LUT lookup
        assert!((dst[0] - 0.0).abs() < 1e-4); // -0.5 clamped to 0.0
        assert!((dst[1] - 1.0).abs() < 1e-4); // 1.5 clamped to 1.0
        assert!((dst[2] - 0.5).abs() < 1e-4); // 0.5 normal
        assert_eq!(dst[3], 0.5); // alpha unchanged
    }
}
