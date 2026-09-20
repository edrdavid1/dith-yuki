//! CMYK angled-screen halftone (`cmyk_halftone`).

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
        key: "halftone_cell_size",
        label: "Cell Size",
        min: 2.0,
        max: 64.0,
        default: 8.0,
        step: Some(1.0),
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
        key: "threshold_bias",
        label: "Threshold Bias",
        min: -0.5,
        max: 0.5,
        default: 0.0,
        step: None,
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[("rgb", "RGB"), ("grayscale", "Grayscale")],
        default: "rgb",
    },
];

pub struct CmykHalftone;

impl FilterAlgorithm for CmykHalftone {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("cmyk_halftone")
    }

    fn display_name(&self) -> &'static str {
        "CMYK Halftone"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::CmykHalftone;
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

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        match serde_json::from_value::<DitherParamsV2>(params.clone()) {
            Ok(p) if p.threshold_bias == 0.0 => GpuEligibility::Eligible,
            Ok(_) => GpuEligibility::Cpu(CpuCheckpointKind::IneligibleDither),
            Err(_) => GpuEligibility::Cpu(CpuCheckpointKind::IneligibleDither),
        }
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
