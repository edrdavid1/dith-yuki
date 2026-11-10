//! Clarity (`clarity`) — midtone local contrast.

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::clarity::apply_clarity_into;
use crate::filters::context::FilterContext;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[ParamField::Slider {
    key: "amount",
    label: "Clarity",
    min: -1.0,
    max: 1.0,
    default: 0.0,
    step: Some(0.01),
}];

#[derive(Deserialize)]
struct ClarityParams {
    #[serde(default)]
    amount: f32,
}

pub struct Clarity;

impl FilterAlgorithm for Clarity {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("clarity")
    }

    fn display_name(&self) -> &'static str {
        "Clarity"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: ClarityParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_clarity_into(&src, ctx.coord, params.amount, tile);
        Ok(())
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
        EffectCategory::ColorAdjust
    }
}
