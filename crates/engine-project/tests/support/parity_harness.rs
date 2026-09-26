//! Shared fixtures for registry parity integration tests.
//!
//! Included via `#[path = "support/parity_harness.rs"]` from each
//! `registry_parity_*.rs` binary (Cargo does not auto-discover this file).
#![allow(dead_code)]

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::algorithms::register_all;
use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::adjust::apply_adjust_into;
use engine_project::filters::apply::apply_filter_to_tile;
use engine_project::filters::crt::apply_crt_into;
use engine_project::filters::curves::{CurveChannel, CurvesFilter};
use engine_project::filters::dither_diffusion::apply_error_diffusion_with_cache_into;
use engine_project::filters::dither_ordered::apply_ordered_with_cache_into;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::filters::glitch::{GlitchFilter, GlitchType};
use engine_project::filters::glow::apply_glow_into;
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterContext, Layer};
use engine_registry::{AlgorithmId, AlgorithmRegistry, GpuEligibility};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

pub fn gradient_tile() -> PixelTile {
    let mut tile = PixelTile::new();
    let full_size = TILE_SIZE + 2 * HALO;
    for y in 0..full_size {
        for x in 0..full_size {
            let val = x as f32 / full_size as f32;
            tile.set(x, y, 0, val);
            tile.set(x, y, 1, val * 0.7);
            tile.set(x, y, 2, 1.0 - val);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

pub fn dither_params(mode: DitherModeV2) -> DitherParamsV2 {
    DitherParamsV2 {
        mode,
        ..DitherParamsV2::default()
    }
}

pub fn params_json(params: &FilterParams) -> serde_json::Value {
    match params {
        FilterParams::DitherV2(p) => serde_json::to_value(p).unwrap(),
        FilterParams::Crt {
            period,
            strength,
            mask_strength,
        } => serde_json::json!({
            "period": period,
            "strength": strength,
            "mask_strength": mask_strength,
        }),
        FilterParams::Glow {
            radius,
            intensity,
            threshold,
        } => serde_json::json!({
            "radius": radius,
            "intensity": intensity,
            "threshold": threshold,
        }),
        FilterParams::Adjust {
            contrast,
            brightness,
            saturation,
            blur,
            sharpness,
            noise,
        } => serde_json::json!({
            "contrast": contrast,
            "brightness": brightness,
            "saturation": saturation,
            "blur": blur,
            "sharpness": sharpness,
            "noise": noise,
        }),
        FilterParams::Curves { curve, channel } => serde_json::json!({
            "curve": curve,
            "channel": channel,
        }),
        FilterParams::Glitch {
            glitch_type,
            intensity,
            seed,
        } => serde_json::json!({
            "glitch_type": glitch_type,
            "intensity": intensity,
            "seed": seed,
        }),
        other => serde_json::to_value(other).unwrap(),
    }
}

pub struct Env {
    pub doc: Document,
    pub palette_cache: PaletteKdCache,
    pub lut_cache: PaletteLutCache,
    pub threshold_cache: ThresholdMapCache,
    pub residuals: ErrorResidualsStore,
    pub block_cache: BlockRepresentativeCache,
    pub coord: TileCoord,
    pub layer_id: LayerId,
}

impl Env {
    pub fn new() -> Self {
        Self {
            doc: Document::new(DocumentId::new(1), 512, 512),
            palette_cache: PaletteKdCache::new(),
            lut_cache: PaletteLutCache::new(),
            threshold_cache: ThresholdMapCache::new(),
            residuals: ErrorResidualsStore::new(),
            block_cache: BlockRepresentativeCache::new(),
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            layer_id: LayerId::new(1),
        }
    }

    pub fn ctx(&self) -> FilterContext<'_> {
        FilterContext::new(
            self.coord,
            &self.doc,
            &self.palette_cache,
            &self.lut_cache,
            &self.threshold_cache,
            &self.residuals,
            &self.block_cache,
            None,
            self.layer_id,
            0,
        )
    }

    pub fn assert_gpu(&self, id: &'static str, params: &FilterParams, expected: GpuEligibility) {
        let mut registry = AlgorithmRegistry::new();
        register_all(&mut registry);
        let algo = registry
            .get(AlgorithmId::new(id))
            .unwrap_or_else(|| panic!("{id} registered"));
        assert_eq!(
            algo.gpu_eligibility(&params_json(params)),
            expected,
            "{id} gpu_eligibility"
        );
    }

    pub fn assert_registry(
        &self,
        id: &'static str,
        kind: FilterKind,
        params: FilterParams,
        src: &PixelTile,
        expected: &PixelTile,
    ) {
        let mut registry = AlgorithmRegistry::new();
        register_all(&mut registry);
        let algo = registry
            .get(AlgorithmId::new(id))
            .unwrap_or_else(|| panic!("{id} registered"));

        let params_json = params_json(&params);
        let ctx = self.ctx();
        let mut via_trait = PixelTile::new();
        via_trait.copy_from(src);
        algo.apply(&mut via_trait, &params_json, &ctx)
            .unwrap_or_else(|e| panic!("{id} apply: {e}"));
        assert_eq!(
            expected.data.as_ref(),
            via_trait.data.as_ref(),
            "{id} trait apply mismatch"
        );

        let mut layer = Layer::new(self.layer_id, LayerKind::Raster, 512, 512);
        let mut filter = FilterInstance::new(kind, params);
        filter.algorithm_id = Some(id.into());
        filter.schema_version = Some(1);
        layer.filters.push(filter);
        let via_dispatch = apply_filter_to_tile(
            src,
            &layer,
            self.coord,
            &self.palette_cache,
            &self.lut_cache,
            &self.threshold_cache,
            &self.doc,
        )
        .unwrap_or_else(|e| panic!("{id} dispatch: {e}"));
        assert_eq!(
            expected.data.as_ref(),
            via_dispatch.data.as_ref(),
            "{id} dispatcher mismatch"
        );
    }

    pub fn assert_ordered(&self, id: &'static str, params: DitherParamsV2) {
        let src = gradient_tile();
        let mut expected = PixelTile::new();
        apply_ordered_with_cache_into(
            &src,
            self.coord,
            &params,
            &self.threshold_cache,
            &self.palette_cache,
            &self.lut_cache,
            &self.doc,
            &self.block_cache,
            self.layer_id,
            &mut expected,
        )
        .expect("ordered");
        self.assert_registry(
            id,
            FilterKind::Dither,
            FilterParams::DitherV2(params),
            &src,
            &expected,
        );
    }

    /// Fresh residuals for the legacy path so the registry apply (empty store)
    /// is not compared against a store already written by the expected run.
    pub fn assert_ed(&self, id: &'static str, params: DitherParamsV2) {
        let src = gradient_tile();
        let mut expected = PixelTile::new();
        let residuals = ErrorResidualsStore::new();
        apply_error_diffusion_with_cache_into(
            &src,
            self.coord,
            &params,
            &residuals,
            self.layer_id,
            0,
            &self.palette_cache,
            &self.lut_cache,
            &self.doc,
            &self.block_cache,
            &mut expected,
        )
        .expect("ed");
        self.assert_registry(
            id,
            FilterKind::Dither,
            FilterParams::DitherV2(params),
            &src,
            &expected,
        );
    }

    pub fn assert_crt(&self, period: u8, strength: f32, mask_strength: f32) {
        let src = gradient_tile();
        let mut expected = PixelTile::new();
        apply_crt_into(
            &src,
            self.coord,
            period,
            strength,
            mask_strength,
            &mut expected,
        );
        let params = FilterParams::Crt {
            period,
            strength,
            mask_strength,
        };
        self.assert_registry("crt", FilterKind::Crt, params, &src, &expected);
    }

    pub fn assert_glow(&self, radius: f32, intensity: f32, threshold: f32) {
        let src = gradient_tile();
        let mut expected = PixelTile::new();
        apply_glow_into(&src, radius, intensity, threshold, &mut expected);
        let params = FilterParams::Glow {
            radius,
            intensity,
            threshold,
        };
        self.assert_registry("glow", FilterKind::Glow, params, &src, &expected);
    }

    pub fn assert_adjust(
        &self,
        contrast: f32,
        brightness: f32,
        saturation: f32,
        blur: f32,
        sharpness: f32,
        noise: f32,
    ) {
        let src = gradient_tile();
        let mut expected = PixelTile::new();
        apply_adjust_into(
            &src,
            self.coord,
            contrast,
            brightness,
            saturation,
            blur,
            sharpness,
            noise,
            &mut expected,
        );
        let params = FilterParams::Adjust {
            contrast,
            brightness,
            saturation,
            blur,
            sharpness,
            noise,
        };
        self.assert_registry("adjust", FilterKind::Adjust, params, &src, &expected);
    }

    pub fn assert_curves(&self, curve: Vec<(f32, f32)>, channel: CurveChannel) {
        let src = gradient_tile();
        let mut filter = CurvesFilter::new(channel);
        for &(input, output) in &curve {
            filter.add_point(input, output).expect("curve point");
        }
        let mut expected = PixelTile::new();
        filter
            .apply_to_tile_into(&src, &mut expected)
            .expect("curves");
        let params = FilterParams::Curves { curve, channel };
        self.assert_registry("curves", FilterKind::Curves, params, &src, &expected);
    }

    pub fn assert_glitch(&self, glitch_type: GlitchType, intensity: f32, seed: u64) {
        let src = gradient_tile();
        let filter = GlitchFilter::new(glitch_type, intensity, seed).expect("glitch");
        let mut expected = PixelTile::new();
        filter
            .apply_to_tile_into(&src, self.coord, &mut expected)
            .expect("glitch apply");
        let params = FilterParams::Glitch {
            glitch_type,
            intensity,
            seed,
        };
        self.assert_registry("glitch", FilterKind::Glitch, params, &src, &expected);
    }
}
