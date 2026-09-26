//! Parity: Registry `crosshatch_dither` vs the ordered/crosshatch path.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::algorithms::register_all;
use engine_project::filter::{
    DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::apply::apply_filter_to_tile;
use engine_project::filters::crosshatch;
use engine_project::filters::dither_ordered::apply_ordered_with_cache_into;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterContext, Layer};
use engine_registry::{AlgorithmId, AlgorithmRegistry, CpuCheckpointKind, GpuEligibility};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

fn make_gradient_tile() -> PixelTile {
    let mut tile = PixelTile::new();
    let full_size = TILE_SIZE + 2 * HALO;
    for y in 0..full_size {
        for x in 0..full_size {
            let val = x as f32 / full_size as f32;
            tile.set(x, y, 0, val);
            tile.set(x, y, 1, val);
            tile.set(x, y, 2, val);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

fn crosshatch_params() -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::CrosshatchDither,
        levels: 2,
        threshold_scale: 1.0,
        wave_wavelength: 8.0,
        pattern_angle: 45.0,
        color_mode: DitherColorMode::Grayscale,
        palette_id: None,
        ..DitherParamsV2::default()
    }
}

#[test]
fn registry_parity_crosshatch_dither() {
    let params = crosshatch_params();
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
    .expect("crosshatch path");

    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);
    let algo = registry
        .get(AlgorithmId::new("crosshatch_dither"))
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
        0,
    );
    let mut via_trait = PixelTile::new();
    via_trait.copy_from(&src);
    algo.apply(
        &mut via_trait,
        &serde_json::to_value(&params).unwrap(),
        &ctx,
    )
    .expect("registry apply");
    assert_eq!(expected.data.as_ref(), via_trait.data.as_ref());

    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);
    let mut filter = FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(params));
    filter.algorithm_id = Some("crosshatch_dither".into());
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
    .expect("dispatcher");
    assert_eq!(expected.data.as_ref(), via_dispatch.data.as_ref());
}

#[test]
fn crosshatch_ladder_matches_module_constants() {
    assert_eq!(crosshatch::HATCH_LADDER.len(), 4);
    assert!(crosshatch::HATCH_LADDER[0].0 < crosshatch::HATCH_LADDER[3].0);
}
