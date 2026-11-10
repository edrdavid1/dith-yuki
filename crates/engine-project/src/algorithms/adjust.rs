//! Contrast / brightness / saturation / grade / blur (`adjust`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::adjust::{apply_adjust_into, AdjustApplyParams};
use crate::filters::context::FilterContext;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "exposure",
        label: "Exposure",
        min: -5.0,
        max: 5.0,
        default: 0.0,
        step: Some(0.01),
    },
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
    ParamField::Slider {
        key: "saturation",
        label: "Saturation",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "vibrance",
        label: "Vibrance",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
    ParamField::Slider {
        key: "hue",
        label: "Hue",
        min: -180.0,
        max: 180.0,
        default: 0.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "clarity",
        label: "Clarity",
        min: -1.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
    ParamField::Slider {
        key: "sharpness",
        label: "Sharpness",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: None,
    },
];

#[derive(Deserialize)]
struct AdjustParams {
    #[serde(default)]
    contrast: f32,
    #[serde(default)]
    brightness: f32,
    #[serde(default)]
    saturation: f32,
    #[serde(default)]
    blur: f32,
    #[serde(default)]
    sharpness: f32,
    #[serde(default)]
    noise: f32,
    #[serde(default)]
    exposure: f32,
    #[serde(default)]
    shadows: f32,
    #[serde(default)]
    highlights: f32,
    #[serde(default)]
    temperature: f32,
    #[serde(default)]
    tint: f32,
    #[serde(default)]
    vibrance: f32,
    #[serde(default)]
    hue: f32,
    #[serde(default)]
    clarity: f32,
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
            AdjustApplyParams {
                contrast: params.contrast,
                brightness: params.brightness,
                saturation: params.saturation,
                blur: params.blur,
                sharpness: params.sharpness,
                noise: params.noise,
                exposure: params.exposure,
                shadows: params.shadows,
                highlights: params.highlights,
                temperature: params.temperature,
                tint: params.tint,
                vibrance: params.vibrance,
                hue: params.hue,
                clarity: params.clarity,
            },
            tile,
        );
        Ok(())
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        match serde_json::from_value::<AdjustParams>(params.clone()) {
            Ok(p) if p.blur == 0.0 && p.sharpness == 0.0 && p.noise == 0.0 && p.clarity == 0.0 => {
                GpuEligibility::Eligible
            }
            Ok(_) => GpuEligibility::Cpu(CpuCheckpointKind::AdjustBlur),
            Err(_) => GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
        }
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        2
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::ColorAdjust
    }
}
