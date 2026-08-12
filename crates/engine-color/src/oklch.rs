//! OkLCH (cylindrical Oklab) and sRGB gamut clipping.
//!
//! # Hue convention
//!
//! Hue `h` is stored and manipulated in **radians** inside the engine
//! (`atan2(b, a)`). Convert to degrees only at UI / IPC boundaries.

use crate::oklab::{oklab_to_linear_unclamped, LinRgb, Oklab};

/// Cylindrical Oklab: Lightness, Chroma, Hue.
///
/// - `l`: Lightness (same as [`Oklab::l`])
/// - `c`: Chroma = hypot(a, b) ≥ 0
/// - `h`: Hue angle in **radians** (engine convention)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OkLch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

impl From<Oklab> for OkLch {
    fn from(lab: Oklab) -> Self {
        // atan2(0, 0) is defined as 0.0 in IEEE-754 / Rust — no NaN.
        let c = lab.a.hypot(lab.b);
        let h = lab.b.atan2(lab.a);
        OkLch {
            l: lab.l,
            c,
            h,
        }
    }
}

impl From<OkLch> for Oklab {
    fn from(lch: OkLch) -> Self {
        Oklab {
            l: lch.l,
            a: lch.c * lch.h.cos(),
            b: lch.c * lch.h.sin(),
        }
    }
}

/// True if any linear sRGB channel is outside the displayable range [0, 1].
#[inline]
pub fn is_out_of_srgb_gamut(rgb: LinRgb) -> bool {
    rgb.r < 0.0 || rgb.r > 1.0 || rgb.g < 0.0 || rgb.g > 1.0 || rgb.b < 0.0 || rgb.b > 1.0
}

fn lch_in_srgb_gamut(lch: OkLch) -> bool {
    let rgb = oklab_to_linear_unclamped(Oklab::from(lch));
    !is_out_of_srgb_gamut(rgb)
}

/// Reduce chroma at fixed L and H until the color is in the sRGB gamut.
///
/// Binary-searches `c` in `[0, c0]`. If `c0` is already in gamut, returns
/// unchanged. Preserves `l` and `h`.
pub fn clip_to_srgb_gamut(lch: OkLch) -> OkLch {
    let c0 = if lch.c.is_finite() && lch.c > 0.0 {
        lch.c
    } else {
        0.0
    };
    let candidate = OkLch {
        l: lch.l,
        c: c0,
        h: lch.h,
    };
    if lch_in_srgb_gamut(candidate) {
        return candidate;
    }

    // Binary search: largest c in [0, c0] that is in gamut.
    let mut lo = 0.0f32;
    let mut hi = c0;
    for _ in 0..32 {
        let mid = (lo + hi) * 0.5;
        let mid_lch = OkLch {
            l: lch.l,
            c: mid,
            h: lch.h,
        };
        if lch_in_srgb_gamut(mid_lch) {
            lo = mid;
        } else {
            hi = mid;
        }
    }

    OkLch {
        l: lch.l,
        c: lo,
        h: lch.h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oklab::{linear_to_oklab, oklab_to_linear};
    use proptest::prelude::*;

    fn assert_oklab_approx(actual: Oklab, expected: Oklab, tol: f32) {
        assert!(
            (actual.l - expected.l).abs() < tol,
            "L: {} vs {}",
            actual.l,
            expected.l
        );
        assert!(
            (actual.a - expected.a).abs() < tol,
            "a: {} vs {}",
            actual.a,
            expected.a
        );
        assert!(
            (actual.b - expected.b).abs() < tol,
            "b: {} vs {}",
            actual.b,
            expected.b
        );
    }

    #[test]
    fn achromatic_atan2_no_nan() {
        let lab = Oklab {
            l: 0.5,
            a: 0.0,
            b: 0.0,
        };
        let lch = OkLch::from(lab);
        assert!(lch.c.is_finite());
        assert!(lch.h.is_finite());
        assert!((lch.c - 0.0).abs() < 1e-7);
        // Hue is meaningless at C=0; atan2(0,0) → 0, no panic/NaN
        assert_eq!(lch.h, 0.0);

        let back = Oklab::from(lch);
        assert_oklab_approx(back, lab, 1e-6);
    }

    #[test]
    fn round_trip_chromatic() {
        let lab = Oklab {
            l: 0.6,
            a: 0.12,
            b: -0.08,
        };
        let back = Oklab::from(OkLch::from(lab));
        assert_oklab_approx(back, lab, 1e-5);
    }

    #[test]
    fn round_trip_via_srgb_sample() {
        let rgb = LinRgb {
            r: 0.8,
            g: 0.2,
            b: 0.1,
        };
        let lab = linear_to_oklab(rgb);
        let back = Oklab::from(OkLch::from(lab));
        assert_oklab_approx(back, lab, 1e-5);
    }

    proptest! {
        #[test]
        fn prop_oklab_oklch_round_trip(
            l in 0.0f32..=1.0,
            a in -0.4f32..=0.4,
            b in -0.4f32..=0.4,
        ) {
            let lab = Oklab { l, a, b };
            let lch = OkLch::from(lab);
            prop_assert!(lch.l.is_finite() && lch.c.is_finite() && lch.h.is_finite());
            prop_assert!(lch.c >= 0.0);
            let back = Oklab::from(lch);
            prop_assert!((back.l - lab.l).abs() < 1e-4);
            prop_assert!((back.a - lab.a).abs() < 1e-4);
            prop_assert!((back.b - lab.b).abs() < 1e-4);
        }
    }

    #[test]
    fn is_out_of_gamut_detects_channels() {
        assert!(!is_out_of_srgb_gamut(LinRgb {
            r: 0.0,
            g: 0.5,
            b: 1.0
        }));
        assert!(is_out_of_srgb_gamut(LinRgb {
            r: -0.01,
            g: 0.5,
            b: 0.5
        }));
        assert!(is_out_of_srgb_gamut(LinRgb {
            r: 0.5,
            g: 1.01,
            b: 0.5
        }));
    }

    #[test]
    fn clip_out_of_gamut_reduces_c_keeps_lh() {
        // High chroma at mid L is typically out of sRGB for many hues.
        let out = OkLch {
            l: 0.7,
            c: 0.4,
            h: 0.5, // radians
        };
        assert!(
            is_out_of_srgb_gamut(oklab_to_linear_unclamped(Oklab::from(out))),
            "fixture should be out of gamut"
        );

        let clipped = clip_to_srgb_gamut(out);
        assert!((clipped.l - out.l).abs() < 1e-6);
        assert!((clipped.h - out.h).abs() < 1e-6);
        assert!(clipped.c <= out.c + 1e-6);
        assert!(clipped.c < out.c); // must have reduced

        let rgb = oklab_to_linear_unclamped(Oklab::from(clipped));
        assert!(!is_out_of_srgb_gamut(rgb));
        // Display path also stays in range
        let display = oklab_to_linear(Oklab::from(clipped));
        assert!((0.0..=1.0).contains(&display.r));
        assert!((0.0..=1.0).contains(&display.g));
        assert!((0.0..=1.0).contains(&display.b));
    }

    #[test]
    fn clip_in_gamut_unchanged() {
        let lab = linear_to_oklab(LinRgb {
            r: 0.3,
            g: 0.5,
            b: 0.7,
        });
        let lch = OkLch::from(lab);
        let clipped = clip_to_srgb_gamut(lch);
        assert!((clipped.l - lch.l).abs() < 1e-5);
        assert!((clipped.c - lch.c).abs() < 1e-4);
        assert!((clipped.h - lch.h).abs() < 1e-5);
    }

    #[test]
    fn clip_zero_chroma() {
        let lch = OkLch {
            l: 0.4,
            c: 0.0,
            h: 1.2,
        };
        let clipped = clip_to_srgb_gamut(lch);
        assert!((clipped.c - 0.0).abs() < 1e-7);
        assert!((clipped.l - 0.4).abs() < 1e-6);
    }
}
