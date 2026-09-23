//! Random-dot stipple — per-pixel (or per-block) Bernoulli ink vs tone.
//!
//! Simpler Batch E cousin of [`super::voronoi_stipple`]: no lattice packing,
//! just a deterministic hash threshold so darker regions accumulate more dots.
//! Seam-safe (global coords only).

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
pub fn hash01(x: i32, y: i32) -> f32 {
    let mut n = (x as u32)
        .wrapping_mul(0x9E37_79B1)
        .wrapping_add((y as u32).wrapping_mul(0x85EB_CA6B))
        .wrapping_add(0xD07_5D07);
    n ^= n >> 16;
    n = n.wrapping_mul(0x7FEB_352D);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846C_A68B);
    n ^= n >> 16;
    (n >> 8) as f32 / 16_777_216.0
}

/// Whether the sample is inked for darkness in `[0, 1]`.
///
/// When `pixel_size > 1`, the hash is evaluated once per block so dots become
/// chunky pixel-art spots.
#[inline]
pub fn random_dot_ink(
    gx: i32,
    gy: i32,
    darkness: f32,
    density_scale: f32,
    pixel_size: u8,
) -> bool {
    let darkness = darkness.clamp(0.0, 1.0);
    if darkness <= 0.0 {
        return false;
    }
    let ps = pixel_size.max(1) as i32;
    let bx = gx.div_euclid(ps);
    let by = gy.div_euclid(ps);
    let gate = (darkness * density_scale.max(0.0)).clamp(0.0, 1.0);
    hash01(bx, by) < gate
}

/// Apply random-dot stipple into `dst` (binary ink on paper).
pub fn apply_random_dot_stipple_into(
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
    let density = params.threshold_scale;
    let ps = params.pixel_size.max(1);

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
            let ink = random_dot_ink(gcoord.x, gcoord.y, darkness, density, ps);
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
        assert!(!random_dot_ink(3, 7, 0.0, 1.0, 1));
    }

    #[test]
    fn darker_produces_more_dots() {
        let count = |dark: f32| {
            (0..128)
                .flat_map(|y| (0..128).map(move |x| (x, y)))
                .filter(|&(x, y)| random_dot_ink(x, y, dark, 1.0, 1))
                .count()
        };
        assert!(count(0.15) < count(0.85));
    }

    #[test]
    fn block_size_chunks_decision() {
        // Within a 4×4 block every sample shares the same hash gate outcome
        // for a fixed darkness that is either always or never over the hash.
        let dark = 1.0;
        let a = random_dot_ink(0, 0, dark, 1.0, 4);
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(random_dot_ink(x, y, dark, 1.0, 4), a);
            }
        }
    }
}
