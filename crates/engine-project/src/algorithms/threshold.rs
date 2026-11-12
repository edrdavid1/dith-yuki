//! Hard luminance threshold (`threshold`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::threshold::apply_threshold_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[ParamField::Slider {
    key: "threshold",
    label: "Threshold",
    min: 0.0,
    max: 1.0,
    default: 0.5,
    step: Some(0.01),
}];

#[derive(Deserialize)]
struct ThresholdParams {
    #[serde(default = "default_threshold")]
    threshold: f32,
}

fn default_threshold() -> f32 {
    0.5
}

pub struct Threshold;

impl FilterAlgorithm for Threshold {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("threshold")
    }

    fn display_name(&self) -> &'static str {
        "Threshold"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: ThresholdParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_threshold_into(&src, params.threshold, tile);
        Ok(())
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        if engine_gpu::pointwise_gpu_enabled() {
            GpuEligibility::Eligible
        } else {
            GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
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
