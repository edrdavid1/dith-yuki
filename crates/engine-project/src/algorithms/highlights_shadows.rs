//! Highlights / Shadows (`highlights_shadows`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::highlights_shadows::apply_highlights_shadows_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "shadows",
        label: "Shadows",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
    ParamField::Slider {
        key: "highlights",
        label: "Highlights",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
];

#[derive(Deserialize)]
struct HighlightsShadowsParams {
    #[serde(default)]
    shadows: f32,
    #[serde(default)]
    highlights: f32,
}

pub struct HighlightsShadows;

impl FilterAlgorithm for HighlightsShadows {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("highlights_shadows")
    }

    fn display_name(&self) -> &'static str {
        "Highlights / Shadows"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: HighlightsShadowsParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_highlights_shadows_into(&src, params.shadows, params.highlights, tile);
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
