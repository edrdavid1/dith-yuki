//! Posterize / tone steps (`posterize`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::posterize::apply_posterize_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[ParamField::Slider {
    key: "levels",
    label: "Levels",
    min: 2.0,
    max: 64.0,
    default: 8.0,
    step: Some(1.0),
}];

#[derive(Deserialize)]
struct PosterizeParams {
    #[serde(default = "default_levels")]
    levels: u32,
}

fn default_levels() -> u32 {
    8
}

pub struct Posterize;

impl FilterAlgorithm for Posterize {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("posterize")
    }

    fn display_name(&self) -> &'static str {
        "Posterize"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: PosterizeParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_posterize_into(&src, params.levels, tile);
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
