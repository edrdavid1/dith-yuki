//! Bridge: PixelTile core ↔ GPU RGBA32 float + eligibility helpers.

use engine_gpu::{
    apply_bayer_gpu, apply_crt_gpu, apply_halftone_gpu, gpu_filters_enabled, BayerGpuParams,
    BayerMatrixSize, CrtGpuParams, GpuContext, HalftoneGpuParams, CORE_SIZE, FLOATS_PER_TILE,
};
use engine_tiles::{PixelTile, TileCoord, HALO};

use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};

/// Extract 256×256 core as tightly packed RGBA32 float.
pub fn extract_core(tile: &PixelTile) -> Vec<f32> {
    let mut out = vec![0.0f32; FLOATS_PER_TILE];
    for y in 0..CORE_SIZE {
        for x in 0..CORE_SIZE {
            let dst = ((y * CORE_SIZE + x) * 4) as usize;
            let sx = x + HALO;
            let sy = y + HALO;
            out[dst] = tile.at(sx, sy, 0);
            out[dst + 1] = tile.at(sx, sy, 1);
            out[dst + 2] = tile.at(sx, sy, 2);
            out[dst + 3] = tile.at(sx, sy, 3);
        }
    }
    out
}

/// Write core buffer into an existing tile (halo untouched).
pub fn write_core(tile: &mut PixelTile, core: &[f32]) {
    debug_assert_eq!(core.len(), FLOATS_PER_TILE);
    for y in 0..CORE_SIZE {
        for x in 0..CORE_SIZE {
            let src = ((y * CORE_SIZE + x) * 4) as usize;
            let dx = x + HALO;
            let dy = y + HALO;
            tile.set(dx, dy, 0, core[src]);
            tile.set(dx, dy, 1, core[src + 1]);
            tile.set(dx, dy, 2, core[src + 2]);
            tile.set(dx, dy, 3, core[src + 3]);
        }
    }
}

fn bayer_matrix(mode: &DitherModeV2) -> Option<BayerMatrixSize> {
    match mode {
        DitherModeV2::Bayer2x2 => Some(BayerMatrixSize::Bayer2),
        DitherModeV2::Bayer4x4 => Some(BayerMatrixSize::Bayer4),
        DitherModeV2::Bayer8x8 => Some(BayerMatrixSize::Bayer8),
        _ => None,
    }
}

/// GpuEligible Bayer: ps==1, no palette, RGB or Gray, prefer-GPU env on.
pub fn try_ordered_bayer_gpu(
    gpu: Option<&GpuContext>,
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
) -> Option<PixelTile> {
    if !gpu_filters_enabled() {
        return None;
    }
    let ctx = gpu?;
    if params.pixel_size != 1 || params.palette_id.is_some() {
        return None;
    }
    let matrix = bayer_matrix(&params.mode)?;
    let color_mode = match params.color_mode {
        DitherColorMode::Rgb => 0u32,
        DitherColorMode::Grayscale => 1u32,
    };
    let input = extract_core(tile);
    let out = apply_bayer_gpu(
        ctx,
        &input,
        BayerGpuParams {
            matrix,
            levels: params.levels,
            threshold_scale: params.threshold_scale,
            color_mode,
            tile_x: coord.x,
            tile_y: coord.y,
        },
    )
    .ok()?;
    let mut result = PixelTile::new();
    result.data.copy_from_slice(&tile.data);
    write_core(&mut result, &out);
    // Halo: leave as source (v1 core-only GPU path).
    Some(result)
}

pub fn try_halftone_gpu(
    gpu: Option<&GpuContext>,
    tile: &PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
) -> Option<PixelTile> {
    if !gpu_filters_enabled() {
        return None;
    }
    let ctx = gpu?;
    if params.pixel_size != 1 || params.palette_id.is_some() {
        return None;
    }
    if !matches!(params.mode, DitherModeV2::CmykHalftone) {
        return None;
    }
    // GPU shader assumes RGB input; grayscale → CPU for now.
    if matches!(params.color_mode, DitherColorMode::Grayscale) {
        return None;
    }
    let input = extract_core(tile);
    let out = apply_halftone_gpu(
        ctx,
        &input,
        HalftoneGpuParams {
            cell_size: params.halftone_cell_size,
            threshold_scale: params.threshold_scale,
            tile_x: coord.x,
            tile_y: coord.y,
        },
    )
    .ok()?;
    let mut result = PixelTile::new();
    result.data.copy_from_slice(&tile.data);
    write_core(&mut result, &out);
    Some(result)
}

pub fn try_crt_gpu(
    gpu: Option<&GpuContext>,
    tile: &PixelTile,
    coord: TileCoord,
    period: u8,
    strength: f32,
    mask_strength: f32,
) -> Option<PixelTile> {
    if !gpu_filters_enabled() {
        return None;
    }
    let ctx = gpu?;
    let input = extract_core(tile);
    let out = apply_crt_gpu(
        ctx,
        &input,
        CrtGpuParams {
            period,
            strength,
            mask_strength,
            tile_x: coord.x,
            tile_y: coord.y,
        },
    )
    .ok()?;
    let mut result = PixelTile::new();
    result.data.copy_from_slice(&tile.data);
    write_core(&mut result, &out);
    Some(result)
}
