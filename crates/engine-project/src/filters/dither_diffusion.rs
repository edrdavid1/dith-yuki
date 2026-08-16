//! Error diffusion dithering engine (V2 redesign).
//!
//! Implements error-diffusion kernels (FS, Atkinson, JJN, Stucki, Burkes, Sierra)
//! with cross-tile error propagation via `ErrorResidualsStore`. Processes the
//! core TILE_SIZE×TILE_SIZE area sequentially (left-to-right, top-to-bottom)
//! and reads source pixels from the halo region for boundary context.
//!
//! **Requirements:** 3.1, 3.2, 3.3, 3.4, 3.5, 4.1, 4.2, 4.3, 4.4, 5.1, 5.2, 6.4, 6.5

use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{DitherColorMode, DitherParamsV2, PaletteDitherMode};
use crate::filters::dither_ordered::{OrderedPalettePicker, SimpleRgbPicker};
use crate::filters::dither_residuals::{ErrorResiduals, ErrorResidualsStore, CORNER_PATCH};
use crate::types::LayerId;
use engine_color::oklab::{linear_to_oklab, LinRgb};
use engine_color::palette::{linear_to_srgb, Palette};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_guided::{default_channel_levels, quantize_channel_guided, ChannelRange};
use engine_color::palette_lut::{PaletteLut3D, PaletteLutCache, DEFAULT_LUT_SIZE};
use engine_tiles::block_cache::{BlockCoord, BlockRepresentativeCache};
use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use std::sync::Arc;

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

enum PaletteQuant<'a> {
    Uniform,
    Strict {
        palette: &'a Palette,
        lut: Arc<PaletteLut3D>,
    },
    Guided {
        ranges: [ChannelRange; 3],
        levels: u8,
    },
    Mixed {
        ranges: [ChannelRange; 3],
        levels: u8,
        picker: OrderedPalettePicker<'a>,
    },
    Simple(SimpleRgbPicker<'a>),
}

fn snap_rgb_to_palette(
    r: f32,
    g: f32,
    b: f32,
    palette: &Palette,
    lut: &PaletteLut3D,
) -> (f32, f32, f32) {
    let oklab = linear_to_oklab(LinRgb { r, g, b });
    let c = &palette.colors[lut.nearest_index(oklab) as usize];
    (c.r, c.g, c.b)
}

fn quantize_ed_rgb(r: f32, g: f32, b: f32, levels: f32, q: &PaletteQuant<'_>) -> (f32, f32, f32) {
    match q {
        PaletteQuant::Uniform => (
            quantize_uniform(r, levels),
            quantize_uniform(g, levels),
            quantize_uniform(b, levels),
        ),
        PaletteQuant::Strict { palette, lut } => {
            snap_rgb_to_palette(r, g, b, palette, lut)
        }
        PaletteQuant::Guided {
            ranges,
            levels: ch,
        } => (
            quantize_channel_guided(r, ranges[0], *ch, 0.5),
            quantize_channel_guided(g, ranges[1], *ch, 0.5),
            quantize_channel_guided(b, ranges[2], *ch, 0.5),
        ),
        PaletteQuant::Mixed {
            ranges,
            levels: ch,
            picker,
        } => {
            // Residual is `adj - this return` (dithered pick of guided RGB).
            let qr = quantize_channel_guided(r, ranges[0], *ch, 0.5);
            let qg = quantize_channel_guided(g, ranges[1], *ch, 0.5);
            let qb = quantize_channel_guided(b, ranges[2], *ch, 0.5);
            picker.pick(qr, qg, qb, 0.5, 1.0)
        }
        PaletteQuant::Simple(picker) => picker.pick(r, g, b, 0.0),
    }
}

/// Old Yuki ED: nearest in sRGB bytes, residual `(old − new) * intensity` in that space.
fn simple_ed_step(
    picker: &SimpleRgbPicker<'_>,
    src_r: f32,
    src_g: f32,
    src_b: f32,
    acc_r: f32,
    acc_g: f32,
    acc_b: f32,
    intensity: f32,
) -> ((f32, f32, f32), [f32; 3]) {
    let old_r = (linear_to_srgb(src_r) as f32 + acc_r).clamp(0.0, 255.0);
    let old_g = (linear_to_srgb(src_g) as f32 + acc_g).clamp(0.0, 255.0);
    let old_b = (linear_to_srgb(src_b) as f32 + acc_b).clamp(0.0, 255.0);
    let (rgb, pal) = picker.pick_srgb(old_r, old_g, old_b);
    let q_err = [
        (old_r - pal[0]) * intensity,
        (old_g - pal[1]) * intensity,
        (old_b - pal[2]) * intensity,
    ];
    (rgb, q_err)
}

/// Convert RGB to luminance using Rec. 709 coefficients.
#[inline]
fn to_luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

// ─── Error Distribution ──────────────────────────────────────────────────────

/// Scan direction for one global row: `+1` L→R, `-1` R→L.
#[inline]
fn row_direction(serpentine: bool, global_y: i32) -> i32 {
    if serpentine && global_y.rem_euclid(2) == 1 {
        -1
    } else {
        1
    }
}

/// Distribute quantization error to neighbors using a published kernel table.
///
/// `row_dir` is `+1` (L→R) or `-1` (R→L serpentine). Kernel `dx` is multiplied
/// by `row_dir` so FS-style `(+1,0)` becomes `(-1,0)` on odd global rows.
///
/// `step` is `pixel_size`: neighbors are the next block representatives
/// (`dx * step`), matching legacy downsample-then-FS. `step == 1` is per-pixel.
///
/// Horizontal overflow still feeds the **wavefront-later** tile (`tx+1`):
/// `nx >= SIZE` → `right_overflow`. Kernel-forward overflow on an R→L row
/// lands at the left edge (`nx < 0`) and targets the **earlier** wavefront
/// neighbor — dropped, because that tile is already processed.
#[inline]
fn distribute_kernel(
    error_buf: &mut [f32],
    x: usize,
    y: usize,
    err: [f32; 3],
    offsets: &[(i32, i32, f32)],
    right_overflow: &mut [f32],
    bottom_overflow: &mut [f32],
    corner_overflow: &mut [f32],
    row_dir: i32,
    step: i32,
) {
    let step = step.max(1);
    for &(dx, dy, weight) in offsets {
        let nx = x as i32 + dx * row_dir * step;
        let ny = y as i32 + dy * step;
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
/// Always applied at screen columns 0..2 (the spatial joint with the
/// wavefront-**earlier** tile). On an R→L row those pixels are visited last,
/// so the seed waits in `error_buf` until scan-end — we do **not** seed from
/// the unprocessed screen-right neighbor.
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

    // Validate and fetch palette path if palette_id is set
    let palette_quant = if let Some(palette_id) = params.palette_id {
        let palette = document.get_palette(palette_id).ok_or_else(|| {
            EngineError::palette_not_found(palette_id)
        })?;
        match params.palette_dither_mode {
            PaletteDitherMode::Guided { channel_levels } => PaletteQuant::Guided {
                ranges: lut_cache.channel_ranges(palette),
                levels: channel_levels.unwrap_or_else(|| default_channel_levels(palette)),
            },
            PaletteDitherMode::Mixed { channel_levels } => PaletteQuant::Mixed {
                ranges: lut_cache.channel_ranges(palette),
                levels: channel_levels.unwrap_or_else(|| default_channel_levels(palette)),
                picker: OrderedPalettePicker::new(palette),
            },
            PaletteDitherMode::Simple => PaletteQuant::Simple(SimpleRgbPicker::new(palette)),
            PaletteDitherMode::Strict => {
                let lut = lut_cache
                    .get_or_build(palette, palette_cache, DEFAULT_LUT_SIZE)
                    .map_err(|_| EngineError::palette_not_found(palette_id))?;
                PaletteQuant::Strict { palette, lut }
            }
        }
    } else {
        PaletteQuant::Uniform
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

    // Halo is not on the wire (256 core). Copying it would leave Adjust's
    // smooth alpha as a 2px contour around every tile when dither_alpha.
    if !params.dither_alpha {
        copy_halo(tile, &mut result);
    }

    // Sequential scan: top-to-bottom. Even global rows L→R; odd global rows
    // R→L when `serpentine` (parity from GlobalCoord.y, never local tile y).
    for y in 0..SIZE {
        let tile_y = y as u32 + HALO;
        let gy = GlobalCoordSigned::from_local_with_halo(coord, HALO, tile_y, HALO).y;
        let scan_parity = if ps > 1 {
            gy.div_euclid(ps as i32)
        } else {
            gy
        };
        let row_dir = row_direction(params.serpentine, scan_parity);
        for i in 0..SIZE {
            let x = if row_dir > 0 { i } else { SIZE - 1 - i };
            // Tile-local coordinates for the core area start at HALO offset
            let tile_x = x as u32 + HALO;

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

            let src_a = if params.dither_alpha && ps > 1 && block_gx >= 0 && block_gy >= 0 {
                let key = BlockCoord::from_global(
                    layer_id.0,
                    block_gx as u32,
                    block_gy as u32,
                    ps,
                );
                if let Some(px) = block_cache.get_raw(key) {
                    px[3]
                } else {
                    let rep_tile_x = block_gx - coord.x as i32 * TILE_SIZE as i32 + HALO as i32;
                    let rep_tile_y = block_gy - coord.y as i32 * TILE_SIZE as i32 + HALO as i32;
                    if rep_tile_x >= 0
                        && rep_tile_y >= 0
                        && rep_tile_x < (TILE_SIZE + 2 * HALO) as i32
                        && rep_tile_y < (TILE_SIZE + 2 * HALO) as i32
                    {
                        tile.at(rep_tile_x as u32, rep_tile_y as u32, 3)
                    } else {
                        tile.at(tile_x, tile_y, 3)
                    }
                }
            } else {
                tile.at(tile_x, tile_y, 3)
            };
            result.set(tile_x, tile_y, 3, params.map_alpha(src_a, 0.5));

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
                    result.set(tile_x, tile_y, 3, result.at(rx, ry, 3));
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
                                match &palette_quant {
                                    PaletteQuant::Uniform => {
                                        let q = quantize_uniform(lum, levels);
                                        (q, q, q)
                                    }
                                    _ => quantize_ed_rgb(lum, lum, lum, levels, &palette_quant),
                                }
                            }
                            DitherColorMode::Rgb => {
                                quantize_ed_rgb(sr, sg, sb, levels, &palette_quant)
                            }
                        };
                        result.set(tile_x, tile_y, 0, qr);
                        result.set(tile_x, tile_y, 1, qg);
                        result.set(tile_x, tile_y, 2, qb);
                    }
                }

                if params.dither_alpha && result.at(tile_x, tile_y, 3) <= 0.0 {
                    result.set(tile_x, tile_y, 0, 0.0);
                    result.set(tile_x, tile_y, 1, 0.0);
                    result.set(tile_x, tile_y, 2, 0.0);
                    result.set(tile_x, tile_y, 3, 0.0);
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
                    if let PaletteQuant::Simple(picker) = &palette_quant {
                        let (rgb, err) = simple_ed_step(
                            picker,
                            src_r,
                            src_g,
                            src_b,
                            acc_err_r,
                            acc_err_g,
                            acc_err_b,
                            params.threshold_scale,
                        );
                        quant_r = rgb.0;
                        quant_g = rgb.1;
                        quant_b = rgb.2;
                        q_err = err;
                    } else {
                        let adj_r = (src_r + acc_err_r).clamp(0.0, 1.0);
                        let adj_g = (src_g + acc_err_g).clamp(0.0, 1.0);
                        let adj_b = (src_b + acc_err_b).clamp(0.0, 1.0);
                        let (qr, qg, qb) =
                            quantize_ed_rgb(adj_r, adj_g, adj_b, levels, &palette_quant);
                        q_err = [adj_r - qr, adj_g - qg, adj_b - qb];
                        quant_r = qr;
                        quant_g = qg;
                        quant_b = qb;
                    }
                }
                DitherColorMode::Grayscale => {
                    if let PaletteQuant::Simple(picker) = &palette_quant {
                        let lum = to_luminance(src_r, src_g, src_b);
                        let (rgb, err) = simple_ed_step(
                            picker,
                            lum,
                            lum,
                            lum,
                            acc_err_r,
                            acc_err_g,
                            acc_err_b,
                            params.threshold_scale,
                        );
                        quant_r = rgb.0;
                        quant_g = rgb.1;
                        quant_b = rgb.2;
                        q_err = err;
                    } else {
                        let lum = to_luminance(src_r, src_g, src_b);
                        let adj_lum = (lum + acc_err_r).clamp(0.0, 1.0);
                        let (qr, qg, qb) = match &palette_quant {
                            PaletteQuant::Uniform => {
                                let q = quantize_uniform(adj_lum, levels);
                                (q, q, q)
                            }
                            _ => quantize_ed_rgb(adj_lum, adj_lum, adj_lum, levels, &palette_quant),
                        };
                        let quant_lum = to_luminance(qr, qg, qb);
                        let lum_err = adj_lum - quant_lum;
                        q_err = [lum_err, lum_err, lum_err];
                        quant_r = qr;
                        quant_g = qg;
                        quant_b = qb;
                    }
                }
            }

            // Write quantized pixel to output (at core area offset)
            result.set(tile_x, tile_y, 0, quant_r);
            result.set(tile_x, tile_y, 1, quant_g);
            result.set(tile_x, tile_y, 2, quant_b);
            if params.dither_alpha && result.at(tile_x, tile_y, 3) <= 0.0 {
                result.set(tile_x, tile_y, 0, 0.0);
                result.set(tile_x, tile_y, 1, 0.0);
                result.set(tile_x, tile_y, 2, 0.0);
            }

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

            // Error from the representative is not diffused inside the same
            // block; kernel neighbors are the next block representatives (`ps`).
            let Some(kernel) = params.mode.diffusion_kernel() else {
                unreachable!("error diffusion engine called with ordered mode");
            };
            distribute_kernel(
                &mut error_buf,
                x,
                y,
                q_err,
                kernel.offsets(),
                &mut right_overflow,
                &mut bottom_overflow,
                &mut corner_overflow,
                row_dir,
                ps.max(1) as i32,
            );
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
    use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, PaletteDitherMode};
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
    fn guided_ed_stays_within_channel_range() {
        use engine_color::palette::LinearColor;
        use engine_color::palette_guided::palette_channel_ranges;

        let tile = make_uniform_tile(0.5, 0.4, 0.6, 1.0);
        let mut params = make_fs_params(4);
        params.palette_dither_mode = PaletteDitherMode::Guided {
            channel_levels: Some(3),
        };
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let palette_id = doc.add_palette(
            "ed-g".into(),
            vec![
                LinearColor { r: 0.2, g: 0.1, b: 0.3 },
                LinearColor { r: 0.7, g: 0.8, b: 0.9 },
            ],
        );
        params.palette_id = Some(palette_id);
        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, LayerId::new(1),
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        let ranges = palette_channel_ranges(palette);
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..3 {
                    let v = result.at(x, y, c);
                    assert!(v >= ranges[c as usize].min - 1e-5 && v <= ranges[c as usize].max + 1e-5);
                }
            }
        }
    }

    #[test]
    fn mixed_ed_output_is_palette_color() {
        use engine_color::palette::LinearColor;

        let tile = make_uniform_tile(0.5, 0.4, 0.6, 1.0);
        let mut params = make_fs_params(4);
        params.palette_dither_mode = PaletteDitherMode::Mixed {
            channel_levels: Some(3),
        };
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let palette_id = doc.add_palette(
            "ed-m".into(),
            vec![
                LinearColor { r: 0.2, g: 0.1, b: 0.3 },
                LinearColor { r: 0.7, g: 0.8, b: 0.9 },
            ],
        );
        params.palette_id = Some(palette_id);
        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, LayerId::new(1),
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                let rgb = [result.at(x, y, 0), result.at(x, y, 1), result.at(x, y, 2)];
                assert!(palette.colors.iter().any(|c| {
                    (c.r - rgb[0]).abs() < 1e-5
                        && (c.g - rgb[1]).abs() < 1e-5
                        && (c.b - rgb[2]).abs() < 1e-5
                }));
            }
        }
    }

    #[test]
    fn mixed_ed_residual_uses_snapped_not_guided() {
        use engine_color::palette::LinearColor;

        // 25% gray, B/W palette. Guided stays ~0.25 (in [0,1]); snap → black.
        // Wrong residual (orig - guided ≈ 0) → entire tile black.
        // Right residual (orig - snapped) → FS mixes black/white, mean ~0.25.
        let tile = make_uniform_tile(0.25, 0.25, 0.25, 1.0);
        let mut params = make_fs_params(4);
        params.palette_dither_mode = PaletteDitherMode::Mixed {
            channel_levels: Some(8),
        };
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let palette_id = doc.add_palette(
            "bw".into(),
            vec![
                LinearColor { r: 0.0, g: 0.0, b: 0.0 },
                LinearColor { r: 1.0, g: 1.0, b: 1.0 },
            ],
        );
        params.palette_id = Some(palette_id);
        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, LayerId::new(1),
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();

        let mut sum = 0.0f64;
        let mut n = 0u32;
        let mut saw_black = false;
        let mut saw_white = false;
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                let lum = result.at(x, y, 0);
                sum += lum as f64;
                n += 1;
                if lum < 0.01 {
                    saw_black = true;
                }
                if lum > 0.99 {
                    saw_white = true;
                }
            }
        }
        let mean = sum / n as f64;
        assert!(
            saw_black && saw_white,
            "Mixed ED must dither both palette neighbors (black={saw_black} white={saw_white})"
        );
        assert!(
            (mean - 0.25).abs() < 0.08,
            "Mixed ED mean {mean} should track source 0.25 (residual from snapped, not guided)"
        );
    }

    #[test]
    fn simple_ed_output_is_palette_color() {
        use engine_color::palette::LinearColor;

        let tile = make_uniform_tile(0.3, 0.5, 0.7, 1.0);
        let mut params = make_fs_params(4);
        params.palette_dither_mode = PaletteDitherMode::Simple;
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let palette_id = doc.add_palette(
            "s".into(),
            vec![
                LinearColor { r: 0.2, g: 0.1, b: 0.3 },
                LinearColor { r: 0.7, g: 0.8, b: 0.9 },
            ],
        );
        params.palette_id = Some(palette_id);
        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, LayerId::new(1),
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                let rgb = [result.at(x, y, 0), result.at(x, y, 1), result.at(x, y, 2)];
                assert!(palette.colors.iter().any(|c| {
                    (c.r - rgb[0]).abs() < 1e-5
                        && (c.g - rgb[1]).abs() < 1e-5
                        && (c.b - rgb[2]).abs() < 1e-5
                }));
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
        let mut params = make_fs_params(4);
        params.dither_alpha = false;
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
    fn dither_alpha_binarizes_soft_core_alpha() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 0.4);
        let mut params = make_fs_params(4);
        params.dither_alpha = true;
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
                assert_eq!(
                    result.at(x, y, 3), 0.0,
                    "0.4 alpha rounds below 0.5 → transparent at ({}, {})", x, y
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

    #[test]
    fn error_diffusion_ignores_threshold_bias() {
        let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
        let mut params = make_fs_params(4);
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let a = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        params.threshold_bias = 0.3;
        let store2 = ErrorResidualsStore::new();
        let b = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store2, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_eq!(a.data, b.data, "ED modes must ignore threshold_bias");
    }

    fn channel0_at(buf: &[f32], x: usize, y: usize) -> f32 {
        buf[(y * SIZE + x) * 3]
    }

    #[test]
    fn unit_error_lands_on_published_offsets() {
        use crate::filter::DiffusionKernel;
        let kernels = [
            DiffusionKernel::FloydSteinberg,
            DiffusionKernel::Atkinson,
            DiffusionKernel::JarvisJudiceNinke,
            DiffusionKernel::Stucki,
            DiffusionKernel::Burkes,
            DiffusionKernel::Sierra,
        ];
        let x = 8usize;
        let y = 8usize;
        let err = [1.0f32, 0.0, 0.0];
        for kernel in kernels {
            let mut error_buf = vec![0.0f32; SIZE * SIZE * 3];
            let mut right = vec![0.0f32; SIZE * 2 * 3];
            let mut bottom = vec![0.0f32; 2 * SIZE * 3];
            let mut corner = vec![0.0f32; CORNER_PATCH * CORNER_PATCH * 3];
            distribute_kernel(
                &mut error_buf,
                x,
                y,
                err,
                kernel.offsets(),
                &mut right,
                &mut bottom,
                &mut corner,
                1,
                1,
            );
            for &(dx, dy, weight) in kernel.offsets() {
                let nx = (x as i32 + dx) as usize;
                let ny = (y as i32 + dy) as usize;
                let got = channel0_at(&error_buf, nx, ny);
                assert!(
                    (got - weight).abs() < 1e-6,
                    "{kernel:?} at ({dx},{dy}): expected {weight}, got {got}"
                );
            }
            assert!(right.iter().all(|v| *v == 0.0), "{kernel:?} interior must not overflow right");
            assert!(bottom.iter().all(|v| *v == 0.0), "{kernel:?} interior must not overflow bottom");
            assert!(corner.iter().all(|v| *v == 0.0), "{kernel:?} interior must not overflow corner");
        }
    }

    #[test]
    fn jjn_right_edge_overflow_uses_depth_2() {
        use crate::filter::DiffusionKernel;
        let mut error_buf = vec![0.0f32; SIZE * SIZE * 3];
        let mut right = vec![0.0f32; SIZE * 2 * 3];
        let mut bottom = vec![0.0f32; 2 * SIZE * 3];
        let mut corner = vec![0.0f32; CORNER_PATCH * CORNER_PATCH * 3];
        let y = 4usize;
        distribute_kernel(
            &mut error_buf,
            SIZE - 1,
            y,
            [1.0, 0.0, 0.0],
            DiffusionKernel::JarvisJudiceNinke.offsets(),
            &mut right,
            &mut bottom,
            &mut corner,
            1,
            1,
        );
        // (dx=+1, dy=0) → col 0 of right overflow, weight 7/48
        let col0 = (y * 2 + 0) * 3;
        // (dx=+2, dy=0) → col 1, weight 5/48
        let col1 = (y * 2 + 1) * 3;
        assert!((right[col0] - 7.0 / 48.0).abs() < 1e-6);
        assert!((right[col1] - 5.0 / 48.0).abs() < 1e-6);
    }

    #[test]
    fn row_direction_uses_global_y_not_local() {
        assert_eq!(row_direction(false, 1), 1);
        assert_eq!(row_direction(false, 0), 1);
        assert_eq!(row_direction(true, 0), 1);
        assert_eq!(row_direction(true, 1), -1);
        assert_eq!(row_direction(true, 256), 1);
        assert_eq!(row_direction(true, 257), -1);
        assert_eq!(row_direction(true, -1), -1);
    }

    #[test]
    fn rtl_mirrors_kernel_in_x() {
        use crate::filter::DiffusionKernel;
        let mut error_buf = vec![0.0f32; SIZE * SIZE * 3];
        let mut right = vec![0.0f32; SIZE * 2 * 3];
        let mut bottom = vec![0.0f32; 2 * SIZE * 3];
        let mut corner = vec![0.0f32; CORNER_PATCH * CORNER_PATCH * 3];
        let x = 8usize;
        let y = 8usize;
        distribute_kernel(
            &mut error_buf,
            x,
            y,
            [1.0, 0.0, 0.0],
            DiffusionKernel::FloydSteinberg.offsets(),
            &mut right,
            &mut bottom,
            &mut corner,
            -1,
            1,
        );
        // (+1,0) 7/16 → screen (-1,0)
        assert!((channel0_at(&error_buf, x - 1, y) - 7.0 / 16.0).abs() < 1e-6);
        // (-1,1) 3/16 → screen (+1,1)
        assert!((channel0_at(&error_buf, x + 1, y + 1) - 3.0 / 16.0).abs() < 1e-6);
        assert!((channel0_at(&error_buf, x, y + 1) - 5.0 / 16.0).abs() < 1e-6);
        assert!((channel0_at(&error_buf, x - 1, y + 1) - 1.0 / 16.0).abs() < 1e-6);
        assert_eq!(channel0_at(&error_buf, x + 1, y), 0.0);
    }

    #[test]
    fn kernel_step_lands_on_next_block_not_next_pixel() {
        use crate::filter::DiffusionKernel;
        let mut error_buf = vec![0.0f32; SIZE * SIZE * 3];
        let mut right = vec![0.0f32; SIZE * 2 * 3];
        let mut bottom = vec![0.0f32; 2 * SIZE * 3];
        let mut corner = vec![0.0f32; CORNER_PATCH * CORNER_PATCH * 3];
        let x = 8usize;
        let y = 8usize;
        distribute_kernel(
            &mut error_buf,
            x,
            y,
            [1.0, 0.0, 0.0],
            DiffusionKernel::FloydSteinberg.offsets(),
            &mut right,
            &mut bottom,
            &mut corner,
            1,
            4,
        );
        assert_eq!(channel0_at(&error_buf, x + 1, y), 0.0);
        assert!((channel0_at(&error_buf, x + 4, y) - 7.0 / 16.0).abs() < 1e-6);
        assert!((channel0_at(&error_buf, x, y + 4) - 5.0 / 16.0).abs() < 1e-6);
    }

    #[test]
    fn fs_pixel_size_4_still_diffuses_between_blocks() {
        use engine_color::palette::LinearColor;
        use std::collections::BTreeSet;

        let tile = make_uniform_tile(0.25, 0.25, 0.25, 1.0);
        let mut params = make_fs_params(2);
        params.pixel_size = 4;
        params.palette_dither_mode = PaletteDitherMode::Simple;
        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let palette_id = doc.add_palette(
            "bw".into(),
            vec![
                LinearColor { r: 0.0, g: 0.0, b: 0.0 },
                LinearColor { r: 1.0, g: 1.0, b: 1.0 },
            ],
        );
        params.palette_id = Some(palette_id);
        let result = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store, LayerId::new(1),
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let mut lums = BTreeSet::new();
        for y in (HALO..HALO + TILE_SIZE).step_by(4) {
            for x in (HALO..HALO + TILE_SIZE).step_by(4) {
                lums.insert(result.at(x, y, 0).to_bits());
            }
        }
        assert!(
            lums.len() >= 2,
            "FS pixel_size=4 must dither across blocks, not nearest-only (got {} unique)",
            lums.len()
        );
    }

    #[test]
    fn serpentine_false_is_bit_identical_to_ltr() {
        let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
        let mut params = make_fs_params(4);
        params.serpentine = false;
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);

        let store_a = ErrorResidualsStore::new();
        let a = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store_a, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let store_b = ErrorResidualsStore::new();
        let defaulted = make_fs_params(4);
        assert!(!defaulted.serpentine);
        let b = apply_error_diffusion(
            &tile, tc(0, 0), &defaulted, &store_b, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_eq!(a.data, b.data);

        let mut on = make_fs_params(4);
        on.serpentine = true;
        let store_c = ErrorResidualsStore::new();
        let c = apply_error_diffusion(
            &tile, tc(0, 0), &on, &store_c, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_ne!(a.data, c.data, "serpentine ON must change odd-row output");
    }

    #[test]
    fn serpentine_false_atkinson_identity() {
        let tile = make_uniform_tile(0.4, 0.5, 0.6, 1.0);
        let mut params = make_atkinson_params(4);
        params.serpentine = false;
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let layer_id = LayerId::new(1);
        let store_a = ErrorResidualsStore::new();
        let a = apply_error_diffusion(
            &tile, tc(0, 0), &params, &store_a, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        let store_b = ErrorResidualsStore::new();
        let b = apply_error_diffusion(
            &tile, tc(0, 0), &make_atkinson_params(4), &store_b, layer_id,
            &palette_cache, &lut_cache, &doc,
        )
        .unwrap();
        assert_eq!(a.data, b.data);
    }
}
