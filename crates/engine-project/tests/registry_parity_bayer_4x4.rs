//! Parity: Registry `bayer_4x4` vs the legacy ordered-dither path.
//!
//! Gate for Phase 1 (task 1.5). Byte-identical output is required before any
//! Phase 2 algorithm migration.

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
use engine_registry::{AlgorithmId, AlgorithmRegistry};
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

fn bayer4x4_params() -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::Bayer4x4,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        ..DitherParamsV2::default()
    }
}

#[test]
fn registry_parity_bayer_4x4() {
    let params = bayer4x4_params();
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
    .expect("legacy ordered path");

    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);
    let algo = registry
        .get(AlgorithmId::new("bayer_4x4"))
        .expect("bayer_4x4 registered");

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
    let params_json = serde_json::to_value(&params).expect("params json");
    let mut via_trait = PixelTile::new();
    via_trait.copy_from(&src);
    algo.apply(&mut via_trait, &params_json, &ctx)
        .expect("registry apply");

    assert_eq!(
        expected.data.as_ref(),
        via_trait.data.as_ref(),
        "Bayer4x4 registry apply must match apply_ordered_with_cache_into"
    );

    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);
    let mut filter = FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(params));
    filter.algorithm_id = Some("bayer_4x4".into());
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
    .expect("dispatcher registry path");

    assert_eq!(
        expected.data.as_ref(),
        via_dispatch.data.as_ref(),
        "Bayer4x4 dispatcher registry path must match apply_ordered_with_cache_into"
    );
}
