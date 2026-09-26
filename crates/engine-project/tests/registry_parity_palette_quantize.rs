//! Parity: Registry `palette_quantize` vs `PaletteQuantizeFilter::apply_into`.
//!
//! Includes a palette-bound nearest-color case (task 2.2).

use engine_color::palette::LinearColor;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::{PaletteLutCache, DEFAULT_LUT_SIZE};
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::algorithms::register_all;
use engine_project::filter::{DiffusionKernel, FilterInstance, FilterKind, FilterParams};
use engine_project::filters::apply::apply_filter_to_tile;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::filters::palette_quantize::PaletteQuantizeFilter;
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
            tile.set(x, y, 1, val * 0.7);
            tile.set(x, y, 2, 1.0 - val);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

fn assert_parity(
    src: &PixelTile,
    doc: &Document,
    palette_id: engine_project::types::PaletteId,
    diffusion: Option<DiffusionKernel>,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
) {
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let layer_id = LayerId::new(1);
    let threshold_cache = ThresholdMapCache::new();
    let residuals = ErrorResidualsStore::new();
    let block_cache = BlockRepresentativeCache::new();

    let palette = doc.get_palette(palette_id).expect("palette");
    let lut = lut_cache
        .get_or_build(doc.id.0, palette, palette_cache, DEFAULT_LUT_SIZE)
        .expect("lut");

    let mut expected = PixelTile::new();
    PaletteQuantizeFilter::apply_into(src, coord, palette, &lut, diffusion, &mut expected)
        .expect("legacy apply_into");

    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);
    let algo = registry
        .get(AlgorithmId::new("palette_quantize"))
        .expect("palette_quantize registered");

    let ctx = FilterContext::new(
        coord,
        doc,
        palette_cache,
        lut_cache,
        &threshold_cache,
        &residuals,
        &block_cache,
        None,
        layer_id,
        0,
    );
    let params_json = serde_json::json!({
        "palette_id": palette_id,
        "diffusion": diffusion,
    });
    let mut via_trait = PixelTile::new();
    via_trait.copy_from(src);
    algo.apply(&mut via_trait, &params_json, &ctx)
        .expect("registry apply");

    assert_eq!(
        expected.data.as_ref(),
        via_trait.data.as_ref(),
        "palette_quantize registry apply must match PaletteQuantizeFilter::apply_into"
    );

    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);
    let mut filter = FilterInstance::new(
        FilterKind::PaletteQuantize,
        FilterParams::PaletteQuantize {
            palette_id,
            diffusion,
        },
    );
    filter.algorithm_id = Some("palette_quantize".into());
    filter.schema_version = Some(1);
    layer.filters.push(filter);

    let via_dispatch = apply_filter_to_tile(
        src,
        &layer,
        coord,
        palette_cache,
        lut_cache,
        &threshold_cache,
        doc,
    )
    .expect("dispatcher registry path");

    assert_eq!(
        expected.data.as_ref(),
        via_dispatch.data.as_ref(),
        "palette_quantize dispatcher must match PaletteQuantizeFilter::apply_into"
    );
}

#[test]
fn registry_parity_palette_quantize_nearest() {
    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let palette_id = doc.add_palette(
        "Test".into(),
        vec![
            LinearColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 0.0,
                g: 1.0,
                b: 0.0,
            },
            LinearColor {
                r: 0.0,
                g: 0.0,
                b: 1.0,
            },
        ],
    );
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let src = make_gradient_tile();
    assert_parity(&src, &doc, palette_id, None, &palette_cache, &lut_cache);
}

#[test]
fn registry_parity_palette_quantize_bound_with_diffusion() {
    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let palette_id = doc.add_palette(
        "Test".into(),
        vec![
            LinearColor {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
            LinearColor {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            },
        ],
    );
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let src = make_gradient_tile();
    assert_parity(
        &src,
        &doc,
        palette_id,
        Some(DiffusionKernel::Atkinson),
        &palette_cache,
        &lut_cache,
    );
}

#[test]
fn palette_quantize_gpu_eligibility_matches_graph_rules() {
    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);
    let algo = registry.get(AlgorithmId::new("palette_quantize")).unwrap();

    let nearest = serde_json::json!({"palette_id": 1, "diffusion": null});
    assert_eq!(algo.gpu_eligibility(&nearest), GpuEligibility::Eligible);

    let ed = serde_json::json!({"palette_id": 1, "diffusion": "FloydSteinberg"});
    assert_eq!(
        algo.gpu_eligibility(&ed),
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion)
    );

    let bad = serde_json::json!({"nope": true});
    assert_eq!(
        algo.gpu_eligibility(&bad),
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    );
}
