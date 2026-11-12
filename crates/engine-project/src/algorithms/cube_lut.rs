//! Adobe `.cube` 3D LUT (`cube_lut`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::cube_lut::apply_cube_lut_into;

use super::scratch::scratch_src;

/// Path is edited via dedicated `CubeLutSettings` (file browse); amount is also there.
const SCHEMA: &[ParamField] = &[ParamField::Slider {
    key: "amount",
    label: "Amount",
    min: 0.0,
    max: 1.0,
    default: 1.0,
    step: Some(0.01),
}];

#[derive(Deserialize)]
struct CubeLutParams {
    #[serde(default)]
    path: String,
    #[serde(default = "default_amount")]
    amount: f32,
}

fn default_amount() -> f32 {
    1.0
}

pub struct CubeLut;

impl FilterAlgorithm for CubeLut {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("cube_lut")
    }

    fn display_name(&self) -> &'static str {
        "CUBE LUT"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: CubeLutParams = serde_json::from_value(params.clone())?;
        let src = scratch_src(tile);
        apply_cube_lut_into(&src, &params.path, params.amount, tile);
        Ok(())
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
