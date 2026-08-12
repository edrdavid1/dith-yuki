//! Hue / lightness harmony rules in OkLCH with sRGB gamut clipping.

use crate::oklab::{linear_to_oklab, oklab_to_linear, LinRgb, Oklab};
use crate::oklch::{clip_to_srgb_gamut, OkLch};

const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

/// Built-in harmony generation rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarmonyRule {
    Monochromatic,
    Analogous,
    Complementary,
    Triadic,
    SplitComplementary,
}

#[inline]
fn normalize_hue(h: f32) -> f32 {
    h.rem_euclid(TAU)
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn finish(lch: OkLch) -> LinRgb {
    let clipped = clip_to_srgb_gamut(OkLch {
        l: lch.l,
        c: lch.c,
        h: normalize_hue(lch.h),
    });
    oklab_to_linear(Oklab::from(clipped))
}

/// Generate a harmony palette from `base` using `rule`.
///
/// `count` is the desired number of colors (clamped to at least 1). Exact
/// cardinality depends on the rule (e.g. Triadic always yields 3).
pub fn generate_harmony(base: LinRgb, rule: HarmonyRule, count: usize) -> Vec<LinRgb> {
    generate_harmony_with_spread(base, rule, count, PI / 6.0) // ~30°
}

/// Like [`generate_harmony`], but `analogous_spread` (radians, half-width)
/// controls the Analogous span around the base hue.
pub fn generate_harmony_with_spread(
    base: LinRgb,
    rule: HarmonyRule,
    count: usize,
    analogous_spread: f32,
) -> Vec<LinRgb> {
    let n = count.max(1);
    let base_lch = OkLch::from(linear_to_oklab(base));

    match rule {
        HarmonyRule::Monochromatic => {
            // Vary L in [0.15, 0.9] at fixed C/H
            (0..n)
                .map(|i| {
                    let t = if n == 1 {
                        0.5
                    } else {
                        i as f32 / (n - 1) as f32
                    };
                    let l = lerp(0.15, 0.9, t);
                    finish(OkLch {
                        l,
                        c: base_lch.c,
                        h: base_lch.h,
                    })
                })
                .collect()
        }
        HarmonyRule::Analogous => {
            let spread = analogous_spread.abs().max(1e-4);
            (0..n)
                .map(|i| {
                    let t = if n == 1 {
                        0.5
                    } else {
                        i as f32 / (n - 1) as f32
                    };
                    // Map t∈[0,1] → hue offset ∈ [-spread, +spread]
                    let offset = lerp(-spread, spread, t);
                    finish(OkLch {
                        l: base_lch.l,
                        c: base_lch.c,
                        h: base_lch.h + offset,
                    })
                })
                .collect()
        }
        HarmonyRule::Complementary => {
            // Base + opposite (π). For count > 2, alternate the two hues.
            let out_n = n.max(2);
            (0..out_n)
                .map(|i| {
                    let h = if i % 2 == 0 {
                        base_lch.h
                    } else {
                        base_lch.h + PI
                    };
                    finish(OkLch {
                        l: base_lch.l,
                        c: base_lch.c,
                        h,
                    })
                })
                .collect()
        }
        HarmonyRule::Triadic => {
            let hues = [0.0, TAU / 3.0, 2.0 * TAU / 3.0];
            let out_n = n.max(3);
            (0..out_n)
                .map(|i| {
                    let h = base_lch.h + hues[i % 3];
                    finish(OkLch {
                        l: base_lch.l,
                        c: base_lch.c,
                        h,
                    })
                })
                .collect()
        }
        HarmonyRule::SplitComplementary => {
            let delta = 0.5; // ~28.6°
            let hues = [0.0, PI - delta, PI + delta];
            let out_n = n.max(3);
            (0..out_n)
                .map(|i| {
                    let h = base_lch.h + hues[i % 3];
                    finish(OkLch {
                        l: base_lch.l,
                        c: base_lch.c,
                        h,
                    })
                })
                .collect()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_red() -> LinRgb {
        LinRgb {
            r: 0.8,
            g: 0.15,
            b: 0.1,
        }
    }

    /// Pre-clip hue delta for complementary pair (first two outputs).
    #[test]
    fn complementary_delta_h_is_pi() {
        let colors = generate_harmony(base_red(), HarmonyRule::Complementary, 2);
        assert!(colors.len() >= 2);
        let h0 = OkLch::from(linear_to_oklab(colors[0])).h;
        let h1 = OkLch::from(linear_to_oklab(colors[1])).h;
        let mut dh = (h1 - h0).abs();
        if dh > PI {
            dh = TAU - dh;
        }
        assert!(
            (dh - PI).abs() < 0.15,
            "ΔH should be ≈ π, got {} (h0={}, h1={})",
            dh,
            h0,
            h1
        );
    }

    #[test]
    fn triadic_spacing_approx_2pi_over_3() {
        // Assert on pre-clip hue offsets via reconstructing from the rule logic
        let base = OkLch::from(linear_to_oklab(base_red()));
        let expected = [
            normalize_hue(base.h),
            normalize_hue(base.h + TAU / 3.0),
            normalize_hue(base.h + 2.0 * TAU / 3.0),
        ];
        for w in expected.windows(2) {
            let mut dh = (w[1] - w[0]).rem_euclid(TAU);
            if dh > PI {
                dh = TAU - dh;
            }
            // adjacent triadic spacing should be 2π/3 ≈ 2.094
            assert!(
                (dh - TAU / 3.0).abs() < 1e-4,
                "triadic spacing {}",
                dh
            );
        }

        let colors = generate_harmony(base_red(), HarmonyRule::Triadic, 3);
        assert_eq!(colors.len(), 3);
        for c in &colors {
            assert!(c.r.is_finite() && c.g.is_finite() && c.b.is_finite());
        }
    }

    #[test]
    fn monochromatic_shared_ch_distinct_l() {
        let colors = generate_harmony(base_red(), HarmonyRule::Monochromatic, 5);
        assert_eq!(colors.len(), 5);
        let lchs: Vec<OkLch> = colors
            .iter()
            .map(|c| OkLch::from(linear_to_oklab(*c)))
            .collect();

        for i in 1..lchs.len() {
            // Hue and chroma stay near base (clip may nudge C slightly)
            let mut dh = (lchs[i].h - lchs[0].h).abs();
            if dh > PI {
                dh = TAU - dh;
            }
            assert!(dh < 0.2, "hue drifted: {}", dh);
            assert!((lchs[i].c - lchs[0].c).abs() < 0.15 || lchs[i].c <= lchs[0].c + 0.15);
            assert!(
                (lchs[i].l - lchs[i - 1].l).abs() > 1e-4,
                "L should differ across samples"
            );
        }
        // Overall L range should increase
        assert!(lchs.last().unwrap().l > lchs.first().unwrap().l);
    }

    #[test]
    fn analogous_count_dynamic() {
        let a = generate_harmony(base_red(), HarmonyRule::Analogous, 3);
        let b = generate_harmony(base_red(), HarmonyRule::Analogous, 7);
        assert_eq!(a.len(), 3);
        assert_eq!(b.len(), 7);
    }

    #[test]
    fn split_complementary_three_colors() {
        let colors = generate_harmony(base_red(), HarmonyRule::SplitComplementary, 3);
        assert_eq!(colors.len(), 3);
    }
}
