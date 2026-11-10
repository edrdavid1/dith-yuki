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

#[inline]
pub(crate) fn adjust_sat_m(saturation: f32) -> f32 {
    if saturation >= 0.0 {
        1.0 + saturation * 2.0
    } else {
        1.0 + saturation
    }
}

/// Pointwise Adjust leg (contrast → brightness → saturation). Shared with P3 fusion.
#[inline]
pub(crate) fn apply_adjust_point_rgba(
    mut r: f32,
    mut g: f32,
    mut b: f32,
    a: f32,
    contrast_m: f32,
    brightness: f32,
    sat_m: f32,
) -> (f32, f32, f32, f32) {
    r = (r - 0.5) * contrast_m + 0.5 + brightness;
    g = (g - 0.5) * contrast_m + 0.5 + brightness;
    b = (b - 0.5) * contrast_m + 0.5 + brightness;

    let lum = luminance(r, g, b);
    r = lum + (r - lum) * sat_m;
    g = lum + (g - lum) * sat_m;
    b = lum + (b - lum) * sat_m;

    if a <= 1e-6 {
        (0.0, 0.0, 0.0, a)
    } else {
        (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0), a)
    }
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
    box_blur_rgb_sampled(src, radius, |sx, sy, c| {
        let w = TILE_FULL_SIZE as i32;
        let h = TILE_FULL_SIZE as i32;
        let sx = sx.clamp(0, w - 1) as u32;
        let sy = sy.clamp(0, h - 1) as u32;
        src.at(sx, sy, c)
    })
}

/// Separable box blur; `sample(lx, ly, channel)` may read outside the tile.
fn box_blur_rgb_sampled(
    src: &PixelTile,
    radius: i32,
    sample: impl Fn(i32, i32, u32) -> f32,
) -> PixelTile {
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
                for c in 0..3u32 {
                    acc[c as usize] += sample(x + dx, y, c);
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
                for c in 0..3u32 {
                    // Vertical pass reads the horizontal temp; out-of-range still
                    // uses `sample` only for the source alpha path — tmp is local.
                    let sy = (y + dy).clamp(0, h - 1);
                    acc[c as usize] += tmp.at(x as u32, sy as u32, c);
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

fn box_blur_rgb_with_neighbors(
    src: &PixelTile,
    coord: TileCoord,
    radius: i32,
    neighbors: &crate::filters::neighbors::SpatialNeighborhood,
) -> PixelTile {
    // Expand horizontal pass across tiles via global sampling. Vertical pass
    // stays on the temp tile (same as in-tile blur); for R ≪ TILE_SIZE the
    // dominant seam is horizontal/vertical edge of the *source*, which the
    // first pass already fixed. For full cross-tile vertical we sample src
    // neighbors into an expanded approach: both passes use global sample of
    // a virtually premultiplied field — rebuild via per-tap global read of src.
    let radius = radius.clamp(1, MAX_BLUR_RADIUS);
    let diam = (2 * radius + 1) as f32;
    let w = TILE_FULL_SIZE as i32;
    let h = TILE_FULL_SIZE as i32;
    let mut tmp = PixelTile::new();
    let mut out = PixelTile::new();

    let sample_src = |lx: i32, ly: i32, c: u32| -> f32 {
        if lx >= 0 && ly >= 0 && lx < w && ly < h {
            return src.at(lx as u32, ly as u32, c);
        }
        let gx = coord.x as i32 * TILE_SIZE as i32 + lx - HALO as i32;
        let gy = coord.y as i32 * TILE_SIZE as i32 + ly - HALO as i32;
        neighbors.sample_rgba(gx, gy)[c as usize]
    };

    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for dx in -radius..=radius {
                for c in 0..3u32 {
                    acc[c as usize] += sample_src(x + dx, y, c);
                }
            }
            for c in 0..3u32 {
                tmp.set(x as u32, y as u32, c, acc[c as usize] / diam);
            }
            tmp.set(x as u32, y as u32, 3, src.at(x as u32, y as u32, 3));
        }
    }

    // Vertical: sample tmp locally when in-range; for out-of-range Y, recompute
    // the horizontal blur tap at (x, y+dy) from source neighbors (no tmp there).
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for dy in -radius..=radius {
                let sy = y + dy;
                if sy >= 0 && sy < h {
                    for c in 0..3u32 {
                        acc[c as usize] += tmp.at(x as u32, sy as u32, c);
                    }
                } else {
                    let mut row = [0.0f32; 3];
                    for dx in -radius..=radius {
                        for c in 0..3u32 {
                            row[c as usize] += sample_src(x + dx, sy, c);
                        }
                    }
                    for c in 0..3u32 {
                        acc[c as usize] += row[c as usize] / diam;
                    }
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
    if contrast == 0.0
        && brightness == 0.0
        && saturation == 0.0
        && blur == 0.0
        && sharpness == 0.0
        && noise == 0.0
    {
        dst.copy_from(tile);
        return;
    }
    let contrast_m = 1.0 + contrast;
    let sat_m = adjust_sat_m(saturation);

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let (r, g, b, a) = apply_adjust_point_rgba(
                tile.at(x, y, 0),
                tile.at(x, y, 1),
                tile.at(x, y, 2),
                tile.at(x, y, 3),
                contrast_m,
                brightness,
                sat_m,
            );
            dst.set(x, y, 0, r);
            dst.set(x, y, 1, g);
            dst.set(x, y, 2, b);
            dst.set(x, y, 3, a);
        }
    }

    let blur_r = blur_radius_px(blur);
    let neighbors = crate::filters::neighbors::current_spatial_neighbors();
    // Incomplete neighborhood: skip spatial (pointwise preview); upgrade later.
    let skip_spatial = matches!(neighbors, Some(n) if !n.complete);

    if !skip_spatial && (blur_r > 0 || sharpness > 1e-6) {
        premultiply_rgb(dst);
    }
    if !skip_spatial && blur_r > 0 {
        let blurred = if let Some(n) = neighbors.filter(|n| n.complete) {
            box_blur_rgb_with_neighbors(dst, coord, blur_r, n)
        } else {
            box_blur_rgb(dst, blur_r)
        };
        dst.copy_from(&blurred);
    }

    if !skip_spatial && sharpness > 1e-6 {
        let sharp_r = blur_r.max(SHARP_RADIUS);
        let blurred = if let Some(n) = neighbors.filter(|n| n.complete) {
            box_blur_rgb_with_neighbors(dst, coord, sharp_r, n)
        } else {
            box_blur_rgb(dst, sharp_r)
        };
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
    if !skip_spatial && (blur_r > 0 || sharpness > 1e-6) {
        unpremultiply_rgb(dst);
    }

    if noise > 1e-6 {
        let origin_x = coord.x * TILE_SIZE;
        let origin_y = coord.y * TILE_SIZE;
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
        TileCoord {
            level: 0,
            x: 0,
            y: 0,
        }
    }

    #[test]
    fn identity_is_noop() {
        let tile = uniform(0.4);
        let out = apply_adjust(&tile, coord(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(out.data, tile.data);
    }

    #[test]
    fn identity_is_noop_bitwise_matrix() {
        let c = coord();
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let v = f32::from_bits(
                    0x3f000000_u32
                        | ((x as u32).wrapping_mul(2654435761)
                            ^ (y as u32).wrapping_mul(2246822519))
                            & 0x007fffff,
                );
                let a = if (x + y) % 5 == 0 {
                    0.0
                } else if (x + y) % 3 == 0 {
                    0.35
                } else {
                    1.0
                };
                tile.set(x, y, 0, v);
                tile.set(x, y, 1, v * 0.5);
                tile.set(x, y, 2, 1.0 - v * 0.25);
                tile.set(x, y, 3, a);
            }
        }
        let out = apply_adjust(&tile, c, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert_eq!(
            out.data, tile.data,
            "all-zero Adjust must be bitwise identity (skip-safe)"
        );
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
        assert!(
            v > 0.15 && v < 0.85,
            "expected mid-gray after blur, got {v}"
        );
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
