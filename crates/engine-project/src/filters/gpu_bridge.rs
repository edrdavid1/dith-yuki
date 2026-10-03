//! Bridge: PixelTile core ↔ GPU RGBA32 float + eligibility helpers.

use engine_gpu::{BayerMatrixSize, CORE_SIZE, FLOATS_PER_TILE};
use engine_tiles::{PixelTile, HALO};

use crate::filter::{DitherModeV2, DitherParamsV2};

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
        DitherModeV2::Bayer16x16 => Some(BayerMatrixSize::Bayer16),
        DitherModeV2::ClusteredDotOrdered => Some(BayerMatrixSize::ClusteredDot),
        DitherModeV2::DispersedDotOrdered => Some(BayerMatrixSize::DispersedDot),
        _ => None,
    }
}

fn track_h_ordered_eligible(params: &DitherParamsV2) -> bool {
    params.pixel_size == 1
        && params.palette_id.is_none()
        && !params.palette_dither_mode.is_guided()
        && params.threshold_bias == 0.0
        && params.pattern_angle == 0.0
}

/// Cpu path is source of truth for Track H: skip GPU when bias/angle are non-default.
pub(crate) fn bayer_gpu_eligible(params: &DitherParamsV2) -> bool {
    track_h_ordered_eligible(params) && bayer_matrix(&params.mode).is_some()
}

/// Same Track H gate as Bayer: ps=1, no palette, default bias/angle.
pub(crate) fn void_and_cluster_gpu_eligible(params: &DitherParamsV2) -> bool {
    track_h_ordered_eligible(params) && matches!(params.mode, DitherModeV2::VoidAndCluster)
}

/// Classical clustered-dot 8×8 — same Track H gate as Bayer.
pub(crate) fn clustered_dot_gpu_eligible(params: &DitherParamsV2) -> bool {
    track_h_ordered_eligible(params) && matches!(params.mode, DitherModeV2::ClusteredDotOrdered)
}

/// Ulichney dispersed-dot 16×16 — same Track H gate as Bayer.
pub(crate) fn dispersed_dot_gpu_eligible(params: &DitherParamsV2) -> bool {
    track_h_ordered_eligible(params) && matches!(params.mode, DitherModeV2::DispersedDotOrdered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, PaletteDitherMode};

    fn bayer_params() -> DitherParamsV2 {
        DitherParamsV2 {
            mode: DitherModeV2::Bayer4x4,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }
    }

    #[test]
    fn bayer_gpu_eligible_at_defaults() {
        assert!(bayer_gpu_eligible(&bayer_params()));
    }

    #[test]
    fn bayer_gpu_skips_non_default_bias() {
        let mut p = bayer_params();
        p.threshold_bias = 0.1;
        assert!(!bayer_gpu_eligible(&p));
    }

    #[test]
    fn bayer_gpu_skips_non_default_angle() {
        let mut p = bayer_params();
        p.pattern_angle = 15.0;
        assert!(!bayer_gpu_eligible(&p));
    }

    #[test]
    fn bayer_gpu_skips_pixel_size() {
        let mut p = bayer_params();
        p.pixel_size = 2;
        assert!(!bayer_gpu_eligible(&p));
    }

    #[test]
    fn guided_gpu_not_eligible() {
        let mut p = bayer_params();
        p.palette_dither_mode = PaletteDitherMode::Guided {
            channel_levels: None,
        };
        assert!(!bayer_gpu_eligible(&p));
        p.palette_id = Some(crate::types::PaletteId::new(1));
        p.palette_dither_mode = PaletteDitherMode::Strict;
        assert!(!bayer_gpu_eligible(&p));
        p.palette_id = None;
        p.palette_dither_mode = PaletteDitherMode::Mixed {
            channel_levels: Some(4),
        };
        assert!(!bayer_gpu_eligible(&p));
    }

    fn vac_params() -> DitherParamsV2 {
        DitherParamsV2 {
            mode: DitherModeV2::VoidAndCluster,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }
    }

    #[test]
    fn vac_gpu_eligible_at_defaults() {
        assert!(void_and_cluster_gpu_eligible(&vac_params()));
    }

    #[test]
    fn vac_gpu_skips_non_default_bias() {
        let mut p = vac_params();
        p.threshold_bias = 0.1;
        assert!(!void_and_cluster_gpu_eligible(&p));
    }

    #[test]
    fn vac_gpu_skips_non_default_angle() {
        let mut p = vac_params();
        p.pattern_angle = 15.0;
        assert!(!void_and_cluster_gpu_eligible(&p));
    }

    #[test]
    fn vac_gpu_skips_pixel_size() {
        let mut p = vac_params();
        p.pixel_size = 2;
        assert!(!void_and_cluster_gpu_eligible(&p));
    }
}
