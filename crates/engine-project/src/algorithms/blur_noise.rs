//! Blur / noise (`blur_noise`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::blur_noise::apply_blur_noise_into;
use crate::filters::context::FilterContext;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "blur",
        label: "Blur",
        min: 0.0,
        max: 2.0,
        default: 0.0,
        step: Some(0.05),
    },
    ParamField::Slider {
        key: "noise",
        label: "Noise",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: Some(0.01),
    },
];

#[derive(Deserialize)]
struct BlurNoiseParams {
    #[serde(default)]
    blur: f32,
    #[serde(default)]
    noise: f32,
}

pub struct BlurNoise;

impl FilterAlgorithm for BlurNoise {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("blur_noise")
    }

    fn display_name(&self) -> &'static str {
        "Blur / Noise"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: BlurNoiseParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_blur_noise_into(&src, ctx.coord, params.blur, params.noise, tile);
        Ok(())
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        match serde_json::from_value::<BlurNoiseParams>(params.clone()) {
            Ok(p) if p.blur == 0.0 && p.noise == 0.0 => GpuEligibility::Eligible,
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
