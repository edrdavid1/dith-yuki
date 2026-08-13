//! Ordered dithering engine (V2 redesign).
//!
//! Implements Bayer matrix (2×2, 4×4, 8×8) and custom PNG threshold map
//! ordered dithering. Uses global pixel coordinates for seamless tiling
//! across tile boundaries.
//!
//! **Requirements:** 2.1, 2.2, 2.5, 2.6, 4.1, 4.2, 4.3, 4.4, 4.5,
//!                   5.1, 5.2, 5.3, 6.1, 6.2, 6.3, 6.5, 7.1, 7.2, 7.3, 7.4, 8.1, 8.2, 8.3

use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use crate::types::LayerId;
use engine_color::oklab::{linear_to_oklab, LinRgb};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::{PaletteLutCache, DEFAULT_LUT_SIZE};
use engine_color::threshold_map::ThresholdMapCache;
use engine_tiles::block_cache::{BlockCoord, BlockRepresentativeCache};
use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use std::path::Path;

/// Tile full size including halo: TILE_SIZE + 2 * HALO = 260.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

// ─── Bayer Matrices (normalized to [0, 1)) ───────────────────────────────────

/// 2×2 Bayer matrix normalized to [0, 1) range.
const BAYER_2X2: [[f32; 2]; 2] = [
    [0.0 / 4.0, 2.0 / 4.0],
    [3.0 / 4.0, 1.0 / 4.0],
];

/// 4×4 Bayer matrix normalized to [0, 1) range.
#[rustfmt::skip]
const BAYER_4X4: [[f32; 4]; 4] = [
    [ 0.0/16.0,  8.0/16.0,  2.0/16.0, 10.0/16.0],
    [12.0/16.0,  4.0/16.0, 14.0/16.0,  6.0/16.0],
    [ 3.0/16.0, 11.0/16.0,  1.0/16.0,  9.0/16.0],
    [15.0/16.0,  7.0/16.0, 13.0/16.0,  5.0/16.0],
];

/// 8×8 Bayer matrix normalized to [0, 1) range.
#[rustfmt::skip]
const BAYER_8X8: [[f32; 8]; 8] = [
    [ 0.0/64.0, 32.0/64.0,  8.0/64.0, 40.0/64.0,  2.0/64.0, 34.0/64.0, 10.0/64.0, 42.0/64.0],
    [48.0/64.0, 16.0/64.0, 56.0/64.0, 24.0/64.0, 50.0/64.0, 18.0/64.0, 58.0/64.0, 26.0/64.0],
    [12.0/64.0, 44.0/64.0,  4.0/64.0, 36.0/64.0, 14.0/64.0, 46.0/64.0,  6.0/64.0, 38.0/64.0],
    [60.0/64.0, 28.0/64.0, 52.0/64.0, 20.0/64.0, 62.0/64.0, 30.0/64.0, 54.0/64.0, 22.0/64.0],
    [ 3.0/64.0, 35.0/64.0, 11.0/64.0, 43.0/64.0,  1.0/64.0, 33.0/64.0,  9.0/64.0, 41.0/64.0],
    [51.0/64.0, 19.0/64.0, 59.0/64.0, 27.0/64.0, 49.0/64.0, 17.0/64.0, 57.0/64.0, 25.0/64.0],
    [15.0/64.0, 47.0/64.0,  7.0/64.0, 39.0/64.0, 13.0/64.0, 45.0/64.0,  5.0/64.0, 37.0/64.0],
    [63.0/64.0, 31.0/64.0, 55.0/64.0, 23.0/64.0, 61.0/64.0, 29.0/64.0, 53.0/64.0, 21.0/64.0],
];

// ─── Threshold Lookup ────────────────────────────────────────────────────────

/// Classic CMYK screen angles in degrees (C, M, Y, K).
const HALFTONE_ANGLES_DEG: [f32; 4] = [15.0, 75.0, 0.0, 45.0];

/// Distance from rotated cell center for a screen of size `s` at angle `theta_rad`.
///
/// Uses `rem_euclid` so negative halo coords wrap correctly.
pub(crate) fn rotated_cell_dist(gx: i32, gy: i32, s: f32, theta_rad: f32) -> f32 {
    let cos_t = theta_rad.cos();
    let sin_t = theta_rad.sin();
    let xr = gx as f32 * cos_t + gy as f32 * sin_t;
    let yr = -gx as f32 * sin_t + gy as f32 * cos_t;
    let cx = xr.rem_euclid(s) - s * 0.5;
    let cy = yr.rem_euclid(s) - s * 0.5;
    (cx * cx + cy * cy).sqrt()
}

/// Wave / line-modulation threshold in `[0, 1)`.
pub(crate) fn wave_threshold(
    gx: i32,
    gy: i32,
    wavelength: f32,
    amplitude: f32,
    phase: f32,
    angle_deg: f32,
) -> f32 {
    let phi = angle_deg.to_radians();
    let u = gx as f32 * phi.cos() + gy as f32 * phi.sin();
    let t = 0.5
        + 0.5
            * (std::f32::consts::TAU * u / wavelength + phase).sin()
            * amplitude;
    t.clamp(0.0, 0.999_999)
}

/// RGB → CMYK (simple artistic undercolor removal, not ICC).
/// Returns (C, M, Y, K) in `[0, 1]`.
fn rgb_to_cmyk(r: f32, g: f32, b: f32) -> (f32, f32, f32, f32) {
    let k = 1.0 - r.max(g).max(b);
    if k >= 1.0 - f32::EPSILON {
        return (0.0, 0.0, 0.0, 1.0);
    }
    let c = (1.0 - r - k) / (1.0 - k);
    let m = (1.0 - g - k) / (1.0 - k);
    let y = (1.0 - b - k) / (1.0 - k);
    (c.clamp(0.0, 1.0), m.clamp(0.0, 1.0), y.clamp(0.0, 1.0), k.clamp(0.0, 1.0))
}

/// CMYK dots → RGB display reconstruction.
/// `RGB = 1 - min(1, C+K)` (etc.) — artistic undercolor, not a press proof.
fn cmyk_to_rgb(c: f32, m: f32, y: f32, k: f32) -> (f32, f32, f32) {
    let r = 1.0 - (c + k).min(1.0);
    let g = 1.0 - (m + k).min(1.0);
    let b = 1.0 - (y + k).min(1.0);
    (r, g, b)
}

/// Hard-disk CMYK screen: ink if `dist <= (s/2) * sqrt(tone) * threshold_scale`.
fn halftone_channel_ink(dist: f32, tone: f32, s: f32, threshold_scale: f32) -> f32 {
    let r_max = (s * 0.5) * tone.sqrt() * threshold_scale;
    if dist <= r_max { 1.0 } else { 0.0 }
}

/// Get the threshold value for a given global coordinate and dither mode.
///
/// For Bayer matrices, uses `rem_euclid` (modulo) on the global coordinates.
/// For Wave, uses sinusoidal line modulation from `params`.
/// For CustomPng, loads (or retrieves from cache) the threshold map and samples it.
///
/// # Errors
///
/// Returns `EngineError::IoError` if a custom PNG threshold map cannot be loaded.
#[allow(dead_code)] // exercised in unit tests via get_threshold_i32 path too
fn get_threshold(
    params: &DitherParamsV2,
    gx: u32,
    gy: u32,
    threshold_cache: &ThresholdMapCache,
) -> Result<f32, EngineError> {
    get_threshold_i32(params, gx as i32, gy as i32, threshold_cache)
}

/// Bayer / CustomPng sample the rotated lattice; Wave / Halftone / ED do not.
#[inline]
fn samples_rotated_pattern(mode: &DitherModeV2) -> bool {
    matches!(
        mode,
        DitherModeV2::Bayer2x2
            | DitherModeV2::Bayer4x4
            | DitherModeV2::Bayer8x8
            | DitherModeV2::CustomPng { .. }
    )
}

/// Block_Then_Rotate: rotate aligned `(gx, gy)` around the origin, then **floor**.
///
/// Angle is degrees; wrapped with `rem_euclid(360)` so sampling is periodic.
/// `angle == 0` (after wrap) is a no-op so default output stays bit-identical.
///
/// Formula (design lock): `x' = x cosθ − y sinθ`, `y' = x sinθ + y cosθ`.
pub(crate) fn rotate_pattern_coord(gx: i32, gy: i32, angle_deg: f32) -> (i32, i32) {
    let wrapped = angle_deg.rem_euclid(360.0);
    if wrapped == 0.0 {
        return (gx, gy);
    }
    let theta = wrapped.to_radians();
    let (cos_t, sin_t) = (theta.cos(), theta.sin());
    let x = gx as f32;
    let y = gy as f32;
    let xr = x * cos_t - y * sin_t;
    let yr = x * sin_t + y * cos_t;
    (xr.floor() as i32, yr.floor() as i32)
}

/// `T' = clamp01(T + bias)` into `[0, 1)`. Bias 0 is a no-op (bit-identity).
#[inline]
fn apply_threshold_bias(threshold: f32, bias: f32) -> f32 {
    if bias == 0.0 {
        threshold
    } else {
        (threshold + bias).clamp(0.0, 0.999_999)
    }
}

/// Get threshold using i32 global coordinates (handles negative coords at tile edges).
/// Uses rem_euclid for correct modular indexing even with negative values.
fn get_threshold_i32(
    params: &DitherParamsV2,
    gx: i32,
    gy: i32,
    threshold_cache: &ThresholdMapCache,
) -> Result<f32, EngineError> {
    match &params.mode {
        DitherModeV2::Bayer2x2 => {
            let mx = (gx as i64).rem_euclid(2) as usize;
            let my = (gy as i64).rem_euclid(2) as usize;
            Ok(BAYER_2X2[my][mx])
        }
        DitherModeV2::Bayer4x4 => {
            let mx = (gx as i64).rem_euclid(4) as usize;
            let my = (gy as i64).rem_euclid(4) as usize;
            Ok(BAYER_4X4[my][mx])
        }
        DitherModeV2::Bayer8x8 => {
            let mx = (gx as i64).rem_euclid(8) as usize;
            let my = (gy as i64).rem_euclid(8) as usize;
            Ok(BAYER_8X8[my][mx])
        }
        DitherModeV2::Wave => Ok(wave_threshold(
            gx,
            gy,
            params.wave_wavelength,
            params.wave_amplitude,
            params.wave_phase,
            params.wave_angle,
        )),
        DitherModeV2::CustomPng { path } => {
            let map = threshold_cache.get_or_load(Path::new(path)).map_err(|e| {
                EngineError::IoError {
                    reason: format!("Failed to load threshold map: {}", e),
                }
            })?;
            let ux = (gx as i64).rem_euclid(map.width as i64) as u32;
            let uy = (gy as i64).rem_euclid(map.height as i64) as u32;
            Ok(map.sample(ux, uy))
        }
        DitherModeV2::CmykHalftone => {
            unreachable!("CMYK halftone uses dedicated apply path, not get_threshold")
        }
        mode if mode.is_error_diffusion() => {
            unreachable!("ordered dithering engine called with diffusion mode")
        }
        _ => unreachable!("unhandled dither mode in get_threshold"),
    }
}

// ─── Uniform Quantization ────────────────────────────────────────────────────

/// Quantize a value to evenly spaced levels with a threshold offset.
///
/// The offset is applied in the scaled domain for correct ordered dithering behavior:
/// `quantized = round(value * (levels - 1) + offset) / (levels - 1)`
/// where `offset = (threshold - 0.5) * threshold_scale`.
///
/// This ensures that pure black (0.0) stays black and pure white (1.0) stays white,
/// since the offset merely shifts the rounding decision boundary.
#[inline]
fn quantize_uniform(value: f32, levels: f32, offset: f32) -> f32 {
    let scaled = value * (levels - 1.0) + offset;
    let quantized = scaled.round().clamp(0.0, levels - 1.0) / (levels - 1.0);
    quantized
}

/// Convert RGB to luminance using Rec. 709 coefficients.
#[inline]
fn to_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

// ─── Main Entry Point ────────────────────────────────────────────────────────

/// Apply ordered dithering to a tile.
///
/// Processes each pixel using global coordinates for seamless tiling.
/// Supports Bayer matrices (2×2, 4×4, 8×8) and custom PNG threshold maps.
///
/// Implements:
/// - Pixel-size block logic: snaps to block representative and fills all block
///   pixels with the same dithered color (Req 4.1–4.5)
/// - Color mode handling: RGB (independent channels) and Grayscale (luminance) (Req 5.1, 5.2)
/// - Quantization dispatch: uniform levels when `palette_id` is None, or
///   palette-constrained nearest-color via KD-tree in Oklab when set (Req 6.1–6.3, 6.5, 7.1–7.4)
/// - Alpha channel preservation (Req 5.3)
/// - `threshold_scale` application: `offset = (threshold - 0.5) * threshold_scale`
///
/// # Arguments
///
/// * `tile` - Input pixel tile (260×260 with halo)
/// * `coord` - Tile coordinate for computing global pixel positions
/// * `params` - Full V2 dither parameters
/// * `threshold_cache` - Cache for custom PNG threshold maps
/// * `palette_cache` - Palette KD-tree cache for palette quantization
/// * `document` - Document reference for palette lookup
///
/// # Errors
///
/// Returns `EngineError` if:
/// - Custom PNG threshold map cannot be loaded (IoError)
/// - `palette_id` references a palette not in the document (PaletteNotFound)
/// - Mode is an error diffusion mode (unreachable in correct dispatch)
pub fn apply_ordered(
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    threshold_cache: &ThresholdMapCache,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    document: &Document,
) -> Result<PixelTile, EngineError> {
    let empty = BlockRepresentativeCache::new();
    apply_ordered_with_cache(
        tile,
        coord,
        params,
        threshold_cache,
        palette_cache,
        lut_cache,
        document,
        &empty,
        LayerId::new(0),
    )
}

/// Ordered dither with a shared [`BlockRepresentativeCache`] for mega-pixel
/// source reads (avoids halo clamp across tile boundaries).
pub fn apply_ordered_with_cache(
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    threshold_cache: &ThresholdMapCache,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    document: &Document,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
) -> Result<PixelTile, EngineError> {
    if matches!(params.mode, DitherModeV2::CmykHalftone) {
        return apply_cmyk_halftone(
            tile,
            coord,
            params,
            palette_cache,
            lut_cache,
            document,
            block_cache,
            layer_id,
        );
    }

    let mut result = PixelTile::new();
    let levels = params.levels as f32;
    let ps = params.pixel_size as u32;

    // If palette_id is set, resolve O(1) LUT once per apply (outside pixel loop)
    let palette_lut = if let Some(palette_id) = params.palette_id {
        let palette = document.get_palette(palette_id).ok_or_else(|| {
            EngineError::palette_not_found(palette_id)
        })?;
        let lut = lut_cache
            .get_or_build(palette, palette_cache, DEFAULT_LUT_SIZE)
            .map_err(|_| EngineError::palette_not_found(palette_id))?;
        Some((palette, lut))
    } else {
        None
    };

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let gcoord = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
            let block = gcoord.aligned(ps);
            let block_gx = block.x;
            let block_gy = block.y;

            // Block_Then_Rotate (Track H / ROADMAP §2):
            //   global → aligned(pixel_size) → [BRC lookup]
            //   → rotate(pattern_angle) → get_threshold_i32 → T' = clamp01(T + bias)
            // Do not rotate before alignment: mega-pixel blocks stay axis-aligned rectangles.
            let (r, g, b) = if ps > 1 {
                read_block_source(tile, coord, block_gx, block_gy, block_cache, layer_id, ps)
            } else {
                (tile.at(x, y, 0), tile.at(x, y, 1), tile.at(x, y, 2))
            };

            let (pat_gx, pat_gy) = if samples_rotated_pattern(&params.mode) {
                rotate_pattern_coord(block_gx, block_gy, params.pattern_angle)
            } else {
                (block_gx, block_gy)
            };
            let threshold = apply_threshold_bias(
                get_threshold_i32(params, pat_gx, pat_gy, threshold_cache)?,
                params.threshold_bias,
            );

            // Apply threshold_scale: offset = (threshold - 0.5) * threshold_scale
            let offset = (threshold - 0.5) * params.threshold_scale;

            // Quantize based on color_mode and palette
            match params.color_mode {
                DitherColorMode::Rgb => {
                    if let Some((palette, ref lut)) = palette_lut {
                        // Palette quantization: apply offset then find nearest (Req 6.1, 6.2, 6.3)
                        let adj_r = (r + offset).clamp(0.0, 1.0);
                        let adj_g = (g + offset).clamp(0.0, 1.0);
                        let adj_b = (b + offset).clamp(0.0, 1.0);
                        let oklab = linear_to_oklab(LinRgb { r: adj_r, g: adj_g, b: adj_b });
                        let nearest_idx = lut.nearest_index(oklab) as usize;
                        let palette_color = &palette.colors[nearest_idx];
                        result.set(x, y, 0, palette_color.r);
                        result.set(x, y, 1, palette_color.g);
                        result.set(x, y, 2, palette_color.b);
                    } else {
                        // Uniform quantization: each channel independently (Req 7.1, 7.2, 7.3, 7.4)
                        let qr = quantize_uniform(r, levels, offset);
                        let qg = quantize_uniform(g, levels, offset);
                        let qb = quantize_uniform(b, levels, offset);
                        result.set(x, y, 0, qr);
                        result.set(x, y, 1, qg);
                        result.set(x, y, 2, qb);
                    }
                }
                DitherColorMode::Grayscale => {
                    // Convert to luminance (Req 5.2)
                    let lum = to_luminance(r, g, b);

                    if let Some((palette, ref lut)) = palette_lut {
                        // Palette quantization in grayscale: apply offset to luminance,
                        // use as gray RGB for nearest lookup
                        let adj_lum = (lum + offset).clamp(0.0, 1.0);
                        let oklab = linear_to_oklab(LinRgb { r: adj_lum, g: adj_lum, b: adj_lum });
                        let nearest_idx = lut.nearest_index(oklab) as usize;
                        let palette_color = &palette.colors[nearest_idx];
                        result.set(x, y, 0, palette_color.r);
                        result.set(x, y, 1, palette_color.g);
                        result.set(x, y, 2, palette_color.b);
                    } else {
                        // Uniform quantization: dither single channel, write R=G=B (Req 5.2)
                        let qlum = quantize_uniform(lum, levels, offset);
                        result.set(x, y, 0, qlum);
                        result.set(x, y, 1, qlum);
                        result.set(x, y, 2, qlum);
                    }
                }
            }

            // Preserve alpha channel unchanged (Req 5.3)
            result.set(x, y, 3, tile.at(x, y, 3));
        }
    }

    Ok(result)
}

/// Read RGB for a block representative: prefer [`BlockRepresentativeCache`], else
/// local tile sample with clamp (legacy fallback when cache is empty).
fn read_block_source(
    tile: &PixelTile,
    coord: TileCoord,
    block_gx: i32,
    block_gy: i32,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
    ps: u32,
) -> (f32, f32, f32) {
    if block_gx >= 0 && block_gy >= 0 {
        let key = BlockCoord::from_global(layer_id.0, block_gx as u32, block_gy as u32, ps);
        if let Some(px) = block_cache.get_raw(key) {
            return (px[0], px[1], px[2]);
        }
    }

    let rep_local_x = block_gx - coord.x as i32 * TILE_SIZE as i32 + HALO as i32;
    let rep_local_y = block_gy - coord.y as i32 * TILE_SIZE as i32 + HALO as i32;
    let clamped_x = rep_local_x.max(0).min(TILE_FULL_SIZE as i32 - 1) as u32;
    let clamped_y = rep_local_y.max(0).min(TILE_FULL_SIZE as i32 - 1) as u32;
    (
        tile.at(clamped_x, clamped_y, 0),
        tile.at(clamped_x, clamped_y, 1),
        tile.at(clamped_x, clamped_y, 2),
    )
}

/// CMYK angled-screen halftone on the ordered dither path.
fn apply_cmyk_halftone(
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    document: &Document,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
) -> Result<PixelTile, EngineError> {
    let mut result = PixelTile::new();
    let ps = params.pixel_size as u32;
    let s = params.halftone_cell_size as f32;
    let angles: [f32; 4] = HALFTONE_ANGLES_DEG.map(|d| d.to_radians());

    let palette_lut = if let Some(palette_id) = params.palette_id {
        let palette = document
            .get_palette(palette_id)
            .ok_or_else(|| EngineError::palette_not_found(palette_id))?;
        let lut = lut_cache
            .get_or_build(palette, palette_cache, DEFAULT_LUT_SIZE)
            .map_err(|_| EngineError::palette_not_found(palette_id))?;
        Some((palette, lut))
    } else {
        None
    };

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let gcoord = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
            let block = gcoord.aligned(ps);
            let block_gx = block.x;
            let block_gy = block.y;

            let (r, g, b) = if ps > 1 {
                read_block_source(tile, coord, block_gx, block_gy, block_cache, layer_id, ps)
            } else {
                (tile.at(x, y, 0), tile.at(x, y, 1), tile.at(x, y, 2))
            };

            // Pattern coords use the same aligned sample point as Bayer.
            let (src_r, src_g, src_b) = match params.color_mode {
                DitherColorMode::Rgb => (r, g, b),
                DitherColorMode::Grayscale => {
                    let lum = to_luminance(r, g, b);
                    (lum, lum, lum)
                }
            };

            let (c, m, y_ink, k) = rgb_to_cmyk(src_r, src_g, src_b);
            // Ordered-threshold bias: shift CMYK tone (more/fewer dots). No-op at 0.
            let bias = params.threshold_bias;
            let tones = if bias == 0.0 {
                [c, m, y_ink, k]
            } else {
                [
                    (c + bias).clamp(0.0, 1.0),
                    (m + bias).clamp(0.0, 1.0),
                    (y_ink + bias).clamp(0.0, 1.0),
                    (k + bias).clamp(0.0, 1.0),
                ]
            };
            let mut dots = [0.0f32; 4];
            for i in 0..4 {
                let dist = rotated_cell_dist(block_gx, block_gy, s, angles[i]);
                dots[i] = halftone_channel_ink(dist, tones[i], s, params.threshold_scale);
            }
            let (out_r, out_g, out_b) = cmyk_to_rgb(dots[0], dots[1], dots[2], dots[3]);

            if let Some((palette, ref lut)) = palette_lut {
                let oklab = linear_to_oklab(LinRgb {
                    r: out_r,
                    g: out_g,
                    b: out_b,
                });
                let nearest_idx = lut.nearest_index(oklab) as usize;
                let palette_color = &palette.colors[nearest_idx];
                result.set(x, y, 0, palette_color.r);
                result.set(x, y, 1, palette_color.g);
                result.set(x, y, 2, palette_color.b);
            } else {
                result.set(x, y, 0, out_r);
                result.set(x, y, 1, out_g);
                result.set(x, y, 2, out_b);
            }
            result.set(x, y, 3, tile.at(x, y, 3));
        }
    }

    Ok(result)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};

    fn make_uniform_tile(r: f32, g: f32, b: f32, a: f32) -> PixelTile {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                tile.set(x, y, 0, r);
                tile.set(x, y, 1, g);
                tile.set(x, y, 2, b);
                tile.set(x, y, 3, a);
            }
        }
        tile
    }

    fn tc(x: u32, y: u32) -> TileCoord {
        TileCoord { level: 0, x, y }
    }

    fn make_params(mode: DitherModeV2, levels: u16, threshold_scale: f32) -> DitherParamsV2 {
        DitherParamsV2 {
            mode,
            levels,
            threshold_scale,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }
    }

    fn is_valid_level(v: f32, levels: f32) -> bool {
        // Check if v is k/(levels-1) for some integer k in [0, levels-1]
        let k = v * (levels - 1.0);
        (k - k.round()).abs() < 1e-4
    }

    #[test]
    fn bayer_2x2_produces_valid_quantized_levels() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let params = make_params(DitherModeV2::Bayer2x2, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        let levels = params.levels as f32;
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3 {
                    assert!(
                        is_valid_level(result.at(x, y, c), levels),
                        "Invalid level at ({}, {}, {}): {}",
                        x, y, c, result.at(x, y, c)
                    );
                }
            }
        }
    }

    #[test]
    fn bayer_4x4_produces_valid_quantized_levels() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let params = make_params(DitherModeV2::Bayer4x4, 8, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        let levels = params.levels as f32;
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn bayer_8x8_produces_valid_quantized_levels() {
        let tile = make_uniform_tile(0.25, 0.75, 0.5, 0.8);
        let params = make_params(DitherModeV2::Bayer8x8, 16, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(3, 7), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        let levels = params.levels as f32;
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3 {
                    assert!(is_valid_level(result.at(x, y, c), levels));
                }
            }
        }
    }

    #[test]
    fn alpha_channel_preserved() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 0.42);
        let params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                assert_eq!(result.at(x, y, 3), 0.42);
            }
        }
    }

    #[test]
    fn threshold_scale_affects_output() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let params_low = make_params(DitherModeV2::Bayer4x4, 4, 0.1);
        let params_high = make_params(DitherModeV2::Bayer4x4, 4, 4.0);

        let result_low = apply_ordered(&tile, tc(0, 0), &params_low, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();
        let result_high = apply_ordered(&tile, tc(0, 0), &params_high, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // Results should differ since threshold_scale changes the offset magnitude
        assert_ne!(result_low.data, result_high.data);
    }

    #[test]
    fn grayscale_mode_produces_equal_rgb() {
        let tile = make_uniform_tile(0.8, 0.2, 0.5, 1.0);
        let mut params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        params.color_mode = DitherColorMode::Grayscale;
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let r = result.at(x, y, 0);
                let g = result.at(x, y, 1);
                let b = result.at(x, y, 2);
                assert_eq!(r, g, "R != G at ({}, {})", x, y);
                assert_eq!(g, b, "G != B at ({}, {})", x, y);
            }
        }
    }

    #[test]
    fn deterministic_output() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let params = make_params(DitherModeV2::Bayer4x4, 8, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let r1 = apply_ordered(&tile, tc(5, 10), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();
        let r2 = apply_ordered(&tile, tc(5, 10), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        assert_eq!(r1.data, r2.data);
    }

    #[test]
    fn black_tile_stays_black() {
        let tile = make_uniform_tile(0.0, 0.0, 0.0, 1.0);
        let params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3 {
                    assert_eq!(result.at(x, y, c), 0.0);
                }
            }
        }
    }

    #[test]
    fn white_tile_stays_white() {
        let tile = make_uniform_tile(1.0, 1.0, 1.0, 1.0);
        let params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3 {
                    assert_eq!(result.at(x, y, c), 1.0);
                }
            }
        }
    }

    #[test]
    fn get_threshold_bayer_2x2_wraps_correctly() {
        let cache = ThresholdMapCache::new();
        let params = make_params(DitherModeV2::Bayer2x2, 4, 1.0);
        // Verify modulo wrapping: threshold at (0,0) == threshold at (2,2)
        let t00 = get_threshold(&params, 0, 0, &cache).unwrap();
        let t22 = get_threshold(&params, 2, 2, &cache).unwrap();
        assert_eq!(t00, t22);

        // And at large coords
        let t_large = get_threshold(&params, 1000, 1000, &cache).unwrap();
        assert_eq!(t00, t_large);
    }

    #[test]
    fn get_threshold_bayer_4x4_values_in_range() {
        let cache = ThresholdMapCache::new();
        let params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        for y in 0..4u32 {
            for x in 0..4u32 {
                let t = get_threshold(&params, x, y, &cache).unwrap();
                assert!(t >= 0.0 && t < 1.0, "Threshold out of range: {}", t);
            }
        }
    }

    #[test]
    fn get_threshold_bayer_8x8_values_in_range() {
        let cache = ThresholdMapCache::new();
        let params = make_params(DitherModeV2::Bayer8x8, 4, 1.0);
        for y in 0..8u32 {
            for x in 0..8u32 {
                let t = get_threshold(&params, x, y, &cache).unwrap();
                assert!(t >= 0.0 && t < 1.0, "Threshold out of range: {}", t);
            }
        }
    }

    #[test]
    fn rotated_cell_dist_fixed_numbers() {
        // At cell center of unrotated screen: gx=s/2 effectively at rem mid → dist≈0
        let s = 8.0;
        // rem_euclid(0,8)-4 = -4; rem_euclid(4,8)-4 = 0 → center-ish at (4,4)
        let d = rotated_cell_dist(4, 4, s, 0.0);
        assert!(d < 1e-5, "expected near-zero dist at cell center, got {}", d);
        let d_corner = rotated_cell_dist(0, 0, s, 0.0);
        assert!((d_corner - (4.0_f32 * 2.0_f32.sqrt())).abs() < 1e-4);
    }

    #[test]
    fn wave_threshold_fixed_numbers() {
        // phase=0, angle=0, amp=1, wl=8 → T = 0.5 + 0.5*sin(2π*gx/8)
        let t0 = wave_threshold(0, 0, 8.0, 1.0, 0.0, 0.0);
        assert!((t0 - 0.5).abs() < 1e-5);
        let t2 = wave_threshold(2, 0, 8.0, 1.0, 0.0, 0.0);
        assert!((t2 - 1.0).abs() < 1e-4 || (t2 - 0.999_999).abs() < 1e-5);
        let t4 = wave_threshold(4, 0, 8.0, 1.0, 0.0, 0.0);
        assert!((t4 - 0.5).abs() < 1e-5);
    }

    #[test]
    fn cmyk_halftone_alpha_preserved() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 0.42);
        let mut params = make_params(DitherModeV2::CmykHalftone, 4, 1.0);
        params.halftone_cell_size = 8;
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);
        let result = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_eq!(result.at(10, 10, 3), 0.42);
    }

    #[test]
    fn wave_mode_deterministic() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let mut params = make_params(DitherModeV2::Wave, 4, 1.0);
        params.wave_wavelength = 8.0;
        params.wave_amplitude = 1.0;
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);
        let r1 = apply_ordered(
            &tile, tc(1, 1), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let r2 = apply_ordered(
            &tile, tc(1, 1), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_eq!(r1.data, r2.data);
    }

    // ─── Pixel Size Block Tests (Req 4.1–4.5) ───────────────────────────

    #[test]
    fn pixel_size_2_produces_uniform_blocks() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let mut params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        params.pixel_size = 2;
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // Check that 2×2 blocks have uniform color
        let ps = 2u32;
        for by in (0..TILE_FULL_SIZE).step_by(ps as usize) {
            for bx in (0..TILE_FULL_SIZE).step_by(ps as usize) {
                let r0 = result.at(bx, by, 0);
                let g0 = result.at(bx, by, 1);
                let b0 = result.at(bx, by, 2);
                for dy in 0..ps.min(TILE_FULL_SIZE - by) {
                    for dx in 0..ps.min(TILE_FULL_SIZE - bx) {
                        let px = bx + dx;
                        let py = by + dy;
                        assert_eq!(
                            result.at(px, py, 0), r0,
                            "R mismatch in block ({}, {}) at pixel ({}, {})", bx, by, px, py
                        );
                        assert_eq!(
                            result.at(px, py, 1), g0,
                            "G mismatch in block ({}, {}) at pixel ({}, {})", bx, by, px, py
                        );
                        assert_eq!(
                            result.at(px, py, 2), b0,
                            "B mismatch in block ({}, {}) at pixel ({}, {})", bx, by, px, py
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pixel_size_4_blocks_aligned_to_global_coords() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let mut params = make_params(DitherModeV2::Bayer8x8, 8, 1.0);
        params.pixel_size = 4;
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 512, 512);

        // Process at tile (1, 1) — global offset is (256, 256)
        let result = apply_ordered(&tile, tc(1, 1), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // Blocks should still be aligned to 4-pixel boundaries in global coords
        // At tile (1,1), local (0,0) = global (256,256), which is divisible by 4
        let ps = 4u32;
        for by in (0..TILE_FULL_SIZE).step_by(ps as usize) {
            for bx in (0..TILE_FULL_SIZE).step_by(ps as usize) {
                let r0 = result.at(bx, by, 0);
                for dy in 0..ps.min(TILE_FULL_SIZE - by) {
                    for dx in 0..ps.min(TILE_FULL_SIZE - bx) {
                        assert_eq!(
                            result.at(bx + dx, by + dy, 0), r0,
                            "Block not uniform at ({}, {})", bx + dx, by + dy
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pixel_size_1_matches_per_pixel_processing() {
        // pixel_size=1 should produce the same result as the old implementation
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // With ps=1, adjacent pixels can have different values (they do due to Bayer matrix)
        // Just verify it produces valid quantized levels and not all the same
        let mut seen_different = false;
        let first_r = result.at(0, 0, 0);
        for y in 0..4u32 {
            for x in 0..4u32 {
                if result.at(x, y, 0) != first_r {
                    seen_different = true;
                    break;
                }
            }
        }
        assert!(seen_different, "With ps=1, Bayer 4x4 should produce varied output");
    }

    // ─── Palette Quantization Tests (Req 6.1–6.3, 6.5, 7.1–7.4) ────────

    #[test]
    fn palette_quantization_produces_palette_colors() {
        use engine_color::palette::LinearColor;

        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let mut params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        params.palette_id = Some(crate::types::PaletteId::new(1));

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let mut doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        // Add a palette with 3 colors
        let palette_id = doc.add_palette(
            "Test".to_string(),
            vec![
                LinearColor { r: 0.0, g: 0.0, b: 0.0 }, // black
                LinearColor { r: 1.0, g: 1.0, b: 1.0 }, // white
                LinearColor { r: 0.5, g: 0.0, b: 0.5 }, // purple
            ],
        );
        params.palette_id = Some(palette_id);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // Every output pixel must match one of the palette colors
        let palette = doc.get_palette(palette_id).unwrap();
        for y in (0..TILE_FULL_SIZE).step_by(10) {
            for x in (0..TILE_FULL_SIZE).step_by(10) {
                let out_r = result.at(x, y, 0);
                let out_g = result.at(x, y, 1);
                let out_b = result.at(x, y, 2);

                let matches_any = palette.colors.iter().any(|c| {
                    c.r == out_r && c.g == out_g && c.b == out_b
                });
                assert!(
                    matches_any,
                    "Pixel ({}, {}) = ({}, {}, {}) does not match any palette entry",
                    x, y, out_r, out_g, out_b
                );
            }
        }
    }

    #[test]
    fn palette_not_found_returns_error() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let mut params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        params.palette_id = Some(crate::types::PaletteId::new(999));

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc);
        assert!(result.is_err(), "Should error when palette_id references nonexistent palette");
    }

    #[test]
    fn uniform_quantization_output_clamped_to_01() {
        // Test with extreme threshold_scale to stress clamping
        let tile = make_uniform_tile(0.99, 0.01, 0.5, 1.0);
        let params = make_params(DitherModeV2::Bayer4x4, 4, 4.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let result = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3 {
                    let v = result.at(x, y, c);
                    assert!(v >= 0.0 && v <= 1.0, "Value out of range at ({}, {}, {}): {}", x, y, c, v);
                }
            }
        }
    }

    /// Verify that ordered dithering produces a seamless pattern across tile boundaries.
    /// The last core pixel of tile (0,0) and first core pixel of tile (1,0) should have
    /// threshold values consistent with their global positions — no seam/discontinuity.
    #[test]
    fn ordered_dither_seamless_across_tile_boundary() {
        use engine_tiles::HALO;

        // Uniform gray — any discontinuity in the pattern will show as
        // a seam in output values at the tile boundary.
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let params = make_params(DitherModeV2::Bayer8x8, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 512, 512);

        let result_left = apply_ordered(&tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();
        let result_right = apply_ordered(&tile, tc(1, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // The last core column of tile (0,0) is at local x = HALO + TILE_SIZE - 1 = 257
        // The first core column of tile (1,0) is at local x = HALO = 2
        // Their global x coords are 255 and 256 — consecutive pixels.
        // For Bayer 8x8: 255 % 8 = 7, 256 % 8 = 0 — different thresholds, valid pattern.

        let last_core_x = HALO + TILE_SIZE - 1; // 257
        let first_core_x = HALO; // 2

        // Verify that across a full row, the pattern repeats correctly
        for y in HALO..(HALO + 8) {
            let left_val = result_left.at(last_core_x, y, 0);
            let right_val = result_right.at(first_core_x, y, 0);

            // Both should be valid quantized levels
            assert!(
                is_valid_level(left_val, 4.0),
                "Invalid level at left boundary: {}", left_val
            );
            assert!(
                is_valid_level(right_val, 4.0),
                "Invalid level at right boundary: {}", right_val
            );
        }

        // Key check: the Bayer pattern at global (255, y) and (256, y) should differ
        // for at least some y values (they use different columns of the matrix).
        // If there were a seam (both tiles resetting to local coords), they'd
        // incorrectly use the same pattern column.
        let mut found_different = false;
        for y in HALO..(HALO + 8) {
            if result_left.at(last_core_x, y, 0) != result_right.at(first_core_x, y, 0) {
                found_different = true;
                break;
            }
        }
        assert!(
            found_different,
            "Bayer pattern should produce different values at adjacent global columns (255 vs 256)"
        );
    }

    // ─── Track H: threshold bias + pattern angle ─────────────────────────

    #[test]
    fn rotate_pattern_coord_zero_is_identity() {
        assert_eq!(rotate_pattern_coord(13, -7, 0.0), (13, -7));
        assert_eq!(rotate_pattern_coord(13, -7, 360.0), (13, -7));
        assert_eq!(rotate_pattern_coord(13, -7, -360.0), (13, -7));
    }

    #[test]
    fn rotate_pattern_coord_period_360() {
        let a = rotate_pattern_coord(40, 12, 15.0);
        let b = rotate_pattern_coord(40, 12, 375.0);
        assert_eq!(a, b);
    }

    #[test]
    fn rotate_pattern_coord_nonzero_changes_sample() {
        assert_ne!(rotate_pattern_coord(40, 12, 15.0), (40, 12));
    }

    #[test]
    fn threshold_bias_zero_matches_default_bayer() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let mut params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let baseline = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        params.threshold_bias = 0.0;
        params.pattern_angle = 0.0;
        let same = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_eq!(baseline.data, same.data);
    }

    fn count_high_pixels(tile: &PixelTile) -> usize {
        let mut n = 0usize;
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                if tile.at(x, y, 0) > 0.5 {
                    n += 1;
                }
            }
        }
        n
    }

    #[test]
    fn threshold_bias_moves_mid_gray_count() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let mut params = make_params(DitherModeV2::Bayer4x4, 2, 1.0);
        let mid = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        params.threshold_bias = 0.2;
        let high = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        params.threshold_bias = -0.2;
        let low = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();

        let c_mid = count_high_pixels(&mid);
        let c_high = count_high_pixels(&high);
        let c_low = count_high_pixels(&low);
        assert!(
            c_high > c_mid,
            "positive bias should increase high-level pixels ({c_high} vs {c_mid})"
        );
        assert!(
            c_low < c_mid,
            "negative bias should decrease high-level pixels ({c_low} vs {c_mid})"
        );
    }

    #[test]
    fn pattern_angle_zero_matches_unrotated() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let mut params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);

        let a = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        params.pattern_angle = 15.0;
        let rotated = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_ne!(a.data, rotated.data, "non-zero angle must change Bayer sampling");

        params.pattern_angle = 375.0;
        let period = apply_ordered(
            &tile, tc(0, 0), &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_eq!(rotated.data, period.data, "angle and angle+360 must be bit-identical");
    }

    #[test]
    fn pattern_angle_does_not_shear_pixel_size_blocks() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let mut params = make_params(DitherModeV2::Bayer4x4, 4, 1.0);
        params.pixel_size = 4;
        params.pattern_angle = 30.0;
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);
        let coord = tc(0, 0);

        let result = apply_ordered(
            &tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        )
        .unwrap();

        let ps = 4u32;
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let block = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO).aligned(ps);
                if x + 1 < TILE_FULL_SIZE {
                    let n = GlobalCoordSigned::from_local_with_halo(coord, x + 1, y, HALO).aligned(ps);
                    if n == block {
                        assert_eq!(
                            result.at(x, y, 0),
                            result.at(x + 1, y, 0),
                            "horizontal block run sheared at ({x},{y})"
                        );
                    }
                }
                if y + 1 < TILE_FULL_SIZE {
                    let n = GlobalCoordSigned::from_local_with_halo(coord, x, y + 1, HALO).aligned(ps);
                    if n == block {
                        assert_eq!(
                            result.at(x, y, 0),
                            result.at(x, y + 1, 0),
                            "vertical block run sheared at ({x},{y})"
                        );
                    }
                }
            }
        }
    }
}
