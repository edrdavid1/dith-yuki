//! Dispersed-dot ordered dither (`dispersed_dot_ordered`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::context::FilterContext;
use crate::filters::dither_ordered::apply_ordered_with_cache_into;

/// Same schema as Bayer / clustered ordered dither.
const DISPERSED_DOT_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "levels",
        label: "Levels",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: None,
    },
    ParamField::Slider {
        key: "threshold_scale",
        label: "Threshold Scale",
        min: 0.1,
        max: 4.0,
        default: 1.0,
        step: None,
    },
    ParamField::Slider {
        key: "pixel_size",
        label: "Pixel Size",
        min: 1.0,
        max: 32.0,
        default: 1.0,
        step: Some(1.0),
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[("rgb", "RGB"), ("grayscale", "Grayscale")],
        default: "rgb",
    },
    ParamField::Slider {
        key: "threshold_bias",
        label: "Threshold Bias",
        min: -0.5,
        max: 0.5,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "pattern_angle",
        label: "Pattern Angle",
        min: 0.0,
        max: 359.0,
        default: 0.0,
        step: None,
    },
];

/// Ulichney dispersed-dot ordered dither (16×16, non-Bayer).
pub struct DispersedDotOrdered;

impl FilterAlgorithm for DispersedDotOrdered {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("dispersed_dot_ordered")
    }

    fn display_name(&self) -> &'static str {
        "Dispersed Dot"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::DispersedDotOrdered;
        let ctx = FilterContext::from_ctx(ctx);
        let mut src = PixelTile::new();
        src.copy_from(tile);
        apply_ordered_with_cache_into(
            &src,
            ctx.coord,
            &params,
            ctx.threshold_cache,
            ctx.palette_cache,
            ctx.lut_cache,
            ctx.document,
            ctx.block_cache,
            ctx.layer_id,
            tile,
        )
        .map_err(FilterError::from)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        DISPERSED_DOT_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }

    fn requires_full_row(&self) -> bool {
        false
    }
}
