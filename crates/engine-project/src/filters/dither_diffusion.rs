//! Error diffusion dithering engine (V2 redesign).
//!
//! Implements Floyd-Steinberg and Atkinson error diffusion with cross-tile
//! error propagation via `ErrorResidualsStore`. Processes the core TILE_SIZE×TILE_SIZE
//! area sequentially (left-to-right, top-to-bottom) and reads source pixels
//! from the halo region for boundary context.
//!
//! **Requirements:** 3.1, 3.2, 3.3, 3.4, 3.5, 4.1, 4.2, 4.3, 4.4, 5.1, 5.2, 6.4, 6.5

use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use crate::filters::dither_residuals::{ErrorResiduals, ErrorResidualsStore, CORNER_PATCH};
use crate::types::LayerId;
use engine_color::oklab::{linear_to_oklab, LinRgb};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::{PaletteLutCache, DEFAULT_LUT_SIZE};
use engine_tiles::block_cache::{BlockCoord, BlockRepresentativeCache};
use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

/// Core tile size as usize for indexing.
const SIZE: usize = TILE_SIZE as usize;

// ─── Quantization Helpers ────────────────────────────────────────────────────

/// Quantize a single channel value to evenly spaced levels.
///
/// Formula: `quantized = round(value * (levels - 1)) / (levels - 1)`
/// Clamped to [0.0, 1.0].
#[inline]
fn quantize_uniform(value: f32, levels: f32) -> f32 {
    let scaled = value * (levels - 1.0);
    scaled.round().clamp(0.0, levels - 1.0) / (levels - 1.0)
}

/// Convert RGB to luminance using Rec. 709 coefficients.
#[inline]
fn to_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

// ─── Error Distribution ──────────────────────────────────────────────────────

/// Floyd-Steinberg kernel offsets and weights.
/// (dx, dy, weight) relative to the current pixel.
const FS_KERNEL: [(i32, i32, f32); 4] = [
    (1, 0, 7.0 / 16.0),
    (-1, 1, 3.0 / 16.0),
    (0, 1, 5.0 / 16.0),
    (1, 1, 1.0 / 16.0),
];

/// Atkinson kernel offsets (each neighbor gets 1/8 of error).
/// Total distributed: 6/8 = 3/4 (intentionally loses 1/4 for sharper results).
const ATKINSON_KERNEL: [(i32, i32); 6] = [
    (1, 0),
    (2, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
    (0, 2),
];

/// Distribute error to neighbor positions using Floyd-Steinberg kernel.
///
/// Error that would propagate outside the tile boundary (x >= SIZE or y >= SIZE)
/// is captured into the `right_overflow` and `bottom_overflow` residual buffers.
#[inline]
fn distribute_fs(
    error_buf: &mut [f32],
    x: usize,
    y: usize,
    err: [f32; 3],
    right_overflow: &mut Vec<f32>,
    bottom_overflow: &mut Vec<f32>,
    corner_overflow: &mut Vec<f32>,
) {
    for &(dx, dy, weight) in &FS_KERNEL {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        let weighted = [err[0] * weight, err[1] * weight, err[2] * weight];

        if nx >= 0 && (nx as usize) < SIZE && (ny as usize) < SIZE {
            // Within tile bounds — accumulate in error buffer
            let idx = (ny as usize * SIZE + nx as usize) * 3;
            error_buf[idx] += weighted[0];
            error_buf[idx + 1] += weighted[1];
            error_buf[idx + 2] += weighted[2];
        } else if nx >= SIZE as i32 && ny >= 0 && (ny as usize) < SIZE {
            // Right overflow: col index relative to tile right edge
            let col = nx as usize - SIZE;
            if col < 2 {
                let idx = (ny as usize * 2 + col) * 3;
                right_overflow[idx] += weighted[0];
                right_overflow[idx + 1] += weighted[1];
                right_overflow[idx + 2] += weighted[2];
            }
        } else if ny >= SIZE as i32 && nx >= 0 && (nx as usize) < SIZE {
            // Bottom overflow: row index relative to tile bottom edge
            let row = ny as usize - SIZE;
            if row < 2 {
                let idx = (row * SIZE + nx as usize) * 3;
                bottom_overflow[idx] += weighted[0];
                bottom_overflow[idx + 1] += weighted[1];
                bottom_overflow[idx + 2] += weighted[2];
            }
        } else if nx >= SIZE as i32 && ny >= SIZE as i32 {
            // Diagonal overflow → IncomingErrorBuffer for tile (tx+1, ty+1)
            let col = nx as usize - SIZE;
            let row = ny as usize - SIZE;
            if col < CORNER_PATCH && row < CORNER_PATCH {
                let idx = (row * CORNER_PATCH + col) * 3;
                corner_overflow[idx] += weighted[0];
                corner_overflow[idx + 1] += weighted[1];
                corner_overflow[idx + 2] += weighted[2];
            }
        }
    }
}

/// Distribute error to neighbor positions using Atkinson kernel (each 1/8).
///
/// Error that would propagate outside the tile boundary is captured into overflow buffers.
#[inline]
fn distribute_atkinson(
    error_buf: &mut [f32],
    x: usize,
    y: usize,
    err: [f32; 3],
    right_overflow: &mut Vec<f32>,
    bottom_overflow: &mut Vec<f32>,
    corner_overflow: &mut Vec<f32>,
) {
    let weight = 1.0 / 8.0;
    for &(dx, dy) in &ATKINSON_KERNEL {
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        let weighted = [err[0] * weight, err[1] * weight, err[2] * weight];

        if nx >= 0 && (nx as usize) < SIZE && (ny as usize) < SIZE {
            let idx = (ny as usize * SIZE + nx as usize) * 3;
            error_buf[idx] += weighted[0];
            error_buf[idx + 1] += weighted[1];
            error_buf[idx + 2] += weighted[2];
        } else if nx >= SIZE as i32 && ny >= 0 && (ny as usize) < SIZE {
            let col = nx as usize - SIZE;
            if col < 2 {
                let idx = (ny as usize * 2 + col) * 3;
                right_overflow[idx] += weighted[0];
                right_overflow[idx + 1] += weighted[1];
                right_overflow[idx + 2] += weighted[2];
            }
        } else if ny >= SIZE as i32 && nx >= 0 && (nx as usize) < SIZE {
            let row = ny as usize - SIZE;
            if row < 2 {
                let idx = (row * SIZE + nx as usize) * 3;
                bottom_overflow[idx] += weighted[0];
                bottom_overflow[idx + 1] += weighted[1];
                bottom_overflow[idx + 2] += weighted[2];
            }
        } else if nx >= SIZE as i32 && ny >= SIZE as i32 {
            let col = nx as usize - SIZE;
            let row = ny as usize - SIZE;
            if col < CORNER_PATCH && row < CORNER_PATCH {
                let idx = (row * CORNER_PATCH + col) * 3;
                corner_overflow[idx] += weighted[0];
                corner_overflow[idx + 1] += weighted[1];
                corner_overflow[idx + 2] += weighted[2];
            }
        }
    }
}

// ─── Cross-Tile Boundary Seeding ─────────────────────────────────────────────

/// Seed the error buffer from the left neighbor's right-edge residuals.
///
/// The left neighbor stored error that propagated past its right edge into our
/// tile's first 2 columns.
///
/// Layout of `left_residuals.right`: `[row * 2 * 3 + col * 3 + channel]`
/// where row ∈ [0, TILE_SIZE), col ∈ {0, 1}.
fn seed_left_boundary(error_buf: &mut [f32], left_residuals: &ErrorResiduals) {
    for row in 0..SIZE {
        for col in 0..2usize {
            let src_idx = (row * 2 + col) * 3;
            let dst_idx = (row * SIZE + col) * 3;
            error_buf[dst_idx] += left_residuals.right[src_idx];
            error_buf[dst_idx + 1] += left_residuals.right[src_idx + 1];
            error_buf[dst_idx + 2] += left_residuals.right[src_idx + 2];
        }
    }
}

/// Seed the error buffer from the top neighbor's bottom-edge residuals.
///
/// The top neighbor stored error that propagated past its bottom edge into our
/// tile's first 2 rows.
///
/// Layout of `top_residuals.bottom`: `[row * TILE_SIZE * 3 + col * 3 + channel]`
/// where row ∈ {0, 1}, col ∈ [0, TILE_SIZE).
fn seed_top_boundary(error_buf: &mut [f32], top_residuals: &ErrorResiduals) {
    for row in 0..2usize {
        for col in 0..SIZE {
            let src_idx = (row * SIZE + col) * 3;
            let dst_idx = (row * SIZE + col) * 3;
            error_buf[dst_idx] += top_residuals.bottom[src_idx];
            error_buf[dst_idx + 1] += top_residuals.bottom[src_idx + 1];
            error_buf[dst_idx + 2] += top_residuals.bottom[src_idx + 2];
        }
    }
}

/// Seed the top-left of the error buffer from the diagonal neighbor's corner patch.
///
/// Tile `(tx-1, ty-1)` stored FS/Atkinson overflow with both `nx >= SIZE` and
/// `ny >= SIZE` into `corner`; that energy belongs at our `(0..CORNER_PATCH,
/// 0..CORNER_PATCH)`.
fn seed_diag_corner(error_buf: &mut [f32], diag_residuals: &ErrorResiduals) {
    for row in 0..CORNER_PATCH {
        for col in 0..CORNER_PATCH {
            let src_idx = (row * CORNER_PATCH + col) * 3;
            let dst_idx = (row * SIZE + col) * 3;
            error_buf[dst_idx] += diag_residuals.corner[src_idx];
            error_buf[dst_idx + 1] += diag_residuals.corner[src_idx + 1];
            error_buf[dst_idx + 2] += diag_residuals.corner[src_idx + 2];
        }
    }
}

// ─── Main Entry Point ────────────────────────────────────────────────────────

/// Apply error diffusion dithering to a tile.
///
/// Processes the core TILE_SIZE×TILE_SIZE area sequentially (left-to-right,
/// top-to-bottom). Reads source pixels from the input tile (which includes
/// halo). The output tile's halo region is copied from the input unchanged.
///
/// Implements:
/// - Cross-tile error propagation (Req 3.3, 3.4, 3.5)
/// - Pixel-size blocking: block representative computes the color, non-representatives
///   copy it (Req 4.1, 4.2, 4.3, 4.4)
/// - Color modes: RGB independent channels, Grayscale luminance (Req 5.1, 5.2)
/// - Palette quantization: error computed in Oklab space (Req 6.4, 6.5)
/// - Alpha preservation (Req 5.3)
///
/// # Arguments
///
/// * `tile` - Input pixel tile (260×260 with halo)
/// * `coord` - Tile coordinate for this tile
/// * `params` - Full V2 dither parameters
/// * `residuals_store` - Cross-tile error residuals store
/// * `layer_id` - Layer identifier for residuals keying
/// * `palette_cache` - Palette KD-tree cache for palette quantization
/// * `document` - Document reference for palette lookup
///
/// # Errors
///
/// Returns `EngineError` if palette lookup fails (when `palette_id` is set).
pub fn apply_error_diffusion(
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    residuals_store: &ErrorResidualsStore,
    layer_id: LayerId,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    document: &Document,
) -> Result<PixelTile, EngineError> {
    let empty = BlockRepresentativeCache::new();
    apply_error_diffusion_with_cache(
        tile,
        coord,
        params,
        residuals_store,
        layer_id,
        palette_cache,
        lut_cache,
        document,
        &empty,
    )
}

/// Error diffusion with shared [`BlockRepresentativeCache`] for mega-pixel
/// source reads and cross-tile dithered block colors.
pub fn apply_error_diffusion_with_cache(
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    residuals_store: &ErrorResidualsStore,
    layer_id: LayerId,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    document: &Document,
    block_cache: &BlockRepresentativeCache,
) -> Result<PixelTile, EngineError> {
    let mut result = PixelTile::new();
    let levels = params.levels as f32;
    let ps = params.pixel_size as u32;

    // Validate and fetch palette + LUT if palette_id is set (Req 6.4, 6.5 / Track B)
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

    // Initialize error buffer for the core tile area (SIZE × SIZE × 3 channels)
    let mut error_buf = vec![0.0f32; SIZE * SIZE * 3];

    // Overflow buffers for cross-tile residuals:
    // Right overflow: SIZE rows × 2 cols × 3 channels
    let mut right_overflow = vec![0.0f32; SIZE * 2 * 3];
    // Bottom overflow: 2 rows × SIZE cols × 3 channels
    let mut bottom_overflow = vec![0.0f32; 2 * SIZE * 3];
    // Diagonal corner → tile (tx+1, ty+1)
    let mut corner_overflow = vec![0.0f32; CORNER_PATCH * CORNER_PATCH * 3];

    // Seed boundary errors from neighbor tiles (Req 3.4 / Track A Req 4)
    if let Some(left_residuals) = residuals_store.get_left(layer_id, coord) {
        seed_left_boundary(&mut error_buf, &left_residuals);
    }
    if let Some(top_residuals) = residuals_store.get_top(layer_id, coord) {
        seed_top_boundary(&mut error_buf, &top_residuals);
    }
    if let Some(diag_residuals) = residuals_store.get_diag(layer_id, coord) {
        seed_diag_corner(&mut error_buf, &diag_residuals);
    }

    // Copy halo region from input to output unchanged
    copy_halo(tile, &mut result);

    // Sequential scan: left-to-right, top-to-bottom (Req 3.1)
    for y in 0..SIZE {
        for x in 0..SIZE {
            // Tile-local coordinates for the core area start at HALO offset
            let tile_x = x as u32 + HALO;
            let tile_y = y as u32 + HALO;

            // Global coordinates — tile_x/tile_y are in full-tile space [0, 260);
            // from_local_with_halo subtracts HALO internally.
            let gcoord = GlobalCoordSigned::from_local_with_halo(coord, tile_x, tile_y, HALO);
            let gx = gcoord.x;
            let gy = gcoord.y;

            // Pixel-size blocking: snap to block representative (Req 4.1, 4.2, 4.4)
            let block = gcoord.aligned(ps);
            let block_gx = block.x;
            let block_gy = block.y;
            let is_representative = gx == block_gx && gy == block_gy;

            // Preserve alpha unchanged (Req 5.3)
            let src_a = tile.at(tile_x, tile_y, 3);
            result.set(tile_x, tile_y, 3, src_a);

            if ps > 1 && !is_representative {
                // Non-representative: copy dithered color from the block representative.
                let rep_tile_x = block_gx - coord.x as i32 * TILE_SIZE as i32 + HALO as i32;
                let rep_tile_y = block_gy - coord.y as i32 * TILE_SIZE as i32 + HALO as i32;

                if rep_tile_x >= HALO as i32
                    && rep_tile_x < (HALO + TILE_SIZE) as i32
                    && rep_tile_y >= HALO as i32
                    && rep_tile_y < (HALO + TILE_SIZE) as i32
                {
                    let rx = rep_tile_x as u32;
                    let ry = rep_tile_y as u32;
                    result.set(tile_x, tile_y, 0, result.at(rx, ry, 0));
                    result.set(tile_x, tile_y, 1, result.at(rx, ry, 1));
                    result.set(tile_x, tile_y, 2, result.at(rx, ry, 2));
                } else if block_gx >= 0 && block_gy >= 0 {
                    let key = BlockCoord::from_global(
                        layer_id.0,
                        block_gx as u32,
                        block_gy as u32,
                        ps,
                    );
                    if let Some(rgb) = block_cache.get_dithered(key) {
                        result.set(tile_x, tile_y, 0, rgb[0]);
                        result.set(tile_x, tile_y, 1, rgb[1]);
                        result.set(tile_x, tile_y, 2, rgb[2]);
                    } else {
                        // Fallback when neighbor dithered rep is not ready: sample raw
                        // (prefer BRC) and quantize with the same palette/uniform path
                        // as a representative — no diffusion (neighbor not processed).
                        let (sr, sg, sb) = if let Some(px) = block_cache.get_raw(key) {
                            (px[0], px[1], px[2])
                        } else if rep_tile_x >= 0
                            && rep_tile_y >= 0
                            && rep_tile_x < (TILE_SIZE + 2 * HALO) as i32
                            && rep_tile_y < (TILE_SIZE + 2 * HALO) as i32
                        {
                            let cx = rep_tile_x as u32;
                            let cy = rep_tile_y as u32;
                            (tile.at(cx, cy, 0), tile.at(cx, cy, 1), tile.at(cx, cy, 2))
                        } else {
                            (
                                tile.at(tile_x, tile_y, 0),
                                tile.at(tile_x, tile_y, 1),
                                tile.at(tile_x, tile_y, 2),
                            )
                        };
                        let (qr, qg, qb) = match params.color_mode {
                            DitherColorMode::Grayscale => {
                                let lum = to_luminance(sr, sg, sb);
                                if let Some((palette, ref lut)) = palette_lut {
                                    let oklab = linear_to_oklab(LinRgb {
                                        r: lum,
                                        g: lum,
                                        b: lum,
                                    });
                                    let c = &palette.colors[lut.nearest_index(oklab) as usize];
                                    (c.r, c.g, c.b)
                                } else {
                                    let q = quantize_uniform(lum, levels);
                                    (q, q, q)
                                }
                            }
                            DitherColorMode::Rgb => {
                                if let Some((palette, ref lut)) = palette_lut {
                                    let oklab = linear_to_oklab(LinRgb {
                                        r: sr,
                                        g: sg,
                                        b: sb,
                                    });
                                    let c = &palette.colors[lut.nearest_index(oklab) as usize];
                                    (c.r, c.g, c.b)
                                } else {
                                    (
                                        quantize_uniform(sr, levels),
                                        quantize_uniform(sg, levels),
                                        quantize_uniform(sb, levels),
                                    )
                                }
                            }
                        };
                        result.set(tile_x, tile_y, 0, qr);
                        result.set(tile_x, tile_y, 1, qg);
                        result.set(tile_x, tile_y, 2, qb);
                    }
                }

                continue;
            }

            // ─── Block representative (or pixel_size == 1) processing ───

            // Prefer cached raw representative when the source would otherwise
            // need a neighbor pixel; for in-tile reps the tile sample matches.
            let (src_r, src_g, src_b) = if ps > 1 && block_gx >= 0 && block_gy >= 0 {
                let key = BlockCoord::from_global(
                    layer_id.0,
                    block_gx as u32,
                    block_gy as u32,
                    ps,
                );
                if let Some(px) = block_cache.get_raw(key) {
                    (px[0], px[1], px[2])
                } else {
                    (
                        tile.at(tile_x, tile_y, 0),
                        tile.at(tile_x, tile_y, 1),
                        tile.at(tile_x, tile_y, 2),
                    )
                }
            } else {
                (
                    tile.at(tile_x, tile_y, 0),
                    tile.at(tile_x, tile_y, 1),
                    tile.at(tile_x, tile_y, 2),
                )
            };

            // Read accumulated error for this pixel
            let err_idx = (y * SIZE + x) * 3;
            let acc_err_r = error_buf[err_idx];
            let acc_err_g = error_buf[err_idx + 1];
            let acc_err_b = error_buf[err_idx + 2];

            // Apply color mode processing and quantize
            let (quant_r, quant_g, quant_b, q_err);

            match params.color_mode {
                DitherColorMode::Rgb => {
                    // Add accumulated error to original pixel (Req 5.1)
                    let adj_r = (src_r + acc_err_r).clamp(0.0, 1.0);
                    let adj_g = (src_g + acc_err_g).clamp(0.0, 1.0);
                    let adj_b = (src_b + acc_err_b).clamp(0.0, 1.0);

                    if let Some((palette, ref lut)) = palette_lut {
                        // Palette quantization: find nearest in Oklab space (Req 6.4)
                        let adj_oklab = linear_to_oklab(LinRgb { r: adj_r, g: adj_g, b: adj_b });
                        let nearest_idx = lut.nearest_index(adj_oklab) as usize;
                        let palette_color = &palette.colors[nearest_idx];

                        // Error distributed in RGB space (the error buffer is RGB-based)
                        q_err = [adj_r - palette_color.r, adj_g - palette_color.g, adj_b - palette_color.b];

                        quant_r = palette_color.r;
                        quant_g = palette_color.g;
                        quant_b = palette_color.b;
                    } else {
                        // Uniform quantization (Req 6.5, 7.1)
                        let qr = quantize_uniform(adj_r, levels);
                        let qg = quantize_uniform(adj_g, levels);
                        let qb = quantize_uniform(adj_b, levels);

                        // Compute quantization error per channel
                        q_err = [adj_r - qr, adj_g - qg, adj_b - qb];
                        quant_r = qr;
                        quant_g = qg;
                        quant_b = qb;
                    }
                }
                DitherColorMode::Grayscale => {
                    // Convert to luminance (Req 5.2)
                    let lum = to_luminance(src_r, src_g, src_b);
                    // For grayscale, use the first channel of error as luminance error
                    let adj_lum = (lum + acc_err_r).clamp(0.0, 1.0);

                    if let Some((palette, ref lut)) = palette_lut {
                        // Palette quantization in grayscale: treat luminance as gray RGB
                        let adj_oklab = linear_to_oklab(LinRgb { r: adj_lum, g: adj_lum, b: adj_lum });
                        let nearest_idx = lut.nearest_index(adj_oklab) as usize;
                        let palette_color = &palette.colors[nearest_idx];

                        // For grayscale with palette, output the palette color directly
                        // Error is computed as luminance difference
                        let quant_lum = to_luminance(palette_color.r, palette_color.g, palette_color.b);
                        let lum_err = adj_lum - quant_lum;
                        q_err = [lum_err, lum_err, lum_err];
                        quant_r = palette_color.r;
                        quant_g = palette_color.g;
                        quant_b = palette_color.b;
                    } else {
                        // Uniform quantization on luminance
                        let qlum = quantize_uniform(adj_lum, levels);

                        // Error is single-channel, stored in all 3 for propagation
                        let lum_err = adj_lum - qlum;
                        q_err = [lum_err, lum_err, lum_err];
                        // Write R=G=B (Req 5.2)
                        quant_r = qlum;
                        quant_g = qlum;
                        quant_b = qlum;
                    }
                }
            }

            // Write quantized pixel to output (at core area offset)
            result.set(tile_x, tile_y, 0, quant_r);
            result.set(tile_x, tile_y, 1, quant_g);
            result.set(tile_x, tile_y, 2, quant_b);

            // Publish dithered block color for cross-tile non-representatives
            if ps > 1 && is_representative && block_gx >= 0 && block_gy >= 0 {
                let key = BlockCoord::from_global(
                    layer_id.0,
                    block_gx as u32,
                    block_gy as u32,
                    ps,
                );
                block_cache.insert_dithered(key, [quant_r, quant_g, quant_b]);
            }

            // Distribute quantization error to neighbors (Req 3.1, 3.2)
            // Error from the representative is NOT diffused to other pixels in the same block.
            match params.mode {
                DitherModeV2::FloydSteinberg => {
                    distribute_fs(
                        &mut error_buf, x, y, q_err,
                        &mut right_overflow, &mut bottom_overflow, &mut corner_overflow,
                    );
                }
                DitherModeV2::Atkinson => {
                    distribute_atkinson(
                        &mut error_buf, x, y, q_err,
                        &mut right_overflow, &mut bottom_overflow, &mut corner_overflow,
                    );
                }
                _ => unreachable!(
                    "error diffusion engine called with ordered mode"
                ),
            }
        }
    }

    // Store edge + diagonal residuals for cross-tile propagation (Req 3.3 / A1.4)
    let residuals = ErrorResiduals {
        right: right_overflow,
        bottom: bottom_overflow,
        corner: corner_overflow,
    };
    residuals_store.store(layer_id, coord, residuals);

    Ok(result)
}

// ─── Halo Copy Helper ────────────────────────────────────────────────────────

/// Copy the halo region (border pixels) from input to output unchanged.
///
/// The core area (HALO..HALO+TILE_SIZE in both dimensions) is handled by
/// the diffusion loop. Everything else (the 2-pixel border) is copied as-is.
fn copy_halo(src: &PixelTile, dst: &mut PixelTile) {
    let full = TILE_SIZE + 2 * HALO;

    // Top halo rows (0..HALO)
    for y in 0..HALO {
        for x in 0..full {
            for c in 0..4u32 {
                dst.set(x, y, c, src.at(x, y, c));
            }
        }
    }

    // Bottom halo rows (HALO + TILE_SIZE..full)
    for y in (HALO + TILE_SIZE)..full {
        for x in 0..full {
            for c in 0..4u32 {
                dst.set(x, y, c, src.at(x, y, c));
            }
        }
    }

    // Left and right halo columns in the core rows
    for y in HALO..(HALO + TILE_SIZE) {
        // Left halo (0..HALO)
        for x in 0..HALO {
            for c in 0..4u32 {
                dst.set(x, y, c, src.at(x, y, c));
            }
        }
        // Right halo (HALO + TILE_SIZE..full)
        for x in (HALO + TILE_SIZE)..full {
            for c in 0..4u32 {
                dst.set(x, y, c, src.at(x, y, c));
            }
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
    use crate::filters::dither_residuals::ErrorResidualsStore;
    use crate::types::DocumentId;

    const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

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

    fn make_fs_params(levels: u16) -> DitherParamsV2 {
        DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }
    }

    fn make_atkinson_params(levels: u16) -> DitherParamsV2 {
        DitherParamsV2 {
            mode: DitherModeV2::Atkinson,
            levels,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }
    }

    fn is_valid_level(v: f32, levels: f32) -> bool {
        let k = v * (levels - 1.0);
        (k - k.round()).abs() < 1e-4
    }

    #[test]
    fn floyd_steinberg_produces_valid_quantized_levels() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let params = make_fs_params(4);
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        let levels = params.levels as f32;
        // Check core area only
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..3 {
                    let v = result.at(x, y, c);
                    assert!(
                        is_valid_level(v, levels),
                        "Invalid level at ({}, {}, {}): {}",
                        x, y, c, v
                    );
                }
            }
        }
    }

    #[test]
    fn atkinson_produces_valid_quantized_levels() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let params = make_atkinson_params(4);
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        let levels = params.levels as f32;
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..3 {
                    let v = result.at(x, y, c);
                    assert!(
                        is_valid_level(v, levels),
                        "Invalid level at ({}, {}, {}): {}",
                        x, y, c, v
                    );
                }
            }
        }
    }

    #[test]
    fn alpha_channel_preserved() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 0.42);
        let params = make_fs_params(4);
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Alpha preserved in core area
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                assert_eq!(
                    result.at(x, y, 3), 0.42,
                    "Alpha mismatch at ({}, {})", x, y
                );
            }
        }
    }

    #[test]
    fn grayscale_mode_produces_equal_rgb() {
        let tile = make_uniform_tile(0.8, 0.2, 0.5, 1.0);
        let mut params = make_fs_params(4);
        params.color_mode = DitherColorMode::Grayscale;
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                let r = result.at(x, y, 0);
                let g = result.at(x, y, 1);
                let b = result.at(x, y, 2);
                assert_eq!(r, g, "R != G at ({}, {})", x, y);
                assert_eq!(g, b, "G != B at ({}, {})", x, y);
            }
        }
    }

    #[test]
    fn black_tile_stays_black() {
        let tile = make_uniform_tile(0.0, 0.0, 0.0, 1.0);
        let params = make_fs_params(4);
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..3 {
                    assert_eq!(result.at(x, y, c), 0.0);
                }
            }
        }
    }

    #[test]
    fn white_tile_stays_white() {
        let tile = make_uniform_tile(1.0, 1.0, 1.0, 1.0);
        let params = make_fs_params(4);
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..3 {
                    assert_eq!(result.at(x, y, c), 1.0);
                }
            }
        }
    }

    #[test]
    fn deterministic_output() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let params = make_fs_params(4);
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let store1 = ErrorResidualsStore::new();
        let r1 = apply_error_diffusion(
            &tile, tc(2, 3), &params, &store1, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        let store2 = ErrorResidualsStore::new();
        let r2 = apply_error_diffusion(
            &tile, tc(2, 3), &params, &store2, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        assert_eq!(r1.data, r2.data);
    }

    #[test]
    fn stores_residuals_after_processing() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let params = make_fs_params(2); // Binary quantization → maximum error
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Residuals should have been stored
        // The right neighbor (1, 0) should be able to get left residuals
        let residuals = store.get_left(layer_id, tc(1, 0));
        assert!(residuals.is_some(), "No residuals stored for right neighbor");

        // The bottom neighbor (0, 1) should be able to get top residuals
        let residuals = store.get_top(layer_id, tc(0, 1));
        assert!(residuals.is_some(), "No residuals stored for bottom neighbor");
    }

    #[test]
    fn cross_tile_seeding_affects_output() {
        // Use a uniform tile at 0.3 — not exactly at a quantization boundary
        let tile = make_uniform_tile(0.3, 0.3, 0.3, 1.0);
        let params = make_fs_params(4); // 4 levels: boundaries at 0, 1/3, 2/3, 1

        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 512, 512);
        let layer_id = LayerId::new(1);

        // Manually inject large residuals to the store to guarantee seeding works
        let store = ErrorResidualsStore::new();
        let mut fake_residuals = ErrorResiduals::new();
        // Set large error in the right edge that will propagate to tile (1,0)
        for row in 0..SIZE {
            let idx = (row * 2 + 0) * 3;
            fake_residuals.right[idx] = 0.4;     // R
            fake_residuals.right[idx + 1] = 0.4; // G
            fake_residuals.right[idx + 2] = 0.4; // B
        }
        store.store(layer_id, tc(0, 0), fake_residuals);

        // Process tile (1,0) WITH injected left residuals
        let result_with = apply_error_diffusion(
            &tile, tc(1, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Process tile (1,0) WITHOUT any residuals
        let empty_store = ErrorResidualsStore::new();
        let result_without = apply_error_diffusion(
            &tile, tc(1, 0), &params, &empty_store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // First column in core area should differ
        let mut differs = false;
        for y in HALO..(HALO + 4) {
            let x = HALO; // first core column
            if result_with.at(x, y, 0) != result_without.at(x, y, 0) {
                differs = true;
                break;
            }
        }
        assert!(differs, "Cross-tile seeding should affect left-edge output");
    }

    #[test]
    fn corner_residuals_captured_and_seed_diag() {
        // Binary FS on mid-gray produces non-zero (+1,+1) overflow at BR pixel.
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let params = make_fs_params(2);
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 512, 512);
        let layer_id = LayerId::new(1);

        let store = ErrorResidualsStore::new();
        apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();

        let from_00 = store.get_diag(layer_id, tc(1, 1)).expect("diag residuals");
        let corner_energy: f32 = from_00.corner.iter().map(|v| v.abs()).sum();
        assert!(
            corner_energy > 1e-6,
            "FS should capture diagonal overflow into corner (energy={corner_energy})"
        );

        // Inject large corner seed and verify tile (1,1) top-left differs.
        // Use mid-gray + 4 levels so ±error can change the quantization bucket.
        let soft = make_uniform_tile(0.3, 0.3, 0.3, 1.0);
        let soft_params = make_fs_params(4);
        let seeded = ErrorResidualsStore::new();
        let mut fake = ErrorResiduals::new();
        fake.corner[0] = 0.4;
        fake.corner[1] = 0.4;
        fake.corner[2] = 0.4;
        seeded.store(layer_id, tc(0, 0), fake);

        let with_diag = apply_error_diffusion(
            &soft, tc(1, 1), &soft_params, &seeded, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let empty = ErrorResidualsStore::new();
        let without = apply_error_diffusion(
            &soft, tc(1, 1), &soft_params, &empty, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();

        assert_ne!(
            with_diag.at(HALO, HALO, 0),
            without.at(HALO, HALO, 0),
            "diagonal corner seed must affect top-left of tile (1,1)"
        );
    }

    #[test]
    fn diffusion_works_at_pyramid_level_gt_0() {
        let tile = make_uniform_tile(0.4, 0.4, 0.4, 1.0);
        let params = make_fs_params(4);
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 1024, 1024);
        let layer_id = LayerId::new(1);
        let level1 = |x, y| TileCoord { level: 1, x, y };

        let store = ErrorResidualsStore::new();
        apply_error_diffusion(
            &tile, level1(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let r10 = apply_error_diffusion(
            &tile, level1(1, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();

        let isolated = ErrorResidualsStore::new();
        let r10_iso = apply_error_diffusion(
            &tile, level1(1, 0), &params, &isolated, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();

        let differs = (HALO..(HALO + 8)).any(|y| {
            r10.at(HALO, y, 0) != r10_iso.at(HALO, y, 0)
        });
        assert!(
            differs,
            "level>0 left residuals must seed neighbor like level 0"
        );

        // Atkinson at level 1 as well
        let atk = DitherParamsV2 {
            mode: DitherModeV2::Atkinson,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        };
        let store_a = ErrorResidualsStore::new();
        apply_error_diffusion(
            &tile, level1(0, 0), &atk, &store_a, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert!(
            store_a.get_left(layer_id, level1(1, 0)).is_some(),
            "Atkinson at level 1 stores residuals keyed by full TileCoord"
        );
    }

    #[test]
    fn halo_region_copied_from_input() {
        let tile = make_uniform_tile(0.75, 0.25, 0.5, 0.9);
        let params = make_fs_params(4);
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Check top-left halo corner
        for y in 0..HALO {
            for x in 0..HALO {
                assert_eq!(result.at(x, y, 0), 0.75);
                assert_eq!(result.at(x, y, 1), 0.25);
                assert_eq!(result.at(x, y, 2), 0.5);
                assert_eq!(result.at(x, y, 3), 0.9);
            }
        }
    }
}
