//! Step A: bare Floyd–Steinberg strip cycle — one 5984-wide tile row, no caches.
//!
//! ```text
//! cargo bench -p engine-project --bench fs_ed_stripe_bench -- --sample-size 20
//! ```

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::apply_error_diffusion_tile_row_strip;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

/// Image width that matches the GUI 24×17 L0 grid (~5984×4172).
const STRIPE_WIDTH_PX: u32 = 5984;
const STRIPE_TILES: usize = ((STRIPE_WIDTH_PX + TILE_SIZE - 1) / TILE_SIZE) as usize;

fn gradient_tile(tx: u32, ty: u32) -> PixelTile {
    let full = TILE_SIZE + 2 * HALO;
    let mut tile = PixelTile::new();
    for y in 0..full {
        for x in 0..full {
            let gx = tx as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
            let gy = ty as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
            let t = ((gx + gy) as f32 * 0.07).sin() * 0.5 + 0.5;
            tile.set(x, y, 0, t);
            tile.set(x, y, 1, t * 0.85);
            tile.set(x, y, 2, 1.0 - t);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

fn fs_params() -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        serpentine: false,
        ..DitherParamsV2::default()
    }
}

fn run_one_stripe() {
    let tiles_owned: Vec<(TileCoord, PixelTile)> = (0..STRIPE_TILES)
        .map(|i| {
            (
                TileCoord {
                    level: 0,
                    x: i as u32,
                    y: 0,
                },
                gradient_tile(i as u32, 0),
            )
        })
        .collect();
    let strip: Vec<(TileCoord, &PixelTile)> = tiles_owned.iter().map(|(c, t)| (*c, t)).collect();
    let mut outputs: Vec<PixelTile> = (0..STRIPE_TILES).map(|_| PixelTile::new()).collect();
    // Fresh stores each iteration — no residual / block cache hits.
    let residuals = ErrorResidualsStore::new();
    let blocks = BlockRepresentativeCache::new();
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), STRIPE_WIDTH_PX, TILE_SIZE);
    let params = fs_params();
    apply_error_diffusion_tile_row_strip(
        &strip,
        &params,
        &residuals,
        LayerId::new(1),
        0,
        &palette_cache,
        &lut_cache,
        &doc,
        &blocks,
        &mut outputs,
    )
    .expect("strip");
    black_box(&outputs);
}

fn fs_ed_5984_stripe(c: &mut Criterion) {
    let pixels = (STRIPE_TILES as u64) * (TILE_SIZE as u64) * (TILE_SIZE as u64);
    // Warm + print absolute ns/pixel once for the Step A report.
    for _ in 0..2 {
        run_one_stripe();
    }
    let trials = 5u32;
    let mut samples = Vec::with_capacity(trials as usize);
    for _ in 0..trials {
        let t0 = Instant::now();
        run_one_stripe();
        samples.push(t0.elapsed().as_secs_f64());
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let median_s = samples[samples.len() / 2];
    let ns_per_px = median_s * 1e9 / pixels as f64;
    let mpix_s = pixels as f64 / median_s / 1e6;
    eprintln!(
        "fs_ed_stripe_bench: tiles={STRIPE_TILES} wide_px≈{STRIPE_WIDTH_PX} \
         pixels={pixels} median_s={median_s:.4} ns/pixel={ns_per_px:.2} MPix/s={mpix_s:.2}"
    );

    c.bench_function("fs_ed_5984_stripe_no_cache", |b| {
        b.iter(|| run_one_stripe());
    });
}

criterion_group!(benches, fs_ed_5984_stripe);
criterion_main!(benches);
