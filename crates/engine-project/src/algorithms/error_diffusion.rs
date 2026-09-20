//! Error-diffusion dither algorithms.

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::context::FilterContext;
use crate::filters::dither_diffusion::apply_error_diffusion_with_cache_into;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
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
    ParamField::Checkbox {
        key: "serpentine",
        label: "Serpentine",
        default: false,
    },
];

fn apply_ed(
    tile: &mut PixelTile,
    mut params: DitherParamsV2,
    mode: DitherModeV2,
    ctx: &dyn FilterCtx,
) -> Result<(), FilterError> {
    params.mode = mode;
    let ctx = FilterContext::from_ctx(ctx);
    let src = scratch_src(tile);
    apply_error_diffusion_with_cache_into(
        &src,
        ctx.coord,
        &params,
        ctx.residuals,
        ctx.layer_id,
        ctx.palette_cache,
        ctx.lut_cache,
        ctx.document,
        ctx.block_cache,
        tile,
    )
    .map_err(FilterError::from)
}

macro_rules! impl_error_diffusion {
    ($ty:ident, $id:literal, $name:literal, $mode:ident) => {
        pub struct $ty;

        impl FilterAlgorithm for $ty {
            fn id(&self) -> AlgorithmId {
                AlgorithmId::new($id)
            }

            fn display_name(&self) -> &'static str {
                $name
            }

            fn apply(
                &self,
                tile: &mut PixelTile,
                params: &serde_json::Value,
                ctx: &dyn FilterCtx,
            ) -> Result<(), FilterError> {
                let params: DitherParamsV2 = serde_json::from_value(params.clone())?;
                apply_ed(tile, params, DitherModeV2::$mode, ctx)
            }

            fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
                GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion)
            }

            fn param_schema(&self) -> &'static [ParamField] {
                SCHEMA
            }

            fn schema_version(&self) -> u32 {
                1
            }

            fn category(&self) -> EffectCategory {
                EffectCategory::Dithering
            }

            fn requires_full_row(&self) -> bool {
                true
            }
        }
    };
}

impl_error_diffusion!(
    FloydSteinberg,
    "floyd_steinberg",
    "Floyd–Steinberg",
    FloydSteinberg
);
impl_error_diffusion!(Atkinson, "atkinson", "Atkinson", Atkinson);
impl_error_diffusion!(
    JarvisJudiceNinke,
    "jarvis_judice_ninke",
    "Jarvis–Judice–Ninke",
    JarvisJudiceNinke
);
impl_error_diffusion!(Stucki, "stucki", "Stucki", Stucki);
impl_error_diffusion!(Burkes, "burkes", "Burkes", Burkes);
impl_error_diffusion!(Sierra, "sierra", "Sierra", Sierra);
