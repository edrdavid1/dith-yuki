//! Perceptually smooth color ramps via Oklab linear interpolation + OkLCH gamut clip.

use crate::oklab::{linear_to_oklab, oklab_to_linear, LinRgb, Oklab};
use crate::oklch::{clip_to_srgb_gamut, OkLch};

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Interpolate `steps` colors from `from` to `to` in Oklab, then gamut-clip each sample.
///
/// - `steps` is clamped to at least 1 (no division-by-zero).
/// - Interpolation is linear in `(L, a, b)`; each point is converted to OkLCH,
///   clipped into sRGB, then returned as linear RGB.
pub fn generate_ramp(from: LinRgb, to: LinRgb, steps: usize) -> Vec<LinRgb> {
    let n = steps.max(1);
    let lab_from = linear_to_oklab(from);
    let lab_to = linear_to_oklab(to);
    let denom = (n.saturating_sub(1)).max(1) as f32;

    (0..n)
        .map(|i| {
            let t = i as f32 / denom;
            let lab = Oklab {
                l: lerp(lab_from.l, lab_to.l, t),
                a: lerp(lab_from.a, lab_to.a, t),
                b: lerp(lab_from.b, lab_to.b, t),
            };
            let clipped = clip_to_srgb_gamut(OkLch::from(lab));
            oklab_to_linear(Oklab::from(clipped))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oklab::linear_to_oklab;

    #[test]
    fn black_to_white_five_steps_non_decreasing_l() {
        let ramp = generate_ramp(
            LinRgb {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            LinRgb {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
            5,
        );
        assert_eq!(ramp.len(), 5);
        let ls: Vec<f32> = ramp.iter().map(|c| linear_to_oklab(*c).l).collect();
        for w in ls.windows(2) {
            assert!(
                w[1] + 1e-5 >= w[0],
                "L not non-decreasing: {:?}",
                ls
            );
        }
        // Mid-ramp L should sit between endpoints
        assert!(ls[2] > ls[0] && ls[2] < ls[4], "mid L = {}", ls[2]);
    }

    #[test]
    fn steps_one_no_div_zero() {
        let ramp = generate_ramp(
            LinRgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            LinRgb {
                r: 0.0,
                g: 0.0,
                b: 1.0,
            },
            1,
        );
        assert_eq!(ramp.len(), 1);
        assert!(ramp[0].r.is_finite() && ramp[0].g.is_finite() && ramp[0].b.is_finite());
    }

    #[test]
    fn steps_zero_treated_as_one() {
        let ramp = generate_ramp(
            LinRgb {
                r: 0.2,
                g: 0.3,
                b: 0.4,
            },
            LinRgb {
                r: 0.8,
                g: 0.7,
                b: 0.6,
            },
            0,
        );
        assert_eq!(ramp.len(), 1);
    }

    #[test]
    fn near_gamut_endpoints_no_nan() {
        // Saturated-ish endpoints that may push midpoints toward gamut edge
        let ramp = generate_ramp(
            LinRgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            LinRgb {
                r: 0.0,
                g: 0.0,
                b: 1.0,
            },
            8,
        );
        assert_eq!(ramp.len(), 8);
        for c in &ramp {
            assert!(c.r.is_finite() && c.g.is_finite() && c.b.is_finite());
            assert!((0.0..=1.0).contains(&c.r));
            assert!((0.0..=1.0).contains(&c.g));
            assert!((0.0..=1.0).contains(&c.b));
        }
    }
}
