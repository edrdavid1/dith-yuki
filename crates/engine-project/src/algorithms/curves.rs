//! Tone curves (`curves`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::curves::{CurveChannel, CurvesFilter};

use super::scratch::scratch_src;

#[derive(Deserialize)]
struct CurvesParams {
    curve: Vec<(f32, f32)>,
    channel: CurveChannel,
}

pub struct Curves;

impl FilterAlgorithm for Curves {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("curves")
    }

    fn display_name(&self) -> &'static str {
        "Curves"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: CurvesParams = serde_json::from_value(params.clone())?;
        let mut filter = CurvesFilter::new(params.channel);
        for &(input, output) in &params.curve {
            filter.add_point(input, output).map_err(FilterError::from)?;
        }
        let src = scratch_src(tile);
        filter
            .apply_to_tile_into(&src, tile)
            .map_err(FilterError::from)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        &[]
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::ColorAdjust
    }
}
