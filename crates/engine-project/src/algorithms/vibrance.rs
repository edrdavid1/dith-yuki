//! Vibrance (`vibrance`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::vibrance::apply_vibrance_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[ParamField::Slider {
    key: "amount",
    label: "Vibrance",
    min: -1.0,
    max: 1.0,
    default: 0.0,
    step: Some(0.01),
}];

#[derive(Deserialize)]
struct VibranceParams {
    #[serde(default)]
    amount: f32,
}

pub struct Vibrance;

impl FilterAlgorithm for Vibrance {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("vibrance")
    }

    fn display_name(&self) -> &'static str {
        "Vibrance"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: VibranceParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_vibrance_into(&src, params.amount, tile);
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
