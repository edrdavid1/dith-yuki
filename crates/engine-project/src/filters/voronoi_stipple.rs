//! Voronoi stipple — jittered-lattice sites with tone-proportional retention.
//!
//! Approximates classic weighted-Voronoi / CVT stippling without a full-document
//! Lloyd pass: a global jittered lattice supplies candidate sites (irregular
//! packing à la Voronoi dual), each site is kept with probability ∝ local
//! darkness, and retained sites are drawn as fixed-radius dots. Pure function of
//! global coordinates + local tone → seam-safe across tiles (Batch E).

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

/// Deterministic hash → `[0, 1)`.
#[inline]
pub fn hash01(x: i32, y: i32, salt: u32) -> f32 {
    let mut n = (x as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((y as u32).wrapping_mul(0x85EB_CA6B))
        .wrapping_add(salt);
    n ^= n >> 16;
    n = n.wrapping_mul(0x7FEB_352D);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846C_A68B);
    n ^= n >> 16;
    (n >> 8) as f32 / 16_777_216.0
}

/// Jittered site centre for lattice cell `(cx, cy)`.
#[inline]
pub fn site_for_cell(cx: i32, cy: i32, cell: f32) -> (f32, f32) {
    let jx = hash01(cx, cy, 0xA2C2_A4C1) - 0.5;
    let jy = hash01(cx, cy, 0xB529_7A4D) - 0.5;
    let amp = 0.42;
    (
        (cx as f32 + 0.5 + jx * amp) * cell,
        (cy as f32 + 0.5 + jy * amp) * cell,
    )
}

/// Nearest jittered lattice site to `(gx, gy)` and its cell indices.
#[inline]
pub fn nearest_site(gx: i32, gy: i32, cell: f32) -> (f32, f32, i32, i32) {
    let cell = cell.max(2.0);
    let base_cx = (gx as f32 / cell).floor() as i32;
    let base_cy = (gy as f32 / cell).floor() as i32;
    let mut best_d2 = f32::INFINITY;
    let mut best = (0.0f32, 0.0f32, base_cx, base_cy);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cx = base_cx + dx;
            let cy = base_cy + dy;
            let (sx, sy) = site_for_cell(cx, cy, cell);
            let ddx = gx as f32 - sx;
            let ddy = gy as f32 - sy;
            let d2 = ddx * ddx + ddy * ddy;
            if d2 < best_d2 {
                best_d2 = d2;
                best = (sx, sy, cx, cy);
            }
        }
    }
    best
}

/// Whether `(gx, gy)` is inked for tone-as-darkness in `[0, 1]`.
///
/// `density_scale` stretches retention (`threshold_scale`); `radius` is the
/// drawn-dot radius in pixels.
#[inline]
pub fn voronoi_stipple_ink(
    gx: i32,
    gy: i32,
    darkness: f32,
    cell: f32,
    radius: f32,
    density_scale: f32,
) -> bool {
    let darkness = darkness.clamp(0.0, 1.0);
    if darkness <= 0.0 {
        return false;
    }
    let (sx, sy, cx, cy) = nearest_site(gx, gy, cell);
    let dx = gx as f32 - sx;
    let dy = gy as f32 - sy;
    let r = radius.max(0.35);
    if dx * dx + dy * dy > r * r {
        return false;
    }
    let gate = (darkness * density_scale.max(0.0)).clamp(0.0, 1.0);
    hash01(cx, cy, 0xC0FF_EE00) < gate
}

/// Apply Voronoi stipple into `dst` (binary ink on paper).
pub fn apply_voronoi_stipple_into(
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
    let cell = (params.halftone_cell_size.max(2) as f32).max(2.0);
    let density = params.threshold_scale;
    // Dot radius: grows gently with pixel_size; stays smaller than half-pitch.
    let radius = ((params.pixel_size.max(1) as f32) * 0.55).clamp(0.45, cell * 0.4);

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
            let darkness = 1.0 - lum;
            let ink = voronoi_stipple_ink(gcoord.x, gcoord.y, darkness, cell, radius, density);
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
    fn white_never_inks() {
        assert!(!voronoi_stipple_ink(4, 4, 0.0, 8.0, 1.0, 1.0));
    }

    #[test]
    fn darker_keeps_more_sites() {
        let count = |dark: f32| {
            (0..64)
                .flat_map(|y| (0..64).map(move |x| (x, y)))
                .filter(|&(x, y)| voronoi_stipple_ink(x, y, dark, 8.0, 1.2, 1.0))
                .count()
        };
        assert!(count(0.2) < count(0.9));
    }

    #[test]
    fn hash01_in_unit_interval() {
        for y in 0..16 {
            for x in 0..16 {
                let u = hash01(x, y, 1);
                assert!((0.0..1.0).contains(&u));
            }
        }
    }

    #[test]
    fn nearest_site_is_finite() {
        let (sx, sy, _, _) = nearest_site(10, 20, 8.0);
        assert!(sx.is_finite() && sy.is_finite());
    }
}
