//! Clarity — midtone local-contrast Post filter (spatial).
//!
//! Unsharp mask at a fixed mid frequency, weighted so shadows/highlights stay
//! mostly untouched. Identity at `amount == 0.0`. Uses Dithered neighbors when
//! the Post neighborhood is complete; otherwise in-tile clamp.

use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

use super::adjust::{
    box_blur_rgb, box_blur_rgb_with_neighbors, premultiply_rgb, unpremultiply_rgb,
};
use super::neighbors::current_spatial_neighbors;

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;
/// Mid-frequency radius (px). Within [`super::adjust`] max blur / neighbor span.
pub(crate) const CLARITY_RADIUS: i32 = 12;
const AMOUNT_SCALE: f32 = 1.5;

#[inline]
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Midtone weight: 0 at black/white, 1 at L=0.5.
#[inline]
fn midtone_weight(lum: f32) -> f32 {
    (4.0 * lum * (1.0 - lum)).clamp(0.0, 1.0)
}

/// Apply clarity into `dst`. `src` and `dst` must not alias.
pub fn apply_clarity_into(src: &PixelTile, coord: TileCoord, amount: f32, dst: &mut PixelTile) {
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());
    let amount = amount.clamp(-1.0, 1.0);
    if amount.abs() < 1e-6 {
        dst.copy_from(src);
        return;
    }

    dst.copy_from(src);
    premultiply_rgb(dst);

    let neighbors = current_spatial_neighbors();
    let cross = neighbors.filter(|n| n.complete);
    let blurred = if let Some(n) = cross {
        box_blur_rgb_with_neighbors(dst, coord, CLARITY_RADIUS, n)
    } else {
        box_blur_rgb(dst, CLARITY_RADIUS)
    };

    let gain = amount * AMOUNT_SCALE;
    let mut out = PixelTile::new();
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let r0 = src.at(x, y, 0);
            let g0 = src.at(x, y, 1);
            let b0 = src.at(x, y, 2);
            let w = midtone_weight(luminance(r0, g0, b0));
            for c in 0..3u32 {
                let orig = dst.at(x, y, c);
                let low = blurred.at(x, y, c);
                out.set(x, y, c, (orig + gain * w * (orig - low)).clamp(0.0, 1.0));
            }
            out.set(x, y, 3, dst.at(x, y, 3));
        }
    }
    dst.copy_from(&out);
    unpremultiply_rgb(dst);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(r: f32, g: f32, b: f32) -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL {
            for x in 0..TILE_FULL {
                t.set(x, y, 0, r);
                t.set(x, y, 1, g);
                t.set(x, y, 2, b);
                t.set(x, y, 3, 1.0);
            }
        }
        t
    }

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
    fn identity_is_noop_bitwise() {
        let src = checker();
        let mut dst = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        apply_clarity_into(&src, coord, 0.0, &mut dst);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn flat_mid_gray_unchanged() {
        let src = solid(0.5, 0.5, 0.5);
        let mut dst = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        apply_clarity_into(&src, coord, 0.8, &mut dst);
        let v = dst.at(HALO, HALO, 0);
        assert!((v - 0.5).abs() < 1e-4, "flat field must stay flat, got {v}");
    }

    #[test]
    fn positive_amount_increases_local_contrast() {
        let src = checker();
        let mut dst = PixelTile::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        apply_clarity_into(&src, coord, 0.9, &mut dst);
        let dark = dst.at(HALO, HALO, 0);
        let light = dst.at(HALO + 1, HALO, 0);
        let src_dark = src.at(HALO, HALO, 0);
        let src_light = src.at(HALO + 1, HALO, 0);
        assert!(
            (light - dark) > (src_light - src_dark) + 1e-4,
            "clarity should widen midtone checker contrast"
        );
    }
}
