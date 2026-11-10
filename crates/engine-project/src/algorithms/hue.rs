//! Hue rotate (`hue`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::hue::apply_hue_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[ParamField::Slider {
    key: "degrees",
    label: "Hue",
    min: -180.0,
    max: 180.0,
    default: 0.0,
    step: Some(1.0),
}];

#[derive(Deserialize)]
struct HueParams {
    #[serde(default)]
    degrees: f32,
}

pub struct Hue;

impl FilterAlgorithm for Hue {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("hue")
    }

    fn display_name(&self) -> &'static str {
        "Hue"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: HueParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_hue_into(&src, params.degrees, tile);
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
