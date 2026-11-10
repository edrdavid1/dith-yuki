//! Error-diffusion dither algorithms.

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField, TemporalStability,
};
use engine_tiles::PixelTile;

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::context::FilterContext;
use crate::filters::dither_diffusion::{
    apply_ed1d_with_cache_into, apply_error_diffusion_with_cache_into,
};

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
    // Ostromoukhov SIGGRAPH 2001 is specified with serpentine scan; force it so
    // registry parity matches the published algorithm regardless of UI default.
    if matches!(
        params.mode,
        DitherModeV2::Ostromoukhov | DitherModeV2::ZhouFang
    ) {
        params.serpentine = true;
    }
    let ctx = FilterContext::from_ctx(ctx);
    let src = scratch_src(tile);
    apply_error_diffusion_with_cache_into(
        &src,
        ctx.coord,
        &params,
        ctx.residuals,
        ctx.layer_id,
        ctx.filter_key,
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

            fn temporal_stability(&self) -> TemporalStability {
                TemporalStability::Unstable
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
impl_error_diffusion!(Fan93, "fan93", "Fan 93", Fan93);
impl_error_diffusion!(Sierra, "sierra", "Sierra", Sierra);
impl_error_diffusion!(SierraLite, "sierra_lite", "Sierra Lite", SierraLite);
impl_error_diffusion!(
    SierraTwoRow,
    "sierra_two_row",
    "Sierra Two-Row",
    SierraTwoRow
);
impl_error_diffusion!(ShiauFan, "shiau_fan", "Shiau–Fan", ShiauFan);
impl_error_diffusion!(
    StevensonArce,
    "stevenson_arce",
    "Stevenson–Arce",
    StevensonArce
);
impl_error_diffusion!(Ostromoukhov, "ostromoukhov", "Ostromoukhov", Ostromoukhov);
impl_error_diffusion!(ZhouFang, "zhou_fang", "Zhou–Fang", ZhouFang);

/// Shared ED controls plus `line_scale` for 1D contrast-weighted carry.
const ED1D_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "levels",
        label: "Tone Steps",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: Some(1.0),
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
        key: "line_scale",
        label: "Line Scale",
        min: 1.0,
        max: 20.0,
        default: 5.0,
        step: Some(1.0),
    },
];

/// 1D ED + contrast (`modulated_diffuse_y`). Carry weakened on sharp source
/// luminance transitions. Category Glitch so it appears in the glitch picker.
pub struct ModulatedDiffuseY;

impl FilterAlgorithm for ModulatedDiffuseY {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("modulated_diffuse_y")
    }

    fn display_name(&self) -> &'static str {
        "Modulated Diffuse Y"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::ModulatedDiffuseY;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_ed1d_with_cache_into(
            &src,
            ctx.coord,
            &params,
            ctx.palette_cache,
            ctx.lut_cache,
            ctx.document,
            tile,
        )
        .map_err(FilterError::from)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        ED1D_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Glitch
    }

    fn requires_full_row(&self) -> bool {
        true
    }

    fn temporal_stability(&self) -> TemporalStability {
        TemporalStability::Unstable
    }
}

/// Schema for `glitch_dither`: shared ED controls + intensity/seed.
const GLITCH_DITHER_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "levels",
        label: "Tone Steps",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: Some(1.0),
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
        key: "glitch_intensity",
        label: "Intensity",
        min: 1.0,
        max: 20.0,
        default: 1.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "glitch_seed",
        label: "Seed",
        min: 0.0,
        max: 99999.0,
        default: 42.0,
        step: Some(1.0),
    },
];

/// 1D ED after source-row corruption (`glitch_dither`). Display: Corrupt Dither.
/// Distinct AlgorithmId from spatial `glitch`.
pub struct GlitchDither;

impl FilterAlgorithm for GlitchDither {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("glitch_dither")
    }

    fn display_name(&self) -> &'static str {
        "Corrupt Dither"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::GlitchDither;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_ed1d_with_cache_into(
            &src,
            ctx.coord,
            &params,
            ctx.palette_cache,
            ctx.lut_cache,
            ctx.document,
            tile,
        )
        .map_err(FilterError::from)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        GLITCH_DITHER_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Glitch
    }

    fn requires_full_row(&self) -> bool {
        true
    }

    fn temporal_stability(&self) -> TemporalStability {
        TemporalStability::Unstable
    }
}

/// 1D ED + contrast, vertical carry (`modulated_diffuse_x`). Same weight as `-y`.
pub struct ModulatedDiffuseX;

impl FilterAlgorithm for ModulatedDiffuseX {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("modulated_diffuse_x")
    }

    fn display_name(&self) -> &'static str {
        "Modulated Diffuse X"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::ModulatedDiffuseX;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_ed1d_with_cache_into(
            &src,
            ctx.coord,
            &params,
            ctx.palette_cache,
            ctx.lut_cache,
            ctx.document,
            tile,
        )
        .map_err(FilterError::from)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        ED1D_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Glitch
    }

    fn requires_full_column(&self) -> bool {
        true
    }

    fn temporal_stability(&self) -> TemporalStability {
        TemporalStability::Unstable
    }
}

/// Schema for `smooth_diffuse`: shared ED1D + smoothness.
const SMOOTH_DIFFUSE_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "levels",
        label: "Tone Steps",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: Some(1.0),
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
        key: "line_scale",
        label: "Line Scale",
        min: 1.0,
        max: 20.0,
        default: 5.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "smoothness",
        label: "Smoothness",
        min: 0.0,
        max: 20.0,
        default: 5.0,
        step: Some(1.0),
    },
];

macro_rules! impl_ed1d_glitch_style {
    ($name:ident, $id:literal, $display:literal, $mode:ident, $schema:expr, $($topo:tt)*) => {
        pub struct $name;

        impl FilterAlgorithm for $name {
            fn id(&self) -> AlgorithmId {
                AlgorithmId::new($id)
            }

            fn display_name(&self) -> &'static str {
                $display
            }

            fn apply(
                &self,
                tile: &mut PixelTile,
                params: &serde_json::Value,
                ctx: &dyn FilterCtx,
            ) -> Result<(), FilterError> {
                let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
                params.mode = DitherModeV2::$mode;
                let ctx = FilterContext::from_ctx(ctx);
                let src = scratch_src(tile);
                apply_ed1d_with_cache_into(
                    &src,
                    ctx.coord,
                    &params,
                    ctx.palette_cache,
                    ctx.lut_cache,
                    ctx.document,
                    tile,
                )
                .map_err(FilterError::from)
            }

            fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
                GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion)
            }

            fn param_schema(&self) -> &'static [ParamField] {
                $schema
            }

            fn schema_version(&self) -> u32 {
                1
            }

            fn category(&self) -> EffectCategory {
                EffectCategory::Glitch
            }

            $($topo)*

            fn temporal_stability(&self) -> TemporalStability {
                TemporalStability::Unstable
            }
        }
    };
}

impl_ed1d_glitch_style!(
    ContrastAwareY,
    "contrast_aware_y",
    "Contrast Aware Y",
    ContrastAwareY,
    ED1D_SCHEMA,
    fn requires_full_row(&self) -> bool {
        true
    }
);

impl_ed1d_glitch_style!(
    ContrastAwareX,
    "contrast_aware_x",
    "Contrast Aware X",
    ContrastAwareX,
    ED1D_SCHEMA,
    fn requires_full_column(&self) -> bool {
        true
    }
);

impl_ed1d_glitch_style!(
    SmoothDiffuse,
    "smooth_diffuse",
    "Smooth Diffuse",
    SmoothDiffuse,
    SMOOTH_DIFFUSE_SCHEMA,
    fn requires_full_row(&self) -> bool {
        true
    }
);

/// Schema for `wave_modulation_*`.
const WAVE_MODULATION_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "levels",
        label: "Tone Steps",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: Some(1.0),
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
        key: "frequency",
        label: "Frequency",
        min: 1.0,
        max: 30.0,
        default: 8.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "amplitude",
        label: "Amplitude",
        min: 0.0,
        max: 100.0,
        default: 50.0,
        step: Some(1.0),
    },
];

impl_ed1d_glitch_style!(
    WaveModulationY,
    "wave_modulation_y",
    "Wave Modulation Y",
    WaveModulationY,
    WAVE_MODULATION_SCHEMA,
    fn requires_full_row(&self) -> bool {
        true
    }
);

impl_ed1d_glitch_style!(
    WaveModulationX,
    "wave_modulation_x",
    "Wave Modulation X",
    WaveModulationX,
    WAVE_MODULATION_SCHEMA,
    fn requires_full_column(&self) -> bool {
        true
    }
);
