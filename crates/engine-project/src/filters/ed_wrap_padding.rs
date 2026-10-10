//! Approximate seamless wrap-padding for error diffusion.
//!
//! Exact ED periodicity is impossible (sequential wavefront). This path pads
//! opposite edges (left←right, top←bottom) by
//! [`DiffusionKernel::edge_margin`](= `pixel_size × max_offset()`), runs ED on
//! the padded buffer, then crops back. Result is **approximate only**.

use crate::error::EngineError;
use crate::filter::{DiffusionKernel, DitherColorMode, DitherParamsV2};

/// Margin in pixels for wrap-padding — same formula as cross-tile ED residuals.
pub fn wrap_padding_margin(kernel: DiffusionKernel, pixel_size: u32) -> usize {
    kernel.edge_margin(pixel_size)
}

/// Pad RGBA f32 by copying opposite edges (toroidal seed).
///
/// Layout: output size `(w + 2m) × (h + 2m)`. Center holds the original;
/// left strip = rightmost `m` columns of src; right strip = leftmost `m`;
/// top/bottom likewise; corners from the opposite corner.
pub fn wrap_pad_rgba_f32(src: &[f32], width: u32, height: u32, margin: usize) -> (Vec<f32>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    let m = margin;
    let pw = w + 2 * m;
    let ph = h + 2 * m;
    let mut out = vec![0.0f32; pw * ph * 4];

    let sample = |sx: isize, sy: isize| -> [f32; 4] {
        let x = sx.rem_euclid(w as isize) as usize;
        let y = sy.rem_euclid(h as isize) as usize;
        let i = (y * w + x) * 4;
        [src[i], src[i + 1], src[i + 2], src[i + 3]]
    };

    for py in 0..ph {
        for px in 0..pw {
            let sx = px as isize - m as isize;
            let sy = py as isize - m as isize;
            let c = sample(sx, sy);
            let di = (py * pw + px) * 4;
            out[di] = c[0];
            out[di + 1] = c[1];
            out[di + 2] = c[2];
            out[di + 3] = c[3];
        }
    }
    (out, pw as u32, ph as u32)
}

/// Crop the center `width × height` out of a padded buffer.
pub fn crop_rgba_f32(
    padded: &[f32],
    padded_w: u32,
    padded_h: u32,
    margin: usize,
    width: u32,
    height: u32,
) -> Result<Vec<f32>, EngineError> {
    let pw = padded_w as usize;
    let ph = padded_h as usize;
    let w = width as usize;
    let h = height as usize;
    let m = margin;
    if pw != w + 2 * m || ph != h + 2 * m || padded.len() != pw * ph * 4 {
        return Err(EngineError::invalid_filter_params(
            "wrap-padding crop size mismatch",
        ));
    }
    let mut out = vec![0.0f32; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let si = ((y + m) * pw + (x + m)) * 4;
            let di = (y * w + x) * 4;
            out[di] = padded[si];
            out[di + 1] = padded[si + 1];
            out[di + 2] = padded[si + 2];
            out[di + 3] = padded[si + 3];
        }
    }
    Ok(out)
}

#[inline]
fn quantize_uniform(value: f32, levels: f32) -> f32 {
    let scaled = value * (levels - 1.0);
    scaled.round().clamp(0.0, levels - 1.0) / (levels - 1.0)
}

#[inline]
fn to_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Sequential ED on a contiguous RGBA f32 buffer (no tile residuals).
fn apply_ed_buffer_inplace(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    kernel: DiffusionKernel,
) -> Result<(), EngineError> {
    let w = width as usize;
    let h = height as usize;
    if rgba.len() != w * h * 4 || w == 0 || h == 0 {
        return Err(EngineError::invalid_filter_params(
            "wrap-padding ED invalid buffer",
        ));
    }
    let levels = params.levels as f32;
    let ps = params.pixel_size.max(1) as i32;
    let serpentine = matches!(
        kernel,
        DiffusionKernel::Ostromoukhov | DiffusionKernel::ZhouFang
    ) || params.serpentine;
    let offsets = kernel.offsets();
    let mut err_buf = vec![0.0f32; w * h * 3];

    for y in 0..h {
        let scan_parity = if ps > 1 {
            (y as i32).div_euclid(ps)
        } else {
            y as i32
        };
        let row_dir = if serpentine && scan_parity.rem_euclid(2) == 1 {
            -1
        } else {
            1
        };
        for i in 0..w {
            let x = if row_dir > 0 { i } else { w - 1 - i };
            let pi = (y * w + x) * 4;
            let ei = (y * w + x) * 3;

            let src_a = rgba[pi + 3];
            let out_a = params.map_alpha(src_a, 0.5);
            rgba[pi + 3] = out_a;

            // Non-representative pixels in a block copy the block origin later.
            let bx = (x as i32).div_euclid(ps) * ps;
            let by = (y as i32).div_euclid(ps) * ps;
            if ps > 1 && (x as i32 != bx || y as i32 != by) {
                continue;
            }

            if params.dither_alpha && out_a <= 0.0 {
                rgba[pi] = 0.0;
                rgba[pi + 1] = 0.0;
                rgba[pi + 2] = 0.0;
                continue;
            }

            let (sr, sg, sb) = (rgba[pi], rgba[pi + 1], rgba[pi + 2]);
            let (ar, ag, ab) = (
                (sr + err_buf[ei]).clamp(0.0, 1.0),
                (sg + err_buf[ei + 1]).clamp(0.0, 1.0),
                (sb + err_buf[ei + 2]).clamp(0.0, 1.0),
            );

            let (qr, qg, qb) = match params.color_mode {
                DitherColorMode::Grayscale => {
                    let lum = to_luminance(ar, ag, ab);
                    let q = quantize_uniform(lum, levels);
                    (q, q, q)
                }
                DitherColorMode::Rgb => (
                    quantize_uniform(ar, levels),
                    quantize_uniform(ag, levels),
                    quantize_uniform(ab, levels),
                ),
            };

            let err = [ar - qr, ag - qg, ab - qb];
            rgba[pi] = qr;
            rgba[pi + 1] = qg;
            rgba[pi + 2] = qb;

            // Fill the rest of the block with the representative result.
            if ps > 1 {
                let x1 = (bx + ps).min(w as i32) as usize;
                let y1 = (by + ps).min(h as i32) as usize;
                for yy in by as usize..y1 {
                    for xx in bx as usize..x1 {
                        if xx == x && yy == y {
                            continue;
                        }
                        let di = (yy * w + xx) * 4;
                        rgba[di] = qr;
                        rgba[di + 1] = qg;
                        rgba[di + 2] = qb;
                        // alpha already set when that pixel's turn comes / was set
                    }
                }
            }

            for &(dx, dy, weight) in offsets {
                let nx = x as i32 + dx * row_dir * ps;
                let ny = y as i32 + dy * ps;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                let ni = (ny as usize * w + nx as usize) * 3;
                err_buf[ni] += err[0] * weight;
                err_buf[ni + 1] += err[1] * weight;
                err_buf[ni + 2] += err[2] * weight;
            }
        }
    }
    Ok(())
}

/// Apply approximate-seamless ED: wrap-pad → ED → crop into `rgba` in place.
///
/// No-op (Ok) when `params.wrap_padding` is false or the mode is not ED.
pub fn apply_ed_with_wrap_padding(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
) -> Result<(), EngineError> {
    if !params.wrap_padding {
        return Ok(());
    }
    let Some(kernel) = params.mode.diffusion_kernel() else {
        // Ordered / non-ED: wrap-padding does not apply.
        return Ok(());
    };
    let margin = wrap_padding_margin(kernel, u32::from(params.pixel_size.max(1)));
    if margin == 0 || width == 0 || height == 0 {
        return Ok(());
    }

    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| EngineError::invalid_filter_params("wrap-padding size overflow"))?;
    if rgba.len() != expected {
        return Err(EngineError::invalid_filter_params(
            "wrap-padding buffer length mismatch",
        ));
    }

    let (mut padded, pw, ph) = wrap_pad_rgba_f32(rgba, width, height, margin);
    apply_ed_buffer_inplace(&mut padded, pw, ph, params, kernel)?;
    let cropped = crop_rgba_f32(&padded, pw, ph, margin, width, height)?;
    rgba.copy_from_slice(&cropped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherModeV2, DitherParamsV2};
    use engine_tiles::TILE_SIZE;

    #[test]
    fn margin_matches_pixel_size_times_max_offset() {
        let k = DiffusionKernel::FloydSteinberg;
        assert_eq!(wrap_padding_margin(k, 1), k.max_offset());
        assert_eq!(wrap_padding_margin(k, 4), 4 * k.max_offset());
        let wide = DiffusionKernel::JarvisJudiceNinke;
        assert_eq!(wrap_padding_margin(wide, 3), 3 * wide.max_offset());
    }

    #[test]
    fn pad_then_crop_round_trips_center() {
        let w = 8u32;
        let h = 8u32;
        let mut src = vec![0.0f32; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            src[i * 4] = (i as f32) / 64.0;
            src[i * 4 + 1] = 0.25;
            src[i * 4 + 2] = 0.5;
            src[i * 4 + 3] = 1.0;
        }
        let m = 2usize;
        let (padded, pw, ph) = wrap_pad_rgba_f32(&src, w, h, m);
        let cropped = crop_rgba_f32(&padded, pw, ph, m, w, h).unwrap();
        assert_eq!(cropped, src);
    }

    #[test]
    fn ordered_mode_unaffected_by_wrap_padding_flag() {
        let mut a = vec![0.5f32; 16 * 16 * 4];
        let mut b = a.clone();
        for i in 0..(16 * 16) {
            a[i * 4 + 3] = 1.0;
            b[i * 4 + 3] = 1.0;
        }
        let mut params = DitherParamsV2 {
            mode: DitherModeV2::Bayer8x8,
            levels: 4,
            wrap_padding: true,
            ..DitherParamsV2::default()
        };
        apply_ed_with_wrap_padding(&mut a, 16, 16, &params).unwrap();
        params.wrap_padding = false;
        apply_ed_with_wrap_padding(&mut b, 16, 16, &params).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn padded_ed_changes_wrap_edges_vs_unpadded() {
        // Width = TILE_SIZE so a hard cut would align with a common tile edge.
        let w = TILE_SIZE;
        let h = 48u32;
        let mut src = vec![0.0f32; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 4;
                // Low-frequency diagonal + edge contrast so opposite edges differ.
                let t = ((x + y) as f32 * 0.07).sin() * 0.5 + 0.5;
                src[i] = t;
                src[i + 1] = t * 0.9;
                src[i + 2] = 1.0 - t;
                src[i + 3] = 1.0;
            }
        }

        let params_off = DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            wrap_padding: false,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };
        let params_on = DitherParamsV2 {
            wrap_padding: true,
            ..params_off.clone()
        };

        let mut no_pad = src.clone();
        let kernel = DiffusionKernel::FloydSteinberg;
        apply_ed_buffer_inplace(&mut no_pad, w, h, &params_off, kernel).unwrap();

        let mut with_pad = src.clone();
        apply_ed_with_wrap_padding(&mut with_pad, w, h, &params_on).unwrap();

        assert_ne!(
            with_pad, no_pad,
            "wrap-padding must alter ED output vs the unpadded path"
        );

        // Compare 2×1 tiled seam: |left-col − right-col| averaged.
        // Approximate wrap should not be worse than a hard cut (allow slack).
        let seam = |img: &[f32]| -> f32 {
            let mut s = 0.0f32;
            for y in 0..h as usize {
                let l = img[y * w as usize * 4];
                let r = img[(y * w as usize + (w as usize - 1)) * 4];
                s += (l - r).abs();
            }
            s / h as f32
        };
        let seam_pad = seam(&with_pad);
        let seam_cut = seam(&no_pad);
        assert!(
            seam_pad <= seam_cut + 0.15,
            "wrap seam {seam_pad} should not exceed hard-cut seam {seam_cut} by much"
        );
    }
}
