//! Bayer ordered-dither algorithms (`bayer_2x2`, `bayer_4x4`, `bayer_8x8`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::context::FilterContext;
use crate::filters::dither_ordered::apply_ordered_with_cache_into;
use crate::filters::gpu_bridge::bayer_gpu_eligible;

/// Parameter schema shared by Bayer ordered-dither algorithms (Req 5.1).
const BAYER_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "levels",
        label: "Levels",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: None,
    },
    ParamField::Slider {
        key: "threshold_scale",
        label: "Threshold Scale",
        min: 0.1,
        max: 4.0,
        default: 1.0,
        step: None,
    },
    ParamField::Slider {
        key: "pixel_size",
        label: "Pixel Size",
        min: 1.0,
        max: 32.0,
        default: 1.0,
        step: Some(1.0),
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[("rgb", "RGB"), ("grayscale", "Grayscale")],
        default: "rgb",
    },
    ParamField::Slider {
        key: "threshold_bias",
        label: "Threshold Bias",
        min: -0.5,
        max: 0.5,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "pattern_angle",
        label: "Pattern Angle",
        min: 0.0,
        max: 359.0,
        default: 0.0,
        step: None,
    },
];

/// 2×2 Bayer ordered dither (`bayer_2x2`).
pub struct Bayer2x2;

/// 4×4 Bayer ordered dither (`bayer_4x4`).
pub struct Bayer4x4;

/// 8×8 Bayer ordered dither (`bayer_8x8`).
pub struct Bayer8x8;

fn parse_dither_params(params: &serde_json::Value) -> Result<DitherParamsV2, serde_json::Error> {
    serde_json::from_value(params.clone())
}

fn gpu_eligibility_for(mode: DitherModeV2, params: &serde_json::Value) -> GpuEligibility {
    match parse_dither_params(params) {
        Ok(mut p) => {
            p.mode = mode;
            if p.pixel_size > 1 {
                return GpuEligibility::Cpu(CpuCheckpointKind::BlockGranularity);
            }
            if matches!(
                p.palette_dither_mode,
                crate::filter::PaletteDitherMode::Guided { .. }
                    | crate::filter::PaletteDitherMode::Mixed { .. }
            ) {
                return GpuEligibility::Eligible;
            }
            if bayer_gpu_eligible(&p) {
                GpuEligibility::Eligible
            } else {
                GpuEligibility::Cpu(CpuCheckpointKind::IneligibleDither)
            }
        }
        Err(_) => GpuEligibility::Cpu(CpuCheckpointKind::IneligibleDither),
    }
}

fn apply_bayer(
    tile: &mut PixelTile,
    mut params: DitherParamsV2,
    mode: DitherModeV2,
    ctx: &dyn FilterCtx,
) -> Result<(), FilterError> {
    params.mode = mode;
    let ctx = FilterContext::from_ctx(ctx);
    // `apply_ordered_with_cache_into` forbids src/dst aliasing.
    let mut src = PixelTile::new();
    src.copy_from(tile);
    apply_ordered_with_cache_into(
        &src,
        ctx.coord,
        &params,
        ctx.threshold_cache,
        ctx.palette_cache,
        ctx.lut_cache,
        ctx.document,
        ctx.block_cache,
        ctx.layer_id,
        tile,
    )
    .map_err(FilterError::from)
}

impl FilterAlgorithm for Bayer2x2 {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("bayer_2x2")
    }

    fn display_name(&self) -> &'static str {
        "Bayer 2×2"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params = parse_dither_params(params)?;
        apply_bayer(tile, params, DitherModeV2::Bayer2x2, ctx)
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        gpu_eligibility_for(DitherModeV2::Bayer2x2, params)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        BAYER_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }

    fn requires_full_row(&self) -> bool {
        false
    }
}

impl FilterAlgorithm for Bayer4x4 {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("bayer_4x4")
    }

    fn display_name(&self) -> &'static str {
        "Bayer 4×4"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params = parse_dither_params(params)?;
        apply_bayer(tile, params, DitherModeV2::Bayer4x4, ctx)
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        gpu_eligibility_for(DitherModeV2::Bayer4x4, params)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        BAYER_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }

    fn requires_full_row(&self) -> bool {
        false
    }
}

impl FilterAlgorithm for Bayer8x8 {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("bayer_8x8")
    }

    fn display_name(&self) -> &'static str {
        "Bayer 8×8"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params = parse_dither_params(params)?;
        apply_bayer(tile, params, DitherModeV2::Bayer8x8, ctx)
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        gpu_eligibility_for(DitherModeV2::Bayer8x8, params)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        BAYER_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }

    fn requires_full_row(&self) -> bool {
        false
    }
}
