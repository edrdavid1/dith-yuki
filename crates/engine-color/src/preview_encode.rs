//! Unified display preview encode for Composite tiles.
//!
//! **Working buffer convention:** Composite/`decode_*` store RGB as `u8/255`
//! (display-referred / sRGB-encoded), not optical linear. Preview must not apply
//! an extra sRGB transfer on proof-off — that double-encodes and looks muddy.
//!
//! - Proof off: `clamp(c) * 255` (same as historical `f32_tile_to_rgba8`)
//! - Proof on: CMS on those encoded values → u8
//!
//! Export may share this buffer convention; soft-proof never touches export.

use crate::palette::srgb_to_linear;
use crate::soft_proof::SoftProofTransform;

/// Size of the optional linear→sRGB 1D LUT (for true-linear helpers / tests).
const SRGB_LUT_SIZE: usize = 4096;

/// Precomputed linear [0,1] → encoded sRGB [0,1] LUT.
pub struct SrgbEncodeLut {
    table: [f32; SRGB_LUT_SIZE],
}

impl Default for SrgbEncodeLut {
    fn default() -> Self {
        Self::new()
    }
}

impl SrgbEncodeLut {
    pub fn new() -> Self {
        let mut table = [0.0f32; SRGB_LUT_SIZE];
        for (i, slot) in table.iter_mut().enumerate() {
            let x = i as f32 / (SRGB_LUT_SIZE - 1) as f32;
            *slot = linear_to_srgb_f32(x);
        }
        Self { table }
    }

    #[inline]
    pub fn encode(&self, linear: f32) -> f32 {
        let x = linear.clamp(0.0, 1.0);
        let max = (SRGB_LUT_SIZE - 1) as f32;
        let f = x * max;
        let i0 = f.floor() as usize;
        let i1 = (i0 + 1).min(SRGB_LUT_SIZE - 1);
        let t = f - i0 as f32;
        self.table[i0] * (1.0 - t) + self.table[i1] * t
    }

    #[inline]
    pub fn encode_u8(&self, linear: f32) -> u8 {
        (self.encode(linear) * 255.0 + 0.5) as u8
    }
}

/// Standard sRGB transfer, returning encoded [0,1] f32 (not u8).
#[inline]
pub fn linear_to_srgb_f32(value: f32) -> f32 {
    let clamped = value.clamp(0.0, 1.0);
    if clamped <= 0.0031308 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    }
}

/// Inverse of [`linear_to_srgb_f32`].
#[inline]
pub fn srgb_f32_to_linear(encoded: f32) -> f32 {
    let c = encoded.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn channel_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Encode interleaved Composite RGBA f32 → RGBA8 for preview.
///
/// Buffer RGB is treated as already sRGB-encoded (`u8/255`). Proof on/off differs
/// only by the CMS block — no transfer-curve jump.
pub fn encode_preview_rgba(
    rgba: &[f32],
    dst_rgba8: &mut [u8],
    proof: Option<&SoftProofTransform>,
) -> Result<(), crate::soft_proof::SoftProofError> {
    if rgba.len() % 4 != 0 || dst_rgba8.len() != rgba.len() {
        return Err(crate::soft_proof::SoftProofError::BufferLength);
    }
    let pixels = rgba.len() / 4;

    match proof {
        None => {
            for i in 0..pixels {
                let a = rgba[i * 4 + 3].clamp(0.0, 1.0);
                // Coverage-weighted RGB: unassociate when α is meaningful.
                let (r, g, b) = if a > 1e-6 {
                    (rgba[i * 4] / a, rgba[i * 4 + 1] / a, rgba[i * 4 + 2] / a)
                } else {
                    (rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2])
                };
                dst_rgba8[i * 4] = channel_to_u8(r);
                dst_rgba8[i * 4 + 1] = channel_to_u8(g);
                dst_rgba8[i * 4 + 2] = channel_to_u8(b);
                dst_rgba8[i * 4 + 3] = channel_to_u8(a);
            }
            Ok(())
        }
        Some(xform) => {
            let mut rgb_enc = vec![0f32; pixels * 3];
            let mut alphas = vec![0f32; pixels];
            for i in 0..pixels {
                let a = rgba[i * 4 + 3].clamp(0.0, 1.0);
                alphas[i] = a;
                let (r, g, b) = if a > 1e-6 {
                    (rgba[i * 4] / a, rgba[i * 4 + 1] / a, rgba[i * 4 + 2] / a)
                } else {
                    (rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2])
                };
                // Already encoded — feed CMS directly (no linear_to_srgb).
                rgb_enc[i * 3] = r.clamp(0.0, 1.0);
                rgb_enc[i * 3 + 1] = g.clamp(0.0, 1.0);
                rgb_enc[i * 3 + 2] = b.clamp(0.0, 1.0);
            }
            let mut out_enc = vec![0f32; pixels * 3];
            xform.apply_srgb_f32(&rgb_enc, &mut out_enc)?;
            for i in 0..pixels {
                dst_rgba8[i * 4] = channel_to_u8(out_enc[i * 3]);
                dst_rgba8[i * 4 + 1] = channel_to_u8(out_enc[i * 3 + 1]);
                dst_rgba8[i * 4 + 2] = channel_to_u8(out_enc[i * 3 + 2]);
                dst_rgba8[i * 4 + 3] = channel_to_u8(alphas[i]);
            }
            Ok(())
        }
    }
}

/// Convenience: RGB (no alpha) already in encoded [0,1] → sRGB8, optionally soft-proofed.
pub fn encode_preview_rgb(
    rgb: &[f32],
    dst_srgb8: &mut [u8],
    proof: Option<&SoftProofTransform>,
) -> Result<(), crate::soft_proof::SoftProofError> {
    if rgb.len() % 3 != 0 || dst_srgb8.len() != rgb.len() {
        return Err(crate::soft_proof::SoftProofError::BufferLength);
    }
    let pixels = rgb.len() / 3;
    match proof {
        None => {
            for i in 0..pixels {
                dst_srgb8[i * 3] = channel_to_u8(rgb[i * 3]);
                dst_srgb8[i * 3 + 1] = channel_to_u8(rgb[i * 3 + 1]);
                dst_srgb8[i * 3 + 2] = channel_to_u8(rgb[i * 3 + 2]);
            }
            Ok(())
        }
        Some(xform) => {
            let mut enc = vec![0f32; rgb.len()];
            for (i, &v) in rgb.iter().enumerate() {
                enc[i] = v.clamp(0.0, 1.0);
            }
            let mut out = vec![0f32; rgb.len()];
            xform.apply_srgb_f32(&enc, &mut out)?;
            for (i, v) in out.iter().enumerate() {
                dst_srgb8[i] = channel_to_u8(*v);
            }
            Ok(())
        }
    }
}

/// ΔE2000 between two sRGB8 triples (encoded). Used by golden / LUT budgets.
pub fn delta_e2000_srgb8(a: [u8; 3], b: [u8; 3]) -> f64 {
    let lab_a = srgb8_to_lab(a);
    let lab_b = srgb8_to_lab(b);
    delta_e2000(lab_a, lab_b)
}

fn srgb8_to_lab(rgb: [u8; 3]) -> [f64; 3] {
    let r = srgb_to_linear(rgb[0]) as f64;
    let g = srgb_to_linear(rgb[1]) as f64;
    let b = srgb_to_linear(rgb[2]) as f64;
    let x = r * 0.4124564 + g * 0.3575761 + b * 0.1804375;
    let y = r * 0.2126729 + g * 0.7151522 + b * 0.0721750;
    let z = r * 0.0193339 + g * 0.1191920 + b * 0.9503041;
    xyz_d65_to_lab(x, y, z)
}

fn xyz_d65_to_lab(x: f64, y: f64, z: f64) -> [f64; 3] {
    const XN: f64 = 0.95047;
    const YN: f64 = 1.0;
    const ZN: f64 = 1.08883;
    fn f(t: f64) -> f64 {
        if t > 0.008856 {
            t.cbrt()
        } else {
            7.787 * t + 16.0 / 116.0
        }
    }
    let fx = f(x / XN);
    let fy = f(y / YN);
    let fz = f(z / ZN);
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 (Sharma et al.).
pub fn delta_e2000(lab1: [f64; 3], lab2: [f64; 3]) -> f64 {
    let (l1, a1, b1) = (lab1[0], lab1[1], lab1[2]);
    let (l2, a2, b2) = (lab2[0], lab2[1], lab2[2]);
    let k_l = 1.0;
    let k_c = 1.0;
    let k_h = 1.0;

    let c1 = (a1 * a1 + b1 * b1).sqrt();
    let c2 = (a2 * a2 + b2 * b2).sqrt();
    let c_bar = (c1 + c2) / 2.0;
    let c_bar7 = c_bar.powi(7);
    let g = 0.5 * (1.0 - (c_bar7 / (c_bar7 + 25.0_f64.powi(7))).sqrt());
    let a1p = (1.0 + g) * a1;
    let a2p = (1.0 + g) * a2;
    let c1p = (a1p * a1p + b1 * b1).sqrt();
    let c2p = (a2p * a2p + b2 * b2).sqrt();

    let h1p = if c1p == 0.0 {
        0.0
    } else {
        b1.atan2(a1p).to_degrees().rem_euclid(360.0)
    };
    let h2p = if c2p == 0.0 {
        0.0
    } else {
        b2.atan2(a2p).to_degrees().rem_euclid(360.0)
    };

    let dl = l2 - l1;
    let dc = c2p - c1p;
    let dh = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dh_ = 2.0 * (c1p * c2p).sqrt() * (dh.to_radians() / 2.0).sin();

    let l_bar = (l1 + l2) / 2.0;
    let c_bar_p = (c1p + c2p) / 2.0;
    let h_bar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };

    let t = 1.0
        - 0.17 * ((h_bar - 30.0).to_radians()).cos()
        + 0.24 * ((2.0 * h_bar).to_radians()).cos()
        + 0.32 * ((3.0 * h_bar + 6.0).to_radians()).cos()
        - 0.20 * ((4.0 * h_bar - 63.0).to_radians()).cos();
    let sl = 1.0 + (0.015 * (l_bar - 50.0).powi(2)) / (20.0 + (l_bar - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * c_bar_p;
    let sh = 1.0 + 0.015 * c_bar_p * t;
    let c_bar_p7 = c_bar_p.powi(7);
    let rt = -2.0
        * (c_bar_p7 / (c_bar_p7 + 25.0_f64.powi(7))).sqrt()
        * (60.0 * (-((h_bar - 275.0) / 25.0).powi(2)).exp())
            .to_radians()
            .sin();

    ((dl / (k_l * sl)).powi(2)
        + (dc / (k_c * sc)).powi(2)
        + (dh_ / (k_h * sh)).powi(2)
        + rt * (dc / (k_c * sc)) * (dh_ / (k_h * sh)))
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::linear_to_srgb;

    #[test]
    fn proof_off_is_passthrough_u8() {
        // Buffer is already encoded (u8/255) — must NOT apply sRGB transfer again.
        let rgba = [0.5f32, 0.25, 0.125, 1.0, 0.0, 0.0, 0.0, 1.0];
        let mut out = [0u8; 8];
        encode_preview_rgba(&rgba, &mut out, None).unwrap();
        assert_eq!(out[0], channel_to_u8(0.5));
        assert_eq!(out[1], channel_to_u8(0.25));
        assert_eq!(out[2], channel_to_u8(0.125));
        assert_eq!(out[3], 255);
        // Must differ from double-encoding via linear_to_srgb.
        assert_ne!(out[0], linear_to_srgb(0.5));
    }

    #[test]
    fn lut_matches_direct_transfer() {
        let lut = SrgbEncodeLut::new();
        for i in 0..=255 {
            let x = i as f32 / 255.0;
            let a = lut.encode_u8(x);
            let b = linear_to_srgb(x);
            assert!((a as i16 - b as i16).abs() <= 1, "x={x} lut={a} direct={b}");
        }
    }
}
