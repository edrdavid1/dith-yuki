//! Result-based ring / seam validation (F1 step 1).
//!
//! Pure functions over quantized output buffers. Independent of Pipeline Tile.
//! Spec: `.local-doc/gamedev-2.md` §6 F1 ring metric; ADR `gamedev_pipeline_tile_f1`.

use serde::{Deserialize, Serialize};

/// ED relative multiplier (locked via fixture suite).
///
/// Axis ok iff `R_boundary ≤ max(k × R_internal, ε)`.
pub const SEAM_K: f64 = 1.25;

/// Near-zero floor as a fraction of pairs (0.3% = 0.003).
///
/// Protects flat regions where `R_internal → 0` without becoming a content threshold.
pub const SEAM_EPSILON: f64 = 0.003;

/// How the buffer encodes quantized output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamPixelFormat {
    /// Packed RGBA8 (sRGB bytes after quantization).
    Rgba8,
    /// One palette index per pixel (`u8`).
    Indexed8,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisSeamStats {
    /// Mismatch fraction on wrap boundary (H: right↔left; V: bottom↔top).
    pub r_boundary: f64,
    /// Mismatch fraction on adjacent internal columns (H) or rows (V).
    pub r_internal: f64,
    /// `r_boundary / r_internal` when `r_internal > 0`, else `None`.
    pub ratio: Option<f64>,
    /// `r_boundary ≤ max(k × r_internal, ε)` (ED) or `r_boundary == 0` (exact).
    pub ok: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamMode {
    /// Ordered / blue-noise: every wrap pair must match (tol 0).
    Exact,
    /// Error diffusion: relative metric with `k` / `ε`.
    Relative,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeamReport {
    pub mode: SeamMode,
    pub width: u32,
    pub height: u32,
    pub horizontal: AxisSeamStats,
    pub vertical: AxisSeamStats,
    /// Both axes ok.
    pub ok: bool,
    pub k: f64,
    pub epsilon: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeamValidateError {
    Empty,
    BufferLen { expected: usize, got: usize },
}

impl std::fmt::Display for SeamValidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "image dimensions must be ≥ 1"),
            Self::BufferLen { expected, got } => {
                write!(f, "buffer length {got}, expected {expected}")
            }
        }
    }
}

impl std::error::Error for SeamValidateError {}

/// Byte-equal quantized pixels with F4 alpha rule.
///
/// - Fully transparent (`a == 0`): RGB ignored; pair matches iff both `a == 0`.
/// - Otherwise: all four bytes must match (or the single index byte for indexed).
#[inline]
pub fn pixels_match_rgba8(a: &[u8; 4], b: &[u8; 4]) -> bool {
    match (a[3], b[3]) {
        (0, 0) => true,
        (0, _) | (_, 0) => false,
        _ => a == b,
    }
}

#[inline]
pub fn pixels_match_index(a: u8, b: u8) -> bool {
    a == b
}

/// Validate wrap seams on a quantized RGBA8 buffer (`width * height * 4` bytes).
pub fn validate_seams_rgba8(
    rgba: &[u8],
    width: u32,
    height: u32,
    mode: SeamMode,
) -> Result<SeamReport, SeamValidateError> {
    validate_seams(rgba, width, height, SeamPixelFormat::Rgba8, mode)
}

/// Validate wrap seams on an indexed buffer (`width * height` bytes).
pub fn validate_seams_indexed(
    indices: &[u8],
    width: u32,
    height: u32,
    mode: SeamMode,
) -> Result<SeamReport, SeamValidateError> {
    validate_seams(indices, width, height, SeamPixelFormat::Indexed8, mode)
}

/// Pure ring check. Does not allocate beyond the report.
pub fn validate_seams(
    buf: &[u8],
    width: u32,
    height: u32,
    format: SeamPixelFormat,
    mode: SeamMode,
) -> Result<SeamReport, SeamValidateError> {
    if width == 0 || height == 0 {
        return Err(SeamValidateError::Empty);
    }
    let w = width as usize;
    let h = height as usize;
    let expected = match format {
        SeamPixelFormat::Rgba8 => w * h * 4,
        SeamPixelFormat::Indexed8 => w * h,
    };
    if buf.len() != expected {
        return Err(SeamValidateError::BufferLen {
            expected,
            got: buf.len(),
        });
    }

    let horizontal = axis_stats_horizontal(buf, w, h, format, mode);
    let vertical = axis_stats_vertical(buf, w, h, format, mode);
    let ok = horizontal.ok && vertical.ok;

    Ok(SeamReport {
        mode,
        width,
        height,
        horizontal,
        vertical,
        ok,
        k: SEAM_K,
        epsilon: SEAM_EPSILON,
    })
}

fn axis_ok(r_boundary: f64, r_internal: f64, mode: SeamMode) -> bool {
    match mode {
        SeamMode::Exact => r_boundary == 0.0,
        SeamMode::Relative => r_boundary <= (SEAM_K * r_internal).max(SEAM_EPSILON),
    }
}

fn ratio(r_boundary: f64, r_internal: f64) -> Option<f64> {
    if r_internal > 0.0 {
        Some(r_boundary / r_internal)
    } else {
        None
    }
}

fn axis_stats_horizontal(
    buf: &[u8],
    w: usize,
    h: usize,
    format: SeamPixelFormat,
    mode: SeamMode,
) -> AxisSeamStats {
    // Boundary: right column (w-1) ↔ left column (0), one pair per row.
    let mut bound_mismatch = 0usize;
    let bound_pairs = h;
    for y in 0..h {
        if !pair_match(buf, w, format, 0, y, w - 1, y) {
            bound_mismatch += 1;
        }
    }
    let r_boundary = frac(bound_mismatch, bound_pairs);

    // Internal: adjacent columns x ↔ x+1 for x in 0..w-2, all rows.
    let mut int_mismatch = 0usize;
    let int_pairs = if w >= 2 { (w - 1) * h } else { 0 };
    if w >= 2 {
        for y in 0..h {
            for x in 0..w - 1 {
                if !pair_match(buf, w, format, x, y, x + 1, y) {
                    int_mismatch += 1;
                }
            }
        }
    }
    let r_internal = frac(int_mismatch, int_pairs.max(1));
    // When w < 2 there are no internal pairs — treat R_internal as 0.
    let r_internal = if int_pairs == 0 { 0.0 } else { r_internal };

    AxisSeamStats {
        r_boundary,
        r_internal,
        ratio: ratio(r_boundary, r_internal),
        ok: axis_ok(r_boundary, r_internal, mode),
    }
}

fn axis_stats_vertical(
    buf: &[u8],
    w: usize,
    h: usize,
    format: SeamPixelFormat,
    mode: SeamMode,
) -> AxisSeamStats {
    let mut bound_mismatch = 0usize;
    let bound_pairs = w;
    for x in 0..w {
        if !pair_match(buf, w, format, x, 0, x, h - 1) {
            bound_mismatch += 1;
        }
    }
    let r_boundary = frac(bound_mismatch, bound_pairs);

    let mut int_mismatch = 0usize;
    let int_pairs = if h >= 2 { (h - 1) * w } else { 0 };
    if h >= 2 {
        for y in 0..h - 1 {
            for x in 0..w {
                if !pair_match(buf, w, format, x, y, x, y + 1) {
                    int_mismatch += 1;
                }
            }
        }
    }
    let r_internal = if int_pairs == 0 {
        0.0
    } else {
        frac(int_mismatch, int_pairs)
    };

    AxisSeamStats {
        r_boundary,
        r_internal,
        ratio: ratio(r_boundary, r_internal),
        ok: axis_ok(r_boundary, r_internal, mode),
    }
}

#[inline]
fn frac(mismatch: usize, pairs: usize) -> f64 {
    if pairs == 0 {
        0.0
    } else {
        mismatch as f64 / pairs as f64
    }
}

#[inline]
fn pair_match(
    buf: &[u8],
    w: usize,
    format: SeamPixelFormat,
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
) -> bool {
    match format {
        SeamPixelFormat::Rgba8 => {
            let i0 = (y0 * w + x0) * 4;
            let i1 = (y1 * w + x1) * 4;
            let a = [buf[i0], buf[i0 + 1], buf[i0 + 2], buf[i0 + 3]];
            let b = [buf[i1], buf[i1 + 1], buf[i1 + 2], buf[i1 + 3]];
            pixels_match_rgba8(&a, &b)
        }
        SeamPixelFormat::Indexed8 => {
            let a = buf[y0 * w + x0];
            let b = buf[y1 * w + x1];
            pixels_match_index(a, b)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fill_rgba(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
        let n = (w * h) as usize;
        let mut out = Vec::with_capacity(n * 4);
        for _ in 0..n {
            out.extend_from_slice(&rgba);
        }
        out
    }

    #[test]
    fn flat_exact_ok() {
        let buf = fill_rgba(32, 32, [10, 20, 30, 255]);
        let r = validate_seams_rgba8(&buf, 32, 32, SeamMode::Exact).unwrap();
        assert!(r.ok);
        assert_eq!(r.horizontal.r_boundary, 0.0);
        assert_eq!(r.vertical.r_boundary, 0.0);
    }

    #[test]
    fn transparent_rgb_garbage_still_matches() {
        let mut buf = fill_rgba(4, 4, [0, 0, 0, 0]);
        // Divergent RGB on fully transparent pixels must not count.
        buf[0] = 99;
        buf[1] = 88;
        buf[2] = 77;
        let right = ((4 - 1) * 4) as usize; // x=3,y=0
        buf[right] = 1;
        buf[right + 1] = 2;
        buf[right + 2] = 3;
        let r = validate_seams_rgba8(&buf, 4, 4, SeamMode::Exact).unwrap();
        assert!(r.horizontal.ok);
    }

    #[test]
    fn opaque_vs_transparent_is_mismatch() {
        let mut buf = fill_rgba(2, 1, [0, 0, 0, 0]);
        buf[4..8].copy_from_slice(&[1, 2, 3, 255]);
        let r = validate_seams_rgba8(&buf, 2, 1, SeamMode::Exact).unwrap();
        assert!(!r.horizontal.ok);
        assert_eq!(r.horizontal.r_boundary, 1.0);
    }

    #[test]
    fn indexed_exact() {
        let buf = vec![3u8; 16];
        let r = validate_seams_indexed(&buf, 4, 4, SeamMode::Exact).unwrap();
        assert!(r.ok);
    }
}
