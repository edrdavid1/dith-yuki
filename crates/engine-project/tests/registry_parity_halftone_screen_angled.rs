//! Parity / oracle: `halftone_screen_angled` vs CMYK halftone primitives.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::algorithms::register_all;
use engine_project::filter::{
    DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::apply::apply_filter_to_tile;
use engine_project::filters::dither_ordered::apply_ordered_with_cache_into;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterContext, Layer};
use engine_registry::{
    AlgorithmId, AlgorithmRegistry, CpuCheckpointKind, GpuEligibility,
};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

fn make_gradient_tile() -> PixelTile {
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

fn base_params(mode: DitherModeV2, pattern_angle: f32) -> DitherParamsV2 {
    DitherParamsV2 {
        mode,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        halftone_cell_size: 8,
        pattern_angle,
        ..DitherParamsV2::default()
    }
}

fn apply_ordered(src: &PixelTile, params: &DitherParamsV2) -> PixelTile {
    let mut out = PixelTile::new();
    apply_ordered_with_cache_into(
        src,
        TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        params,
        &ThresholdMapCache::new(),
        &PaletteKdCache::new(),
        &PaletteLutCache::new(),
        &Document::new(DocumentId::new(1), 512, 512),
        &BlockRepresentativeCache::new(),
        LayerId::new(1),
        &mut out,
    )
    .expect("ordered");
    out
}

#[test]
fn angled_zero_matches_cmyk_halftone_oracle() {
    let src = make_gradient_tile();
    let cmyk = apply_ordered(&src, &base_params(DitherModeV2::CmykHalftone, 0.0));
    let angled = apply_ordered(&src, &base_params(DitherModeV2::HalftoneScreenAngled, 0.0));
    assert_eq!(
        cmyk.data.as_ref(),
        angled.data.as_ref(),
        "pattern_angle=0 must be byte-identical to cmyk_halftone"
    );
}

#[test]
fn angled_nonzero_differs_from_cmyk() {
    let src = make_gradient_tile();
    let cmyk = apply_ordered(&src, &base_params(DitherModeV2::CmykHalftone, 0.0));
    let angled = apply_ordered(&src, &base_params(DitherModeV2::HalftoneScreenAngled, 15.0));
    assert_ne!(
        cmyk.data.as_ref(),
        angled.data.as_ref(),
        "non-zero screen angle must rotate the CMYK plate set"
    );
}

#[test]
fn registry_parity_halftone_screen_angled() {
    let params = base_params(DitherModeV2::HalftoneScreenAngled, 22.5);
    let src = make_gradient_tile();
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let layer_id = LayerId::new(1);
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let residuals = ErrorResidualsStore::new();
    let block_cache = BlockRepresentativeCache::new();

    let mut expected = PixelTile::new();
    apply_ordered_with_cache_into(
        &src,
        coord,
        &params,
        &threshold_cache,
        &palette_cache,
        &lut_cache,
        &doc,
        &block_cache,
        layer_id,
        &mut expected,
    )
    .expect("ordered");

    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);
    let algo = registry
        .get(AlgorithmId::new("halftone_screen_angled"))
        .expect("registered");

    assert!(matches!(
        algo.gpu_eligibility(&serde_json::to_value(&params).unwrap()),
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    ));

    let ctx = FilterContext::new(
        coord,
        &doc,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &residuals,
        &block_cache,
        None,
        layer_id,
    );
    let mut via_trait = PixelTile::new();
    via_trait.copy_from(&src);
    algo.apply(&mut via_trait, &serde_json::to_value(&params).unwrap(), &ctx)
        .expect("apply");
    assert_eq!(expected.data.as_ref(), via_trait.data.as_ref());

    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);
    let mut filter = FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(params));
    filter.algorithm_id = Some("halftone_screen_angled".into());
    filter.schema_version = Some(1);
    layer.filters.push(filter);
    let via_dispatch = apply_filter_to_tile(
        &src,
        &layer,
        coord,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
    )
    .expect("dispatch");
    assert_eq!(expected.data.as_ref(), via_dispatch.data.as_ref());
}
