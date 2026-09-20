//! Contrast / brightness / saturation / blur (`adjust`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::adjust::apply_adjust_into;
use crate::filters::context::FilterContext;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "contrast",
        label: "Contrast",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "brightness",
        label: "Brightness",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "saturation",
        label: "Saturation",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "blur",
        label: "Blur",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "sharpness",
        label: "Sharpness",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "noise",
        label: "Noise",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
];

#[derive(Deserialize)]
struct AdjustParams {
    contrast: f32,
    brightness: f32,
    saturation: f32,
    blur: f32,
    sharpness: f32,
    noise: f32,
}

pub struct Adjust;

impl FilterAlgorithm for Adjust {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("adjust")
    }

    fn display_name(&self) -> &'static str {
        "Adjust"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: AdjustParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_adjust_into(
            &src,
            ctx.coord,
            params.contrast,
            params.brightness,
            params.saturation,
            params.blur,
            params.sharpness,
            params.noise,
            tile,
        );
        Ok(())
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        match serde_json::from_value::<AdjustParams>(params.clone()) {
            Ok(p) if p.blur == 0.0 => GpuEligibility::Eligible,
            Ok(_) => GpuEligibility::Cpu(CpuCheckpointKind::AdjustBlur),
            Err(_) => GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
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
