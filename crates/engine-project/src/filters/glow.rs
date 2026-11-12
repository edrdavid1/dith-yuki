//! Soft glow / bloom filter (CPU).
//!
//! Threshold bright mask → N× separable box blur (≈ Gaussian when softness>1)
//! → add to source. Radius may exceed [`HALO`]; when a complete spatial
//! neighborhood is available we sample across tiles, otherwise clamp in-tile.
//!
//! Alpha policy: blur/composite RGB only; alpha is copied from the source tile.

use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

use super::neighbors::current_spatial_neighbors;

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;
const MAX_RADIUS: i32 = 16;
const MAX_SOFTNESS: u32 = 3;
/// Soft knee around threshold (matches GPU `glow*.wgsl`).
pub const THRESHOLD_KNEE: f32 = 0.05;

/// Rec. 709 luminance.
#[inline]
pub fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

#[inline]
pub fn soft_mask_weight(lum: f32, threshold: f32) -> f32 {
    if threshold <= 0.0 {
        return 1.0;
    }
    let lo = (threshold - THRESHOLD_KNEE).max(0.0);
    let hi = (threshold + THRESHOLD_KNEE).min(1.0);
    if hi <= lo {
        return if lum >= threshold { 1.0 } else { 0.0 };
    }
    let t = ((lum - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Effective gather radius for neighbor Post (px).
pub fn glow_spatial_radius_px(radius: f32, softness: u8) -> i32 {
    let r = radius.round().clamp(1.0, MAX_RADIUS as f32) as i32;
    let s = (softness as i32).clamp(1, MAX_SOFTNESS as i32);
    (r * s).min(MAX_RADIUS * MAX_SOFTNESS as i32)
}

/// Apply glow: threshold bright mask → box blur → add to source.
///
/// Deterministic: no RNG. Same input + params → same output.
pub fn apply_glow(tile: &PixelTile, radius: f32, intensity: f32, threshold: f32) -> PixelTile {
    let mut result = PixelTile::new();
    apply_glow_into(tile, radius, intensity, threshold, 1, &mut result);
    result
}

/// Glow into an existing buffer (coord 0,0 — tests / no neighbors).
pub fn apply_glow_into(
    tile: &PixelTile,
    radius: f32,
    intensity: f32,
    threshold: f32,
    softness: u8,
    dst: &mut PixelTile,
) {
    apply_glow_into_at(
        tile,
        TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        radius,
        intensity,
        threshold,
        softness,
        dst,
    );
}

/// Glow with tile coord so cross-tile neighbors can be sampled when available.
pub fn apply_glow_into_at(
    tile: &PixelTile,
    coord: TileCoord,
    radius: f32,
    intensity: f32,
    threshold: f32,
    softness: u8,
    dst: &mut PixelTile,
) {
    let r_px = radius.round().clamp(1.0, MAX_RADIUS as f32) as i32;
    let passes = (softness as u32).clamp(1, MAX_SOFTNESS);
    let intensity = intensity.clamp(0.0, 4.0);
    let threshold = threshold.clamp(0.0, 1.0);

    let neighbors = current_spatial_neighbors();
    let cross = neighbors.filter(|n| n.complete);

    let mask = if let Some(nb) = cross {
        build_and_blur_mask_with_neighbors(tile, coord, threshold, r_px, passes, nb)
    } else {
        let mut m = build_bright_mask_in_tile(tile, threshold);
        for _ in 0..passes {
            m = box_blur_rgb_plane(&m, r_px);
        }
        m
    };

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let idx = ((y * TILE_FULL_SIZE + x) * 3) as usize;
            for c in 0..3u32 {
                let src = tile.at(x, y, c);
                let glow = mask[idx + c as usize] * intensity;
                dst.set(x, y, c, (src + glow).clamp(0.0, 1.0));
            }
            dst.set(x, y, 3, tile.at(x, y, 3));
        }
    }
}

fn build_bright_mask_in_tile(tile: &PixelTile, threshold: f32) -> Vec<f32> {
    let mut mask = vec![0.0f32; (TILE_FULL_SIZE * TILE_FULL_SIZE * 3) as usize];
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let r = tile.at(x, y, 0);
            let g = tile.at(x, y, 1);
            let b = tile.at(x, y, 2);
            let w = soft_mask_weight(luminance(r, g, b), threshold);
            let idx = ((y * TILE_FULL_SIZE + x) * 3) as usize;
            mask[idx] = r * w;
            mask[idx + 1] = g * w;
            mask[idx + 2] = b * w;
        }
    }
    mask
}

fn build_and_blur_mask_with_neighbors(
    tile: &PixelTile,
    coord: TileCoord,
    threshold: f32,
    radius: i32,
    passes: u32,
    neighbors: &super::neighbors::SpatialNeighborhood,
) -> Vec<f32> {
    let radius = radius.clamp(1, MAX_RADIUS);
    let pad = radius * passes as i32;
    let w = TILE_FULL_SIZE as i32;
    let h = TILE_FULL_SIZE as i32;
    let bw = (w + 2 * pad) as usize;
    let bh = (h + 2 * pad) as usize;
    let mut field = vec![0.0f32; bw * bh * 3];

    for ly in -pad..(h + pad) {
        for lx in -pad..(w + pad) {
            let (r, g, b) = if lx >= 0 && ly >= 0 && lx < w && ly < h {
                (
                    tile.at(lx as u32, ly as u32, 0),
                    tile.at(lx as u32, ly as u32, 1),
                    tile.at(lx as u32, ly as u32, 2),
                )
            } else {
                let gx = coord.x as i32 * TILE_SIZE as i32 + lx - HALO as i32;
                let gy = coord.y as i32 * TILE_SIZE as i32 + ly - HALO as i32;
                let rgba = neighbors.sample_rgba(gx, gy);
                (rgba[0], rgba[1], rgba[2])
            };
            let weight = soft_mask_weight(luminance(r, g, b), threshold);
            let ix = (lx + pad) as usize;
            let iy = (ly + pad) as usize;
            let base = (iy * bw + ix) * 3;
            field[base] = r * weight;
            field[base + 1] = g * weight;
            field[base + 2] = b * weight;
        }
    }

    for _ in 0..passes {
        field = box_blur_plane_wh(&field, bw, bh, radius);
    }

    let mut out = vec![0.0f32; (TILE_FULL_SIZE * TILE_FULL_SIZE * 3) as usize];
    for y in 0..h {
        for x in 0..w {
            let sx = (x + pad) as usize;
            let sy = (y + pad) as usize;
            let src = (sy * bw + sx) * 3;
            let dst = ((y as u32 * TILE_FULL_SIZE + x as u32) * 3) as usize;
            out[dst] = field[src];
            out[dst + 1] = field[src + 1];
            out[dst + 2] = field[src + 2];
        }
    }
    out
}

/// Separable box blur over an RGB plane (`w*h*3`) with sliding-window sums.
fn box_blur_rgb_plane(src: &[f32], radius: i32) -> Vec<f32> {
    box_blur_plane_wh(
        src,
        TILE_FULL_SIZE as usize,
        TILE_FULL_SIZE as usize,
        radius,
    )
}

fn box_blur_plane_wh(src: &[f32], w: usize, h: usize, radius: i32) -> Vec<f32> {
    let radius = radius.clamp(1, MAX_RADIUS);
    let diam = (2 * radius + 1) as f32;
    let wi = w as i32;
    let hi = h as i32;
    let mut tmp = vec![0.0f32; src.len()];
    let mut out = vec![0.0f32; src.len()];
    let mut prefix = vec![0.0f32; w.max(h) + 1];

    // Horizontal via prefix sums (clamp-to-edge).
    for y in 0..h {
        for c in 0..3usize {
            prefix[0] = 0.0;
            for x in 0..w {
                prefix[x + 1] = prefix[x] + src[(y * w + x) * 3 + c];
            }
            for x in 0..wi {
                let lo = (x - radius).clamp(0, wi - 1) as usize;
                let hi_x = (x + radius).clamp(0, wi - 1) as usize;
                // Expand edge taps so the divisor stays `diam` under clamp.
                let mut sum = prefix[hi_x + 1] - prefix[lo];
                let left_extra = (radius - x).max(0);
                let right_extra = (x + radius - (wi - 1)).max(0);
                if left_extra > 0 {
                    sum += src[(y * w + 0) * 3 + c] * left_extra as f32;
                }
                if right_extra > 0 {
                    sum += src[(y * w + (w - 1)) * 3 + c] * right_extra as f32;
                }
                tmp[(y * w + x as usize) * 3 + c] = sum / diam;
            }
        }
    }

    // Vertical via prefix sums (clamp-to-edge).
    for x in 0..w {
        for c in 0..3usize {
            prefix[0] = 0.0;
            for y in 0..h {
                prefix[y + 1] = prefix[y] + tmp[(y * w + x) * 3 + c];
            }
            for y in 0..hi {
                let lo = (y - radius).clamp(0, hi - 1) as usize;
                let hi_y = (y + radius).clamp(0, hi - 1) as usize;
                let mut sum = prefix[hi_y + 1] - prefix[lo];
                let top_extra = (radius - y).max(0);
                let bot_extra = (y + radius - (hi - 1)).max(0);
                if top_extra > 0 {
                    sum += tmp[(0 * w + x) * 3 + c] * top_extra as f32;
                }
                if bot_extra > 0 {
                    sum += tmp[((h - 1) * w + x) * 3 + c] * bot_extra as f32;
                }
                out[(y as usize * w + x) * 3 + c] = sum / diam;
            }
        }
    }
    out
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

    #[test]
    fn glow_deterministic_and_preserves_alpha() {
        let tile = uniform(0.6);
        let a = apply_glow(&tile, 2.0, 1.0, 0.0);
        let b = apply_glow(&tile, 2.0, 1.0, 0.0);
        assert_eq!(a.data, b.data);
        assert_eq!(a.at(10, 10, 3), 1.0);
    }

    #[test]
    fn glow_flat_field_uniform_core() {
        let tile = uniform(0.5);
        let out = apply_glow(&tile, 2.0, 0.5, 0.0);
        let v0 = out.at(HALO + 10, HALO + 10, 0);
        for y in (HALO + 5)..(HALO + 20) {
            for x in (HALO + 5)..(HALO + 20) {
                assert!((out.at(x, y, 0) - v0).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn softness_increases_spread() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let v = if x == HALO + 32 && y == HALO + 32 {
                    0.8
                } else {
                    0.0
                };
                tile.set(x, y, 0, v);
                tile.set(x, y, 1, v);
                tile.set(x, y, 2, v);
                tile.set(x, y, 3, 1.0);
            }
        }
        let mut soft1 = PixelTile::new();
        let mut soft3 = PixelTile::new();
        // Low intensity so additive glow is not clamped at the peak.
        apply_glow_into(&tile, 2.0, 0.5, 0.0, 1, &mut soft1);
        apply_glow_into(&tile, 2.0, 0.5, 0.0, 3, &mut soft3);
        let peak1 = soft1.at(HALO + 32, HALO + 32, 0);
        let peak3 = soft3.at(HALO + 32, HALO + 32, 0);
        assert!(peak3 < peak1, "peak1={peak1} peak3={peak3}");
        let mid1 = soft1.at(HALO + 32 + 4, HALO + 32, 0);
        let mid3 = soft3.at(HALO + 32 + 4, HALO + 32, 0);
        assert!(mid3 > mid1, "mid1={mid1} mid3={mid3}");
    }
}
