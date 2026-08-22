//! Soft glow / bloom filter (CPU).
//!
//! v1: separable box blur with radius capped to [`HALO`] so the existing tile
//! halo is sufficient — no multi-tile gather.
//!
//! Alpha policy: blur/composite RGB only; alpha is copied from the source tile.

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

/// Rec. 709 luminance.
#[inline]
fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Apply glow: threshold bright mask → box blur (radius ≤ HALO) → add to source.
///
/// Deterministic: no RNG. Same input + params → same output.
pub fn apply_glow(tile: &PixelTile, radius: f32, intensity: f32, threshold: f32) -> PixelTile {
    let mut result = PixelTile::new();
    apply_glow_into(tile, radius, intensity, threshold, &mut result);
    result
}

/// Glow into an existing buffer. Mask blur still uses a small scratch Vec;
/// the output tile itself is not allocated.
pub fn apply_glow_into(
    tile: &PixelTile,
    radius: f32,
    intensity: f32,
    threshold: f32,
    dst: &mut PixelTile,
) {
    let r_px = radius.round().clamp(1.0, HALO as f32) as i32;

    let mut mask = vec![0.0f32; (TILE_FULL_SIZE * TILE_FULL_SIZE * 3) as usize];
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let r = tile.at(x, y, 0);
            let g = tile.at(x, y, 1);
            let b = tile.at(x, y, 2);
            let lum = luminance(r, g, b);
            let idx = ((y * TILE_FULL_SIZE + x) * 3) as usize;
            if lum >= threshold {
                mask[idx] = r;
                mask[idx + 1] = g;
                mask[idx + 2] = b;
            }
        }
    }

    let blurred = box_blur_separable(&mask, r_px);

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let idx = ((y * TILE_FULL_SIZE + x) * 3) as usize;
            for c in 0..3u32 {
                let src = tile.at(x, y, c);
                let glow = blurred[idx + c as usize] * intensity;
                dst.set(x, y, c, (src + glow).clamp(0.0, 1.0));
            }
            dst.set(x, y, 3, tile.at(x, y, 3));
        }
    }
}

fn box_blur_separable(src: &[f32], radius: i32) -> Vec<f32> {
    let w = TILE_FULL_SIZE as i32;
    let h = TILE_FULL_SIZE as i32;
    let mut tmp = vec![0.0f32; src.len()];
    let mut out = vec![0.0f32; src.len()];
    let diam = (2 * radius + 1) as f32;

    // Horizontal
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for dx in -radius..=radius {
                let sx = (x + dx).clamp(0, w - 1) as u32;
                let idx = ((y as u32 * TILE_FULL_SIZE + sx) * 3) as usize;
                acc[0] += src[idx];
                acc[1] += src[idx + 1];
                acc[2] += src[idx + 2];
            }
            let o = ((y as u32 * TILE_FULL_SIZE + x as u32) * 3) as usize;
            tmp[o] = acc[0] / diam;
            tmp[o + 1] = acc[1] / diam;
            tmp[o + 2] = acc[2] / diam;
        }
    }

    // Vertical
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for dy in -radius..=radius {
                let sy = (y + dy).clamp(0, h - 1) as u32;
                let idx = ((sy * TILE_FULL_SIZE + x as u32) * 3) as usize;
                acc[0] += tmp[idx];
                acc[1] += tmp[idx + 1];
                acc[2] += tmp[idx + 2];
            }
            let o = ((y as u32 * TILE_FULL_SIZE + x as u32) * 3) as usize;
            out[o] = acc[0] / diam;
            out[o + 1] = acc[1] / diam;
            out[o + 2] = acc[2] / diam;
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
        // Core pixels of a flat field should be uniform (no seam artifact inside one tile).
        let v0 = out.at(HALO + 10, HALO + 10, 0);
        for y in (HALO + 5)..(HALO + 20) {
            for x in (HALO + 5)..(HALO + 20) {
                assert!((out.at(x, y, 0) - v0).abs() < 1e-5);
            }
        }
    }
}
