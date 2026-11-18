//! CRT scanline + subpixel mask + light screen traits.
//!
//! Scanline / mask phase keys off global coords via `GlobalCoordSigned`.
//! Spatial knobs (curvature, bleed, sharpness) stay within [`HALO`] so GPU
//! and tiled CPU stay seam-safe.
//!
//! Alpha is preserved unchanged.

use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

/// Tile full size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

/// Max sample offset for curvature / bleed / sharpness (matches [`HALO`]).
const SPATIAL_CAP: f32 = HALO as f32;

/// CRT mask geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CrtMaskType {
    /// Vertical RGB aperture grille (classic triad columns).
    #[default]
    Aperture = 0,
    /// Staggered RGB shadow mask.
    Shadow = 1,
}

impl CrtMaskType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Shadow,
            _ => Self::Aperture,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Aperture => "aperture",
            Self::Shadow => "shadow",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "shadow" => Self::Shadow,
            _ => Self::Aperture,
        }
    }
}

/// Scanline orientation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum CrtOrientation {
    #[default]
    Horizontal = 0,
    Vertical = 1,
}

impl CrtOrientation {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Vertical,
            _ => Self::Horizontal,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Horizontal => "horizontal",
            Self::Vertical => "vertical",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "vertical" => Self::Vertical,
            _ => Self::Horizontal,
        }
    }
}

/// All CRT apply knobs (CPU + GPU parity surface).
#[derive(Clone, Copy, Debug)]
pub struct CrtApplyParams {
    pub period: u8,
    pub strength: f32,
    pub mask_strength: f32,
    /// 0 = hard bands (v1), 1 = full sine soft.
    pub soft_scanlines: f32,
    /// Dark scanline width in CRT-cell units (1 ..= period). Default period/2.
    pub line_width: u8,
    /// Enlarge the whole CRT pattern (scanlines + mask). Each CRT cell is
    /// `scale` document pixels. Default 1.
    pub scale: u8,
    pub orientation: CrtOrientation,
    pub mask_type: CrtMaskType,
    /// Lift average brightness after scanline/mask attenuation (0..=1).
    pub brightness_compensate: f32,
    /// Barrel warp amount (0..=1); displacement capped to [`HALO`] px.
    pub curvature: f32,
    /// Edge darkening vs document frame (0..=1).
    pub vignette: f32,
    /// 1px RGB phosphor bleed along the scan axis (0..=1).
    pub phosphor_bleed: f32,
    /// 1px unsharp amount on the source sample (0..=1).
    pub sharpness: f32,
    pub doc_w: u32,
    pub doc_h: u32,
}

impl Default for CrtApplyParams {
    fn default() -> Self {
        Self {
            period: 2,
            strength: 0.5,
            mask_strength: 0.0,
            soft_scanlines: 0.0,
            line_width: 1,
            scale: 1,
            orientation: CrtOrientation::Horizontal,
            mask_type: CrtMaskType::Aperture,
            brightness_compensate: 0.0,
            curvature: 0.0,
            vignette: 0.0,
            phosphor_bleed: 0.0,
            sharpness: 0.0,
            doc_w: 1024,
            doc_h: 1024,
        }
    }
}

/// Map a document-space axis into CRT-cell space.
#[inline]
pub(crate) fn crt_cell(coord: i32, scale: u8) -> i32 {
    let s = scale.max(1) as i64;
    (coord as i64).div_euclid(s) as i32
}

impl CrtApplyParams {
    /// Clamp line width into `1 ..= period`.
    pub fn clamped_line_width(self) -> u8 {
        let p = self.period.clamp(2, 8);
        self.line_width.clamp(1, p)
    }

    pub fn clamped_scale(self) -> u8 {
        self.scale.clamp(1, 16)
    }

    /// Mean attenuation from scanlines alone (for brightness compensate).
    pub fn mean_scanline_gain(self) -> f32 {
        let s = self.strength.clamp(0.0, 1.0);
        let soft = self.soft_scanlines.clamp(0.0, 1.0);
        let p = self.period.max(2) as f32;
        let frac = self.clamped_line_width() as f32 / p;
        let hard_mean = 1.0 - s * frac;
        // cos² mean over a period ≈ 0.5.
        let soft_mean = 1.0 - s * 0.5;
        hard_mean * (1.0 - soft) + soft_mean * soft
    }

    /// Mean attenuation from the subpixel mask alone.
    pub fn mean_mask_gain(self) -> f32 {
        let m = self.mask_strength.clamp(0.0, 1.0);
        if m <= 0.0 {
            return 1.0;
        }
        // One of three channels full; two attenuated → mean 1 - 2m/3.
        1.0 - (2.0 / 3.0) * m
    }

    pub fn compensate_scale(self) -> f32 {
        let c = self.brightness_compensate.clamp(0.0, 1.0);
        if c <= 0.0 {
            return 1.0;
        }
        let mean = (self.mean_scanline_gain() * self.mean_mask_gain()).max(1e-3);
        let full = 1.0 / mean;
        1.0 + (full - 1.0) * c
    }
}

/// Scanline gain for a phase coordinate along the scan axis.
///
/// `line_width` is the dark band height/width in pixels (clamped to `1 ..= period`).
pub(crate) fn scanline_gain(
    axis: i32,
    period: u8,
    strength: f32,
    line_width: u8,
    soft: f32,
) -> f32 {
    let p = period.max(2) as i32;
    let strength = strength.clamp(0.0, 1.0);
    let dark_rows = (line_width as i32).clamp(1, p);
    let frac = dark_rows as f32 / p as f32;
    let soft = soft.clamp(0.0, 1.0);
    let line = (axis as i64).rem_euclid(p as i64) as i32;
    let t = line as f32 / p as f32; // [0, 1)

    let hard = if line < dark_rows {
        1.0 - strength
    } else {
        1.0
    };

    // Raised-cosine trough; wider line_width → broader soft trough.
    let soft_gain = {
        let phase = t * std::f32::consts::TAU;
        let cos01 = 0.5 + 0.5 * phase.cos(); // 1 at line start, 0 at mid
        let shaped = cos01.powf(1.0 + (1.0 - frac) * 2.0);
        1.0 - strength * shaped
    };

    hard * (1.0 - soft) + soft_gain * soft
}

/// RGB mask multipliers for global X/Y.
pub(crate) fn rgb_mask_gain(
    x_g: i32,
    y_g: i32,
    channel: usize,
    mask_strength: f32,
    mask_type: CrtMaskType,
) -> f32 {
    if mask_strength <= 0.0 {
        return 1.0;
    }
    let col = match mask_type {
        CrtMaskType::Aperture => (x_g as i64).rem_euclid(3) as usize,
        CrtMaskType::Shadow => ((x_g + y_g.wrapping_mul(2)) as i64).rem_euclid(3) as usize,
    };
    if col == channel {
        1.0
    } else {
        1.0 - mask_strength
    }
}

fn doc_uv(gx: i32, gy: i32, doc_w: u32, doc_h: u32) -> (f32, f32) {
    let w = doc_w.max(1) as f32;
    let h = doc_h.max(1) as f32;
    let u = (gx as f32 + 0.5) / w * 2.0 - 1.0;
    let v = (gy as f32 + 0.5) / h * 2.0 - 1.0;
    (u, v)
}

fn barrel_offset(u: f32, v: f32, curvature: f32) -> (f32, f32) {
    let k = curvature.clamp(0.0, 1.0);
    if k <= 0.0 {
        return (0.0, 0.0);
    }
    // Destination → source: pull edges inward. Cap to HALO px.
    let r2 = u * u + v * v;
    let factor = k * 0.35 * r2;
    let dx = (u * factor * (TILE_SIZE as f32 * 0.25)).clamp(-SPATIAL_CAP, SPATIAL_CAP);
    let dy = (v * factor * (TILE_SIZE as f32 * 0.25)).clamp(-SPATIAL_CAP, SPATIAL_CAP);
    (dx, dy)
}

fn vignette_gain(u: f32, v: f32, amount: f32) -> f32 {
    let a = amount.clamp(0.0, 1.0);
    if a <= 0.0 {
        return 1.0;
    }
    let r = (u * u + v * v).sqrt();
    let t = ((r - 0.55) / 0.85).clamp(0.0, 1.0);
    1.0 - a * t * t
}

fn sample_rgba(tile: &PixelTile, lx: i32, ly: i32) -> [f32; 4] {
    let max = (TILE_FULL_SIZE - 1) as i32;
    let x = lx.clamp(0, max) as u32;
    let y = ly.clamp(0, max) as u32;
    [
        tile.at(x, y, 0),
        tile.at(x, y, 1),
        tile.at(x, y, 2),
        tile.at(x, y, 3),
    ]
}

fn sample_rgba_bilinear(tile: &PixelTile, fx: f32, fy: f32) -> [f32; 4] {
    let max = (TILE_FULL_SIZE - 1) as f32;
    let fx = fx.clamp(0.0, max);
    let fy = fy.clamp(0.0, max);
    let x0 = fx.floor() as i32;
    let y0 = fy.floor() as i32;
    let x1 = (x0 + 1).min(max as i32);
    let y1 = (y0 + 1).min(max as i32);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let c00 = sample_rgba(tile, x0, y0);
    let c10 = sample_rgba(tile, x1, y0);
    let c01 = sample_rgba(tile, x0, y1);
    let c11 = sample_rgba(tile, x1, y1);
    let mut out = [0.0; 4];
    for i in 0..4 {
        let a = c00[i] * (1.0 - tx) + c10[i] * tx;
        let b = c01[i] * (1.0 - tx) + c11[i] * tx;
        out[i] = a * (1.0 - ty) + b * ty;
    }
    out
}

fn apply_phosphor_bleed(center: [f32; 4], left: [f32; 4], right: [f32; 4], amount: f32) -> [f32; 4] {
    let a = amount.clamp(0.0, 1.0);
    if a <= 0.0 {
        return center;
    }
    // Mild channel smear: R picks up left, B picks up right, G stays mostly center.
    [
        center[0] * (1.0 - 0.55 * a) + left[0] * (0.55 * a),
        center[1] * (1.0 - 0.25 * a) + left[1] * (0.125 * a) + right[1] * (0.125 * a),
        center[2] * (1.0 - 0.55 * a) + right[2] * (0.55 * a),
        center[3],
    ]
}

fn apply_sharpen(center: [f32; 4], n: [f32; 4], s: [f32; 4], e: [f32; 4], w: [f32; 4], amount: f32) -> [f32; 4] {
    let a = amount.clamp(0.0, 1.0);
    if a <= 0.0 {
        return center;
    }
    let mut out = center;
    for i in 0..3 {
        let blur = (n[i] + s[i] + e[i] + w[i]) * 0.25;
        out[i] = (center[i] + (center[i] - blur) * a).clamp(0.0, 1.0);
    }
    out
}

/// Apply CRT to a tile.
pub fn apply_crt(
    tile: &PixelTile,
    coord: TileCoord,
    period: u8,
    strength: f32,
    mask_strength: f32,
) -> PixelTile {
    let mut result = PixelTile::new();
    let params = CrtApplyParams {
        period,
        strength,
        mask_strength,
        ..CrtApplyParams::default()
    };
    apply_crt_into(tile, coord, params, &mut result);
    result
}

/// CRT into an existing buffer (full 260² write, no alloc).
pub fn apply_crt_into(
    tile: &PixelTile,
    coord: TileCoord,
    params: CrtApplyParams,
    dst: &mut PixelTile,
) {
    let period = params.period.clamp(2, 8);
    let strength = params.strength.clamp(0.0, 1.0);
    let mask_strength = params.mask_strength.clamp(0.0, 1.0);
    let soft = params.soft_scanlines.clamp(0.0, 1.0);
    let line_width = params.clamped_line_width();
    let scale = params.clamped_scale();
    let compensate = params.compensate_scale();
    let need_spatial = params.curvature > 1e-6
        || params.phosphor_bleed > 1e-6
        || params.sharpness > 1e-6;
    // Phosphor bleed steps with pattern scale, but stays inside HALO.
    let bleed_step = (scale as f32).min(SPATIAL_CAP).max(1.0);

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
            let (u, v) = doc_uv(g.x, g.y, params.doc_w, params.doc_h);
            let cx = crt_cell(g.x, scale);
            let cy = crt_cell(g.y, scale);

            let rgba = if need_spatial {
                let (dx, dy) = barrel_offset(u, v, params.curvature);
                let fx = x as f32 + dx;
                let fy = y as f32 + dy;
                let center = sample_rgba_bilinear(tile, fx, fy);
                let axis_h = matches!(params.orientation, CrtOrientation::Horizontal);
                // Bleed along the phosphor axis (perpendicular to scanlines).
                let (lx, ly, rx, ry) = if axis_h {
                    (fx - bleed_step, fy, fx + bleed_step, fy)
                } else {
                    (fx, fy - bleed_step, fx, fy + bleed_step)
                };
                let left = sample_rgba_bilinear(tile, lx, ly);
                let right = sample_rgba_bilinear(tile, rx, ry);
                let bled = apply_phosphor_bleed(center, left, right, params.phosphor_bleed);
                let n = sample_rgba_bilinear(tile, fx, fy - 1.0);
                let s = sample_rgba_bilinear(tile, fx, fy + 1.0);
                let e = sample_rgba_bilinear(tile, fx + 1.0, fy);
                let w = sample_rgba_bilinear(tile, fx - 1.0, fy);
                apply_sharpen(bled, n, s, e, w, params.sharpness)
            } else {
                sample_rgba(tile, x as i32, y as i32)
            };

            let axis = match params.orientation {
                CrtOrientation::Horizontal => cy,
                CrtOrientation::Vertical => cx,
            };
            let gain = scanline_gain(axis, period, strength, line_width, soft);
            let vig = vignette_gain(u, v, params.vignette);

            for c in 0..3usize {
                let mask = rgb_mask_gain(cx, cy, c, mask_strength, params.mask_type);
                let v = rgba[c] * gain * mask * vig * compensate;
                dst.set(x, y, c as u32, v.clamp(0.0, 1.0));
            }
            dst.set(x, y, 3, rgba[3]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanline_gain_hard_matches_v1_even_period() {
        // line_width 1, soft 0 → same as legacy period/2 dark rows for period=2.
        assert!((scanline_gain(0, 2, 1.0, 1, 0.0) - 0.0).abs() < 1e-6);
        assert!((scanline_gain(1, 2, 1.0, 1, 0.0) - 1.0).abs() < 1e-6);
        assert!((scanline_gain(-1, 2, 0.5, 1, 0.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn soft_scanlines_are_between_hard_extremes() {
        let soft = scanline_gain(0, 4, 0.8, 2, 1.0);
        assert!(soft > 0.0 && soft < 1.0);
    }

    #[test]
    fn wider_line_width_darkens_more_rows() {
        assert!((scanline_gain(0, 4, 1.0, 1, 0.0) - 0.0).abs() < 1e-6);
        assert!((scanline_gain(1, 4, 1.0, 1, 0.0) - 1.0).abs() < 1e-6);
        assert!((scanline_gain(1, 4, 1.0, 2, 0.0) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn scale_enlarges_pattern_cells() {
        assert_eq!(crt_cell(0, 3), 0);
        assert_eq!(crt_cell(2, 3), 0);
        assert_eq!(crt_cell(3, 3), 1);
        assert_eq!(crt_cell(-1, 3), -1);
        assert_eq!(crt_cell(-3, 3), -1);
        assert_eq!(crt_cell(-4, 3), -2);
    }

    #[test]
    fn crt_seamless_horizontal_boundary() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                tile.set(x, y, 0, 0.8);
                tile.set(x, y, 1, 0.8);
                tile.set(x, y, 2, 0.8);
                tile.set(x, y, 3, 1.0);
            }
        }
        let params = CrtApplyParams {
            period: 2,
            strength: 0.5,
            mask_strength: 0.0,
            ..CrtApplyParams::default()
        };
        let top = {
            let mut out = PixelTile::new();
            apply_crt_into(
                &tile,
                TileCoord {
                    level: 0,
                    x: 0,
                    y: 0,
                },
                params,
                &mut out,
            );
            out
        };
        let bottom = {
            let mut out = PixelTile::new();
            apply_crt_into(
                &tile,
                TileCoord {
                    level: 0,
                    x: 0,
                    y: 1,
                },
                params,
                &mut out,
            );
            out
        };

        let last_core_y = HALO + TILE_SIZE - 1;
        let first_core_y = HALO;
        let g_top = GlobalCoordSigned::from_local_with_halo(
            TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            HALO,
            last_core_y,
            HALO,
        );
        let g_bot = GlobalCoordSigned::from_local_with_halo(
            TileCoord {
                level: 0,
                x: 0,
                y: 1,
            },
            HALO,
            first_core_y,
            HALO,
        );
        assert_eq!(g_top.y + 1, g_bot.y);

        let v_top = top.at(HALO, last_core_y, 0);
        let v_bot = bottom.at(HALO, first_core_y, 0);
        let expected_top = 0.8 * scanline_gain(g_top.y, 2, 0.5, 1, 0.0);
        let expected_bot = 0.8 * scanline_gain(g_bot.y, 2, 0.5, 1, 0.0);
        assert!((v_top - expected_top).abs() < 1e-5);
        assert!((v_bot - expected_bot).abs() < 1e-5);
        assert!((v_top - v_bot).abs() > 0.1);
    }

    #[test]
    fn brightness_compensate_raises_mean() {
        let base = CrtApplyParams {
            period: 2,
            strength: 1.0,
            line_width: 1,
            soft_scanlines: 0.0,
            brightness_compensate: 0.0,
            ..CrtApplyParams::default()
        };
        let lifted = CrtApplyParams {
            brightness_compensate: 1.0,
            ..base
        };
        assert!(lifted.compensate_scale() > base.compensate_scale());
    }
}
