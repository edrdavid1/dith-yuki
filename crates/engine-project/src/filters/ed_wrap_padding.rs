//! Approximate wrap-padding for error-diffusion dithering.
//!
//! # Semantics
//!
//! Pads the source image with wrapped edges, runs ED on the padded buffer,
//! crops back to the original rect. This approximates seamless tiling for
//! error diffusion, which cannot be made exactly seamless because ED is
//! sequential.
//!
//! # Non-goals
//!
//! - Bit-exact seamless output. Not achievable with ED.
//! - Ordered dithering. The flag is ignored for ordered modes.
//! - Multi-pass ED: **no** — engine is single-pass only (FR-6).
//! - Serpentine ED: **yes** — supported; both horizontal sides are padded (FR-5).
//!
//! # Reference
//!
//! See docs/adr/ed-wrap-padding.md

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::error::EngineError;
use crate::filter::{DiffusionKernel, DitherColorMode, DitherParamsV2};

/// Soft memory ceiling for the padded RGBA f32 buffer (≈200 MPix × 16 B ≈ 3.2 GiB).
pub const MAX_PADDED_PIXELS: usize = 200_000_000;
/// Hard ceiling on either margin axis.
pub const MAX_MARGIN_PX: u32 = 512;

/// Poll cancellation every N rows (NFR-2).
const CANCEL_POLL_ROWS: u32 = 64;

/// Canonical UI / ADR / help hint (must stay byte-identical everywhere).
pub const WRAP_PADDING_HINT: &str = "Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.";

/// Errors from the wrap-padding path (§5.3 / §8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdWrapError {
    Cancelled,
    UnsupportedKernel,
    SizeLimitExceeded { padded_pixels: usize, limit: usize },
    Overflow,
}

impl std::fmt::Display for EdWrapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Cancelled => write!(f, "ed wrap-padding cancelled"),
            Self::UnsupportedKernel => write!(f, "ed wrap-padding unsupported kernel"),
            Self::SizeLimitExceeded { padded_pixels, limit } => {
                write!(
                    f,
                    "ed wrap-padding size limit: padded_pixels={padded_pixels} > limit={limit}"
                )
            }
            Self::Overflow => write!(f, "ed wrap-padding size overflow"),
        }
    }
}

impl std::error::Error for EdWrapError {}

impl From<EdWrapError> for EngineError {
    fn from(e: EdWrapError) -> Self {
        match e {
            EdWrapError::Cancelled => EngineError::invalid_state(e.to_string()),
            other => EngineError::invalid_filter_params(other.to_string()),
        }
    }
}

/// Lightweight counters (Prometheus-equivalent; NFR-5).
#[derive(Debug, Default)]
pub struct WrapPaddingMetrics {
    pub full_document_triggered: AtomicU64,
    pub fallback_size_limit: AtomicU64,
    pub fallback_overflow: AtomicU64,
    pub fallback_unsupported_kernel: AtomicU64,
    pub cancelled: AtomicU64,
}

pub static WRAP_PADDING_METRICS: WrapPaddingMetrics = WrapPaddingMetrics {
    full_document_triggered: AtomicU64::new(0),
    fallback_size_limit: AtomicU64::new(0),
    fallback_overflow: AtomicU64::new(0),
    fallback_unsupported_kernel: AtomicU64::new(0),
    cancelled: AtomicU64::new(0),
};

/// Wrap-padding margin `(mx, my)` = `pixel_size × (|dx|max, |dy|max)` (FR glossary).
///
/// Returns a pair, not a scalar — asymmetric kernels (Shiau–Fan) need separate axes.
pub fn edge_margin(kernel: DiffusionKernel, pixel_size: u32) -> (u32, u32) {
    kernel.edge_margin_xy(pixel_size)
}

/// True when padded size fits memory / margin ceilings (UI enable rule §6.1).
pub fn wrap_padding_fits_limits(
    width: u32,
    height: u32,
    kernel: DiffusionKernel,
    pixel_size: u32,
) -> bool {
    let (mx, my) = edge_margin(kernel, pixel_size);
    if mx > MAX_MARGIN_PX || my > MAX_MARGIN_PX {
        return false;
    }
    match padded_pixel_count(width, height, mx, my) {
        Ok(n) => n <= MAX_PADDED_PIXELS,
        Err(_) => false,
    }
}

fn padded_pixel_count(width: u32, height: u32, mx: u32, my: u32) -> Result<usize, EdWrapError> {
    let pw = (width as u64)
        .checked_add(2 * mx as u64)
        .ok_or(EdWrapError::Overflow)?;
    let ph = (height as u64)
        .checked_add(2 * my as u64)
        .ok_or(EdWrapError::Overflow)?;
    let n = pw.checked_mul(ph).ok_or(EdWrapError::Overflow)?;
    usize::try_from(n).map_err(|_| EdWrapError::Overflow)
}

/// Non-negative modulo map into `[lo, hi)` (FR-2).
pub fn wrap_index(v: i64, lo: i64, hi: i64) -> i64 {
    let span = hi - lo;
    if span <= 0 {
        return lo;
    }
    let mut r = (v - lo) % span;
    if r < 0 {
        r += span;
    }
    lo + r
}

/// Pad RGBA f32 with wrapped opposite edges (FR-2). Fresh allocation; reads source only.
pub fn wrap_pad_rgba_f32(
    src: &[f32],
    width: u32,
    height: u32,
    mx: u32,
    my: u32,
) -> Result<(Vec<f32>, u32, u32), EdWrapError> {
    let w = width as usize;
    let h = height as usize;
    let expected = w
        .checked_mul(h)
        .and_then(|n| n.checked_mul(4))
        .ok_or(EdWrapError::Overflow)?;
    if src.len() != expected || w == 0 || h == 0 {
        return Err(EdWrapError::Overflow);
    }
    if mx > MAX_MARGIN_PX || my > MAX_MARGIN_PX {
        return Err(EdWrapError::SizeLimitExceeded {
            padded_pixels: 0,
            limit: MAX_MARGIN_PX as usize,
        });
    }
    let padded_pixels = padded_pixel_count(width, height, mx, my)?;
    if padded_pixels > MAX_PADDED_PIXELS {
        return Err(EdWrapError::SizeLimitExceeded {
            padded_pixels,
            limit: MAX_PADDED_PIXELS,
        });
    }
    let pw = w + 2 * mx as usize;
    let ph = h + 2 * my as usize;
    let floats = padded_pixels
        .checked_mul(4)
        .ok_or(EdWrapError::Overflow)?;
    let mut out = Vec::new();
    out.try_reserve(floats).map_err(|_| EdWrapError::Overflow)?;
    out.resize(floats, 0.0);

    let sample = |sx: i64, sy: i64| -> [f32; 4] {
        let x = wrap_index(sx, 0, w as i64) as usize;
        let y = wrap_index(sy, 0, h as i64) as usize;
        let i = (y * w + x) * 4;
        [src[i], src[i + 1], src[i + 2], src[i + 3]]
    };

    for py in 0..ph {
        for px in 0..pw {
            let sx = px as i64 - mx as i64;
            let sy = py as i64 - my as i64;
            let c = sample(sx, sy);
            let di = (py * pw + px) * 4;
            out[di] = c[0];
            out[di + 1] = c[1];
            out[di + 2] = c[2];
            out[di + 3] = c[3];
        }
    }
    Ok((out, pw as u32, ph as u32))
}

/// Crop center `width × height` from a padded buffer (FR-4).
pub fn crop_rgba_f32(
    padded: &[f32],
    padded_w: u32,
    padded_h: u32,
    mx: u32,
    my: u32,
    width: u32,
    height: u32,
) -> Result<Vec<f32>, EdWrapError> {
    let pw = padded_w as usize;
    let ph = padded_h as usize;
    let w = width as usize;
    let h = height as usize;
    if pw != w + 2 * mx as usize
        || ph != h + 2 * my as usize
        || padded.len() != pw.saturating_mul(ph).saturating_mul(4)
    {
        return Err(EdWrapError::Overflow);
    }
    let mut out = Vec::new();
    let floats = w.checked_mul(h).and_then(|n| n.checked_mul(4)).ok_or(EdWrapError::Overflow)?;
    out.try_reserve(floats).map_err(|_| EdWrapError::Overflow)?;
    out.resize(floats, 0.0);
    for y in 0..h {
        for x in 0..w {
            let si = ((y + my as usize) * pw + (x + mx as usize)) * 4;
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

/// Sequential ED on a contiguous RGBA f32 buffer (same kernel tables as tiled ED).
fn apply_ed_buffer_inplace(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    kernel: DiffusionKernel,
    cancel: &AtomicBool,
    mut progress: Option<&mut dyn FnMut(f32)>,
) -> Result<(), EdWrapError> {
    let w = width as usize;
    let h = height as usize;
    if rgba.len() != w * h * 4 || w == 0 || h == 0 {
        return Err(EdWrapError::Overflow);
    }
    // Single-pass only (FR-6) — engine has no multi-pass ED parameter.

    let levels = params.levels as f32;
    let ps = params.pixel_size.max(1) as i32;
    // Serpentine supported (FR-5): pad both sides; scan direction flips per row.
    let serpentine = matches!(
        kernel,
        DiffusionKernel::Ostromoukhov | DiffusionKernel::ZhouFang
    ) || params.serpentine;
    let offsets = kernel.offsets();
    let mut err_buf = Vec::new();
    let err_len = w.checked_mul(h).and_then(|n| n.checked_mul(3)).ok_or(EdWrapError::Overflow)?;
    err_buf.try_reserve(err_len).map_err(|_| EdWrapError::Overflow)?;
    err_buf.resize(err_len, 0.0);

    for y in 0..h {
        if (y as u32).is_multiple_of(CANCEL_POLL_ROWS) && cancel.load(Ordering::Relaxed) {
            WRAP_PADDING_METRICS.cancelled.fetch_add(1, Ordering::Relaxed);
            log::warn!("ed_wrap_padding cancelled at row {y}/{h}");
            return Err(EdWrapError::Cancelled);
        }
        if let Some(ref mut cb) = progress {
            cb((y as f32) / (h as f32).max(1.0));
        }
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
    if let Some(ref mut cb) = progress {
        cb(1.0);
    }
    Ok(())
}

/// Apply approximate-seamless ED: wrap-pad → ED → crop into `rgba` in place (FR-1…4).
///
/// No-op (`Ok`) when `params.wrap_padding` is false or the mode is not ED.
/// On size/overflow errors the caller should fall back to tile-strip (§8).
pub fn apply_ed_with_wrap_padding(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    cancel: &AtomicBool,
    progress: Option<&mut dyn FnMut(f32)>,
) -> Result<(), EdWrapError> {
    if !params.wrap_padding {
        return Ok(());
    }
    let Some(kernel) = params.mode.diffusion_kernel() else {
        // Ordered / non-ED: ignore flag (FR-1).
        return Ok(());
    };

    WRAP_PADDING_METRICS
        .full_document_triggered
        .fetch_add(1, Ordering::Relaxed);

    let (mx, my) = edge_margin(kernel, u32::from(params.pixel_size.max(1)));
    if mx == 0 && my == 0 {
        return Ok(());
    }

    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(EdWrapError::Overflow)?;
    if rgba.len() != expected {
        return Err(EdWrapError::Overflow);
    }

    let (mut padded, pw, ph) = match wrap_pad_rgba_f32(rgba, width, height, mx, my) {
        Ok(v) => v,
        Err(EdWrapError::SizeLimitExceeded { padded_pixels, limit }) => {
            WRAP_PADDING_METRICS
                .fallback_size_limit
                .fetch_add(1, Ordering::Relaxed);
            log::info!(
                "ed_wrap_padding fallback: padded_pixels={padded_pixels} > limit={limit}"
            );
            return Err(EdWrapError::SizeLimitExceeded {
                padded_pixels,
                limit,
            });
        }
        Err(EdWrapError::Overflow) => {
            WRAP_PADDING_METRICS
                .fallback_overflow
                .fetch_add(1, Ordering::Relaxed);
            log::warn!("ed_wrap_padding fallback: overflow");
            return Err(EdWrapError::Overflow);
        }
        Err(e) => return Err(e),
    };

    apply_ed_buffer_inplace(&mut padded, pw, ph, params, kernel, cancel, progress)?;
    let cropped = crop_rgba_f32(&padded, pw, ph, mx, my, width, height)?;
    rgba.copy_from_slice(&cropped);
    Ok(())
}

/// Convenience: no cancel / progress (tests).
pub fn apply_ed_with_wrap_padding_simple(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
) -> Result<(), EdWrapError> {
    let cancel = AtomicBool::new(false);
    apply_ed_with_wrap_padding(rgba, width, height, params, &cancel, None)
}

/// Run ED on `rgba` without wrap-padding (seam / baseline tests; FR-8).
pub fn apply_ed_buffer_no_wrap(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
) -> Result<(), EdWrapError> {
    let Some(kernel) = params.mode.diffusion_kernel() else {
        return Ok(());
    };
    let cancel = AtomicBool::new(false);
    apply_ed_buffer_inplace(rgba, width, height, params, kernel, &cancel, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::DitherModeV2;
    use engine_tiles::TILE_SIZE;

    #[test]
    fn edge_margin_symmetric_kernel() {
        assert_eq!(
            edge_margin(DiffusionKernel::FloydSteinberg, 1),
            (1, 1)
        );
    }

    #[test]
    fn edge_margin_asymmetric_kernel() {
        // Shiau–Fan: |dx|max=3, |dy|max=1
        assert_eq!(edge_margin(DiffusionKernel::ShiauFan, 1), (3, 1));
    }

    #[test]
    fn edge_margin_pixel_size_scales() {
        assert_eq!(edge_margin(DiffusionKernel::ShiauFan, 3), (9, 3));
    }

    #[test]
    fn wrap_handles_negative() {
        assert_eq!(wrap_index(-1, 0, 10), 9);
    }

    #[test]
    fn wrap_handles_positive_overflow() {
        assert_eq!(wrap_index(12, 0, 10), 2);
    }

    #[test]
    fn pad_identity_when_margin_zero() {
        let src = vec![0.1f32, 0.2, 0.3, 1.0, 0.4, 0.5, 0.6, 1.0];
        let (out, pw, ph) = wrap_pad_rgba_f32(&src, 2, 1, 0, 0).unwrap();
        assert_eq!((pw, ph), (2, 1));
        assert_eq!(out, src);
    }

    #[test]
    fn crop_returns_original_dimensions() {
        let mut src = vec![0.0f32; 8 * 8 * 4];
        for i in 0..64 {
            src[i * 4] = i as f32 / 64.0;
            src[i * 4 + 3] = 1.0;
        }
        let (padded, pw, ph) = wrap_pad_rgba_f32(&src, 8, 8, 2, 1).unwrap();
        let cropped = crop_rgba_f32(&padded, pw, ph, 2, 1, 8, 8).unwrap();
        assert_eq!(cropped.len(), src.len());
        assert_eq!(cropped, src);
    }

    #[test]
    fn overflow_checked() {
        let err = padded_pixel_count(u32::MAX, u32::MAX, 10, 10).unwrap_err();
        assert_eq!(err, EdWrapError::Overflow);
    }

    #[test]
    fn size_limit_returns_error() {
        let src = vec![0.0f32; 4];
        // Margin ceiling (§4 NFR-1).
        let err = wrap_pad_rgba_f32(&src, 1, 1, MAX_MARGIN_PX + 1, 1).unwrap_err();
        assert!(matches!(err, EdWrapError::SizeLimitExceeded { .. }));

        // Pixel ceiling without allocating the padded buffer.
        let n = padded_pixel_count(20_000, 20_000, 50, 50).unwrap();
        assert!(n > MAX_PADDED_PIXELS);
        assert!(!wrap_padding_fits_limits(
            20_000,
            20_000,
            DiffusionKernel::FloydSteinberg,
            50
        ));
    }

    #[test]
    fn pad_copies_from_source_not_diffused() {
        // Pad reads only the source slice; ED runs afterward on a fresh buffer.
        let mut src = vec![0.25f32; 4 * 4 * 4];
        for i in 0..16 {
            src[i * 4] = i as f32 / 16.0;
            src[i * 4 + 3] = 1.0;
        }
        let src_before = src.clone();
        let (padded, pw, ph) = wrap_pad_rgba_f32(&src, 4, 4, 1, 1).unwrap();
        assert_eq!(src, src_before, "pad must not mutate source");
        let cropped = crop_rgba_f32(&padded, pw, ph, 1, 1, 4, 4).unwrap();
        assert_eq!(cropped, src_before);
    }

    #[test]
    fn cancellation_returns_cancelled() {
        let mut rgba = vec![0.5f32; 64 * 128 * 4];
        for i in 0..(64 * 128) {
            rgba[i * 4 + 3] = 1.0;
        }
        let params = DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            wrap_padding: true,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };
        let cancel = AtomicBool::new(true);
        let err = apply_ed_with_wrap_padding(&mut rgba, 64, 128, &params, &cancel, None)
            .unwrap_err();
        assert_eq!(err, EdWrapError::Cancelled);
    }

    #[test]
    fn progress_reported_per_row() {
        let mut rgba = vec![0.5f32; 32 * 32 * 4];
        for i in 0..(32 * 32) {
            rgba[i * 4 + 3] = 1.0;
        }
        let params = DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            wrap_padding: true,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };
        let cancel = AtomicBool::new(false);
        let mut values = Vec::new();
        let mut on_progress = |p: f32| values.push(p);
        apply_ed_with_wrap_padding(
            &mut rgba,
            32,
            32,
            &params,
            &cancel,
            Some(&mut on_progress),
        )
        .unwrap();
        assert!(!values.is_empty());
        assert!((values.last().copied().unwrap_or(0.0) - 1.0).abs() < 1e-5);
        for w in values.windows(2) {
            assert!(w[1] + 1e-6 >= w[0], "progress must be non-decreasing");
        }
    }

    #[test]
    fn ordered_ignores_flag() {
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
        apply_ed_with_wrap_padding_simple(&mut a, 16, 16, &params).unwrap();
        params.wrap_padding = false;
        apply_ed_with_wrap_padding_simple(&mut b, 16, 16, &params).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn padded_ed_changes_vs_unpadded() {
        let w = TILE_SIZE;
        let h = 48u32;
        let mut src = vec![0.0f32; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let i = (y * w as usize + x) * 4;
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
        let cancel = AtomicBool::new(false);
        apply_ed_buffer_inplace(
            &mut no_pad,
            w,
            h,
            &params_off,
            DiffusionKernel::FloydSteinberg,
            &cancel,
            None,
        )
        .unwrap();
        let mut with_pad = src;
        apply_ed_with_wrap_padding_simple(&mut with_pad, w, h, &params_on).unwrap();
        assert_ne!(with_pad, no_pad);
    }

    #[test]
    fn hint_text_nonempty() {
        assert!(WRAP_PADDING_HINT.contains("approximate"));
        assert!(WRAP_PADDING_HINT.contains("Error diffusion is sequential"));
    }

    #[test]
    fn hint_text_matches_adr() {
        // Canonical hint must appear byte-identical in ADR (§6.2).
        let adr = include_str!("../../../../docs/adr/ed-wrap-padding.md");
        assert!(
            adr.contains(WRAP_PADDING_HINT),
            "ADR must contain WRAP_PADDING_HINT verbatim"
        );
        let help = include_str!("../../../../docs/gamedev-toolkit.md");
        assert!(
            help.contains(WRAP_PADDING_HINT),
            "help docs must contain WRAP_PADDING_HINT verbatim"
        );
    }

    #[test]
    fn wrap_padding_fits_limits_small_ok() {
        assert!(wrap_padding_fits_limits(
            64,
            64,
            DiffusionKernel::FloydSteinberg,
            1
        ));
    }
}
