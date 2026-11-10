//! Post-filter microbench: one 256² (+halo) tile for Adjust / Curves / Levels / Glitch.
//!
//! Feeds the Step-2 measurement table in `docs/post-processing-levers.md`
//! (GPU Post / P4 go/no-go). Run:
//!   cargo bench -p engine-project --bench filter_bench -- --sample-size 40

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, Criterion};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filter::{FilterInstance, FilterKind, FilterParams};
use engine_project::filters::apply::apply_enabled_filters_to_tile;
use engine_project::filters::curves::CurveChannel;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::filters::glitch::{GlitchParams, GlitchSubOp};
use engine_project::types::{DocumentId, LayerId};
use engine_project::Document;
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{with_tile_buffer_park, PixelTile, TileCoord, HALO, TILE_SIZE};

fn gradient_tile() -> PixelTile {
    let mut tile = PixelTile::new();
    let full = TILE_SIZE + 2 * HALO;
    for y in 0..full {
        for x in 0..full {
            let t = x as f32 / full as f32;
            tile.set(x, y, 0, t);
            tile.set(x, y, 1, t * 0.7);
            tile.set(x, y, 2, 1.0 - t);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

fn run_post(tile: &PixelTile, filters: &[&FilterInstance]) {
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let palette = PaletteKdCache::new();
    let lut = PaletteLutCache::new();
    let thr = ThresholdMapCache::new();
    let residuals = ErrorResidualsStore::new();
    let blocks = BlockRepresentativeCache::new();
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let out = with_tile_buffer_park(|park| {
        apply_enabled_filters_to_tile(
            tile,
            filters,
            LayerId::new(1),
            coord,
            &palette,
            &lut,
            &thr,
            &doc,
            &residuals,
            &blocks,
            None,
            park,
        )
        .expect("post apply")
    });
    black_box(out);
}

fn adjust_pointwise() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Adjust,
        FilterParams::Adjust {
            contrast: 0.25,
            brightness: 0.1,
            saturation: 0.2,
            blur: 0.0,
            sharpness: 0.0,
            noise: 0.0,
        },
    )
}

fn adjust_blur() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Adjust,
        FilterParams::Adjust {
            contrast: 0.0,
            brightness: 0.0,
            saturation: 0.0,
            blur: 0.4,
            sharpness: 0.0,
            noise: 0.0,
        },
    )
}

fn curves_s() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Curves,
        FilterParams::Curves {
            curve: vec![(0.0, 0.0), (0.5, 0.65), (1.0, 1.0)],
            channel: CurveChannel::All,
        },
    )
}

fn levels_mid() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Levels,
        FilterParams::Levels {
            input_black: 0.05,
            input_white: 0.95,
            gamma: 1.2,
            output_black: 0.0,
            output_white: 1.0,
            channel_r: true,
            channel_g: true,
            channel_b: true,
        },
    )
}

fn glitch_active() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Glitch,
        FilterParams::Glitch(GlitchParams {
            enabled: true,
            seed: 99,
            block_size: 8,
            group_size: 4,
            scanline_segment: 64,
            block_shift: GlitchSubOp {
                enabled: true,
                amount: 16.0,
            },
            block_scramble: GlitchSubOp {
                enabled: true,
                amount: 20.0,
            },
            channel_swap: GlitchSubOp {
                enabled: true,
                amount: 80.0,
            },
            scanline: GlitchSubOp {
                enabled: true,
                amount: 80.0,
            },
            interlace: GlitchSubOp {
                enabled: true,
                amount: 70.0,
            },
        }),
    )
}

fn post_tile_benches(c: &mut Criterion) {
    let tile = gradient_tile();
    let adjust = adjust_pointwise();
    let blur = adjust_blur();
    let curves = curves_s();
    let levels = levels_mid();
    let glitch = glitch_active();
    let fused_stack = [adjust_pointwise(), curves_s(), levels_mid()];

    c.bench_function("post_tile_adjust_pointwise", |b| {
        b.iter(|| run_post(&tile, &[&adjust]))
    });
    c.bench_function("post_tile_adjust_blur", |b| {
        b.iter(|| run_post(&tile, &[&blur]))
    });
    c.bench_function("post_tile_curves", |b| {
        b.iter(|| run_post(&tile, &[&curves]))
    });
    c.bench_function("post_tile_levels", |b| {
        b.iter(|| run_post(&tile, &[&levels]))
    });
    c.bench_function("post_tile_glitch", |b| {
        b.iter(|| run_post(&tile, &[&glitch]))
    });
    c.bench_function("post_tile_fused_adjust_curves_levels", |b| {
        let refs: Vec<&FilterInstance> = fused_stack.iter().collect();
        b.iter(|| run_post(&tile, &refs))
    });
}

criterion_group!(benches, post_tile_benches);
criterion_main!(benches);
