//! Detail — clarity + sharpness (spatial Post; split out of Adjust).
//!
//! Order: clarity (midtone local contrast) → unsharp. Defaults above the
//! dither barrier so sharpening does not rewrite the dither pattern unless
//! the user moves the layer under.

use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

use super::adjust::{
    box_blur_rgb, box_blur_rgb_with_neighbors, premultiply_rgb, unpremultiply_rgb,
};
use super::clarity::{apply_clarity_into, CLARITY_RADIUS};
use super::neighbors::current_spatial_neighbors;

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;
const SHARP_RADIUS: i32 = 2;

/// Neighbor gather radius for a Detail instance.
pub fn detail_spatial_radius_px(clarity: f32, sharpness: f32) -> i32 {
    let mut r = 0;
    if clarity.abs() > 1e-6 {
        r = r.max(CLARITY_RADIUS);
    }
    if sharpness > 1e-6 {
        r = r.max(SHARP_RADIUS);
    }
    r
}

/// Apply clarity then sharpness into `dst`. Identity when both are 0.
pub fn apply_detail_into(
    src: &PixelTile,
    coord: TileCoord,
    clarity: f32,
    sharpness: f32,
    dst: &mut PixelTile,
) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let clarity = clarity.clamp(-1.0, 1.0);
    let sharpness = sharpness.clamp(0.0, 2.0);
    if clarity.abs() < 1e-6 && sharpness <= 1e-6 {
        dst.copy_from(src);
        return;
    }

    if clarity.abs() > 1e-6 {
        apply_clarity_into(src, coord, clarity, dst);
    } else {
        dst.copy_from(src);
    }

    if sharpness <= 1e-6 {
        return;
    }

    let neighbors = current_spatial_neighbors();
    let cross = neighbors.filter(|n| n.complete);
    premultiply_rgb(dst);
    let blurred = if let Some(n) = cross {
        box_blur_rgb_with_neighbors(dst, coord, SHARP_RADIUS, n)
    } else {
        box_blur_rgb(dst, SHARP_RADIUS)
    };
    let amount = sharpness * 1.5;
    let mut sharpened = PixelTile::new();
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            for c in 0..3u32 {
                let orig = dst.at(x, y, c);
                let low = blurred.at(x, y, c);
                sharpened.set(x, y, c, (orig + amount * (orig - low)).clamp(0.0, 1.0));
            }
            sharpened.set(x, y, 3, dst.at(x, y, 3));
        }
    }
    dst.copy_from(&sharpened);
    unpremultiply_rgb(dst);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
                let v = if (x + y) % 2 == 0 { 0.35 } else { 0.65 };
                t.set(x, y, 0, v);
                t.set(x, y, 1, v);
                t.set(x, y, 2, v);
                t.set(x, y, 3, 1.0);
            }
        }
        t
    }

    #[test]
    fn identity_is_noop() {
        let src = checker();
        let mut dst = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        apply_detail_into(&src, coord, 0.0, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn sharpness_boosts_step_edge() {
        let src = checker();
        let mut dst = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        apply_detail_into(&src, coord, 0.0, 1.0, &mut dst);
        let dark = dst.at(HALO, HALO, 0);
        let light = dst.at(HALO + 1, HALO, 0);
        let src_dark = src.at(HALO, HALO, 0);
        let src_light = src.at(HALO + 1, HALO, 0);
        assert!((light - dark) > (src_light - src_dark) + 1e-4);
    }
}
