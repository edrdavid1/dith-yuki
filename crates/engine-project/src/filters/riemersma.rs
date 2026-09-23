//! Riemersma dithering — Hilbert-curve error diffusion with an exponential
//! error queue (Thiadmer Riemersma, C/C++ Users Journal, Dec 1998).
//!
//! Classical parameters: queue length `q = 16`, base `r = 16`. Weights are
//! `w_i = r^(-(i+1)/q)` for `i = 0..q-1` (newest error at `i = 0`), then
//! normalised so they sum to 1. Error is diffused only along the Hilbert
//! visit order — not to spatial neighbours.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::filter::{DitherColorMode, DitherParamsV2};

use super::hilbert::for_each_hilbert_cell;

/// Classical Riemersma queue length.
pub const QUEUE_LEN: usize = 16;
/// Classical weight base (`r` in `r^(-(i+1)/q)`).
pub const WEIGHT_RATIO: f32 = 16.0;

/// Precomputed normalised weights for the classical `(q, r) = (16, 16)` queue.
pub fn classic_weights() -> [f32; QUEUE_LEN] {
    let mut w = [0.0f32; QUEUE_LEN];
    let mut sum = 0.0f32;
    for (i, slot) in w.iter_mut().enumerate() {
        // Newest residual (i = 0) gets the largest weight.
        let v = WEIGHT_RATIO.powf(-((i + 1) as f32) / QUEUE_LEN as f32);
        *slot = v;
        sum += v;
    }
    if sum > 0.0 {
        for slot in &mut w {
            *slot /= sum;
        }
    }
    w
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

/// Apply Riemersma dithering in-place to a document-sized RGBA f32 buffer.
///
/// `rgba` is row-major, 4 floats per pixel, length `width * height * 4`.
/// Alpha is preserved. When `should_cancel` loads `true`, the pass aborts
/// immediately and returns `Err(Cancelled)`.
pub fn apply_riemersma_rgba(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    should_cancel: &AtomicBool,
) -> Result<(), RiemersmaError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or(RiemersmaError::InvalidBuffer)?;
    if rgba.len() != expected || width == 0 || height == 0 {
        return Err(RiemersmaError::InvalidBuffer);
    }

    let levels = (params.levels.max(2) as f32).min(256.0);
    let scale = params.threshold_scale.max(0.0);
    let weights = classic_weights();
    // Per-channel error queues (newest at index 0).
    let mut err_r = [0.0f32; QUEUE_LEN];
    let mut err_g = [0.0f32; QUEUE_LEN];
    let mut err_b = [0.0f32; QUEUE_LEN];

    let mut cancelled = false;
    let mut iter = 0u64;
    for_each_hilbert_cell(u64::from(width), u64::from(height), |x, y| {
        // Check cancellation every 4096 pixels (§0.4).
        if iter & 4095 == 0 && should_cancel.load(Ordering::Relaxed) {
            cancelled = true;
            return false;
        }
        iter += 1;

        let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
        let (src_r, src_g, src_b) = (rgba[idx], rgba[idx + 1], rgba[idx + 2]);
        let alpha = rgba[idx + 3];

        let (adj_r, adj_g, adj_b) = match params.color_mode {
            DitherColorMode::Grayscale => {
                let lum = to_luminance(src_r, src_g, src_b);
                let mut adj = lum;
                for i in 0..QUEUE_LEN {
                    adj += weights[i] * err_r[i] * scale;
                }
                (adj, adj, adj)
            }
            DitherColorMode::Rgb => {
                let mut ar = src_r;
                let mut ag = src_g;
                let mut ab = src_b;
                for i in 0..QUEUE_LEN {
                    ar += weights[i] * err_r[i] * scale;
                    ag += weights[i] * err_g[i] * scale;
                    ab += weights[i] * err_b[i] * scale;
                }
                (ar, ag, ab)
            }
        };

        let (qr, qg, qb) = match params.color_mode {
            DitherColorMode::Grayscale => {
                let q = quantize_uniform(adj_r.clamp(0.0, 1.0), levels);
                (q, q, q)
            }
            DitherColorMode::Rgb => (
                quantize_uniform(adj_r.clamp(0.0, 1.0), levels),
                quantize_uniform(adj_g.clamp(0.0, 1.0), levels),
                quantize_uniform(adj_b.clamp(0.0, 1.0), levels),
            ),
        };

        // Rotate queues: shift older residuals toward the tail, insert newest at 0.
        for i in (1..QUEUE_LEN).rev() {
            err_r[i] = err_r[i - 1];
            err_g[i] = err_g[i - 1];
            err_b[i] = err_b[i - 1];
        }
        err_r[0] = adj_r - qr;
        err_g[0] = adj_g - qg;
        err_b[0] = adj_b - qb;

        rgba[idx] = qr;
        rgba[idx + 1] = qg;
        rgba[idx + 2] = qb;
        rgba[idx + 3] = alpha;
        true
    });

    if cancelled {
        Err(RiemersmaError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiemersmaError {
    InvalidBuffer,
    Cancelled,
}

impl std::fmt::Display for RiemersmaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidBuffer => write!(f, "riemersma: invalid buffer dimensions"),
            Self::Cancelled => write!(f, "riemersma: cancelled"),
        }
    }
}

impl std::error::Error for RiemersmaError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::DitherModeV2;
    use std::sync::atomic::AtomicBool;

    fn params(levels: u16) -> DitherParamsV2 {
        DitherParamsV2 {
            mode: DitherModeV2::Riemersma,
            levels,
            threshold_scale: 1.0,
            ..DitherParamsV2::default()
        }
    }

    #[test]
    fn weights_sum_to_one() {
        let w = classic_weights();
        let sum: f32 = w.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5, "sum={sum}");
        // Newest (index 0) must dominate.
        assert!(w[0] > w[1]);
        assert!(w[QUEUE_LEN - 2] > w[QUEUE_LEN - 1]);
    }

    #[test]
    fn flat_midgray_stays_in_level_set() {
        let w = 8u32;
        let h = 8u32;
        let mut rgba = vec![0.5f32; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            rgba[i * 4 + 3] = 1.0;
        }
        let cancel = AtomicBool::new(false);
        apply_riemersma_rgba(&mut rgba, w, h, &params(2), &cancel).unwrap();
        for i in 0..(w * h) as usize {
            let v = rgba[i * 4];
            assert!(
                (v - 0.0).abs() < 1e-5 || (v - 1.0).abs() < 1e-5,
                "pixel {i} = {v}"
            );
            assert_eq!(rgba[i * 4 + 3], 1.0);
        }
    }

    #[test]
    fn non_power_of_two_covers_all_pixels() {
        let w = 7u32;
        let h = 5u32;
        let mut rgba = vec![0.3f32; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            rgba[i * 4 + 3] = 1.0;
        }
        let cancel = AtomicBool::new(false);
        apply_riemersma_rgba(&mut rgba, w, h, &params(4), &cancel).unwrap();
        // Every pixel must have been written (alpha untouched, RGB quantized).
        let levels = 4.0f32;
        for i in 0..(w * h) as usize {
            let v = rgba[i * 4];
            let k = v * (levels - 1.0);
            assert!((k - k.round()).abs() < 1e-4, "pixel {i} not on level grid: {v}");
        }
    }

    #[test]
    fn cancel_aborts_mid_pass() {
        let w = 512u32;
        let h = 512u32;
        let mut rgba = vec![0.5f32; (w * h * 4) as usize];
        let cancel = AtomicBool::new(false);
        // Cancel immediately so the first cancellation check trips.
        cancel.store(true, Ordering::Relaxed);
        let err = apply_riemersma_rgba(&mut rgba, w, h, &params(2), &cancel).unwrap_err();
        assert_eq!(err, RiemersmaError::Cancelled);
    }
}
