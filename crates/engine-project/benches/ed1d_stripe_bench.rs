//! Horizontal 1D ED strip vs FS on one ~6K-wide tile row.
//!
//! ```text
//! cargo bench -p engine-project --bench ed1d_stripe_bench -- --sample-size 20
//! ```

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::{
    apply_ed1d_tile_row_strip, apply_error_diffusion_tile_row_strip,
};
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const STRIPE_WIDTH_PX: u32 = 5984;
const STRIPE_TILES: usize = ((STRIPE_WIDTH_PX + TILE_SIZE - 1) / TILE_SIZE) as usize;

fn gradient_tile(tx: u32) -> PixelTile {
    let full = TILE_SIZE + 2 * HALO;
    let mut tile = PixelTile::new();
    for y in 0..full {
        for x in 0..full {
            let gx = tx as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
            let gy = y as i32 - HALO as i32;
            let t = ((gx + gy) as f32 * 0.07).sin() * 0.5 + 0.5;
            tile.set(x, y, 0, t);
            tile.set(x, y, 1, t * 0.85);
            tile.set(x, y, 2, 1.0 - t);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

fn make_strip() -> (
    Vec<(TileCoord, PixelTile)>,
    Document,
    PaletteKdCache,
    PaletteLutCache,
) {
    let tiles_owned: Vec<(TileCoord, PixelTile)> = (0..STRIPE_TILES)
        .map(|i| {
            (
                TileCoord {
                    level: 0,
                    x: i as u32,
                    y: 0,
                },
                gradient_tile(i as u32),
            )
        })
        .collect();
    let doc = Document::new(DocumentId::new(1), STRIPE_WIDTH_PX, TILE_SIZE);
    (
        tiles_owned,
        doc,
        PaletteKdCache::new(),
        PaletteLutCache::new(),
    )
}

fn run_ed1d(
    tiles_owned: &[(TileCoord, PixelTile)],
    doc: &Document,
    pc: &PaletteKdCache,
    lc: &PaletteLutCache,
) {
    let strip: Vec<(TileCoord, &PixelTile)> = tiles_owned.iter().map(|(c, t)| (*c, t)).collect();
    let mut outputs: Vec<PixelTile> = (0..STRIPE_TILES).map(|_| PixelTile::new()).collect();
    let params = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseY,
        levels: 4,
        line_scale: 5,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    apply_ed1d_tile_row_strip(&strip, &params, pc, lc, doc, &mut outputs).expect("ed1d");
    black_box(&outputs);
}

fn run_fs(
    tiles_owned: &[(TileCoord, PixelTile)],
    doc: &Document,
    pc: &PaletteKdCache,
    lc: &PaletteLutCache,
) {
    let strip: Vec<(TileCoord, &PixelTile)> = tiles_owned.iter().map(|(c, t)| (*c, t)).collect();
    let mut outputs: Vec<PixelTile> = (0..STRIPE_TILES).map(|_| PixelTile::new()).collect();
    let residuals = ErrorResidualsStore::new();
    let blocks = BlockRepresentativeCache::new();
    let params = DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 4,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    apply_error_diffusion_tile_row_strip(
        &strip,
        &params,
        &residuals,
        LayerId::new(1),
        0,
        pc,
        lc,
        doc,
        &blocks,
        &mut outputs,
    )
    .expect("fs");
    black_box(&outputs);
}

fn ed1d_vs_fs_stripe(c: &mut Criterion) {
    let (tiles, doc, pc, lc) = make_strip();
    let pixels = (STRIPE_TILES as u64) * (TILE_SIZE as u64) * (TILE_SIZE as u64);

    for _ in 0..2 {
        run_ed1d(&tiles, &doc, &pc, &lc);
        run_fs(&tiles, &doc, &pc, &lc);
    }

    let median = |samples: &mut [f64]| {
        samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
        samples[samples.len() / 2]
    };

    let mut ed1d_s = Vec::new();
    let mut fs_s = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        run_ed1d(&tiles, &doc, &pc, &lc);
        ed1d_s.push(t0.elapsed().as_secs_f64());
        let t1 = Instant::now();
        run_fs(&tiles, &doc, &pc, &lc);
        fs_s.push(t1.elapsed().as_secs_f64());
    }
    let ed1d_m = median(&mut ed1d_s);
    let fs_m = median(&mut fs_s);
    eprintln!(
        "ed1d_stripe_bench: tiles={STRIPE_TILES} pixels={pixels} \
         ed1d_median_s={ed1d_m:.4} ({:.2} MPix/s) fs_median_s={fs_m:.4} ({:.2} MPix/s) ratio={:.2}",
        pixels as f64 / ed1d_m / 1e6,
        pixels as f64 / fs_m / 1e6,
        ed1d_m / fs_m
    );

    c.bench_function("ed1d_5984_stripe", |b| {
        b.iter(|| run_ed1d(&tiles, &doc, &pc, &lc));
    });
}

criterion_group!(benches, ed1d_vs_fs_stripe);
criterion_main!(benches);
