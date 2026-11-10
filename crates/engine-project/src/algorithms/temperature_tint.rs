//! Temperature / Tint (`temperature_tint`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::temperature_tint::apply_temperature_tint_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "temperature",
        label: "Temperature",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
    ParamField::Slider {
        key: "tint",
        label: "Tint",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
];

#[derive(Deserialize)]
struct TemperatureTintParams {
    #[serde(default)]
    temperature: f32,
    #[serde(default)]
    tint: f32,
}

pub struct TemperatureTint;

impl FilterAlgorithm for TemperatureTint {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("temperature_tint")
    }

    fn display_name(&self) -> &'static str {
        "Temperature / Tint"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: TemperatureTintParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_temperature_tint_into(&src, params.temperature, params.tint, tile);
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
