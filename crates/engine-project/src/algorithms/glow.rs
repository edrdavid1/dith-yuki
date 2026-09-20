//! Glow / bloom (`glow`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::glow::apply_glow_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "radius",
        label: "Radius",
        min: 0.5,
        max: 2.0,
        default: 1.0,
        step: None,
    },
    ParamField::Slider {
        key: "intensity",
        label: "Intensity",
        min: 0.0,
        max: 4.0,
        default: 1.0,
        step: None,
    },
    ParamField::Slider {
        key: "threshold",
        label: "Threshold",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
];

#[derive(Deserialize)]
struct GlowParams {
    radius: f32,
    intensity: f32,
    threshold: f32,
}

pub struct Glow;

impl FilterAlgorithm for Glow {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("glow")
    }

    fn display_name(&self) -> &'static str {
        "Glow"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: GlowParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_glow_into(
            &src,
            params.radius,
            params.intensity,
            params.threshold,
            tile,
        );
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
        EffectCategory::Stylize
    }
}
