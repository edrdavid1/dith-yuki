//! Alpha policy + pixel-perfect helpers (F4).

use super::context::{AlphaPolicy, PixelPerfect};

/// True if any opaque-ish pixel has alpha strictly between 0 and 255.
pub fn has_semi_transparent_rgba8(rgba: &[u8]) -> bool {
    for px in rgba.chunks_exact(4) {
        let a = px[3];
        if a > 0 && a < 255 {
            return true;
        }
    }
    false
}

pub fn count_semi_transparent_rgba8(rgba: &[u8]) -> usize {
    rgba.chunks_exact(4)
        .filter(|px| px[3] > 0 && px[3] < 255)
        .count()
}

/// Hard-threshold alpha in place (f32 linear buffer). Values below `threshold`
/// become 0; at/above become 1. Optionally clear RGB where transparent.
pub fn threshold_alpha_rgba_f32(rgba: &mut [f32], policy: &AlphaPolicy) {
    let t = policy.threshold.clamp(0.0, 1.0);
    for px in rgba.chunks_exact_mut(4) {
        let a = if px[3] >= t { 1.0 } else { 0.0 };
        px[3] = a;
        if policy.clear_rgb_in_transparent && a <= 0.0 {
            px[0] = 0.0;
            px[1] = 0.0;
            px[2] = 0.0;
        }
    }
}

/// Hard-threshold alpha on RGBA8 in place.
pub fn threshold_alpha_rgba8(rgba: &mut [u8], policy: &AlphaPolicy) {
    let t = (policy.threshold.clamp(0.0, 1.0) * 255.0).round() as u8;
    for px in rgba.chunks_exact_mut(4) {
        let a = if px[3] >= t { 255 } else { 0 };
        px[3] = a;
        if policy.clear_rgb_in_transparent && a == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        }
    }
}

/// Clear RGB for fully transparent pixels (does not change alpha).
pub fn clear_transparent_rgb_rgba8(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        if px[3] == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        }
    }
}

pub fn clear_transparent_rgb_rgba_f32(rgba: &mut [f32]) {
    for px in rgba.chunks_exact_mut(4) {
        if px[3] <= 0.0 {
            px[0] = 0.0;
            px[1] = 0.0;
            px[2] = 0.0;
        }
    }
}

/// Apply post-dither alpha policy to an RGBA8 buffer.
pub fn apply_alpha_policy_rgba8(rgba: &mut [u8], policy: &AlphaPolicy) {
    if policy.no_dither_alpha {
        // Soft alpha may remain from source copy — hard-threshold when policy says so.
        // Callers that already binary-dithered can still clear RGB.
        threshold_alpha_rgba8(rgba, policy);
    } else if policy.clear_rgb_in_transparent {
        clear_transparent_rgb_rgba8(rgba);
    }
}

/// Nearest-neighbor downsample of an RGBA f32 buffer to `nw×nh`.
pub fn nearest_downsample_rgba_f32(
    src: &[f32],
    sw: u32,
    sh: u32,
    nw: u32,
    nh: u32,
) -> Result<Vec<f32>, String> {
    if nw == 0 || nh == 0 {
        return Err("target size must be ≥ 1".into());
    }
    if nw == sw && nh == sh {
        return Ok(src.to_vec());
    }
    let expected = (sw as usize).saturating_mul(sh as usize).saturating_mul(4);
    if src.len() != expected {
        return Err(format!("rgba length {} != {}×{}×4", src.len(), sw, sh));
    }
    let mut out = vec![0.0f32; (nw as usize) * (nh as usize) * 4];
    for y in 0..nh as usize {
        let sy = (y as u64 * sh as u64 / nh as u64) as usize;
        for x in 0..nw as usize {
            let sx = (x as u64 * sw as u64 / nw as u64) as usize;
            let si = (sy * sw as usize + sx) * 4;
            let di = (y * nw as usize + x) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    Ok(out)
}

/// Nearest-neighbor upscale of an RGBA f32 buffer by an integer factor.
pub fn integer_nearest_upscale_rgba_f32(
    src: &[f32],
    width: u32,
    height: u32,
    factor: u32,
) -> Result<(Vec<f32>, u32, u32), String> {
    if factor == 0 {
        return Err("integer upscale factor must be ≥ 1".into());
    }
    if factor == 1 {
        return Ok((src.to_vec(), width, height));
    }
    let w = width as usize;
    let h = height as usize;
    let expected = w.saturating_mul(h).saturating_mul(4);
    if src.len() != expected {
        return Err(format!("rgba length {} != {}×{}×4", src.len(), width, height));
    }
    let nw = width.saturating_mul(factor);
    let nh = height.saturating_mul(factor);
    let mut out = vec![0.0f32; (nw as usize) * (nh as usize) * 4];
    let f = factor as usize;
    for y in 0..h {
        for x in 0..w {
            let si = (y * w + x) * 4;
            let px = &src[si..si + 4];
            for dy in 0..f {
                for dx in 0..f {
                    let di = ((y * f + dy) * (nw as usize) + (x * f + dx)) * 4;
                    out[di..di + 4].copy_from_slice(px);
                }
            }
        }
    }
    Ok((out, nw, nh))
}

/// Resolve working dither size from canvas + pixel-perfect settings.
/// Returns `(work_w, work_h, upscale_back)` where `work * upscale_back == canvas`
/// when an integer scale applies; otherwise returns canvas size with factor 1.
pub fn resolve_pixel_perfect_work_size(
    canvas_w: u32,
    canvas_h: u32,
    pp: &PixelPerfect,
) -> (u32, u32, u32) {
    if let Some((tw, th)) = pp.target_res {
        if tw > 0 && th > 0 && canvas_w % tw == 0 && canvas_h % th == 0 {
            let fx = canvas_w / tw;
            let fy = canvas_h / th;
            if fx == fy && fx >= 1 {
                return (tw, th, fx);
            }
        }
    }
    let factor = effective_upscale(pp);
    if factor > 1 && canvas_w % factor == 0 && canvas_h % factor == 0 {
        return (canvas_w / factor, canvas_h / factor, factor);
    }
    (canvas_w, canvas_h, 1)
}

/// Integer nearest-neighbor upscale (factor ≥ 1). factor 1 returns a copy.
pub fn integer_nearest_upscale_rgba8(
    src: &[u8],
    width: u32,
    height: u32,
    factor: u32,
) -> Result<(Vec<u8>, u32, u32), String> {
    if factor == 0 {
        return Err("integer upscale factor must be ≥ 1".into());
    }
    if factor == 1 {
        return Ok((src.to_vec(), width, height));
    }
    let w = width as usize;
    let h = height as usize;
    let expected = w.saturating_mul(h).saturating_mul(4);
    if src.len() != expected {
        return Err(format!(
            "rgba length {} != {}×{}×4",
            src.len(),
            width,
            height
        ));
    }
    let nw = width.saturating_mul(factor);
    let nh = height.saturating_mul(factor);
    let mut out = vec![0u8; (nw as usize) * (nh as usize) * 4];
    let f = factor as usize;
    for y in 0..h {
        for x in 0..w {
            let si = (y * w + x) * 4;
            let px = &src[si..si + 4];
            for dy in 0..f {
                for dx in 0..f {
                    let di = ((y * f + dy) * (nw as usize) + (x * f + dx)) * 4;
                    out[di..di + 4].copy_from_slice(px);
                }
            }
        }
    }
    Ok((out, nw, nh))
}

/// Whether pixel-perfect settings request a working resolution other than canvas.
pub fn pixel_perfect_active(pp: &PixelPerfect) -> bool {
    pp.target_res.is_some() || pp.integer_upscale.is_some_and(|f| f > 1)
}

/// Effective upscale factor (defaults to 1).
pub fn effective_upscale(pp: &PixelPerfect) -> u32 {
    pp.integer_upscale.unwrap_or(1).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_removes_soft_alpha() {
        let mut rgba = vec![
            255u8, 0, 0, 127, // soft below 0.5
            0, 255, 0, 255, // opaque
            0, 0, 255, 10, // soft low
        ];
        let policy = AlphaPolicy {
            threshold: 0.5,
            no_dither_alpha: true,
            clear_rgb_in_transparent: true,
        };
        assert!(has_semi_transparent_rgba8(&rgba));
        threshold_alpha_rgba8(&mut rgba, &policy);
        assert!(!has_semi_transparent_rgba8(&rgba));
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
        assert_eq!(&rgba[4..8], &[0, 255, 0, 255]);
        assert_eq!(&rgba[8..12], &[0, 0, 0, 0]);
    }

    #[test]
    fn transparent_rgb_cleared_no_bleed_fixture() {
        // Soft fringe next to opaque: after threshold+clear, transparent RGB is 0
        // so neighbor sampling cannot pick fringe color.
        let mut rgba = vec![
            200u8, 50, 50, 40, // soft red fringe
            10, 10, 10, 255, // opaque gray
        ];
        let policy = AlphaPolicy::default();
        threshold_alpha_rgba8(&mut rgba, &policy);
        assert_eq!(rgba[3], 0);
        assert_eq!(&rgba[0..3], &[0, 0, 0]);
        assert_eq!(&rgba[4..8], &[10, 10, 10, 255]);
    }

    #[test]
    fn nearest_upscale_2x() {
        let src = vec![1u8, 2, 3, 255, 4, 5, 6, 255];
        let (out, w, h) = integer_nearest_upscale_rgba8(&src, 2, 1, 2).unwrap();
        assert_eq!((w, h), (4, 2));
        assert_eq!(out.len(), 4 * 2 * 4);
        // Top-left 2×2 block from first pixel
        assert_eq!(&out[0..4], &[1, 2, 3, 255]);
        assert_eq!(&out[4..8], &[1, 2, 3, 255]);
        assert_eq!(&out[16..20], &[1, 2, 3, 255]);
    }

    #[test]
    fn resolve_work_size_from_upscale() {
        let mut pp = PixelPerfect::default();
        pp.integer_upscale = Some(2);
        assert_eq!(resolve_pixel_perfect_work_size(64, 32, &pp), (32, 16, 2));
        pp.integer_upscale = Some(3);
        // 64 not divisible by 3 → no-op
        assert_eq!(resolve_pixel_perfect_work_size(64, 32, &pp), (64, 32, 1));
    }

    #[test]
    fn downsample_then_upscale_roundtrip_dims() {
        let src = vec![0.2f32; 8 * 4 * 4];
        let down = nearest_downsample_rgba_f32(&src, 8, 4, 4, 2).unwrap();
        let (up, w, h) = integer_nearest_upscale_rgba_f32(&down, 4, 2, 2).unwrap();
        assert_eq!((w, h), (8, 4));
        assert_eq!(up.len(), src.len());
    }
}
