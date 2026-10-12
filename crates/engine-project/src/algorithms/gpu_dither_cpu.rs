//! CPU reference for GPU dither families.
//!
//! Geometry from `.local-doc/Dither GPU-аналоги ED — схема реализации.md` §1.1:
//! workgroup 64×64 = 56×56 core + 8px left/top warm-up. Fixed-point only.
//! Defines the canonical bytes that the GPU path must match.

use crate::algorithms::blue_noise_texture::BlueNoiseTexture;
use crate::algorithms::fixed_point::{
    blue_noise_shift_q8, clamp_adjusted_q8, distribute_fs, q8_to_u8_rounded, u8_to_q8, MAX_U8_Q8,
};
use crate::error::EngineError;
use crate::filter::DitherColorMode;

/// Workgroup / padded-tile side (64×64).
pub const WG_SIZE: u32 = 64;
/// Left/top warm-up (§1.1).
pub const PAD: u32 = 8;
/// Central written region: `WG_SIZE - PAD` = 56.
pub const CORE: u32 = WG_SIZE - PAD;
/// Legacy alias — step size equals core, not the padded workgroup.
pub const TILE: u32 = CORE;
/// Default pad (warm-up).
pub const DEFAULT_PAD: u32 = PAD;
/// Blue-noise texture side.
pub const BLUE_NOISE_SIZE: usize = 128;
/// Default threshold spread in u8 color units.
pub const DEFAULT_SPREAD: u32 = 256;

/// Packed RGBA8 pixel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba8 {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

/// Display-referred palette entry (RGB only; alpha ignored for distance).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaletteRgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Max channel steps for a synthetic RGB cube (`16³ = 4096`). Larger `levels`
/// values are clamped so the GPU storage buffer stays bounded.
pub const MAX_SYNTHETIC_CUBE_LEVELS: u16 = 16;

/// Build a levels palette matching CPU ED’s uniform quantizer when no document
/// palette is bound. RGB → L³ cube (nearest ≡ per-channel uniform); Grayscale →
/// L-step ramp. Never hard-codes bilevel unless `levels == 2` and grayscale.
pub fn levels_palette(levels: u16, color_mode: DitherColorMode) -> Vec<PaletteRgb> {
    let levels = levels.clamp(2, 256);
    match color_mode {
        DitherColorMode::Grayscale => {
            let n = levels as u32;
            let denom = (n - 1).max(1);
            (0..n)
                .map(|i| {
                    let v = ((i * 255) / denom) as u8;
                    PaletteRgb { r: v, g: v, b: v }
                })
                .collect()
        }
        DitherColorMode::Rgb => {
            let n = u32::from(levels.min(MAX_SYNTHETIC_CUBE_LEVELS));
            let denom = (n - 1).max(1);
            let mut out = Vec::with_capacity((n * n * n) as usize);
            for ir in 0..n {
                let r = ((ir * 255) / denom) as u8;
                for ig in 0..n {
                    let g = ((ig * 255) / denom) as u8;
                    for ib in 0..n {
                        let b = ((ib * 255) / denom) as u8;
                        out.push(PaletteRgb { r, g, b });
                    }
                }
            }
            out
        }
    }
}

/// Euclidean nearest palette color in display-referred RGB (§9).
pub fn nearest_palette(color: (u8, u8, u8), palette: &[PaletteRgb]) -> Result<(u8, u8, u8), EngineError> {
    if palette.is_empty() {
        return Err(EngineError::invalid_filter_params("InvalidPalette: empty"));
    }
    if palette.len() == 1 {
        let c = palette[0];
        return Ok((c.r, c.g, c.b));
    }
    let (cr, cg, cb) = color;
    let mut best_i = 0usize;
    let mut best_d = u32::MAX;
    for (i, p) in palette.iter().enumerate() {
        let dr = cr as i32 - p.r as i32;
        let dg = cg as i32 - p.g as i32;
        let db = cb as i32 - p.b as i32;
        let d = (dr * dr + dg * dg + db * db) as u32;
        if d < best_d {
            best_d = d;
            best_i = i;
        }
    }
    let p = palette[best_i];
    Ok((p.r, p.g, p.b))
}

fn read_clamped(input: &[Rgba8], w: usize, h: usize, x: i32, y: i32) -> Rgba8 {
    let cx = x.clamp(0, w.saturating_sub(1) as i32) as usize;
    let cy = y.clamp(0, h.saturating_sub(1) as i32) as usize;
    input[cy * w + cx]
}

/// ED-style mega-pixel grid: sample the top-left of each `ps×ps` block, then
/// expand the dithered block grid back to full resolution.
///
/// Matches `docs/tile-pipeline.md` §6 (“Block representative”). Does **not**
/// change tile warm-up / Q8.8 arithmetic — only the sampling lattice.
/// `ps <= 1` is a no-op (bit-identical to running `f` on `input`).
pub fn with_pixel_size_blocks<F>(
    input: &[Rgba8],
    width: u32,
    height: u32,
    pixel_size: u32,
    f: F,
) -> Result<Vec<Rgba8>, EngineError>
where
    F: FnOnce(&[Rgba8], u32, u32) -> Result<Vec<Rgba8>, EngineError>,
{
    let ps = pixel_size.max(1);
    let w = width as usize;
    let h = height as usize;
    if input.len() != w * h {
        return Err(EngineError::invalid_filter_params(
            "pixel_size blocks: buffer size",
        ));
    }
    if ps <= 1 {
        return f(input, width, height);
    }
    let bw = width.div_ceil(ps);
    let bh = height.div_ceil(ps);
    let mut small = Vec::with_capacity((bw * bh) as usize);
    for by in 0..bh {
        for bx in 0..bw {
            let x = (bx * ps) as usize;
            let y = (by * ps) as usize;
            small.push(input[y * w + x]);
        }
    }
    let dithered = f(&small, bw, bh)?;
    if dithered.len() != (bw * bh) as usize {
        return Err(EngineError::invalid_filter_params(
            "pixel_size blocks: dither size mismatch",
        ));
    }
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let by = (y as u32 / ps) as usize;
        for x in 0..w {
            let bx = (x as u32 / ps) as usize;
            let p = dithered[by * (bw as usize) + bx];
            // Alpha comes from the source pixel (ED keeps alpha; block color from rep).
            let src = input[y * w + x];
            out.push(Rgba8::new(p.r, p.g, p.b, src.a));
        }
    }
    Ok(out)
}

/// Approximate blocked Floyd–Steinberg with left/top warm-up.
///
/// `tile_w`/`tile_h` are the **core** stride (default [`CORE`]). Each tile
/// processes a `(tile_w+pad)×(tile_h+pad)` region; only the core is written.
pub fn blocked_fs_cpu(
    input: &[Rgba8],
    width: u32,
    height: u32,
    palette: &[PaletteRgb],
    tile_w: u32,
    tile_h: u32,
    pad: u32,
) -> Result<Vec<Rgba8>, EngineError> {
    if palette.is_empty() {
        return Err(EngineError::invalid_filter_params("InvalidPalette: empty"));
    }
    let w = width as usize;
    let h = height as usize;
    if input.len() != w * h {
        return Err(EngineError::invalid_filter_params("blocked_fs: buffer size"));
    }
    let mut output = input.to_vec();
    let tile_w = tile_w.max(1) as usize;
    let tile_h = tile_h.max(1) as usize;
    let pad = pad as i32;

    let mut ty = 0usize;
    while ty < h {
        let mut tx = 0usize;
        while tx < w {
            let x0 = tx as i32 - pad;
            let y0 = ty as i32 - pad;
            let x1 = (tx + tile_w) as i32;
            let y1 = (ty + tile_h) as i32;
            let pw = (x1 - x0) as usize;
            let ph = (y1 - y0) as usize;

            let mut buffer: Vec<[u8; 3]> = vec![[0; 3]; pw * ph];
            let mut alpha: Vec<u8> = vec![255; pw * ph];
            for ly in 0..ph {
                for lx in 0..pw {
                    let px = read_clamped(input, w, h, x0 + lx as i32, y0 + ly as i32);
                    buffer[ly * pw + lx] = [px.r, px.g, px.b];
                    alpha[ly * pw + lx] = px.a;
                }
            }

            for ch in 0..3 {
                let mut err = vec![0i32; (ph + 2) * (pw + 2)];
                let stride = pw + 2;
                let t_max = (pw.saturating_sub(1)) + 2 * (ph.saturating_sub(1));
                for t in 0..=t_max {
                    for y in 0..ph {
                        let x_i = t as i32 - 2 * y as i32;
                        if x_i < 0 || x_i >= pw as i32 {
                            continue;
                        }
                        let x = x_i as usize;
                        let ei = (y + 1) * stride + (x + 1);
                        let v_q8 = u8_to_q8(buffer[y * pw + x][ch]);
                        let adjusted = v_q8 + err[ei];
                        let clamped = clamp_adjusted_q8(adjusted);
                        let color_u8 = q8_to_u8_rounded(clamped);
                        // Nearest palette uses full RGB with this channel replaced.
                        let mut rgb = buffer[y * pw + x];
                        rgb[ch] = color_u8;
                        let (qr, qg, qb) = nearest_palette((rgb[0], rgb[1], rgb[2]), palette)?;
                        let quantized = [qr, qg, qb][ch];
                        let error = clamped - u8_to_q8(quantized);
                        debug_assert!(error.abs() <= 65536);
                        let (e7, e3, e5, e1) = distribute_fs(error);
                        err[ei + 1] += e7;
                        if x > 0 {
                            err[ei + stride - 1] += e3;
                        }
                        err[ei + stride] += e5;
                        err[ei + stride + 1] += e1;
                        buffer[y * pw + x][ch] = quantized;
                    }
                }
            }

            // Re-quantize final RGB jointly so output uses palette only.
            for ly in 0..ph {
                for lx in 0..pw {
                    let (qr, qg, qb) = nearest_palette(
                        (
                            buffer[ly * pw + lx][0],
                            buffer[ly * pw + lx][1],
                            buffer[ly * pw + lx][2],
                        ),
                        palette,
                    )?;
                    buffer[ly * pw + lx] = [qr, qg, qb];
                }
            }

            let copy_w = tile_w.min(w - tx);
            let copy_h = tile_h.min(h - ty);
            for ly in 0..copy_h {
                for lx in 0..copy_w {
                    let sx = (pad as usize) + lx;
                    let sy = (pad as usize) + ly;
                    let rgb = buffer[sy * pw + sx];
                    output[(ty + ly) * w + (tx + lx)] = Rgba8::new(
                        rgb[0],
                        rgb[1],
                        rgb[2],
                        alpha[sy * pw + sx],
                    );
                }
            }

            tx += tile_w;
        }
        ty += tile_h;
    }
    let _ = MAX_U8_Q8;
    Ok(output)
}

/// Threshold-shift blue noise (§3.2.2).
pub fn blue_noise_cpu(
    input: &[Rgba8],
    width: u32,
    height: u32,
    palette: &[PaletteRgb],
    texture: &[u16],
    spread: u32,
) -> Result<Vec<Rgba8>, EngineError> {
    if palette.is_empty() {
        return Err(EngineError::invalid_filter_params("InvalidPalette: empty"));
    }
    if texture.len() != BLUE_NOISE_SIZE * BLUE_NOISE_SIZE {
        return Err(EngineError::invalid_filter_params(
            "blue_noise: texture must be 128×128 u16",
        ));
    }
    let w = width as usize;
    let h = height as usize;
    if input.len() != w * h {
        return Err(EngineError::invalid_filter_params("blue_noise: buffer size"));
    }
    let mut output = Vec::with_capacity(input.len());
    for y in 0..h {
        for x in 0..w {
            let px = input[y * w + x];
            let rank = texture[(y % BLUE_NOISE_SIZE) * BLUE_NOISE_SIZE + (x % BLUE_NOISE_SIZE)];
            let shift = blue_noise_shift_q8(rank, spread);
            let mut shifted = [0u8; 3];
            for (i, ch) in [px.r, px.g, px.b].into_iter().enumerate() {
                let shifted_q8 = clamp_adjusted_q8(u8_to_q8(ch) + shift);
                shifted[i] = q8_to_u8_rounded(shifted_q8);
            }
            let (qr, qg, qb) = nearest_palette((shifted[0], shifted[1], shifted[2]), palette)?;
            output.push(Rgba8::new(qr, qg, qb, px.a));
        }
    }
    Ok(output)
}

/// 128×128 u16 blue-noise ranks from the committed void-and-cluster table.
pub fn blue_noise_ranks() -> &'static [u16] {
    BlueNoiseTexture::embedded().as_slice()
}

/// Same as [`blue_noise_ranks`], exposed for call sites that want the texture API.
pub fn embedded_blue_noise_ranks() -> &'static [u16] {
    blue_noise_ranks()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_ramp(w: usize, h: usize) -> Vec<Rgba8> {
        let mut v = Vec::with_capacity(w * h);
        let denom = (w.max(2) - 1) as u32;
        for _y in 0..h {
            for x in 0..w {
                let g = ((x as u32 * 255) / denom) as u8;
                v.push(Rgba8::new(g, g, g, 255));
            }
        }
        v
    }

    #[test]
    fn blue_noise_zero_color_palette_errors() {
        let input = vec![Rgba8::new(128, 128, 128, 255); 4];
        let tex = blue_noise_ranks();
        let err = blue_noise_cpu(&input, 2, 2, &[], tex, DEFAULT_SPREAD).unwrap_err();
        assert!(err.to_string().contains("InvalidPalette"));
    }

    #[test]
    fn blue_noise_one_color_palette() {
        let input = gray_ramp(32, 32);
        let pal = [PaletteRgb {
            r: 40,
            g: 80,
            b: 120,
        }];
        let out = blue_noise_cpu(&input, 32, 32, &pal, blue_noise_ranks(), DEFAULT_SPREAD).unwrap();
        assert!(out.iter().all(|p| p.r == 40 && p.g == 80 && p.b == 120));
    }

    #[test]
    fn blue_noise_spread_zero_equals_posterize() {
        let input = gray_ramp(64, 64);
        let pal = [
            PaletteRgb { r: 0, g: 0, b: 0 },
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255,
            },
        ];
        let out = blue_noise_cpu(&input, 64, 64, &pal, blue_noise_ranks(), 0).unwrap();
        for (i, px) in input.iter().enumerate() {
            let (qr, qg, qb) = nearest_palette((px.r, px.g, px.b), &pal).unwrap();
            assert_eq!(out[i], Rgba8::new(qr, qg, qb, px.a));
        }
    }

    #[test]
    fn blue_noise_ranks_are_permutation() {
        let ranks = blue_noise_ranks();
        assert_eq!(ranks.len(), 16384);
        let mut seen = vec![false; 16384];
        for &r in ranks {
            assert!((r as usize) < 16384);
            assert!(!seen[r as usize], "duplicate rank {r}");
            seen[r as usize] = true;
        }
    }

    #[test]
    fn blue_noise_texture_hash() {
        use sha2::{Digest, Sha256};
        let ranks = blue_noise_ranks();
        let mut bytes = Vec::with_capacity(ranks.len() * 2);
        for &r in ranks {
            bytes.extend_from_slice(&r.to_le_bytes());
        }
        let digest = Sha256::digest(&bytes);
        let hex = format!("{digest:x}");
        let expected = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/blue_noise_128_u16.sha256"
        ))
        .split_whitespace()
        .next()
        .expect("sha256 file");
        assert_eq!(hex, expected, "committed blue-noise ranks hash mismatch");
    }

    /// Golden hash of CPU blue-noise output on a fixed 256×256 ramp + 6-color palette.
    #[test]
    fn blue_noise_output_hash_stable() {
        use sha2::{Digest, Sha256};
        let input = gray_ramp(256, 256);
        let palette = [
            PaletteRgb { r: 0, g: 0, b: 0 },
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255,
            },
            PaletteRgb {
                r: 255,
                g: 0,
                b: 0,
            },
            PaletteRgb {
                r: 0,
                g: 255,
                b: 0,
            },
            PaletteRgb {
                r: 0,
                g: 0,
                b: 255,
            },
            PaletteRgb {
                r: 128,
                g: 128,
                b: 128,
            },
        ];
        let output = blue_noise_cpu(
            &input,
            256,
            256,
            &palette,
            BlueNoiseTexture::embedded().as_slice(),
            DEFAULT_SPREAD,
        )
        .unwrap();
        let mut bytes = Vec::with_capacity(output.len() * 4);
        for px in &output {
            bytes.extend_from_slice(&[px.r, px.g, px.b, px.a]);
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        assert_eq!(
            hash, "98ca5f93dd4f4f931b70687a52309b0618c73af557107eb5277ba50896fdef79",
            "blue_noise_cpu golden hash changed — update only if the VAC texture or formula intentionally changed"
        );
    }

    #[test]
    fn blocked_fs_geometry_is_56_plus_8() {
        assert_eq!(CORE, 56);
        assert_eq!(PAD, 8);
        assert_eq!(WG_SIZE, 64);
        assert_eq!(CORE + PAD, WG_SIZE);
    }

    #[test]
    fn blocked_fs_output_size_matches() {
        let input = gray_ramp(96, 80);
        let pal = [
            PaletteRgb { r: 0, g: 0, b: 0 },
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255,
            },
        ];
        let out = blocked_fs_cpu(&input, 96, 80, &pal, CORE, CORE, PAD).unwrap();
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn blocked_fs_uses_palette_only() {
        let input = gray_ramp(48, 48);
        let pal = [
            PaletteRgb { r: 0, g: 0, b: 0 },
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255,
            },
            PaletteRgb {
                r: 128,
                g: 128,
                b: 128,
            },
        ];
        let out = blocked_fs_cpu(&input, 48, 48, &pal, CORE, CORE, PAD).unwrap();
        for px in &out {
            assert!(
                pal.iter()
                    .any(|p| p.r == px.r && p.g == px.g && p.b == px.b)
            );
        }
    }

    /// Golden hash — 3×3 cores (168×168) with 6-color palette (§6.1 / §5).
    #[test]
    fn blocked_fs_golden_hash_3x3_tiles() {
        use sha2::{Digest, Sha256};
        let side = (CORE * 3) as usize; // 168
        let input = gray_ramp(side, side);
        let palette = [
            PaletteRgb { r: 0, g: 0, b: 0 },
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255,
            },
            PaletteRgb {
                r: 255,
                g: 0,
                b: 0,
            },
            PaletteRgb {
                r: 0,
                g: 255,
                b: 0,
            },
            PaletteRgb {
                r: 0,
                g: 0,
                b: 255,
            },
            PaletteRgb {
                r: 128,
                g: 128,
                b: 128,
            },
        ];
        let output = blocked_fs_cpu(
            &input,
            side as u32,
            side as u32,
            &palette,
            CORE,
            CORE,
            PAD,
        )
        .unwrap();
        let mut bytes = Vec::with_capacity(output.len() * 4);
        for px in &output {
            bytes.extend_from_slice(&[px.r, px.g, px.b, px.a]);
        }
        let hash = format!("{:x}", Sha256::digest(&bytes));
        // Captured once for geometry 56+8; bump only on intentional algorithm change.
        assert_eq!(
            hash, "c5ba99f8fc2af14d27d9d7b936b97d47349fa5674ac33c8d0517d00328364062",
            "blocked_fs golden hash changed — update only on intentional geometry/algorithm change"
        );
    }

    #[test]
    fn alpha_preserved_blue_noise() {
        let mut input = gray_ramp(16, 16);
        input[3].a = 77;
        let pal = [PaletteRgb {
            r: 0,
            g: 0,
            b: 0,
        }];
        let out = blue_noise_cpu(&input, 16, 16, &pal, blue_noise_ranks(), 128).unwrap();
        assert_eq!(out[3].a, 77);
    }

    #[test]
    fn determinism_cpu_blocked_fs() {
        let input = gray_ramp(70, 70);
        let pal = [
            PaletteRgb { r: 0, g: 0, b: 0 },
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255,
            },
        ];
        let a = blocked_fs_cpu(&input, 70, 70, &pal, CORE, CORE, PAD).unwrap();
        let b = blocked_fs_cpu(&input, 70, 70, &pal, CORE, CORE, PAD).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn levels_palette_rgb_default_is_not_bilevel() {
        use crate::filter::DitherColorMode;
        // Default Levels=4 → 4³ = 64 colors (CPU ED uniform), not black/white.
        let pal = levels_palette(4, DitherColorMode::Rgb);
        assert_eq!(pal.len(), 64);
        assert!(pal.iter().any(|c| c.r != c.g || c.g != c.b));
    }

    #[test]
    fn levels_palette_grayscale_two_is_bilevel() {
        use crate::filter::DitherColorMode;
        let pal = levels_palette(2, DitherColorMode::Grayscale);
        assert_eq!(pal.len(), 2);
        assert_eq!(pal[0], PaletteRgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            pal[1],
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255
            }
        );
    }
}
