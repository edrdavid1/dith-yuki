//! Glitch effects (`glitch`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::context::FilterContext;
use crate::filters::glitch::{GlitchFilter, GlitchType};

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Dropdown {
        key: "glitch_type",
        label: "Type",
        options: &[
            ("RGBShift", "RGB Shift"),
            ("BlockDisplace", "Block Displace"),
        ],
        default: "RGBShift",
    },
    ParamField::Slider {
        key: "intensity",
        label: "Intensity",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: None,
    },
];

#[derive(Deserialize)]
struct GlitchParams {
    glitch_type: GlitchType,
    intensity: f32,
    seed: u64,
}

pub struct Glitch;

impl FilterAlgorithm for Glitch {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("glitch")
    }

    fn display_name(&self) -> &'static str {
        "Glitch"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: GlitchParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let filter = GlitchFilter::new(params.glitch_type, params.intensity, params.seed)
            .map_err(FilterError::from)?;
        let src = scratch_src(tile);
        filter
            .apply_to_tile_into(&src, ctx.coord, tile)
            .map_err(FilterError::from)
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
        EffectCategory::Glitch
    }
}
