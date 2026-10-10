//! Soft-proof via pure-Rust ICC (`moxcms`).
//!
//! Display-only pipeline on Composite tiles. Working buffers store RGB as
//! `u8/255` (display-referred / sRGB-encoded), not optical linear — so the CMS
//! is fed those values directly (no extra sRGB transfer before proof).
//!
//! Black-point compensation for Relative intent is **app-owned** (XYZ-D50 scale
//! from measured proof black) until/unless moxcms exposes a native BPC flag.
//!
//! Optional CPU 3D LUT accelerates tile apply when within the ΔE2000 budget.

use moxcms::{
    ColorProfile, DataColorSpace, Layout, ProfileClass, ProfileText, RenderingIntent,
    TransformOptions,
};
use std::sync::Arc;
use thiserror::Error;

use crate::preview_encode::{
    delta_e2000_srgb8, linear_to_srgb_f32, srgb_f32_to_linear, SrgbEncodeLut,
};

/// Builtin profile id for PSO Coated v3 / FOGRA51.
pub const BUILTIN_FOGRA51_ID: &str = "builtin:fogra51";
/// Builtin profile id for PSO Uncoated v3 / FOGRA52.
pub const BUILTIN_FOGRA52_ID: &str = "builtin:fogra52";

/// Embedded PSO Coated v3 (FOGRA51) ICC bytes.
pub static BUILTIN_FOGRA51_ICC: &[u8] =
    include_bytes!("../../../src-tauri/cmyk/pso-coated_v3/PSOcoated_v3.icc");

/// Preferred 3D LUT grid. If the ΔE budget fails, build tries [`FALLBACK_PROOF_LUT_SIZE`].
pub const DEFAULT_PROOF_LUT_SIZE: usize = 33;
/// Fallback grid when 33³ fails the probe budget; if this also fails → exact f32 path.
pub const FALLBACK_PROOF_LUT_SIZE: usize = 49;

/// Max ΔE2000 vs exact f32 transform to accept a baked LUT (near-black + gamut probes).
pub const LUT_MAX_DELTA_E2000: f64 = 1.0;

/// LUT sizes tried in order when constructing a transform (exact path if all fail).
pub const LUT_SIZE_CANDIDATES: &[usize] = &[DEFAULT_PROOF_LUT_SIZE, FALLBACK_PROOF_LUT_SIZE];

/// Probe set for LUT ΔE budget: coarse grid + near-black + primaries / secondaries
/// (CMYK gamut corners after proof tend to stress tetrahedral cells).
const LUT_PROBE_POINTS: &[[f32; 3]] = &[
    // Coarse lattice
    [0.0, 0.0, 0.0],
    [0.25, 0.25, 0.25],
    [0.5, 0.5, 0.5],
    [0.75, 0.75, 0.75],
    [1.0, 1.0, 1.0],
    // Near-black (worst tetrahedral cells)
    [0.01, 0.01, 0.01],
    [0.02, 0.0, 0.0],
    [0.0, 0.02, 0.0],
    [0.0, 0.0, 0.02],
    [0.04, 0.03, 0.02],
    // Primaries / secondaries (gamut boundary)
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 1.0, 0.0],
    [0.0, 1.0, 1.0],
    [1.0, 0.0, 1.0],
    // Mid-gamut skin / memory colors
    [0.87, 0.64, 0.53],
    [0.2, 0.4, 0.8],
    [0.9, 0.2, 0.1],
];

/// Gray-axis / K-ramp: LUT luma must be non-decreasing as input gray rises
/// (no “steps” that reverse tone). Compares adjacent samples on t∈[0,1].
fn lut_gray_axis_monotone(lut: &SoftProofLut3D, exact: &SoftProofTransform) -> bool {
    let mut prev_y = -1.0f32;
    let n = 32;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let approx = lut.lookup(t, t, t);
        let y = 0.2126 * approx[0] + 0.7152 * approx[1] + 0.0722 * approx[2];
        if i > 0 && y + 0.02 < prev_y {
            // Allow tiny noise; reject clear reversals.
            return false;
        }
        // Also stay close to exact on the gray ramp (banding / step guard).
        let mut e = [0f32; 3];
        if exact.apply_cms_exact(&[t, t, t], &mut e).is_err() {
            return false;
        }
        let ye = 0.2126 * e[0] + 0.7152 * e[1] + 0.0722 * e[2];
        if (y - ye).abs() > 0.04 {
            return false;
        }
        prev_y = y;
    }
    true
}

/// Per-document soft-proof settings (persisted in `.dyproj` `document.json`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SoftProofConfig {
    #[serde(default)]
    pub enabled: bool,
    /// `builtin:fogra51` / `builtin:fogra52` / `import:{sha256 hex}`.
    #[serde(default = "default_profile_id")]
    pub profile_id: String,
    #[serde(default)]
    pub intent: SoftProofIntent,
    /// Black-point compensation (Relative only). App-owned until moxcms exposes BPC.
    #[serde(default = "default_true")]
    pub bpc: bool,
    /// Sanitized display name for missing-profile UX (capped / escaped on write).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_display_name: Option<String>,
}

fn default_profile_id() -> String {
    BUILTIN_FOGRA51_ID.to_string()
}

fn default_true() -> bool {
    true
}

impl Default for SoftProofConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            profile_id: default_profile_id(),
            intent: SoftProofIntent::Relative,
            bpc: true,
            profile_display_name: None,
        }
    }
}

impl SoftProofConfig {
    /// True when on-disk writers must declare the `soft-proof` format feature.
    pub fn requires_format_feature(&self) -> bool {
        let defaults = Self::default();
        self.enabled
            || self.profile_id != defaults.profile_id
            || self.intent != defaults.intent
            || self.bpc != defaults.bpc
            || self.profile_display_name.is_some()
    }

    /// Cap and strip control characters from a profile display name for persistence.
    pub fn sanitize_display_name(name: &str) -> String {
        let mut out = String::with_capacity(name.len().min(120));
        for ch in name.chars().take(120) {
            if ch.is_control() {
                continue;
            }
            out.push(ch);
        }
        if out.is_empty() {
            "CMYK profile".into()
        } else {
            out
        }
    }
}

/// Rendering intents exposed in the Soft proof UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SoftProofIntent {
    Relative,
    Perceptual,
    Absolute,
}

impl SoftProofIntent {
    pub fn to_moxcms(self) -> RenderingIntent {
        match self {
            SoftProofIntent::Relative => RenderingIntent::RelativeColorimetric,
            SoftProofIntent::Perceptual => RenderingIntent::Perceptual,
            SoftProofIntent::Absolute => RenderingIntent::AbsoluteColorimetric,
        }
    }
}

impl Default for SoftProofIntent {
    fn default() -> Self {
        SoftProofIntent::Relative
    }
}

#[derive(Debug, Error)]
pub enum SoftProofError {
    #[error("failed to parse ICC profile: {0}")]
    Parse(String),
    #[error("profile is not CMYK (got {0:?})")]
    NotCmyk(DataColorSpace),
    #[error("profile class not allowed for soft-proof (got {0:?}; need Output/Input)")]
    BadClass(ProfileClass),
    #[error("failed to build soft-proof transform: {0}")]
    Transform(String),
    #[error("transform buffer length mismatch")]
    BufferLength,
    #[error("soft-proof CMS panicked: {0}")]
    Panic(String),
}

/// Metadata extracted when loading a candidate proof profile.
#[derive(Debug, Clone)]
pub struct ProofProfileInfo {
    pub description: String,
    pub color_space: DataColorSpace,
    pub class: ProfileClass,
    pub has_perceptual: bool,
    pub has_relative: bool,
    pub has_absolute: bool,
}

/// Validated CMYK printer/scanner profile used as the soft-proof target.
#[derive(Debug)]
pub struct ProofProfile {
    pub info: ProofProfileInfo,
    profile: ColorProfile,
}

impl ProofProfile {
    pub fn from_icc_bytes(bytes: &[u8]) -> Result<Self, SoftProofError> {
        // Structural reject before moxcms (timeout on import does not kill the worker).
        crate::icc_precheck::precheck_icc_bytes(bytes)?;

        // Requires workspace `[profile.release] panic = "unwind"` — abort makes this a no-op.
        let profile = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ColorProfile::new_from_slice(bytes)
        }))
        .map_err(|_| SoftProofError::Panic("ICC parse".into()))?
        .map_err(|e| SoftProofError::Parse(e.to_string()))?;

        if profile.color_space != DataColorSpace::Cmyk {
            return Err(SoftProofError::NotCmyk(profile.color_space));
        }

        match profile.profile_class {
            ProfileClass::OutputDevice | ProfileClass::InputDevice => {}
            other => return Err(SoftProofError::BadClass(other)),
        }

        let description = SoftProofConfig::sanitize_display_name(&profile_description(&profile));
        let info = ProofProfileInfo {
            description,
            color_space: profile.color_space,
            class: profile.profile_class,
            has_perceptual: probe_intent(&profile, RenderingIntent::Perceptual),
            has_relative: probe_intent(&profile, RenderingIntent::RelativeColorimetric),
            has_absolute: probe_intent(&profile, RenderingIntent::AbsoluteColorimetric),
        };

        Ok(Self { info, profile })
    }

    pub fn black_point_xyz(&self) -> Option<[f32; 3]> {
        self.profile
            .black_point
            .map(|bp| [bp.x as f32, bp.y as f32, bp.z as f32])
    }
}

fn profile_description(profile: &ColorProfile) -> String {
    match &profile.description {
        Some(ProfileText::PlainString(s)) if !s.is_empty() => s.clone(),
        Some(ProfileText::Description(d)) if !d.ascii_string.is_empty() => d.ascii_string.clone(),
        Some(ProfileText::Description(d)) if !d.unicode_string.is_empty() => {
            d.unicode_string.clone()
        }
        Some(ProfileText::Localizable(items)) => items
            .iter()
            .find(|s| !s.value.is_empty())
            .map(|s| s.value.clone())
            .unwrap_or_else(|| "CMYK profile".into()),
        _ => "CMYK profile".into(),
    }
}

fn probe_intent(cmyk: &ColorProfile, intent: RenderingIntent) -> bool {
    let srgb = ColorProfile::new_srgb();
    let opts = TransformOptions {
        rendering_intent: intent,
        ..Default::default()
    };
    srgb.create_transform_f32(Layout::Rgb, cmyk, Layout::Rgba, opts)
        .and_then(|_| cmyk.create_transform_f32(Layout::Rgba, &srgb, Layout::Rgb, opts))
        .is_ok()
}

/// D50 white point (ICC PCS).
const D50: [f32; 3] = [0.9642, 1.0, 0.8249];

/// XYZ-D50 black-point compensation (moxcms / ICC style scale).
#[inline]
fn apply_bpc_xyz(xyz: &mut [f32; 3], src_bp: [f32; 3], dst_bp: [f32; 3]) {
    for i in 0..3 {
        let denom = D50[i] - src_bp[i];
        if denom.abs() < 1e-10 {
            continue;
        }
        let a = (D50[i] - dst_bp[i]) / denom;
        let b = dst_bp[i] - a * src_bp[i];
        xyz[i] = a * xyz[i] + b;
    }
}

fn srgb_encoded_to_xyz_d50(rgb: [f32; 3]) -> [f32; 3] {
    let r = srgb_f32_to_linear(rgb[0]);
    let g = srgb_f32_to_linear(rgb[1]);
    let b = srgb_f32_to_linear(rgb[2]);
    let x65 = r * 0.4124564 + g * 0.3575761 + b * 0.1804375;
    let y65 = r * 0.2126729 + g * 0.7151522 + b * 0.0721750;
    let z65 = r * 0.0193339 + g * 0.1191920 + b * 0.9503041;
    [
        x65 * 1.0478112 + y65 * 0.0228866 + z65 * -0.0501270,
        x65 * 0.0295424 + y65 * 0.9904844 + z65 * -0.0170491,
        x65 * -0.0092345 + y65 * 0.0150436 + z65 * 0.7521316,
    ]
}

fn xyz_d50_to_srgb_encoded(xyz: [f32; 3]) -> [f32; 3] {
    let x65 = xyz[0] * 0.9555766 + xyz[1] * -0.0230393 + xyz[2] * 0.0631636;
    let y65 = xyz[0] * -0.0282895 + xyz[1] * 1.0099416 + xyz[2] * 0.0210077;
    let z65 = xyz[0] * 0.0122982 + xyz[1] * -0.0204830 + xyz[2] * 1.3299098;
    let r = x65 * 3.2404542 + y65 * -1.5371385 + z65 * -0.4985314;
    let g = x65 * -0.9692660 + y65 * 1.8760108 + z65 * 0.0415560;
    let b = x65 * 0.0556434 + y65 * -0.2040259 + z65 * 1.0572252;
    [
        linear_to_srgb_f32(r),
        linear_to_srgb_f32(g),
        linear_to_srgb_f32(b),
    ]
}

/// CPU tetrahedral 3D LUT over encoded sRGB [0,1]³.
#[derive(Clone)]
pub struct SoftProofLut3D {
    pub size: usize,
    /// Interleaved RGB f32, length `size³ * 3`.
    data: Vec<f32>,
}

impl SoftProofLut3D {
    /// Flat RGB f32 volume for a future GPU 3D texture upload (R→G→B major).
    /// Color is still computed in Rust; a shader would only sample this buffer.
    pub fn as_rgb_f32_volume(&self) -> &[f32] {
        &self.data
    }

    pub fn lookup(&self, r: f32, g: f32, b: f32) -> [f32; 3] {
        let n = self.size;
        let max = (n - 1) as f32;
        let rf = r.clamp(0.0, 1.0) * max;
        let gf = g.clamp(0.0, 1.0) * max;
        let bf = b.clamp(0.0, 1.0) * max;
        let r0 = rf.floor() as usize;
        let g0 = gf.floor() as usize;
        let b0 = bf.floor() as usize;
        let r1 = (r0 + 1).min(n - 1);
        let g1 = (g0 + 1).min(n - 1);
        let b1 = (b0 + 1).min(n - 1);
        let tr = rf - r0 as f32;
        let tg = gf - g0 as f32;
        let tb = bf - b0 as f32;

        // Tetrahedral interpolation (standard CMS form).
        let c000 = self.sample(r0, g0, b0);
        let c100 = self.sample(r1, g0, b0);
        let c010 = self.sample(r0, g1, b0);
        let c001 = self.sample(r0, g0, b1);
        let c110 = self.sample(r1, g1, b0);
        let c101 = self.sample(r1, g0, b1);
        let c011 = self.sample(r0, g1, b1);
        let c111 = self.sample(r1, g1, b1);

        let out = if tr > tg {
            if tg > tb {
                // r > g > b
                [
                    c000[0]
                        + tr * (c100[0] - c000[0])
                        + tg * (c110[0] - c100[0])
                        + tb * (c111[0] - c110[0]),
                    c000[1]
                        + tr * (c100[1] - c000[1])
                        + tg * (c110[1] - c100[1])
                        + tb * (c111[1] - c110[1]),
                    c000[2]
                        + tr * (c100[2] - c000[2])
                        + tg * (c110[2] - c100[2])
                        + tb * (c111[2] - c110[2]),
                ]
            } else if tr > tb {
                // r > b > g
                [
                    c000[0]
                        + tr * (c100[0] - c000[0])
                        + tb * (c101[0] - c100[0])
                        + tg * (c111[0] - c101[0]),
                    c000[1]
                        + tr * (c100[1] - c000[1])
                        + tb * (c101[1] - c100[1])
                        + tg * (c111[1] - c101[1]),
                    c000[2]
                        + tr * (c100[2] - c000[2])
                        + tb * (c101[2] - c100[2])
                        + tg * (c111[2] - c101[2]),
                ]
            } else {
                // b > r > g
                [
                    c000[0]
                        + tb * (c001[0] - c000[0])
                        + tr * (c101[0] - c001[0])
                        + tg * (c111[0] - c101[0]),
                    c000[1]
                        + tb * (c001[1] - c000[1])
                        + tr * (c101[1] - c001[1])
                        + tg * (c111[1] - c101[1]),
                    c000[2]
                        + tb * (c001[2] - c000[2])
                        + tr * (c101[2] - c001[2])
                        + tg * (c111[2] - c101[2]),
                ]
            }
        } else if tb > tg {
            // b > g > r
            [
                c000[0]
                    + tb * (c001[0] - c000[0])
                    + tg * (c011[0] - c001[0])
                    + tr * (c111[0] - c011[0]),
                c000[1]
                    + tb * (c001[1] - c000[1])
                    + tg * (c011[1] - c001[1])
                    + tr * (c111[1] - c011[1]),
                c000[2]
                    + tb * (c001[2] - c000[2])
                    + tg * (c011[2] - c001[2])
                    + tr * (c111[2] - c011[2]),
            ]
        } else if tb > tr {
            // g > b > r
            [
                c000[0]
                    + tg * (c010[0] - c000[0])
                    + tb * (c011[0] - c010[0])
                    + tr * (c111[0] - c011[0]),
                c000[1]
                    + tg * (c010[1] - c000[1])
                    + tb * (c011[1] - c010[1])
                    + tr * (c111[1] - c011[1]),
                c000[2]
                    + tg * (c010[2] - c000[2])
                    + tb * (c011[2] - c010[2])
                    + tr * (c111[2] - c011[2]),
            ]
        } else {
            // g > r > b
            [
                c000[0]
                    + tg * (c010[0] - c000[0])
                    + tr * (c110[0] - c010[0])
                    + tb * (c111[0] - c110[0]),
                c000[1]
                    + tg * (c010[1] - c000[1])
                    + tr * (c110[1] - c010[1])
                    + tb * (c111[1] - c110[1]),
                c000[2]
                    + tg * (c010[2] - c000[2])
                    + tr * (c110[2] - c010[2])
                    + tb * (c111[2] - c110[2]),
            ]
        };
        out
    }

    #[inline]
    fn sample(&self, r: usize, g: usize, b: usize) -> [f32; 3] {
        let n = self.size;
        let i = ((r * n + g) * n + b) * 3;
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }
}

/// Reusable soft-proofing transform (sRGB encoded f32 ↔ CMYK ↔ sRGB display).
pub struct SoftProofTransform {
    to_cmyk: Arc<moxcms::TransformF32Executor>,
    to_display: Arc<moxcms::TransformF32Executor>,
    pub intent: SoftProofIntent,
    /// True when BPC is active (Relative + requested).
    pub bpc_active: bool,
    /// Source black point in XYZ D50 used for BPC (display black = 0).
    src_bp: [f32; 3],
    /// Optional CPU 3D LUT (includes BPC when active).
    lut: Option<SoftProofLut3D>,
}

impl SoftProofTransform {
    pub fn new(
        proof: &ProofProfile,
        intent: SoftProofIntent,
        bpc_requested: bool,
    ) -> Result<Self, SoftProofError> {
        Self::build(proof, intent, bpc_requested, Some(LUT_SIZE_CANDIDATES))
    }

    /// Build without attempting a 3D LUT (exact f32 path only).
    pub fn new_exact(
        proof: &ProofProfile,
        intent: SoftProofIntent,
        bpc_requested: bool,
    ) -> Result<Self, SoftProofError> {
        Self::build(proof, intent, bpc_requested, None)
    }

    pub fn with_lut_size(
        proof: &ProofProfile,
        intent: SoftProofIntent,
        bpc_requested: bool,
        lut_size: usize,
    ) -> Result<Self, SoftProofError> {
        let size = lut_size.max(2);
        Self::build(proof, intent, bpc_requested, Some(&[size]))
    }

    fn build(
        proof: &ProofProfile,
        intent: SoftProofIntent,
        bpc_requested: bool,
        lut_sizes: Option<&[usize]>,
    ) -> Result<Self, SoftProofError> {
        if matches!(intent, SoftProofIntent::Perceptual) && !proof.info.has_perceptual {
            return Err(SoftProofError::Transform(
                "profile has no usable Perceptual transform".into(),
            ));
        }
        if matches!(intent, SoftProofIntent::Relative) && !proof.info.has_relative {
            return Err(SoftProofError::Transform(
                "profile has no usable Relative Colorimetric transform".into(),
            ));
        }
        if matches!(intent, SoftProofIntent::Absolute) && !proof.info.has_absolute {
            return Err(SoftProofError::Transform(
                "profile has no usable Absolute Colorimetric transform".into(),
            ));
        }

        let srgb = ColorProfile::new_srgb();
        let opts = TransformOptions {
            rendering_intent: intent.to_moxcms(),
            ..Default::default()
        };

        // Absolute: Absolute forward + Relative return (classic proofing).
        let (forward_opts, return_opts) = match intent {
            SoftProofIntent::Absolute => (
                TransformOptions {
                    rendering_intent: RenderingIntent::AbsoluteColorimetric,
                    ..Default::default()
                },
                TransformOptions {
                    rendering_intent: RenderingIntent::RelativeColorimetric,
                    ..Default::default()
                },
            ),
            _ => (opts, opts),
        };

        let to_cmyk = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            srgb.create_transform_f32(Layout::Rgb, &proof.profile, Layout::Rgba, forward_opts)
        }))
        .map_err(|_| SoftProofError::Panic("create_transform_f32 forward".into()))?
        .map_err(|e| SoftProofError::Transform(e.to_string()))?;

        let to_display = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            proof
                .profile
                .create_transform_f32(Layout::Rgba, &srgb, Layout::Rgb, return_opts)
        }))
        .map_err(|_| SoftProofError::Panic("create_transform_f32 return".into()))?
        .map_err(|e| SoftProofError::Transform(e.to_string()))?;

        // BPC only for Relative (Absolute ignores BPC by design).
        let bpc_active = bpc_requested && matches!(intent, SoftProofIntent::Relative);
        let mut xform = Self {
            to_cmyk,
            to_display,
            intent,
            bpc_active,
            src_bp: [0.0; 3],
            lut: None,
        };

        if bpc_active {
            xform.src_bp = xform.measure_black_point(proof);
        }

        if let Some(sizes) = lut_sizes {
            for &size in sizes {
                if let Some(lut) = xform.try_build_lut(size) {
                    log::info!(
                        target: "soft_proof",
                        "3D LUT accepted size={size}³ (exact path unused)"
                    );
                    xform.lut = Some(lut);
                    break;
                }
                log::info!(
                    target: "soft_proof",
                    "3D LUT rejected size={size}³; trying next candidate or exact f32"
                );
            }
        }

        Ok(xform)
    }

    fn measure_black_point(&self, proof: &ProofProfile) -> [f32; 3] {
        if let Some(bp) = proof.black_point_xyz() {
            if bp[1] > 1e-6 {
                return bp;
            }
        }
        let mut cmyk = [0f32; 4];
        let mut out = [0f32; 3];
        let _ = self.to_cmyk.transform(&[0.0, 0.0, 0.0], &mut cmyk);
        let _ = self.to_display.transform(&cmyk, &mut out);
        srgb_encoded_to_xyz_d50([
            out[0].clamp(0.0, 1.0),
            out[1].clamp(0.0, 1.0),
            out[2].clamp(0.0, 1.0),
        ])
    }

    fn apply_cms_exact(&self, src_enc: &[f32], dst_enc: &mut [f32]) -> Result<(), SoftProofError> {
        if src_enc.len() != dst_enc.len() || src_enc.len() % 3 != 0 {
            return Err(SoftProofError::BufferLength);
        }
        let pixels = src_enc.len() / 3;
        let mut cmyk = vec![0f32; pixels * 4];
        self.to_cmyk
            .transform(src_enc, &mut cmyk)
            .map_err(|e| SoftProofError::Transform(e.to_string()))?;
        self.to_display
            .transform(&cmyk, dst_enc)
            .map_err(|e| SoftProofError::Transform(e.to_string()))?;

        if self.bpc_active {
            let dst_bp = [0.0f32; 3];
            for i in 0..pixels {
                let rgb = [
                    dst_enc[i * 3].clamp(0.0, 1.0),
                    dst_enc[i * 3 + 1].clamp(0.0, 1.0),
                    dst_enc[i * 3 + 2].clamp(0.0, 1.0),
                ];
                let mut xyz = srgb_encoded_to_xyz_d50(rgb);
                apply_bpc_xyz(&mut xyz, self.src_bp, dst_bp);
                let out = xyz_d50_to_srgb_encoded(xyz);
                dst_enc[i * 3] = out[0];
                dst_enc[i * 3 + 1] = out[1];
                dst_enc[i * 3 + 2] = out[2];
            }
        } else {
            for v in dst_enc.iter_mut() {
                *v = v.clamp(0.0, 1.0);
            }
        }
        Ok(())
    }

    fn try_build_lut(&self, size: usize) -> Option<SoftProofLut3D> {
        let n = size;
        let mut data = vec![0f32; n * n * n * 3];
        let mut src = vec![0f32; n * n * n * 3];
        let mut idx = 0;
        for ir in 0..n {
            for ig in 0..n {
                for ib in 0..n {
                    src[idx] = ir as f32 / (n - 1) as f32;
                    src[idx + 1] = ig as f32 / (n - 1) as f32;
                    src[idx + 2] = ib as f32 / (n - 1) as f32;
                    idx += 3;
                }
            }
        }
        if self.apply_cms_exact(&src, &mut data).is_err() {
            return None;
        }

        let lut = SoftProofLut3D {
            size: n,
            data: data.clone(),
        };
        let mut max_de = 0.0f64;
        for p in LUT_PROBE_POINTS {
            let [r, g, b] = *p;
            let mut exact = [0f32; 3];
            if self.apply_cms_exact(&[r, g, b], &mut exact).is_err() {
                return None;
            }
            let approx = lut.lookup(r, g, b);
            let a = [
                (exact[0] * 255.0 + 0.5) as u8,
                (exact[1] * 255.0 + 0.5) as u8,
                (exact[2] * 255.0 + 0.5) as u8,
            ];
            let c = [
                (approx[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
                (approx[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
                (approx[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            ];
            max_de = max_de.max(delta_e2000_srgb8(a, c));
        }
        if max_de > LUT_MAX_DELTA_E2000 {
            log::warn!(
                "soft-proof 3D LUT rejected: size={n} max ΔE2000={max_de:.3} > {LUT_MAX_DELTA_E2000}"
            );
            return None;
        }
        if !lut_gray_axis_monotone(&lut, self) {
            log::warn!("soft-proof 3D LUT rejected: size={n} gray-axis non-monotone");
            return None;
        }
        Some(lut)
    }

    /// Soft-proof display-referred pixels (Composite convention).
    pub fn apply_display_rgb(
        &self,
        src: &[crate::display_rgb::DisplayRgbF32],
        dst: &mut [crate::display_rgb::DisplayRgbF32],
    ) -> Result<(), SoftProofError> {
        if src.len() != dst.len() {
            return Err(SoftProofError::BufferLength);
        }
        let enc = crate::display_rgb::DisplayRgbF32::unpack_to_interleaved(src);
        let mut out = vec![0f32; enc.len()];
        self.apply_srgb_f32(&enc, &mut out)?;
        for (i, chunk) in out.chunks_exact(3).enumerate() {
            dst[i] = crate::display_rgb::DisplayRgbF32::new(chunk[0], chunk[1], chunk[2]);
        }
        Ok(())
    }

    /// Soft-proof packed sRGB-encoded f32 RGB pixels: `src` → `dst` (both [0,1] * 3N).
    pub fn apply_srgb_f32(&self, src: &[f32], dst: &mut [f32]) -> Result<(), SoftProofError> {
        if src.len() != dst.len() || src.len() % 3 != 0 {
            return Err(SoftProofError::BufferLength);
        }
        if let Some(lut) = &self.lut {
            for (i, chunk) in src.chunks_exact(3).enumerate() {
                let out = lut.lookup(chunk[0], chunk[1], chunk[2]);
                dst[i * 3] = out[0];
                dst[i * 3 + 1] = out[1];
                dst[i * 3 + 2] = out[2];
            }
            return Ok(());
        }
        self.apply_cms_exact(src, dst)
    }

    /// GPU path: upload [`SoftProofLut3D::as_rgb_f32_volume`] as a 3D texture.
    /// Gated behind the `gpu-lut` feature so unused volume export does not accumulate.
    #[cfg(feature = "gpu-lut")]
    pub fn gpu_lut_volume(&self) -> Option<(usize, &[f32])> {
        self.lut.as_ref().map(|l| (l.size, l.as_rgb_f32_volume()))
    }

    /// Soft-proof packed sRGB8 RGB pixels: `src` → `dst`.
    pub fn apply_srgb8(&self, src: &[u8], dst: &mut [u8]) -> Result<(), SoftProofError> {
        if src.len() != dst.len() || src.len() % 3 != 0 {
            return Err(SoftProofError::BufferLength);
        }
        let mut enc = vec![0f32; src.len()];
        for (i, &v) in src.iter().enumerate() {
            enc[i] = v as f32 / 255.0;
        }
        let mut out = vec![0f32; src.len()];
        self.apply_srgb_f32(&enc, &mut out)?;
        for (i, v) in out.iter().enumerate() {
            dst[i] = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        Ok(())
    }

    /// Soft-proof **optical linear** RGB f32 → sRGB8 (applies sRGB transfer then CMS).
    ///
    /// Preview tiles should use [`crate::preview_encode::encode_preview_rgba`] instead:
    /// Composite buffers are already encoded (`u8/255`), not optical linear.
    pub fn apply_linear_rgb_f32(
        &self,
        linear_rgb: &[f32],
        dst_srgb8: &mut [u8],
    ) -> Result<(), SoftProofError> {
        if linear_rgb.len() % 3 != 0 || dst_srgb8.len() != linear_rgb.len() {
            return Err(SoftProofError::BufferLength);
        }
        let lut = SrgbEncodeLut::new();
        let mut enc = vec![0f32; linear_rgb.len()];
        for (i, &v) in linear_rgb.iter().enumerate() {
            enc[i] = lut.encode(v);
        }
        let mut out = vec![0f32; linear_rgb.len()];
        self.apply_srgb_f32(&enc, &mut out)?;
        for (i, v) in out.iter().enumerate() {
            dst_srgb8[i] = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        Ok(())
    }

    /// Soft-proof already-encoded sRGB [0,1] RGB → sRGB8 (Composite buffer convention).
    pub fn apply_encoded_rgb_f32(
        &self,
        encoded_rgb: &[f32],
        dst_srgb8: &mut [u8],
    ) -> Result<(), SoftProofError> {
        if encoded_rgb.len() % 3 != 0 || dst_srgb8.len() != encoded_rgb.len() {
            return Err(SoftProofError::BufferLength);
        }
        let mut enc = vec![0f32; encoded_rgb.len()];
        for (i, &v) in encoded_rgb.iter().enumerate() {
            enc[i] = v.clamp(0.0, 1.0);
        }
        let mut out = vec![0f32; encoded_rgb.len()];
        self.apply_srgb_f32(&enc, &mut out)?;
        for (i, v) in out.iter().enumerate() {
            dst_srgb8[i] = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
        Ok(())
    }

    /// Soft-proof interleaved linear RGBA f32 → RGBA8 (alpha passed through).
    pub fn apply_linear_rgba_f32_to_rgba8(
        &self,
        linear_rgba: &[f32],
        dst_rgba8: &mut [u8],
    ) -> Result<(), SoftProofError> {
        crate::preview_encode::encode_preview_rgba(linear_rgba, dst_rgba8, Some(self))
    }

    pub fn uses_lut(&self) -> bool {
        self.lut.is_some()
    }

    /// Exact f32 CMS without LUT (for golden / LUT validation).
    pub fn apply_srgb_f32_exact(&self, src: &[f32], dst: &mut [f32]) -> Result<(), SoftProofError> {
        self.apply_cms_exact(src, dst)
    }
}

/// Bump when preview encode semantics change so process-wide RGBA8 caches miss.
const PREVIEW_ENCODE_CACHE_EPOCH: u64 = 2;

/// Config hash for preview RGBA8 cache keys.
pub fn soft_proof_config_hash(cfg: &SoftProofConfig) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    PREVIEW_ENCODE_CACHE_EPOCH.hash(&mut h);
    cfg.enabled.hash(&mut h);
    cfg.profile_id.hash(&mut h);
    cfg.intent.hash(&mut h);
    cfg.bpc.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview_encode::encode_preview_rgb;
    use std::path::PathBuf;
    use std::time::Instant;

    fn pso_coated_v3_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../src-tauri/cmyk/pso-coated_v3/PSOcoated_v3.icc")
    }

    #[test]
    fn load_pso_coated_v3() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read PSOcoated_v3.icc");
        let proof = ProofProfile::from_icc_bytes(&bytes).expect("parse CMYK profile");
        assert_eq!(proof.info.color_space, DataColorSpace::Cmyk);
        assert!(
            proof.info.has_relative,
            "expected Relative Colorimetric table"
        );
    }

    #[test]
    fn reject_srgb_as_proof_target() {
        let srgb = ColorProfile::new_srgb();
        let bytes = srgb
            .encode()
            .expect("encode sRGB profile for negative test");
        let err = ProofProfile::from_icc_bytes(&bytes).unwrap_err();
        assert!(
            matches!(err, SoftProofError::NotCmyk(_)),
            "expected NotCmyk, got {err:?}"
        );
    }

    #[test]
    fn soft_proof_primaries_relative() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read ICC");
        let proof = ProofProfile::from_icc_bytes(&bytes).unwrap();
        let xform = SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, true).unwrap();

        let linear: Vec<f32> = [
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.5, 0.5, 0.5],
            [1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0],
        ]
        .into_iter()
        .flatten()
        .collect();
        let mut out = vec![0u8; linear.len()];
        xform.apply_linear_rgb_f32(&linear, &mut out).unwrap();

        let white = &out[12..15];
        assert!(
            white[0] > 230 && white[1] > 230 && white[2] > 230,
            "white={white:?}"
        );
        let black = &out[15..18];
        assert!(
            black[0] < 40 && black[1] < 40 && black[2] < 40,
            "black={black:?}"
        );
    }

    #[test]
    fn unified_encode_proof_off_is_passthrough() {
        // Composite stores u8/255 — proof-off must not apply another transfer.
        let encoded: Vec<f32> = (0..64)
            .flat_map(|i| {
                let t = i as f32 / 63.0;
                [t, t * 0.5, 1.0 - t]
            })
            .collect();
        let mut off = vec![0u8; encoded.len()];
        encode_preview_rgb(&encoded, &mut off, None).unwrap();
        for i in 0..64 {
            let t = i as f32 / 63.0;
            assert_eq!(off[i * 3], (t.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
            assert_eq!(
                off[i * 3 + 1],
                ((t * 0.5).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
            );
            assert_eq!(
                off[i * 3 + 2],
                ((1.0 - t).clamp(0.0, 1.0) * 255.0 + 0.5) as u8
            );
        }
    }

    #[test]
    fn proof_on_off_differ_for_saturated_primary() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read ICC");
        let proof = ProofProfile::from_icc_bytes(&bytes).unwrap();
        let xform = SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, true).unwrap();
        // Saturated blue as already-encoded sRGB (Composite convention).
        let rgb = [0.0f32, 0.0, 1.0];
        let mut off = [0u8; 3];
        let mut on = [0u8; 3];
        encode_preview_rgb(&rgb, &mut off, None).unwrap();
        encode_preview_rgb(&rgb, &mut on, Some(&xform)).unwrap();
        assert_eq!(off, [0, 0, 255]);
        assert!(
            on != off,
            "soft-proof must change saturated blue: on={on:?} off={off:?}"
        );
    }

    #[test]
    fn bpc_affects_shadows_relative() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read ICC");
        let proof = ProofProfile::from_icc_bytes(&bytes).unwrap();
        let with = SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, true).unwrap();
        let without =
            SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, false).unwrap();
        assert!(with.bpc_active);
        assert!(!without.bpc_active);

        let linear = [0.02f32, 0.02, 0.02];
        let mut a = [0u8; 3];
        let mut b = [0u8; 3];
        with.apply_linear_rgb_f32(&linear, &mut a).unwrap();
        without.apply_linear_rgb_f32(&linear, &mut b).unwrap();
        // BPC should pull deep shadows toward display black (lower or equal luma).
        let luma = |c: [u8; 3]| c[0] as i32 + c[1] as i32 + c[2] as i32;
        assert!(
            luma(a) <= luma(b) + 3,
            "BPC shadows should be darker/equal: with={a:?} without={b:?}"
        );
    }

    #[test]
    fn absolute_ignores_bpc_flag() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read ICC");
        let proof = ProofProfile::from_icc_bytes(&bytes).unwrap();
        let x = SoftProofTransform::new_exact(&proof, SoftProofIntent::Absolute, true).unwrap();
        assert!(!x.bpc_active);
    }

    #[test]
    fn gradient_no_extreme_banding_f32() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read ICC");
        let proof = ProofProfile::from_icc_bytes(&bytes).unwrap();
        let xform = SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, true).unwrap();
        let n = 256;
        let mut linear = vec![0f32; n * 3];
        for i in 0..n {
            let t = i as f32 / (n - 1) as f32;
            linear[i * 3] = t;
            linear[i * 3 + 1] = t;
            linear[i * 3 + 2] = t;
        }
        let mut out = vec![0u8; n * 3];
        xform.apply_linear_rgb_f32(&linear, &mut out).unwrap();
        let mut max_step = 0i16;
        for i in 1..n {
            let d = (out[i * 3] as i16 - out[(i - 1) * 3] as i16).abs();
            max_step = max_step.max(d);
        }
        // Smooth gray ramp should not jump by more than ~10 code values between neighbors.
        assert!(max_step <= 10, "banding: max neighbor step={max_step}");
    }

    #[test]
    fn lut_within_delta_e_budget() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read ICC");
        let proof = ProofProfile::from_icc_bytes(&bytes).unwrap();
        let xform =
            SoftProofTransform::with_lut_size(&proof, SoftProofIntent::Relative, true, 33).unwrap();
        assert!(xform.uses_lut(), "33³ LUT should pass ΔE budget");
    }

    #[test]
    fn soft_proof_tile_timing_256() {
        let bytes = std::fs::read(pso_coated_v3_path()).expect("read ICC");
        let proof = ProofProfile::from_icc_bytes(&bytes).unwrap();
        let build_start = Instant::now();
        let xform = SoftProofTransform::new(&proof, SoftProofIntent::Relative, true).unwrap();
        let build_ms = build_start.elapsed().as_secs_f64() * 1000.0;

        let n = 256 * 256;
        let mut linear = vec![0.0f32; n * 3];
        for y in 0..256 {
            for x in 0..256 {
                let i = (y * 256 + x) * 3;
                linear[i] = x as f32 / 255.0;
                linear[i + 1] = y as f32 / 255.0;
                linear[i + 2] = 0.5;
            }
        }
        let mut out = vec![0u8; n * 3];
        let t0 = Instant::now();
        xform.apply_linear_rgb_f32(&linear, &mut out).unwrap();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        eprintln!(
            "soft-proof timing: transform_build={build_ms:.1}ms tile_256={ms:.1}ms lut={}",
            xform.uses_lut()
        );
        assert!(out.iter().any(|&v| v > 0));
    }
}
