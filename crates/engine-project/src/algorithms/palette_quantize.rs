//! Palette quantization (`palette_quantize`).
//!
//! Maps each pixel to the nearest palette color in Oklab. `FilterContext` is
//! sufficient: apply uses `document`, `palette_cache`, and `lut_cache` only.

use engine_color::palette_lut::DEFAULT_LUT_SIZE;
use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;
use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::filter::DiffusionKernel;
use crate::filters::context::FilterContext;
use crate::filters::palette_quantize::PaletteQuantizeFilter;
use crate::types::PaletteId;

const SCHEMA: &[ParamField] = &[ParamField::Dropdown {
    key: "diffusion",
    label: "Diffusion",
    options: &[
        ("none", "None (Nearest Only)"),
        ("FloydSteinberg", "Floyd-Steinberg"),
        ("Atkinson", "Atkinson"),
        ("JarvisJudiceNinke", "Jarvis-Judice-Ninke"),
        ("Stucki", "Stucki"),
        ("Burkes", "Burkes"),
        ("Sierra", "Sierra"),
    ],
    default: "none",
}];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PaletteQuantizeParams {
    palette_id: PaletteId,
    #[serde(default)]
    diffusion: Option<DiffusionKernel>,
}

/// Oklab palette quantize (`palette_quantize`).
pub struct PaletteQuantizeAlgo;

fn parse_params(params: &serde_json::Value) -> Result<PaletteQuantizeParams, serde_json::Error> {
    serde_json::from_value(params.clone())
}

impl FilterAlgorithm for PaletteQuantizeAlgo {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("palette_quantize")
    }

    fn display_name(&self) -> &'static str {
        "Palette Quantize"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params = parse_params(params)?;
        let ctx = FilterContext::from_ctx(ctx);
        let palette = ctx
            .document
            .get_palette(params.palette_id)
            .ok_or_else(|| EngineError::palette_not_found(params.palette_id))?;
        let lut = ctx
            .lut_cache
            .get_or_build(
                ctx.document.id.0,
                palette,
                ctx.palette_cache,
                DEFAULT_LUT_SIZE,
            )
            .map_err(|e| {
                EngineError::invalid_filter_params(format!("Failed to build palette LUT: {e}"))
            })?;
        let mut src = PixelTile::new();
        src.copy_from(tile);
        PaletteQuantizeFilter::apply_into(&src, ctx.coord, palette, &lut, params.diffusion, tile)
            .map_err(FilterError::from)
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        // Params-only subset of `gpu_graph::filter_to_spec` /
        // `try_palette_quantize_spec`: LUT presence is resolved at graph
        // compile time when document caches are available.
        match parse_params(params) {
            Err(_) => GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
            Ok(p) if p.diffusion.is_some() => {
                GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion)
            }
            Ok(_) => GpuEligibility::Eligible,
        }
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Palette
    }

    fn requires_full_row(&self) -> bool {
        false
    }
}
