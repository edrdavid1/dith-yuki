//! Nearest-color palette remap without dithering (`nearest_color`).
//!
//! Metrics: Oklab (default), brightness (Oklab `L` only), sRGB Euclidean (Simple).

use engine_color::brightness_sorted::BrightnessSortedPalette;
use engine_color::oklab::{linear_to_oklab, LinRgb};
use engine_color::palette::{linear_to_srgb, Palette};
use engine_color::palette_lut::DEFAULT_LUT_SIZE;
use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField, TemporalStability,
};
use engine_tiles::{PixelTile, HALO, TILE_SIZE};
use serde::{Deserialize, Serialize};

use crate::error::EngineError;
use crate::filters::context::FilterContext;
use crate::types::PaletteId;

const FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

const SCHEMA: &[ParamField] = &[ParamField::Dropdown {
    key: "metric",
    label: "Match by",
    options: &[
        ("oklab", "Oklab (perceptual)"),
        ("brightness", "Brightness (lightness only)"),
        ("srgb", "sRGB (Simple)"),
    ],
    default: "oklab",
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum NearestMetric {
    #[default]
    Oklab,
    Brightness,
    Srgb,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NearestColorParams {
    palette_id: PaletteId,
    #[serde(default)]
    metric: NearestMetric,
}

/// Palette remap without dither (`nearest_color`).
pub struct NearestColorAlgo;

fn parse_params(params: &serde_json::Value) -> Result<NearestColorParams, serde_json::Error> {
    serde_json::from_value(params.clone())
}

fn apply_nearest(
    src: &PixelTile,
    dst: &mut PixelTile,
    palette: &Palette,
    metric: NearestMetric,
    lut_index: impl Fn(LinRgb) -> usize,
) {
    let brightness = match metric {
        NearestMetric::Brightness => BrightnessSortedPalette::build(palette).ok(),
        _ => None,
    };
    let srgb_palette: Option<Vec<[f32; 3]>> = match metric {
        NearestMetric::Srgb => Some(
            palette
                .colors
                .iter()
                .map(|c| {
                    [
                        linear_to_srgb(c.r) as f32,
                        linear_to_srgb(c.g) as f32,
                        linear_to_srgb(c.b) as f32,
                    ]
                })
                .collect(),
        ),
        _ => None,
    };

    for y in 0..FULL_SIZE {
        for x in 0..FULL_SIZE {
            let r = src.at(x, y, 0);
            let g = src.at(x, y, 1);
            let b = src.at(x, y, 2);
            let a = src.at(x, y, 3);

            let idx = match metric {
                NearestMetric::Oklab => lut_index(LinRgb { r, g, b }),
                NearestMetric::Brightness => {
                    let query_l = linear_to_oklab(LinRgb { r, g, b }).l;
                    brightness
                        .as_ref()
                        .map(|s| s.nearest(query_l))
                        .unwrap_or(0)
                }
                NearestMetric::Srgb => {
                    let qr = linear_to_srgb(r) as f32;
                    let qg = linear_to_srgb(g) as f32;
                    let qb = linear_to_srgb(b) as f32;
                    let pals = srgb_palette.as_ref().unwrap();
                    let mut best = 0usize;
                    let mut best_d = f32::INFINITY;
                    for (i, p) in pals.iter().enumerate() {
                        let dr = qr - p[0];
                        let dg = qg - p[1];
                        let db = qb - p[2];
                        let d = dr * dr + dg * dg + db * db;
                        if d < best_d {
                            best_d = d;
                            best = i;
                        }
                    }
                    best
                }
            };

            let c = &palette.colors[idx.min(palette.colors.len().saturating_sub(1))];
            dst.set(x, y, 0, c.r);
            dst.set(x, y, 1, c.g);
            dst.set(x, y, 2, c.b);
            dst.set(x, y, 3, a);
        }
    }
}

impl FilterAlgorithm for NearestColorAlgo {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("nearest_color")
    }

    fn display_name(&self) -> &'static str {
        "Nearest Color (no dither)"
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
        if palette.colors.is_empty() {
            return Err(FilterError::from(EngineError::invalid_filter_params(
                "Nearest Color requires a non-empty palette",
            )));
        }

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
        apply_nearest(&src, tile, palette, params.metric, |lin| {
            lut.nearest_index(linear_to_oklab(lin)) as usize
        });
        Ok(())
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        // Oklab nearest matches palette_quantize GPU path; brightness/sRGB stay CPU for now.
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
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

    fn temporal_stability(&self) -> TemporalStability {
        TemporalStability::Stable
    }
}
