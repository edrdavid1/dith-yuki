//! Auto-interpolate: fill large Oklab gaps between L-sorted palette neighbors.
//!
//! Reuses [`crate::ramps::generate_ramp`] for interior samples. Draft list order
//! is preserved — new colors are appended (analysis sorts by lightness only).

use crate::oklab::{linear_to_oklab, oklab_dist_sq, LinRgb, Oklab};
use crate::ramps::generate_ramp;

/// Adjacent-distance mean × this factor → gap threshold (adaptive per palette).
pub const GAP_THRESHOLD_FACTOR: f32 = 2.0;

/// Max colors inserted into a single gap in one pass.
pub const MAX_INSERT_PER_GAP: usize = 3;

/// Result of [`auto_interpolate`].
#[derive(Debug, Clone, PartialEq)]
pub struct AutoInterpolateResult {
    /// Original colors plus any appended fills (same order prefix).
    pub colors: Vec<LinRgb>,
    /// How many colors were appended (`0` → no gaps / nothing to do).
    pub inserted: usize,
}

/// Find L-sorted neighbor gaps (full Oklab distance) and append Oklab-lerp fills.
///
/// - Fewer than 2 colors → unchanged, `inserted = 0`.
/// - Threshold per gap = mean of **other** adjacent distances × [`GAP_THRESHOLD_FACTOR`]
///   (leave-one-out; global mean×2 never fires for a 3-swatch “tiny + huge” pair).
/// - Per gap: `floor(dist / threshold).clamp(1, MAX_INSERT_PER_GAP)` interiors
///   (or 1 fill when other gaps are ~0).
/// - Two-color palettes (single interval) never interpolate.
pub fn auto_interpolate(colors: &[LinRgb]) -> AutoInterpolateResult {
    if colors.len() < 2 {
        return AutoInterpolateResult {
            colors: colors.to_vec(),
            inserted: 0,
        };
    }

    let mut indexed: Vec<(f32, usize, Oklab)> = colors
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let lab = linear_to_oklab(*c);
            (lab.l, i, lab)
        })
        .collect();
    indexed.sort_by(|a, b| a.0.total_cmp(&b.0));

    let dists: Vec<f32> = indexed
        .windows(2)
        .map(|w| oklab_dist_sq(w[0].2, w[1].2).sqrt())
        .collect();

    // Leave-one-out mean × factor: with only two intervals, a huge gap vs a
    // tiny neighbor still exceeds threshold (global mean×2 would equal the
    // huge gap and never fire). Single-interval palettes (2 colors) → no gaps.
    if dists.len() < 2 {
        return AutoInterpolateResult {
            colors: colors.to_vec(),
            inserted: 0,
        };
    }

    let mut insertions: Vec<LinRgb> = Vec::new();
    for (i, &dist) in dists.iter().enumerate() {
        let others: f32 = dists
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, d)| *d)
            .sum();
        let other_mean = others / (dists.len() - 1) as f32;
        let threshold = other_mean * GAP_THRESHOLD_FACTOR;
        if !dist.is_finite() || dist <= threshold {
            continue;
        }
        // other_mean ≈ 0 and dist > 0 also counts (clustered neighbors + outlier).
        let count = if threshold <= f32::EPSILON {
            1
        } else {
            ((dist / threshold).floor() as usize).clamp(1, MAX_INSERT_PER_GAP)
        };
        let from = colors[indexed[i].1];
        let to = colors[indexed[i + 1].1];
        let ramp = generate_ramp(from, to, count + 2);
        if ramp.len() >= 3 {
            insertions.extend_from_slice(&ramp[1..ramp.len() - 1]);
        }
    }

    let inserted = insertions.len();
    if inserted == 0 {
        return AutoInterpolateResult {
            colors: colors.to_vec(),
            inserted: 0,
        };
    }

    let mut out = colors.to_vec();
    out.extend(insertions);
    AutoInterpolateResult {
        colors: out,
        inserted,
    }
}

/// `true` when [`auto_interpolate`] would append at least one color.
#[inline]
pub fn would_auto_interpolate(colors: &[LinRgb]) -> bool {
    auto_interpolate(colors).inserted > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb(r: f32, g: f32, b: f32) -> LinRgb {
        LinRgb { r, g, b }
    }

    #[test]
    fn fewer_than_two_colors_noop() {
        assert_eq!(auto_interpolate(&[]).inserted, 0);
        assert_eq!(auto_interpolate(&[rgb(0.5, 0.5, 0.5)]).inserted, 0);
    }

    #[test]
    fn two_colors_never_gaps_adaptive_threshold() {
        // Single adjacent pair → mean = dist → threshold = 2*dist → never a gap.
        let r = auto_interpolate(&[rgb(0.0, 0.0, 0.0), rgb(1.0, 1.0, 1.0)]);
        assert_eq!(r.inserted, 0);
        assert_eq!(r.colors.len(), 2);
    }

    #[test]
    fn uniform_oklab_l_steps_no_gaps() {
        // Equal ΔL in Oklab (not equal linear RGB) → leave-one-out thresholds idle.
        let labs = [0.0f32, 0.25, 0.5, 0.75, 1.0];
        let colors: Vec<LinRgb> = labs
            .iter()
            .map(|&l| crate::oklab::oklab_to_linear(Oklab { l, a: 0.0, b: 0.0 }))
            .collect();
        assert_eq!(auto_interpolate(&colors).inserted, 0);
        assert!(!would_auto_interpolate(&colors));
    }

    #[test]
    fn large_l_gap_inserts_midpoints() {
        // Two tight darks + one distant light → one large L gap.
        let colors = [
            rgb(0.0, 0.0, 0.0),
            rgb(0.02, 0.02, 0.02),
            rgb(1.0, 1.0, 1.0),
        ];
        let r = auto_interpolate(&colors);
        assert!(r.inserted >= 1, "expected fills, got {}", r.inserted);
        assert_eq!(r.colors.len(), colors.len() + r.inserted);
        // Prefix preserved
        assert_eq!(&r.colors[..3], &colors);
        assert!(would_auto_interpolate(&colors));
    }

    #[test]
    fn pink_outlier_gap_detected() {
        // Cold neighbors + warm light outlier (dog-portrait style gap).
        let colors = [
            rgb(0.02, 0.05, 0.08), // dark cool
            rgb(0.05, 0.08, 0.12),
            rgb(0.08, 0.12, 0.18),
            rgb(0.95, 0.55, 0.65), // warm/pink light — L jump + chroma
        ];
        let r = auto_interpolate(&colors);
        assert!(
            r.inserted >= 1,
            "pink outlier should create a gap, inserted={}",
            r.inserted
        );
        assert!(r.inserted <= MAX_INSERT_PER_GAP * 3);
    }

    #[test]
    fn insert_count_clamped_to_max_per_gap() {
        // Extreme: black + near-black + white → huge gap, still ≤ 3 fills for that gap.
        let colors = [
            rgb(0.0, 0.0, 0.0),
            rgb(0.001, 0.001, 0.001),
            rgb(1.0, 1.0, 1.0),
        ];
        let r = auto_interpolate(&colors);
        assert!(r.inserted <= MAX_INSERT_PER_GAP);
        assert!(r.inserted >= 1);
    }

    #[test]
    fn all_identical_colors_noop() {
        let colors = [rgb(0.4, 0.4, 0.4); 5];
        assert_eq!(auto_interpolate(&colors).inserted, 0);
    }
}
