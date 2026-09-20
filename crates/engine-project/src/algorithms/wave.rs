//! Wave ordered dither (`wave`).

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
        key: "levels",
        label: "Levels",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: None,
    },
    ParamField::Slider {
        key: "wave_wavelength",
        label: "Wavelength",
        min: 2.0,
        max: 256.0,
        default: 8.0,
        step: None,
    },
    ParamField::Slider {
        key: "wave_amplitude",
        label: "Amplitude",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: None,
    },
    ParamField::Slider {
        key: "wave_phase",
        label: "Phase",
        min: 0.0,
        max: 6.2831855,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "wave_angle",
        label: "Angle",
        min: 0.0,
        max: 359.0,
        default: 0.0,
        step: None,
    },
];

pub struct Wave;

impl FilterAlgorithm for Wave {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("wave")
    }

    fn display_name(&self) -> &'static str {
        "Wave"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::Wave;
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
        GpuEligibility::Cpu(CpuCheckpointKind::IneligibleDither)
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
