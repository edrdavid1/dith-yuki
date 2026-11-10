//! Blur + grain noise — spatial Post filter (split out of Adjust).

use engine_tiles::{PixelTile, TileCoord, TILE_SIZE};

use super::adjust::{
    box_blur_rgb, box_blur_rgb_with_neighbors, premultiply_rgb, unpremultiply_rgb,
};
use super::neighbors::current_spatial_neighbors;

const TILE_FULL: u32 = engine_tiles::TILE_SIZE + 2 * engine_tiles::HALO;
const BLUR_PX_PER_UNIT: f32 = 12.0;
const MAX_BLUR_RADIUS: i32 = 24;
const NOISE_CELL: u32 = 4;

fn blur_radius_px(blur: f32) -> i32 {
    if blur <= 1e-4 {
        return 0;
    }
    (blur * BLUR_PX_PER_UNIT)
        .round()
        .clamp(1.0, MAX_BLUR_RADIUS as f32) as i32
}

fn hash01(x: u32, y: u32, ch: u32) -> f32 {
    let mut n = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(ch.wrapping_mul(1274126177));
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^= n >> 16;
    (n as f32) / (u32::MAX as f32)
}

/// Blur then noise into `dst`. Identity when both are 0.
pub fn apply_blur_noise_into(
    src: &PixelTile,
    coord: TileCoord,
    blur: f32,
    noise: f32,
    dst: &mut PixelTile,
) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let blur = blur.clamp(0.0, 2.0);
    let noise = noise.clamp(0.0, 1.0);
    if blur <= 1e-4 && noise <= 1e-6 {
        dst.copy_from(src);
        return;
    }

    dst.copy_from(src);
    let blur_r = blur_radius_px(blur);
    let neighbors = current_spatial_neighbors();
    let cross = neighbors.filter(|n| n.complete);

    if blur_r > 0 {
        premultiply_rgb(dst);
        let blurred = if let Some(n) = cross {
            box_blur_rgb_with_neighbors(dst, coord, blur_r, n)
        } else {
            box_blur_rgb(dst, blur_r)
        };
        dst.copy_from(&blurred);
        unpremultiply_rgb(dst);
    }

    if noise > 1e-6 {
        let origin_x = coord.x * TILE_SIZE;
        let origin_y = coord.y * TILE_SIZE;
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
                let gx = origin_x.wrapping_add(x);
                let gy = origin_y.wrapping_add(y);
                let cx = gx / NOISE_CELL;
                let cy = gy / NOISE_CELL;
                let n = hash01(cx, cy, 0) * 2.0 - 1.0;
                for c in 0..3u32 {
                    let v = dst.at(x, y, c) + n * noise;
                    dst.set(x, y, c, v.clamp(0.0, 1.0));
                }
            }
        }
    }
}

/// Radius in px for neighbor gather (0 if no blur).
pub fn blur_noise_spatial_radius_px(blur: f32) -> i32 {
    blur_radius_px(blur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_tiles::HALO;

    fn solid(v: f32) -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
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
        let src = solid(0.4);
        let mut dst = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        apply_blur_noise_into(&src, coord, 0.0, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn noise_is_deterministic() {
        let src = solid(0.5);
        let coord = TileCoord {
            level: 0,
            x: 1,
            y: 2,
        };
        let mut a = PixelTile::new();
        let mut b = PixelTile::new();
        apply_blur_noise_into(&src, coord, 0.0, 0.4, &mut a);
        apply_blur_noise_into(&src, coord, 0.0, 0.4, &mut b);
        assert_eq!(a.data, b.data);
        assert!(
            (a.at(HALO, HALO, 0) - 0.5).abs() > 1e-6
                || (a.at(HALO + 4, HALO, 0) - 0.5).abs() > 1e-6
        );
    }
}
