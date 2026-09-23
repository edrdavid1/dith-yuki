//! Crosshatch dither (`crosshatch_dither`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::context::FilterContext;
use crate::filters::dither_ordered::apply_ordered_with_cache_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "wave_wavelength",
        label: "Spacing",
        min: 2.0,
        max: 64.0,
        default: 8.0,
        step: None,
    },
    ParamField::Slider {
        key: "pattern_angle",
        label: "Base Angle",
        min: 0.0,
        max: 359.0,
        default: 45.0,
        step: None,
    },
    ParamField::Slider {
        key: "threshold_scale",
        label: "Line Thickness",
        min: 0.1,
        max: 4.0,
        default: 1.0,
        step: None,
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[("rgb", "RGB"), ("grayscale", "Grayscale")],
        default: "grayscale",
    },
];

pub struct CrosshatchDither;

impl FilterAlgorithm for CrosshatchDither {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("crosshatch_dither")
    }

    fn display_name(&self) -> &'static str {
        "Crosshatch"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::CrosshatchDither;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
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

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }
}
