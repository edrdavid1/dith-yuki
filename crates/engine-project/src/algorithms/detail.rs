//! Detail (`detail`) — clarity + sharpness.

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::context::FilterContext;
use crate::filters::detail::apply_detail_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "clarity",
        label: "Clarity",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
    ParamField::Slider {
        key: "sharpness",
        label: "Sharpness",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: Some(0.05),
    },
];

#[derive(Deserialize)]
struct DetailParams {
    #[serde(default)]
    clarity: f32,
    #[serde(default)]
    sharpness: f32,
}

pub struct Detail;

impl FilterAlgorithm for Detail {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("detail")
    }

    fn display_name(&self) -> &'static str {
        "Detail"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: DetailParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_detail_into(&src, ctx.coord, params.clarity, params.sharpness, tile);
        Ok(())
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        match serde_json::from_value::<DetailParams>(params.clone()) {
            Ok(p) if p.clarity == 0.0 && p.sharpness == 0.0 => GpuEligibility::Eligible,
            Ok(_) => GpuEligibility::Cpu(CpuCheckpointKind::AdjustBlur),
            Err(_) => GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
        }
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::ColorAdjust
    }
}
