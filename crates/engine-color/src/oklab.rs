//! Oklab color space conversions (linear RGB ↔ Oklab).
//!
//! The LMS matrix used here assumes **sRGB/Rec.709 primaries**.
//! Inputs from non-sRGB working spaces require prior ICC-based
//! conversion to linear sRGB before calling these functions.

/// Linear RGB color (f32, channels in [0.0, 1.0]).
/// Matches the internal representation of PixelTile.
///
/// NOTE: The conversion matrices assume sRGB/Rec.709 primaries.
/// If your source data uses a different color space (e.g., Adobe RGB,
/// DCI-P3), convert to linear sRGB first.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinRgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// Oklab perceptually uniform color space.
///
/// - `l`: Lightness, nominally [0, 1]
/// - `a`: Green–red axis, approximately [-0.5, 0.5]
/// - `b`: Blue–yellow axis, approximately [-0.5, 0.5]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

// ---------------------------------------------------------------------------
// Björn Ottosson matrices for sRGB/Rec.709 linear RGB ↔ Oklab.
// Reference: https://bottosson.github.io/posts/oklab/
// ---------------------------------------------------------------------------

/// M1: Linear RGB → LMS (sRGB/Rec.709 primaries)
const M1: [[f32; 3]; 3] = [
    [0.4122214708, 0.5363325363, 0.0514459929],
    [0.2119034982, 0.6806995451, 0.1073969566],
    [0.0883024619, 0.2817188376, 0.6299787005],
];

/// M2: L'M'S' (cube-root LMS) → Oklab
const M2: [[f32; 3]; 3] = [
    [0.2104542553, 0.7936177850, -0.0040720468],
    [1.9779984951, -2.4285922050, 0.4505937099],
    [0.0259040371, 0.7827717662, -0.8086757660],
];

/// Inverse of M2: Oklab → L'M'S'
const M2_INV: [[f32; 3]; 3] = [
    [1.0000000000, 0.3963377774, 0.2158037573],
    [1.0000000000, -0.1055613458, -0.0638541728],
    [1.0000000000, -0.0894841775, -1.2914855480],
];

/// Inverse of M1: LMS → Linear RGB
const M1_INV: [[f32; 3]; 3] = [
    [4.0767416621, -3.3077115913, 0.2309699292],
    [-1.2684380046, 2.6097574011, -0.3413193965],
    [-0.0041960863, -0.7034186147, 1.7076147010],
];

/// Replace NaN or Inf with 0.0.
#[inline]
fn sanitize(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Convert linear RGB to Oklab.
///
/// Assumes sRGB/Rec.709 primaries. Input channels are clamped to [0, 1].
/// NaN/Inf values are replaced with 0.0 before conversion.
///
/// Algorithm:
/// 1. Sanitize inputs (NaN/Inf → 0.0), clamp to [0, 1]
/// 2. Linear RGB → LMS via M1 matrix
/// 3. LMS → L'M'S' via cube root
/// 4. L'M'S' → Oklab via M2 matrix
pub fn linear_to_oklab(rgb: LinRgb) -> Oklab {
    // Step 1: Sanitize and clamp
    let r = sanitize(rgb.r).clamp(0.0, 1.0);
    let g = sanitize(rgb.g).clamp(0.0, 1.0);
    let b = sanitize(rgb.b).clamp(0.0, 1.0);

    // Step 2: Linear RGB → LMS
    let l = M1[0][0] * r + M1[0][1] * g + M1[0][2] * b;
    let m = M1[1][0] * r + M1[1][1] * g + M1[1][2] * b;
    let s = M1[2][0] * r + M1[2][1] * g + M1[2][2] * b;

    // Step 3: LMS → L'M'S' via cube root
    let l_ = l.cbrt();
    let m_ = m.cbrt();
    let s_ = s.cbrt();

    // Step 4: L'M'S' → Oklab via M2
    Oklab {
        l: M2[0][0] * l_ + M2[0][1] * m_ + M2[0][2] * s_,
        a: M2[1][0] * l_ + M2[1][1] * m_ + M2[1][2] * s_,
        b: M2[2][0] * l_ + M2[2][1] * m_ + M2[2][2] * s_,
    }
}

/// Convert Oklab back to linear RGB.
///
/// Result channels are clamped to [0.0, 1.0].
/// NaN/Inf values are replaced with 0.0 before conversion.
///
/// Algorithm:
/// 1. Sanitize inputs (NaN/Inf → 0.0)
/// 2. Oklab → L'M'S' via inverse M2
/// 3. L'M'S' → LMS via cube (x³)
/// 4. LMS → Linear RGB via inverse M1
/// 5. Clamp to [0.0, 1.0]
pub fn oklab_to_linear(lab: Oklab) -> LinRgb {
    let rgb = oklab_to_linear_unclamped(lab);
    LinRgb {
        r: rgb.r.clamp(0.0, 1.0),
        g: rgb.g.clamp(0.0, 1.0),
        b: rgb.b.clamp(0.0, 1.0),
    }
}

/// Convert Oklab to linear RGB **without** clamping channels to [0, 1].
///
/// Used for sRGB gamut tests / clipping. Non-finite inputs are sanitized to 0.0.
pub fn oklab_to_linear_unclamped(lab: Oklab) -> LinRgb {
    // Step 1: Sanitize
    let l = sanitize(lab.l);
    let a = sanitize(lab.a);
    let b = sanitize(lab.b);

    // Step 2: Oklab → L'M'S' via M2_INV
    let l_ = M2_INV[0][0] * l + M2_INV[0][1] * a + M2_INV[0][2] * b;
    let m_ = M2_INV[1][0] * l + M2_INV[1][1] * a + M2_INV[1][2] * b;
    let s_ = M2_INV[2][0] * l + M2_INV[2][1] * a + M2_INV[2][2] * b;

    // Step 3: L'M'S' → LMS via cube
    let l_lms = l_ * l_ * l_;
    let m_lms = m_ * m_ * m_;
    let s_lms = s_ * s_ * s_;

    // Step 4: LMS → Linear RGB via M1_INV (no clamp)
    LinRgb {
        r: M1_INV[0][0] * l_lms + M1_INV[0][1] * m_lms + M1_INV[0][2] * s_lms,
        g: M1_INV[1][0] * l_lms + M1_INV[1][1] * m_lms + M1_INV[1][2] * s_lms,
        b: M1_INV[2][0] * l_lms + M1_INV[2][1] * m_lms + M1_INV[2][2] * s_lms,
    }
}

/// Squared Euclidean distance between two Oklab colors.
///
/// Avoids the sqrt for efficient nearest-neighbor comparisons.
#[inline]
pub fn oklab_dist_sq(a: Oklab, b: Oklab) -> f32 {
    let dl = a.l - b.l;
    let da = a.a - b.a;
    let db = a.b - b.b;
    dl * dl + da * da + db * db
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: assert two LinRgb values are approximately equal.
    fn assert_rgb_approx(actual: LinRgb, expected: LinRgb, tol: f32) {
        assert!(
            (actual.r - expected.r).abs() < tol,
            "r: {} vs {} (diff {})",
            actual.r, expected.r, (actual.r - expected.r).abs()
        );
        assert!(
            (actual.g - expected.g).abs() < tol,
            "g: {} vs {} (diff {})",
            actual.g, expected.g, (actual.g - expected.g).abs()
        );
        assert!(
            (actual.b - expected.b).abs() < tol,
            "b: {} vs {} (diff {})",
            actual.b, expected.b, (actual.b - expected.b).abs()
        );
    }

    /// Helper: assert two Oklab values are approximately equal.
    fn assert_oklab_approx(actual: Oklab, expected: Oklab, tol: f32) {
        assert!(
            (actual.l - expected.l).abs() < tol,
            "L: {} vs {} (diff {})",
            actual.l, expected.l, (actual.l - expected.l).abs()
        );
        assert!(
            (actual.a - expected.a).abs() < tol,
            "a: {} vs {} (diff {})",
            actual.a, expected.a, (actual.a - expected.a).abs()
        );
        assert!(
            (actual.b - expected.b).abs() < tol,
            "b: {} vs {} (diff {})",
            actual.b, expected.b, (actual.b - expected.b).abs()
        );
    }

    #[test]
    fn test_black() {
        let black = LinRgb { r: 0.0, g: 0.0, b: 0.0 };
        let lab = linear_to_oklab(black);
        assert_oklab_approx(lab, Oklab { l: 0.0, a: 0.0, b: 0.0 }, 1e-6);

        let back = oklab_to_linear(lab);
        assert_rgb_approx(back, black, 1e-6);
    }

    #[test]
    fn test_white() {
        let white = LinRgb { r: 1.0, g: 1.0, b: 1.0 };
        let lab = linear_to_oklab(white);
        // White should have L ≈ 1.0 and a ≈ 0, b ≈ 0
        assert!((lab.l - 1.0).abs() < 1e-5, "white L = {}", lab.l);
        assert!(lab.a.abs() < 1e-5, "white a = {}", lab.a);
        assert!(lab.b.abs() < 1e-5, "white b = {}", lab.b);

        let back = oklab_to_linear(lab);
        assert_rgb_approx(back, white, 1e-5);
    }

    #[test]
    fn test_pure_red() {
        let red = LinRgb { r: 1.0, g: 0.0, b: 0.0 };
        let lab = linear_to_oklab(red);
        // Red should have positive L, positive a (red direction), positive b
        assert!(lab.l > 0.0, "red L should be positive: {}", lab.l);
        assert!(lab.a > 0.0, "red a should be positive: {}", lab.a);
        assert!(lab.b > 0.0, "red b should be positive: {}", lab.b);

        let back = oklab_to_linear(lab);
        assert_rgb_approx(back, red, 1e-5);
    }

    #[test]
    fn test_pure_green() {
        let green = LinRgb { r: 0.0, g: 1.0, b: 0.0 };
        let lab = linear_to_oklab(green);
        // Green should have positive L, negative a (green direction)
        assert!(lab.l > 0.0, "green L should be positive: {}", lab.l);
        assert!(lab.a < 0.0, "green a should be negative: {}", lab.a);

        let back = oklab_to_linear(lab);
        assert_rgb_approx(back, green, 1e-5);
    }

    #[test]
    fn test_pure_blue() {
        let blue = LinRgb { r: 0.0, g: 0.0, b: 1.0 };
        let lab = linear_to_oklab(blue);
        // Blue should have positive L, negative b (blue direction)
        assert!(lab.l > 0.0, "blue L should be positive: {}", lab.l);
        assert!(lab.b < 0.0, "blue b should be negative: {}", lab.b);

        let back = oklab_to_linear(lab);
        assert_rgb_approx(back, blue, 1e-5);
    }

    #[test]
    fn test_mid_gray() {
        // Mid-gray in linear space (0.5, 0.5, 0.5)
        let gray = LinRgb { r: 0.5, g: 0.5, b: 0.5 };
        let lab = linear_to_oklab(gray);
        // Gray should be achromatic: a ≈ 0, b ≈ 0
        assert!(lab.a.abs() < 1e-5, "gray a = {}", lab.a);
        assert!(lab.b.abs() < 1e-5, "gray b = {}", lab.b);
        // L should be between 0 and 1
        assert!(lab.l > 0.0 && lab.l < 1.0, "gray L = {}", lab.l);

        let back = oklab_to_linear(lab);
        assert_rgb_approx(back, gray, 1e-5);
    }

    #[test]
    fn test_round_trip_various_colors() {
        let colors = [
            LinRgb { r: 0.2, g: 0.4, b: 0.6 },
            LinRgb { r: 0.8, g: 0.1, b: 0.3 },
            LinRgb { r: 0.0, g: 0.5, b: 1.0 },
            LinRgb { r: 1.0, g: 1.0, b: 0.0 },
            LinRgb { r: 0.0, g: 1.0, b: 1.0 },
            LinRgb { r: 1.0, g: 0.0, b: 1.0 },
        ];

        for &color in &colors {
            let lab = linear_to_oklab(color);
            let back = oklab_to_linear(lab);
            assert_rgb_approx(back, color, 1e-5);
        }
    }

    #[test]
    fn test_nan_handling_forward() {
        let nan_rgb = LinRgb { r: f32::NAN, g: 0.5, b: f32::NAN };
        let lab = linear_to_oklab(nan_rgb);
        // NaN channels treated as 0.0, so this is like (0.0, 0.5, 0.0)
        let expected = linear_to_oklab(LinRgb { r: 0.0, g: 0.5, b: 0.0 });
        assert_oklab_approx(lab, expected, 1e-6);
    }

    #[test]
    fn test_inf_handling_forward() {
        let inf_rgb = LinRgb { r: f32::INFINITY, g: f32::NEG_INFINITY, b: 0.5 };
        let lab = linear_to_oklab(inf_rgb);
        // Inf → 0.0, so this is like (0.0, 0.0, 0.5)
        let expected = linear_to_oklab(LinRgb { r: 0.0, g: 0.0, b: 0.5 });
        assert_oklab_approx(lab, expected, 1e-6);
    }

    #[test]
    fn test_nan_handling_inverse() {
        let nan_lab = Oklab { l: f32::NAN, a: 0.0, b: 0.0 };
        let rgb = oklab_to_linear(nan_lab);
        // NaN → 0.0, so this is like Oklab(0, 0, 0) → black
        let expected = oklab_to_linear(Oklab { l: 0.0, a: 0.0, b: 0.0 });
        assert_rgb_approx(rgb, expected, 1e-6);
    }

    #[test]
    fn test_inf_handling_inverse() {
        let inf_lab = Oklab { l: f32::INFINITY, a: f32::NEG_INFINITY, b: f32::NAN };
        let rgb = oklab_to_linear(inf_lab);
        // All non-finite → 0.0, same as black
        let expected = oklab_to_linear(Oklab { l: 0.0, a: 0.0, b: 0.0 });
        assert_rgb_approx(rgb, expected, 1e-6);
    }

    #[test]
    fn test_oklab_dist_sq_identical() {
        let a = Oklab { l: 0.5, a: 0.1, b: -0.2 };
        assert_eq!(oklab_dist_sq(a, a), 0.0);
    }

    #[test]
    fn test_oklab_dist_sq_known() {
        let a = Oklab { l: 0.0, a: 0.0, b: 0.0 };
        let b = Oklab { l: 1.0, a: 0.0, b: 0.0 };
        assert!((oklab_dist_sq(a, b) - 1.0).abs() < 1e-10);

        let c = Oklab { l: 0.0, a: 3.0, b: 4.0 };
        // dist_sq = 0 + 9 + 16 = 25
        assert!((oklab_dist_sq(a, c) - 25.0).abs() < 1e-6);
    }

    #[test]
    fn test_oklab_dist_sq_symmetry() {
        let a = Oklab { l: 0.3, a: 0.1, b: -0.2 };
        let b = Oklab { l: 0.7, a: -0.1, b: 0.3 };
        assert!((oklab_dist_sq(a, b) - oklab_dist_sq(b, a)).abs() < 1e-10);
    }
}
