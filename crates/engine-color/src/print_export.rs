//! CMYK print export: RGB result → CMYK8 via **lcms2** (native BPC).
//!
//! Soft proof remains on moxcms. Bake-off (see `tests/export_cms_bakeoff.rs`):
//! forward Relative without BPC agrees within a few code values; app-owned
//! forward BPC does not — so export uses lcms2 for the print file path only.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use lcms2::{Flags, Intent, PixelFormat, Profile, Transform};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::preview_encode::delta_e2000_srgb8;
use crate::soft_proof::SoftProofIntent;

/// Unique-color threshold for the palette conversion path.
pub const MAX_PALETTE_COLORS: usize = 4096;

/// ΔE2000 above which a palette color is reported as gamut-shifted.
pub const GAMUT_DELTA_E2000_THRESHOLD: f64 = 2.0;

/// Max ΔE2000 (via Lab) budget for export LUT acceptance (stricter than soft proof).
pub const EXPORT_LUT_MAX_DELTA_E2000: f64 = 0.3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrintExportFormat {
    Tiff,
}

impl Default for PrintExportFormat {
    fn default() -> Self {
        Self::Tiff
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TiffCompression {
    None,
    Lzw,
}

impl Default for TiffCompression {
    fn default() -> Self {
        Self::Lzw
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintExportConfig {
    #[serde(default)]
    pub format: PrintExportFormat,
    pub profile_id: String,
    #[serde(default)]
    pub intent: SoftProofIntent,
    #[serde(default = "default_true")]
    pub bpc: bool,
    /// Pixels per inch written into the TIFF.
    #[serde(default = "default_ppi")]
    pub ppi: f64,
    /// Integer nearest-neighbor scale (≥ 1).
    #[serde(default = "default_scale")]
    pub scale: u32,
    /// Exact RGB(0,0,0) → CMYK(0,0,0,100).
    #[serde(default = "default_true")]
    pub pure_black_k: bool,
    #[serde(default)]
    pub compression: TiffCompression,
}

fn default_true() -> bool {
    true
}
fn default_ppi() -> f64 {
    300.0
}
fn default_scale() -> u32 {
    1
}

impl Default for PrintExportConfig {
    fn default() -> Self {
        Self {
            format: PrintExportFormat::Tiff,
            profile_id: crate::soft_proof::BUILTIN_FOGRA51_ID.to_string(),
            intent: SoftProofIntent::Relative,
            bpc: true,
            ppi: 300.0,
            scale: 1,
            pure_black_k: true,
            compression: TiffCompression::Lzw,
        }
    }
}

#[derive(Debug, Error)]
pub enum PrintExportError {
    #[error("invalid ICC profile: {0}")]
    Profile(String),
    #[error("failed to build CMS transform: {0}")]
    Transform(String),
    #[error("buffer length mismatch")]
    BufferLength,
    #[error("invalid scale (must be ≥ 1)")]
    BadScale,
    #[error("invalid PPI (must be > 0 and finite)")]
    BadPpi,
    #[error("image dimensions overflow after scale")]
    DimensionOverflow,
    #[error("export cancelled")]
    Cancelled,
    #[error("{0}")]
    Message(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintExportEstimate {
    pub width: u32,
    pub height: u32,
    pub out_width: u32,
    pub out_height: u32,
    pub ppi: f64,
    pub width_mm: f64,
    pub height_mm: f64,
    pub unique_colors: u32,
    pub uses_palette_path: bool,
    /// Uncompressed CMYK payload bytes (width×height×4×scale²).
    pub uncompressed_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamutShiftedColor {
    pub rgb: [u8; 3],
    pub cmyk: [u8; 4],
    pub delta_e2000: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamutReport {
    pub unique_colors: u32,
    pub out_of_gamut_fraction: f64,
    pub shifted: Vec<GamutShiftedColor>,
    pub palette_collisions: u32,
    pub max_ink_coverage: f64,
    pub warning_palette_collapse: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportSummary {
    pub out_width: u32,
    pub out_height: u32,
    pub unique_colors: u32,
    pub used_palette_path: bool,
    pub path: String,
}

fn intent_to_lcms(intent: SoftProofIntent) -> Intent {
    match intent {
        SoftProofIntent::Relative => Intent::RelativeColorimetric,
        SoftProofIntent::Perceptual => Intent::Perceptual,
        SoftProofIntent::Absolute => Intent::AbsoluteColorimetric,
    }
}

/// Forward sRGB8 → CMYK8 (+ reverse for gamut report) via lcms2.
pub struct PrintExportTransform {
    to_cmyk: Transform<u8, u8>,
    to_srgb: Transform<u8, u8>,
    pub intent: SoftProofIntent,
    pub bpc_active: bool,
}

impl PrintExportTransform {
    pub fn new(
        icc_bytes: &[u8],
        intent: SoftProofIntent,
        bpc_requested: bool,
    ) -> Result<Self, PrintExportError> {
        let srgb = Profile::new_srgb();
        let proof =
            Profile::new_icc(icc_bytes).map_err(|e| PrintExportError::Profile(format!("{e:?}")))?;

        let bpc_active = bpc_requested && matches!(intent, SoftProofIntent::Relative);
        let mut flags = Flags::default();
        if bpc_active {
            flags = flags | Flags::BLACKPOINT_COMPENSATION;
        }
        let lcms_intent = intent_to_lcms(intent);

        let to_cmyk = Transform::new_flags(
            &srgb,
            PixelFormat::RGB_8,
            &proof,
            PixelFormat::CMYK_8,
            lcms_intent,
            flags,
        )
        .map_err(|e| PrintExportError::Transform(format!("{e:?}")))?;

        let to_srgb = Transform::new_flags(
            &proof,
            PixelFormat::CMYK_8,
            &srgb,
            PixelFormat::RGB_8,
            lcms_intent,
            flags,
        )
        .map_err(|e| PrintExportError::Transform(format!("{e:?}")))?;

        Ok(Self {
            to_cmyk,
            to_srgb,
            intent,
            bpc_active,
        })
    }

    /// Convert packed RGB8 → packed CMYK8 (lengths must be 3N / 4N).
    pub fn rgb8_to_cmyk8(&self, src: &[u8], dst: &mut [u8]) -> Result<(), PrintExportError> {
        if src.len() % 3 != 0 || dst.len() != src.len() / 3 * 4 {
            return Err(PrintExportError::BufferLength);
        }
        self.to_cmyk.transform_pixels(src, dst);
        Ok(())
    }

    pub fn cmyk8_to_rgb8(&self, src: &[u8], dst: &mut [u8]) -> Result<(), PrintExportError> {
        if src.len() % 4 != 0 || dst.len() != src.len() / 4 * 3 {
            return Err(PrintExportError::BufferLength);
        }
        self.to_srgb.transform_pixels(src, dst);
        Ok(())
    }
}

pub fn validate_config(cfg: &PrintExportConfig) -> Result<(), PrintExportError> {
    if cfg.scale < 1 {
        return Err(PrintExportError::BadScale);
    }
    if !(cfg.ppi.is_finite() && cfg.ppi > 0.0) {
        return Err(PrintExportError::BadPpi);
    }
    Ok(())
}

pub fn scaled_dimensions(
    width: u32,
    height: u32,
    scale: u32,
) -> Result<(u32, u32), PrintExportError> {
    let w = (width as u64)
        .checked_mul(scale as u64)
        .ok_or(PrintExportError::DimensionOverflow)?;
    let h = (height as u64)
        .checked_mul(scale as u64)
        .ok_or(PrintExportError::DimensionOverflow)?;
    if w > u32::MAX as u64 || h > u32::MAX as u64 {
        return Err(PrintExportError::DimensionOverflow);
    }
    Ok((w as u32, h as u32))
}

/// Count unique opaque RGB colors in an RGBA8 buffer (alpha ignored for keying).
pub fn count_unique_rgb(rgba: &[u8]) -> usize {
    let mut set = HashSet::new();
    for px in rgba.chunks_exact(4) {
        let key = u32::from_be_bytes([0, px[0], px[1], px[2]]);
        set.insert(key);
    }
    set.len()
}

pub fn estimate_export(
    width: u32,
    height: u32,
    rgba: &[u8],
    cfg: &PrintExportConfig,
) -> Result<PrintExportEstimate, PrintExportError> {
    validate_config(cfg)?;
    let (out_w, out_h) = scaled_dimensions(width, height, cfg.scale)?;
    let unique = count_unique_rgb(rgba);
    let mm_per_inch = 25.4;
    Ok(PrintExportEstimate {
        width,
        height,
        out_width: out_w,
        out_height: out_h,
        ppi: cfg.ppi,
        width_mm: (out_w as f64) / cfg.ppi * mm_per_inch,
        height_mm: (out_h as f64) / cfg.ppi * mm_per_inch,
        unique_colors: unique as u32,
        uses_palette_path: unique <= MAX_PALETTE_COLORS,
        uncompressed_bytes: (out_w as u64) * (out_h as u64) * 4,
    })
}

#[inline]
fn apply_pure_black_white(rgb: [u8; 3], cmyk: &mut [u8; 4], pure_black_k: bool) {
    if rgb == [255, 255, 255] {
        *cmyk = [0, 0, 0, 0];
        return;
    }
    if pure_black_k && rgb == [0, 0, 0] {
        *cmyk = [0, 0, 0, 255];
    }
}

/// Build RGB→CMYK map for unique colors (palette path).
pub fn convert_palette(
    colors: &[[u8; 3]],
    xform: &PrintExportTransform,
    pure_black_k: bool,
) -> Result<Vec<[u8; 4]>, PrintExportError> {
    let mut rgb_flat = Vec::with_capacity(colors.len() * 3);
    for c in colors {
        rgb_flat.extend_from_slice(c);
    }
    let mut cmyk_flat = vec![0u8; colors.len() * 4];
    xform.rgb8_to_cmyk8(&rgb_flat, &mut cmyk_flat)?;
    let mut out = Vec::with_capacity(colors.len());
    for (i, rgb) in colors.iter().enumerate() {
        let mut cmyk = [
            cmyk_flat[i * 4],
            cmyk_flat[i * 4 + 1],
            cmyk_flat[i * 4 + 2],
            cmyk_flat[i * 4 + 3],
        ];
        apply_pure_black_white(*rgb, &mut cmyk, pure_black_k);
        out.push(cmyk);
    }
    Ok(out)
}

fn collect_unique_colors(rgba: &[u8]) -> Vec<[u8; 3]> {
    let mut map: HashMap<u32, [u8; 3]> = HashMap::new();
    for px in rgba.chunks_exact(4) {
        let key = u32::from_be_bytes([0, px[0], px[1], px[2]]);
        map.entry(key).or_insert([px[0], px[1], px[2]]);
    }
    let mut colors: Vec<_> = map.into_values().collect();
    colors.sort_unstable(); // determinism
    colors
}

/// Convert full RGBA8 image to CMYK8 (unscaled). Prefers palette path when cheap.
pub fn rgba8_to_cmyk8(
    rgba: &[u8],
    xform: &PrintExportTransform,
    pure_black_k: bool,
    cancel: Option<&AtomicBool>,
) -> Result<(Vec<u8>, bool, usize), PrintExportError> {
    let pixels = rgba.len() / 4;
    let colors = collect_unique_colors(rgba);
    let unique = colors.len();

    if unique <= MAX_PALETTE_COLORS {
        let cmyk_palette = convert_palette(&colors, xform, pure_black_k)?;
        let mut key_to_idx: HashMap<u32, usize> = HashMap::with_capacity(unique);
        for (i, c) in colors.iter().enumerate() {
            key_to_idx.insert(u32::from_be_bytes([0, c[0], c[1], c[2]]), i);
        }
        let mut out = vec![0u8; pixels * 4];
        for (i, px) in rgba.chunks_exact(4).enumerate() {
            if i & 0x3fff == 0 {
                if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    return Err(PrintExportError::Cancelled);
                }
            }
            let key = u32::from_be_bytes([0, px[0], px[1], px[2]]);
            let idx = key_to_idx[&key];
            let c = cmyk_palette[idx];
            out[i * 4] = c[0];
            out[i * 4 + 1] = c[1];
            out[i * 4 + 2] = c[2];
            out[i * 4 + 3] = c[3];
        }
        return Ok((out, true, unique));
    }

    // Pointwise path (batch by rows for cancel checks).
    let mut rgb = Vec::with_capacity(pixels * 3);
    for px in rgba.chunks_exact(4) {
        rgb.extend_from_slice(&px[..3]);
    }
    let mut cmyk = vec![0u8; pixels * 4];
    // Chunk to allow cancel.
    const CHUNK: usize = 65_536;
    for start in (0..pixels).step_by(CHUNK) {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            return Err(PrintExportError::Cancelled);
        }
        let end = (start + CHUNK).min(pixels);
        let src = &rgb[start * 3..end * 3];
        let dst = &mut cmyk[start * 4..end * 4];
        xform.rgb8_to_cmyk8(src, dst)?;
        for i in start..end {
            let rgb_px = [rgb[i * 3], rgb[i * 3 + 1], rgb[i * 3 + 2]];
            let mut c = [
                cmyk[i * 4],
                cmyk[i * 4 + 1],
                cmyk[i * 4 + 2],
                cmyk[i * 4 + 3],
            ];
            apply_pure_black_white(rgb_px, &mut c, pure_black_k);
            cmyk[i * 4] = c[0];
            cmyk[i * 4 + 1] = c[1];
            cmyk[i * 4 + 2] = c[2];
            cmyk[i * 4 + 3] = c[3];
        }
    }
    Ok((cmyk, false, unique))
}

/// Integer nearest-neighbor scale of packed CMYK8.
pub fn scale_cmyk8_nearest(
    cmyk: &[u8],
    width: u32,
    height: u32,
    scale: u32,
) -> Result<Vec<u8>, PrintExportError> {
    if scale == 1 {
        return Ok(cmyk.to_vec());
    }
    let (ow, oh) = scaled_dimensions(width, height, scale)?;
    let mut out = vec![0u8; (ow as usize) * (oh as usize) * 4];
    let s = scale as usize;
    let w = width as usize;
    let h = height as usize;
    for y in 0..h {
        for x in 0..w {
            let src = ((y * w + x) * 4)..((y * w + x) * 4 + 4);
            let px = &cmyk[src];
            for dy in 0..s {
                for dx in 0..s {
                    let ox = x * s + dx;
                    let oy = y * s + dy;
                    let di = (oy * ow as usize + ox) * 4;
                    out[di..di + 4].copy_from_slice(px);
                }
            }
        }
    }
    Ok(out)
}

pub fn gamut_report(
    rgba: &[u8],
    xform: &PrintExportTransform,
    pure_black_k: bool,
    cancel: Option<&AtomicBool>,
) -> Result<GamutReport, PrintExportError> {
    let colors = collect_unique_colors(rgba);
    let unique = colors.len() as u32;
    if colors.is_empty() {
        return Ok(GamutReport {
            unique_colors: 0,
            out_of_gamut_fraction: 0.0,
            shifted: vec![],
            palette_collisions: 0,
            max_ink_coverage: 0.0,
            warning_palette_collapse: false,
        });
    }
    if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
        return Err(PrintExportError::Cancelled);
    }

    let cmyk_palette = convert_palette(&colors, xform, pure_black_k)?;

    // Collision: distinct RGB → same CMYK.
    let mut cmyk_to_count: HashMap<[u8; 4], u32> = HashMap::new();
    for c in &cmyk_palette {
        *cmyk_to_count.entry(*c).or_insert(0) += 1;
    }
    let palette_collisions = cmyk_to_count.values().filter(|&&n| n > 1).sum::<u32>();
    // Number of colors that share a CMYK with at least one other.
    let collapsed_colors: u32 = cmyk_to_count.values().filter(|&&n| n > 1).map(|&n| n).sum();

    let mut cmyk_flat = Vec::with_capacity(colors.len() * 4);
    for c in &cmyk_palette {
        cmyk_flat.extend_from_slice(c);
    }
    let mut round_rgb = vec![0u8; colors.len() * 3];
    xform.cmyk8_to_rgb8(&cmyk_flat, &mut round_rgb)?;

    let mut shifted = Vec::new();
    let mut max_ink = 0.0f64;

    // Pixel-weighted OOG: count pixels whose RGB maps to a shifted palette entry.
    let mut key_shift: HashMap<u32, f64> = HashMap::new();

    for (i, rgb) in colors.iter().enumerate() {
        let rt = [round_rgb[i * 3], round_rgb[i * 3 + 1], round_rgb[i * 3 + 2]];
        let de = delta_e2000_srgb8(*rgb, rt);
        let ink = (cmyk_palette[i][0] as f64
            + cmyk_palette[i][1] as f64
            + cmyk_palette[i][2] as f64
            + cmyk_palette[i][3] as f64)
            / 255.0
            * 100.0;
        max_ink = max_ink.max(ink);
        if de >= GAMUT_DELTA_E2000_THRESHOLD {
            key_shift.insert(u32::from_be_bytes([0, rgb[0], rgb[1], rgb[2]]), de);
            if shifted.len() < 64 {
                shifted.push(GamutShiftedColor {
                    rgb: *rgb,
                    cmyk: cmyk_palette[i],
                    delta_e2000: de,
                });
            }
        }
    }
    shifted.sort_by(|a, b| {
        b.delta_e2000
            .partial_cmp(&a.delta_e2000)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut oog_pixels = 0u64;
    let total_pixels = (rgba.len() / 4) as u64;
    for px in rgba.chunks_exact(4) {
        let key = u32::from_be_bytes([0, px[0], px[1], px[2]]);
        if key_shift.contains_key(&key) {
            oog_pixels += 1;
        }
    }

    Ok(GamutReport {
        unique_colors: unique,
        out_of_gamut_fraction: if total_pixels == 0 {
            0.0
        } else {
            oog_pixels as f64 / total_pixels as f64
        },
        shifted,
        palette_collisions,
        max_ink_coverage: max_ink,
        warning_palette_collapse: collapsed_colors >= 2,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::soft_proof::BUILTIN_FOGRA51_ICC;

    fn xform(bpc: bool) -> PrintExportTransform {
        PrintExportTransform::new(BUILTIN_FOGRA51_ICC, SoftProofIntent::Relative, bpc).unwrap()
    }

    #[test]
    fn pure_black_and_white() {
        let x = xform(true);
        let rgba = vec![
            0, 0, 0, 255, // black
            255, 255, 255, 255, // white
            128, 64, 32, 255, // other
        ];
        let (cmyk, used_pal, n) = rgba8_to_cmyk8(&rgba, &x, true, None).unwrap();
        assert!(used_pal);
        assert_eq!(n, 3);
        assert_eq!(&cmyk[0..4], &[0, 0, 0, 255]);
        assert_eq!(&cmyk[4..8], &[0, 0, 0, 0]);
        // Midtone should not be pure K.
        assert!(cmyk[8..12] != [0, 0, 0, 255]);
    }

    #[test]
    fn scale_nearest_blocks() {
        // 2×1 CMYK, scale 2 → 4×2
        let cmyk = vec![10, 20, 30, 40, 50, 60, 70, 80];
        let out = scale_cmyk8_nearest(&cmyk, 2, 1, 2).unwrap();
        assert_eq!(out.len(), 4 * 2 * 4);
        assert_eq!(&out[0..4], &[10, 20, 30, 40]);
        assert_eq!(&out[4..8], &[10, 20, 30, 40]);
        assert_eq!(&out[8..12], &[50, 60, 70, 80]);
    }

    #[test]
    fn determinism_two_runs() {
        let x = xform(false);
        let mut rgba = Vec::new();
        for i in 0..64u8 {
            rgba.extend_from_slice(&[i, i.wrapping_mul(3), 255u8.wrapping_sub(i), 255]);
        }
        let (a, _, _) = rgba8_to_cmyk8(&rgba, &x, true, None).unwrap();
        let (b, _, _) = rgba8_to_cmyk8(&rgba, &x, true, None).unwrap();
        assert_eq!(a, b);
    }
}
