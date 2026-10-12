//! Seamless / tileable texture helpers for ordered dither.
//!
//! Ordered patterns are periodic with period `N × pixel_size` on each axis
//! (N = threshold-matrix side). Document dimensions that are multiples of that
//! period do not introduce a dither seam at wrap boundaries.
//!
//! Error diffusion is sequential and cannot be exactly periodic — see
//! [`SeamlessKind::ApproximateOnly`].

use crate::filter::DitherModeV2;
use serde::{Deserialize, Serialize};

/// How confident we are about seamlessness for a given mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamlessKind {
    /// Exact when size % period == 0 and pattern angle is axis-aligned.
    ExactOrdered,
    /// Error diffusion / sequential — never exact; wrap-padding would only approximate.
    ApproximateOnly,
    /// Mode has no fixed lattice period (or unknown custom map size).
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeamlessReport {
    pub kind: SeamlessKind,
    /// Pattern period in pixels (N × pixel_size). `None` when unknown / N/A.
    pub period_x: Option<u32>,
    pub period_y: Option<u32>,
    pub width_ok: bool,
    pub height_ok: bool,
    /// `pattern_angle` not a multiple of 90° → lattice not axis-periodic.
    pub angle_breaks_periodicity: bool,
    pub nearest_width: Option<(u32, u32)>,
    pub nearest_height: Option<(u32, u32)>,
}

/// Matrix side length N for known ordered modes (threshold map cells).
///
/// Custom PNG returns `None` — caller must supply map dimensions.
pub fn ordered_matrix_side(mode: &DitherModeV2) -> Option<(u32, u32)> {
    match mode {
        DitherModeV2::Bayer2x2 => Some((2, 2)),
        DitherModeV2::Bayer4x4 => Some((4, 4)),
        DitherModeV2::Bayer8x8 => Some((8, 8)),
        DitherModeV2::Bayer16x16 => Some((16, 16)),
        DitherModeV2::ClusteredDotOrdered => Some((8, 8)),
        DitherModeV2::DispersedDotOrdered => Some((16, 16)),
        DitherModeV2::VoidAndCluster => {
            let s = crate::filters::void_and_cluster::SIZE as u32;
            Some((s, s))
        }
        DitherModeV2::CustomPng { .. } => None,
        DitherModeV2::CmykHalftone | DitherModeV2::HalftoneScreenAngled => None,
        DitherModeV2::Wave => None,
        mode if mode.is_error_diffusion() => None,
        _ => None,
    }
}

/// True when `pattern_angle` (degrees) breaks axis-aligned periodicity.
///
/// Spec: angles other than 0° / 90° (and equivalents) break wrap seams.
pub fn pattern_angle_breaks_periodicity(angle_deg: f32) -> bool {
    if !angle_deg.is_finite() {
        return true;
    }
    let mut a = angle_deg % 360.0;
    if a < 0.0 {
        a += 360.0;
    }
    // Snap near-integers to avoid float noise (15.000001 → 15).
    let nearest = a.round();
    let a = if (a - nearest).abs() < 1e-3 {
        nearest
    } else {
        a
    };
    let m = a % 90.0;
    m > 1e-3 && (90.0 - m) > 1e-3
}

/// Nearest multiples of `period` at or below / above `size` (at least `period`).
pub fn nearest_multiples(size: u32, period: u32) -> Option<(u32, u32)> {
    if period == 0 {
        return None;
    }
    let below = (size / period) * period;
    let above = below.saturating_add(period);
    let below = if below == 0 { period } else { below };
    // If size already exact, both sides are the size itself.
    if size.is_multiple_of(period) {
        return Some((size, size));
    }
    Some((below, above))
}

/// Period in pixels = matrix side × pixel_size.
pub fn pattern_period_px(matrix_side: u32, pixel_size: u8) -> u32 {
    matrix_side.saturating_mul(u32::from(pixel_size.max(1)))
}

/// Evaluate seamless suitability for a document size + ordered dither params.
pub fn check_seamless(
    mode: &DitherModeV2,
    pixel_size: u8,
    pattern_angle: f32,
    width: u32,
    height: u32,
    custom_map_size: Option<(u32, u32)>,
) -> SeamlessReport {
    if mode.is_error_diffusion() {
        return SeamlessReport {
            kind: SeamlessKind::ApproximateOnly,
            period_x: None,
            period_y: None,
            width_ok: false,
            height_ok: false,
            angle_breaks_periodicity: false,
            nearest_width: None,
            nearest_height: None,
        };
    }

    let sides = ordered_matrix_side(mode).or(custom_map_size);
    let Some((nx, ny)) = sides else {
        return SeamlessReport {
            kind: SeamlessKind::Unknown,
            period_x: None,
            period_y: None,
            width_ok: false,
            height_ok: false,
            angle_breaks_periodicity: pattern_angle_breaks_periodicity(pattern_angle),
            nearest_width: None,
            nearest_height: None,
        };
    };

    let period_x = pattern_period_px(nx, pixel_size);
    let period_y = pattern_period_px(ny, pixel_size);
    let angle_bad = pattern_angle_breaks_periodicity(pattern_angle);
    let width_ok = period_x > 0 && width.is_multiple_of(period_x) && !angle_bad;
    let height_ok = period_y > 0 && height.is_multiple_of(period_y) && !angle_bad;

    SeamlessReport {
        kind: SeamlessKind::ExactOrdered,
        period_x: Some(period_x),
        period_y: Some(period_y),
        width_ok,
        height_ok,
        angle_breaks_periodicity: angle_bad,
        nearest_width: nearest_multiples(width, period_x),
        nearest_height: nearest_multiples(height, period_y),
    }
}

/// Human-readable warning lines for UI (empty when seamless).
pub fn seamless_warning_lines(report: &SeamlessReport, width: u32, height: u32) -> Vec<String> {
    match report.kind {
        SeamlessKind::ApproximateOnly => {
            vec!["Error diffusion cannot be exactly seamless (sequential). \
             Enable Approximate wrap-padding for a statistical edge fix — \
             results are approximate only, not bit-periodic."
                .into()]
        }
        SeamlessKind::Unknown => {
            let mut lines =
                vec!["Seamless period unknown for this mode (or custom map size missing).".into()];
            if report.angle_breaks_periodicity {
                lines
                    .push("Pattern angle is not 0°/90° — the lattice is not axis-periodic.".into());
            }
            lines
        }
        SeamlessKind::ExactOrdered => {
            let mut lines = Vec::new();
            if report.angle_breaks_periodicity {
                lines.push("Pattern angle is not 0°/90° — tiling will show a dither seam.".into());
            }
            if let (Some(px), Some(py)) = (report.period_x, report.period_y) {
                if !report.width_ok || !report.height_ok {
                    let mut msg = format!(
                        "Size {width}×{height} is not a multiple of the pattern period {px}×{py} px — \
                         a seam will appear at the tile edge."
                    );
                    if let (Some((wb, wa)), Some((hb, ha))) =
                        (report.nearest_width, report.nearest_height)
                    {
                        msg.push_str(&format!(
                            " Nearest sizes: width {wb}/{wa}, height {hb}/{ha}."
                        ));
                    }
                    lines.push(msg);
                }
            }
            lines
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bayer8_pixel_size_2_period_16() {
        let r = check_seamless(&DitherModeV2::Bayer8x8, 2, 0.0, 1024, 1000, None);
        assert_eq!(r.period_x, Some(16));
        assert_eq!(r.period_y, Some(16));
        assert!(r.width_ok);
        assert!(!r.height_ok);
        assert_eq!(r.nearest_height, Some((992, 1008)));
    }

    #[test]
    fn angle_45_breaks() {
        assert!(pattern_angle_breaks_periodicity(45.0));
        assert!(!pattern_angle_breaks_periodicity(0.0));
        assert!(!pattern_angle_breaks_periodicity(90.0));
        assert!(!pattern_angle_breaks_periodicity(180.0));
        assert!(!pattern_angle_breaks_periodicity(-90.0));
    }

    #[test]
    fn ed_is_approximate_only() {
        let r = check_seamless(&DitherModeV2::FloydSteinberg, 1, 0.0, 64, 64, None);
        assert_eq!(r.kind, SeamlessKind::ApproximateOnly);
        let lines = seamless_warning_lines(&r, 64, 64);
        assert!(!lines.is_empty());
        assert!(lines[0].contains("approximate only") || lines[0].contains("Approximate"));
    }

    #[test]
    fn custom_png_uses_supplied_map_size() {
        let r = check_seamless(
            &DitherModeV2::CustomPng {
                path: "map.png".into(),
            },
            1,
            0.0,
            64,
            50,
            Some((8, 8)),
        );
        assert_eq!(r.period_x, Some(8));
        assert!(r.width_ok);
        assert!(!r.height_ok);
        assert_eq!(r.nearest_height, Some((48, 56)));
    }
}
