//! CRT scanlines (`crt`).

use engine_registry::{
    AlgorithmId, EffectCategory, FilterAlgorithm, FilterCtx, FilterError, GpuEligibility,
    ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::context::FilterContext;
use crate::filters::crt::apply_crt_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "period",
        label: "Period",
        min: 2.0,
        max: 8.0,
        default: 2.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "strength",
        label: "Strength",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: None,
    },
    ParamField::Slider {
        key: "mask_strength",
        label: "Mask Strength",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
];

#[derive(Deserialize)]
struct CrtParams {
    period: u8,
    strength: f32,
    #[serde(default)]
    mask_strength: f32,
}

pub struct Crt;

impl FilterAlgorithm for Crt {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("crt")
    }

    fn display_name(&self) -> &'static str {
        "CRT"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: CrtParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_crt_into(
            &src,
            ctx.coord,
            params.period,
            params.strength,
            params.mask_strength,
            tile,
        );
        Ok(())
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Eligible
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
