//! Levels / RGB channel remap (`levels`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::levels::LevelsFilter;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "input_black",
        label: "Input Black",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.001),
    },
    ParamField::Slider {
        key: "input_white",
        label: "Input White",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: Some(0.001),
    },
    ParamField::Slider {
        key: "gamma",
        label: "Gamma",
        min: 0.1,
        max: 10.0,
        default: 1.0,
        step: Some(0.01),
    },
    ParamField::Slider {
        key: "output_black",
        label: "Output Black",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.001),
    },
    ParamField::Slider {
        key: "output_white",
        label: "Output White",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        step: Some(0.001),
    },
];

fn default_channel_on() -> bool {
    true
}

#[derive(Deserialize)]
struct LevelsParams {
    #[serde(default)]
    input_black: f32,
    #[serde(default = "default_one")]
    input_white: f32,
    #[serde(default = "default_one")]
    gamma: f32,
    #[serde(default)]
    output_black: f32,
    #[serde(default = "default_one")]
    output_white: f32,
    #[serde(default = "default_channel_on")]
    channel_r: bool,
    #[serde(default = "default_channel_on")]
    channel_g: bool,
    #[serde(default = "default_channel_on")]
    channel_b: bool,
}

fn default_one() -> f32 {
    1.0
}

pub struct Levels;

impl FilterAlgorithm for Levels {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("levels")
    }

    fn display_name(&self) -> &'static str {
        "Levels"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: LevelsParams = serde_json::from_value(params.clone())?;
        let mut filter = LevelsFilter::new();
        filter.input_black = params.input_black;
        filter.input_white = params.input_white;
        filter.gamma = params.gamma;
        filter.output_black = params.output_black;
        filter.output_white = params.output_white;
        filter.channel_r = params.channel_r;
        filter.channel_g = params.channel_g;
        filter.channel_b = params.channel_b;
        filter.rebuild_lut();
        let src = scratch_src(tile);
        filter
            .apply_to_tile_into(&src, tile)
            .map_err(FilterError::from)
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
