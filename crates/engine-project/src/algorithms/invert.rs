//! Invert RGB (`invert`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

use crate::filters::invert::apply_invert_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[];

pub struct Invert;

impl FilterAlgorithm for Invert {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("invert")
    }

    fn display_name(&self) -> &'static str {
        "Invert"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        _params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let src = scratch_src(tile);
        apply_invert_into(&src, tile);
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
