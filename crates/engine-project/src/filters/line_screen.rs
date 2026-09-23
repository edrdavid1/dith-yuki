//! Line-screen halftone — parallel ink stripes whose width tracks local tone.
//!
//! Parametric cousin of round-dot CMYK screens ([`super::dither_ordered`]
//! `halftone_channel_ink`): distance is measured to the nearest line of a
//! rotated lattice instead of to a cell centre. Spec Batch E `line_screen`.

use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{DitherColorMode, DitherParamsV2};
use crate::types::LayerId;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_tiles::block_cache::BlockRepresentativeCache;

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;

#[inline]
fn to_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Absolute distance from `(gx, gy)` to the nearest line of a family with
/// pitch `cell_size` and orientation `angle_deg`.
#[inline]
pub fn line_screen_distance(gx: i32, gy: i32, cell_size: f32, angle_deg: f32) -> f32 {
    let s = cell_size.max(1.0);
    let phi = angle_deg.to_radians();
    let u = gx as f32 * phi.cos() + gy as f32 * phi.sin();
    let d = u.rem_euclid(s);
    (d - s * 0.5).abs()
}

/// Whether the sample is inked for tone in `[0, 1]` (1 = full ink / black).
///
/// Stripe half-width grows linearly with tone (classic line screen), scaled by
/// `threshold_scale`. At tone 0 the screen is blank; at tone 1 stripes meet.
#[inline]
pub fn line_screen_ink(
    gx: i32,
    gy: i32,
    tone: f32,
    cell_size: f32,
    angle_deg: f32,
    threshold_scale: f32,
) -> bool {
    let tone = tone.clamp(0.0, 1.0);
    if tone <= 0.0 {
        return false;
    }
    let s = cell_size.max(1.0);
    let half = (s * 0.5) * tone * threshold_scale.max(0.0);
    line_screen_distance(gx, gy, s, angle_deg) <= half
}

/// Apply line-screen halftone into `dst` (binary ink on paper, luminance-driven).
pub fn apply_line_screen_into(
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
    let cell = (params.halftone_cell_size.max(2) as f32).max(1.0);
    let angle = params.pattern_angle;
    let scale = params.threshold_scale;

    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r = tile.at(x, y, 0);
            let g = tile.at(x, y, 1);
            let b = tile.at(x, y, 2);
            let a = tile.at(x, y, 3);
            let gcoord =
                engine_tiles::coords::GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);

            let lum = match params.color_mode {
                DitherColorMode::Grayscale | DitherColorMode::Rgb => to_luminance(r, g, b),
            };
            // Tone for ink coverage: dark regions print wider stripes.
            let tone = 1.0 - lum;
            let ink = line_screen_ink(gcoord.x, gcoord.y, tone, cell, angle, scale);
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
    fn distance_zero_at_cell_centre_line() {
        // Angle 0: lines are vertical families? u = x; centre of cell [0,s) is s/2.
        let s = 8.0f32;
        assert!(line_screen_distance(4, 0, s, 0.0) < 1e-5);
        assert!((line_screen_distance(0, 0, s, 0.0) - 4.0).abs() < 1e-5);
    }

    #[test]
    fn white_never_inks() {
        assert!(!line_screen_ink(4, 0, 0.0, 8.0, 0.0, 1.0));
    }

    #[test]
    fn full_black_fills_stripe_centre() {
        assert!(line_screen_ink(4, 0, 1.0, 8.0, 0.0, 1.0));
    }

    #[test]
    fn darker_covers_more_pixels() {
        let count = |tone: f32| {
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .filter(|&(x, y)| line_screen_ink(x, y, tone, 8.0, 45.0, 1.0))
                .count()
        };
        assert!(count(0.25) < count(0.85));
    }
}
