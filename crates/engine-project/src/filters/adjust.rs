//! Photo-style adjustments: contrast, brightness, saturation, blur, sharpness, noise.
//!
//! Order: point ops (contrast → brightness → saturation) → blur → unsharp → noise.
//! Alpha is copied from the source.
//!
//! Blur/sharpness use an in-tile radius larger than [`HALO`] so the sliders are
//! visible (tile-edge clamp; seams possible at very high blur). Noise is coarse
//! so it still reads after a dither filter on top.

use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;
/// Slider `blur` 0..=2 maps to this many pixels (in-tile clamp).
const BLUR_PX_PER_UNIT: f32 = 12.0;
const MAX_BLUR_RADIUS: i32 = 24;
/// Unsharp radius in px (in-tile).
const SHARP_RADIUS: i32 = 2;
/// Same grain for this many pixels so dither does not eat the noise.
const NOISE_CELL: u32 = 4;

#[inline]
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Hash → roughly uniform [0, 1).
fn hash01(x: u32, y: u32, ch: u32) -> f32 {
    let mut n = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(ch.wrapping_mul(1274126177));
    n = (n ^ (n >> 13)).wrapping_mul(1274126177);
    n ^= n >> 16;
    (n as f32) / (u32::MAX as f32)
}

fn box_blur_rgb(src: &PixelTile, radius: i32) -> PixelTile {
    let w = TILE_FULL_SIZE as i32;
    let h = TILE_FULL_SIZE as i32;
    let radius = radius.clamp(1, MAX_BLUR_RADIUS);
    let diam = (2 * radius + 1) as f32;
    let mut tmp = PixelTile::new();
    let mut out = PixelTile::new();

    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for dx in -radius..=radius {
                let sx = (x + dx).clamp(0, w - 1) as u32;
                for c in 0..3u32 {
                    acc[c as usize] += src.at(sx, y as u32, c);
                }
            }
            for c in 0..3u32 {
                tmp.set(x as u32, y as u32, c, acc[c as usize] / diam);
            }
            tmp.set(x as u32, y as u32, 3, src.at(x as u32, y as u32, 3));
        }
    }

    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for dy in -radius..=radius {
                let sy = (y + dy).clamp(0, h - 1) as u32;
                for c in 0..3u32 {
                    acc[c as usize] += tmp.at(x as u32, sy, c);
                }
            }
            for c in 0..3u32 {
                out.set(x as u32, y as u32, c, acc[c as usize] / diam);
            }
            out.set(x as u32, y as u32, 3, src.at(x as u32, y as u32, 3));
        }
    }
    out
}

fn blur_radius_px(blur: f32) -> i32 {
    if blur <= 1e-4 {
        return 0;
    }
    (blur * BLUR_PX_PER_UNIT)
        .round()
        .clamp(1.0, MAX_BLUR_RADIUS as f32) as i32
}

fn premultiply_rgb(tile: &mut PixelTile) {
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let a = tile.at(x, y, 3);
            for c in 0..3u32 {
                tile.set(x, y, c, tile.at(x, y, c) * a);
            }
        }
    }
}

fn unpremultiply_rgb(tile: &mut PixelTile) {
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let a = tile.at(x, y, 3);
            if a <= 1e-6 {
                tile.set(x, y, 0, 0.0);
                tile.set(x, y, 1, 0.0);
                tile.set(x, y, 2, 0.0);
            } else {
                for c in 0..3u32 {
                    tile.set(x, y, c, (tile.at(x, y, c) / a).clamp(0.0, 1.0));
                }
            }
        }
    }
}

/// Apply adjustments. Identity when all params are 0.
pub fn apply_adjust(
    tile: &PixelTile,
    coord: TileCoord,
    contrast: f32,
    brightness: f32,
    saturation: f32,
    blur: f32,
    sharpness: f32,
    noise: f32,
) -> PixelTile {
    let mut out = PixelTile::new();
    apply_adjust_into(
        tile, coord, contrast, brightness, saturation, blur, sharpness, noise, &mut out,
    );
    out
}

/// Adjust into an existing buffer.
///
/// Point ops / noise write directly to `dst`. Blur/sharpness still use internal
/// `PixelTile` temps (Wave 3 exception — see tile-memory-inplace SPEC).
pub fn apply_adjust_into(
    tile: &PixelTile,
    coord: TileCoord,
    contrast: f32,
    brightness: f32,
    saturation: f32,
    blur: f32,
    sharpness: f32,
    noise: f32,
    dst: &mut PixelTile,
) {
    let contrast_m = 1.0 + contrast;
    let sat_m = if saturation >= 0.0 {
        1.0 + saturation * 2.0
    } else {
        1.0 + saturation
    };

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let mut r = tile.at(x, y, 0);
            let mut g = tile.at(x, y, 1);
            let mut b = tile.at(x, y, 2);
            let a = tile.at(x, y, 3);

            r = (r - 0.5) * contrast_m + 0.5 + brightness;
            g = (g - 0.5) * contrast_m + 0.5 + brightness;
            b = (b - 0.5) * contrast_m + 0.5 + brightness;

            let lum = luminance(r, g, b);
            r = lum + (r - lum) * sat_m;
            g = lum + (g - lum) * sat_m;
            b = lum + (b - lum) * sat_m;

            dst.set(x, y, 0, r.clamp(0.0, 1.0));
            dst.set(x, y, 1, g.clamp(0.0, 1.0));
            dst.set(x, y, 2, b.clamp(0.0, 1.0));
            dst.set(x, y, 3, a);
            if a <= 1e-6 {
                dst.set(x, y, 0, 0.0);
                dst.set(x, y, 1, 0.0);
                dst.set(x, y, 2, 0.0);
            }
        }
    }

    let blur_r = blur_radius_px(blur);
    if blur_r > 0 || sharpness > 1e-6 {
        premultiply_rgb(dst);
    }
    if blur_r > 0 {
        let blurred = box_blur_rgb(dst, blur_r);
        dst.copy_from(&blurred);
    }

    if sharpness > 1e-6 {
        let sharp_r = blur_r.max(SHARP_RADIUS);
        let blurred = box_blur_rgb(dst, sharp_r);
        let amount = sharpness * 1.5;
        let mut sharpened = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    let orig = dst.at(x, y, c);
                    let low = blurred.at(x, y, c);
                    sharpened.set(x, y, c, (orig + amount * (orig - low)).clamp(0.0, 1.0));
                }
                sharpened.set(x, y, 3, dst.at(x, y, 3));
            }
        }
        dst.copy_from(&sharpened);
    }
    if blur_r > 0 || sharpness > 1e-6 {
        unpremultiply_rgb(dst);
    }

    if noise > 1e-6 {
        let origin_x = coord.x as u32 * TILE_SIZE;
        let origin_y = coord.y as u32 * TILE_SIZE;
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn uniform(v: f32) -> PixelTile {
        let mut t = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                t.set(x, y, 0, v);
                t.set(x, y, 1, v);
                t.set(x, y, 2, v);
                t.set(x, y, 3, 1.0);
            }
        }
        t
    }

    fn coord() -> TileCoord {
        TileCoord { level: 0, x: 0, y: 0 }
    }

    #[test]
    fn identity_is_noop() {
        let tile = uniform(0.4);
        let out = apply_adjust(&tile, coord(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(out.data, tile.data);
    }

    #[test]
    fn brightness_raises_gray() {
        let tile = uniform(0.4);
        let out = apply_adjust(&tile, coord(), 0.0, 0.2, 0.0, 0.0, 0.0, 0.0);
        let v = out.at(HALO + 4, HALO + 4, 0);
        assert!((v - 0.6).abs() < 1e-5);
        assert_eq!(out.at(HALO + 4, HALO + 4, 3), 1.0);
    }

    #[test]
    fn saturation_minus_one_is_grayscale() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                tile.set(x, y, 0, 0.8);
                tile.set(x, y, 1, 0.1);
                tile.set(x, y, 2, 0.1);
                tile.set(x, y, 3, 1.0);
            }
        }
        let out = apply_adjust(&tile, coord(), 0.0, 0.0, -1.0, 0.0, 0.0, 0.0);
        let r = out.at(HALO, HALO, 0);
        let g = out.at(HALO, HALO, 1);
        let b = out.at(HALO, HALO, 2);
        assert!((r - g).abs() < 1e-5);
        assert!((g - b).abs() < 1e-5);
    }

    #[test]
    fn saturation_boost_increases_chroma() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                tile.set(x, y, 0, 0.7);
                tile.set(x, y, 1, 0.3);
                tile.set(x, y, 2, 0.3);
                tile.set(x, y, 3, 1.0);
            }
        }
        let out = apply_adjust(&tile, coord(), 0.0, 0.0, 1.0, 0.0, 0.0, 0.0);
        let dr0 = 0.7 - 0.3;
        let dr1 = out.at(HALO, HALO, 0) - out.at(HALO, HALO, 1);
        assert!(dr1 > dr0 + 0.1);
    }

    #[test]
    fn blur_softens_checker() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let v = if (x / 2 + y / 2) % 2 == 0 { 1.0 } else { 0.0 };
                tile.set(x, y, 0, v);
                tile.set(x, y, 1, v);
                tile.set(x, y, 2, v);
                tile.set(x, y, 3, 1.0);
            }
        }
        let out = apply_adjust(&tile, coord(), 0.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        let v = out.at(HALO + 8, HALO + 8, 0);
        assert!(v > 0.15 && v < 0.85, "expected mid-gray after blur, got {v}");
    }

    #[test]
    fn noise_is_deterministic() {
        let tile = uniform(0.5);
        let a = apply_adjust(&tile, coord(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.3);
        let b = apply_adjust(&tile, coord(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.3);
        assert_eq!(a.data, b.data);
        assert_ne!(a.data, tile.data);
    }

    #[test]
    fn sharpness_boosts_step_edge() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let v = if x < TILE_FULL_SIZE / 2 { 0.2 } else { 0.8 };
                tile.set(x, y, 0, v);
                tile.set(x, y, 1, v);
                tile.set(x, y, 2, v);
                tile.set(x, y, 3, 1.0);
            }
        }
        let ident = apply_adjust(&tile, coord(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        let sharp = apply_adjust(&tile, coord(), 0.0, 0.0, 0.0, 0.0, 2.0, 0.0);
        let x0 = TILE_FULL_SIZE / 2 - 1;
        let x1 = TILE_FULL_SIZE / 2;
        let d0 = (ident.at(x1, HALO + 8, 0) - ident.at(x0, HALO + 8, 0)).abs();
        let d1 = (sharp.at(x1, HALO + 8, 0) - sharp.at(x0, HALO + 8, 0)).abs();
        assert!(d1 > d0 + 0.02, "sharp {d1} vs ident {d0}");
    }
}
