//! Exposure in EV stops (`exposure`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::exposure::apply_exposure_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[ParamField::Slider {
    key: "stops",
    label: "Exposure",
    min: -5.0,
    max: 5.0,
    default: 0.0,
    step: Some(0.01),
}];

#[derive(Deserialize)]
struct ExposureParams {
    #[serde(default)]
    stops: f32,
}

pub struct Exposure;

impl FilterAlgorithm for Exposure {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("exposure")
    }

    fn display_name(&self) -> &'static str {
        "Exposure"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: ExposureParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_exposure_into(&src, params.stops, tile);
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
