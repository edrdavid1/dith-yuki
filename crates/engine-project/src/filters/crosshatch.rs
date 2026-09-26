//! Crosshatch / engraving-style hatch dither.
//!
//! Builds tone with a ladder of hatch layers (classic copperplate progression):
//! sparse base lines, denser same-angle lines, then cross-angle layers. Each
//! layer activates only where local darkness exceeds its threshold — light
//! areas stay paper-white; shadows accumulate dense cross-hatching.
//!
//! Oracle (§1.4 / Batch E): layer thresholds and angles are fixed published
//! ladder defaults (not a free-form ED kernel). Unit tests lock the geometry
//! helpers (`on_hatch_line`, layer activation).

use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{DitherColorMode, DitherParamsV2};
use crate::types::LayerId;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_tiles::block_cache::BlockRepresentativeCache;

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

/// Engraving ladder: `(darkness_threshold, angle_offset_deg, spacing_scale)`.
/// Thresholds span ~0.12..0.85; cross-hatch begins at layer 2 (90° offset).
pub const HATCH_LADDER: [(f32, f32, f32); 4] = [
    (0.12, 0.0, 1.0),
    (0.38, 0.0, 0.62),
    (0.58, 90.0, 0.62),
    (0.82, 45.0, 0.45),
];

#[inline]
fn to_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Distance from `(gx, gy)` to the nearest line of a family with `spacing`
/// and orientation `angle_deg`. Lines pass through the origin family.
#[inline]
pub fn hatch_line_distance(gx: i32, gy: i32, angle_deg: f32, spacing: f32) -> f32 {
    let spacing = spacing.max(1.0);
    let phi = angle_deg.to_radians();
    let u = gx as f32 * phi.cos() + gy as f32 * phi.sin();
    let d = u.rem_euclid(spacing);
    d.min(spacing - d)
}

/// Whether the sample lies on an inked hatch stroke.
#[inline]
pub fn on_hatch_line(gx: i32, gy: i32, angle_deg: f32, spacing: f32, half_width: f32) -> bool {
    hatch_line_distance(gx, gy, angle_deg, spacing) <= half_width.max(0.25)
}

/// Return true if any active hatch layer inks this sample for `darkness` in `[0,1]`.
pub fn crosshatch_ink(
    gx: i32,
    gy: i32,
    darkness: f32,
    base_spacing: f32,
    base_angle_deg: f32,
    thickness_scale: f32,
) -> bool {
    let darkness = darkness.clamp(0.0, 1.0);
    let base_spacing = base_spacing.max(2.0);
    let thickness_scale = thickness_scale.max(0.05);

    for &(thresh, ang_off, sp_mul) in &HATCH_LADDER {
        if darkness < thresh {
            continue;
        }
        let spacing = (base_spacing * sp_mul).max(2.0);
        // ~15% of pitch, scaled by threshold_scale (clamped so lines never fill the cell).
        let half_w = (spacing * 0.15 * thickness_scale).clamp(0.35, spacing * 0.45);
        if on_hatch_line(gx, gy, base_angle_deg + ang_off, spacing, half_w) {
            return true;
        }
    }
    false
}

/// Apply crosshatch dither into `dst`.
pub fn apply_crosshatch_into(
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    _palette_cache: &PaletteKdCache,
    _lut_cache: &PaletteLutCache,
    _document: &Document,
    _block_cache: &BlockRepresentativeCache,
    _layer_id: LayerId,
    dst: &mut PixelTile,
) -> Result<(), EngineError> {
    let spacing = params.wave_wavelength.max(2.0);
    let base_angle = params.pattern_angle;
    let thickness = params.threshold_scale;

    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = tile.at(x, y, 0);
            let g = tile.at(x, y, 1);
            let b = tile.at(x, y, 2);
            let a = tile.at(x, y, 3);

            let gcoord =
                engine_tiles::coords::GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
            let gx = gcoord.x;
            let gy = gcoord.y;

            let lum = match params.color_mode {
                DitherColorMode::Grayscale | DitherColorMode::Rgb => to_luminance(r, g, b),
            };
            let darkness = 1.0 - lum;
            let ink = crosshatch_ink(gx, gy, darkness, spacing, base_angle, thickness);
            let v = if ink { 0.0 } else { 1.0 };

            dst.set(x, y, 0, v);
            dst.set(x, y, 1, v);
            dst.set(x, y, 2, v);
            dst.set(
                x,
                y,
                3,
                if params.dither_alpha && a <= 0.0 {
                    0.0
                } else {
                    a
                },
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hatch_line_distance_zero_on_origin_family() {
        // Horizontal lines (angle 0): u = x, spacing 8 → x=0,8,16 on lines.
        assert!(hatch_line_distance(0, 5, 0.0, 8.0) < 1e-5);
        assert!(hatch_line_distance(8, 3, 0.0, 8.0) < 1e-5);
        assert!((hatch_line_distance(4, 0, 0.0, 8.0) - 4.0).abs() < 1e-5);
    }

    #[test]
    fn white_has_no_ink() {
        assert!(!crosshatch_ink(10, 10, 0.0, 8.0, 45.0, 1.0));
    }

    #[test]
    fn near_black_inks_somewhere_on_grid() {
        // Dense darkness must hit at least one stroke in a local neighbourhood.
        let mut any = false;
        for y in 0..32 {
            for x in 0..32 {
                if crosshatch_ink(x, y, 0.95, 8.0, 45.0, 1.0) {
                    any = true;
                    break;
                }
            }
        }
        assert!(any, "near-black should activate hatch strokes");
    }

    #[test]
    fn midtone_sparser_than_shadow() {
        let count = |dark: f32| {
            let mut n = 0usize;
            for y in 0..64 {
                for x in 0..64 {
                    if crosshatch_ink(x, y, dark, 8.0, 45.0, 1.0) {
                        n += 1;
                    }
                }
            }
            n
        };
        assert!(
            count(0.3) < count(0.9),
            "darker tones must accumulate more ink"
        );
    }
}
