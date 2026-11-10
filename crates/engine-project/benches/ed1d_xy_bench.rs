//! Compare `modulated_diffuse_x` column strip vs `modulated_diffuse_y` row strip.
//!
//! ```text
//! cargo bench -p engine-project --bench ed1d_xy_bench -- --sample-size 10
//! ```
//! Acceptance: -x ≤ 1.5× -y wall time on a ~6K-tall column / ~6K-wide row.

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::{
    apply_ed1d_tile_column_strip, apply_ed1d_tile_row_strip,
};
use engine_project::types::DocumentId;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const STRIPE_TILES: usize = 24; // ~6K

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

fn run_y(doc: &Document, pc: &PaletteKdCache, lc: &PaletteLutCache) {
    let tiles: Vec<(TileCoord, PixelTile)> = (0..STRIPE_TILES)
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
    let strip: Vec<(TileCoord, &PixelTile)> = tiles.iter().map(|(c, t)| (*c, t)).collect();
    let mut outputs: Vec<PixelTile> = (0..STRIPE_TILES).map(|_| PixelTile::new()).collect();
    let params = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseY,
        levels: 4,
        line_scale: 5,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    apply_ed1d_tile_row_strip(&strip, &params, pc, lc, doc, &mut outputs).unwrap();
    black_box(&outputs);
}

fn run_x(doc: &Document, pc: &PaletteKdCache, lc: &PaletteLutCache) {
    let tiles: Vec<(TileCoord, PixelTile)> = (0..STRIPE_TILES)
        .map(|i| {
            (
                TileCoord {
                    level: 0,
                    x: 0,
                    y: i as u32,
                },
                gradient_tile(0, i as u32),
            )
        })
        .collect();
    let strip: Vec<(TileCoord, &PixelTile)> = tiles.iter().map(|(c, t)| (*c, t)).collect();
    let mut outputs: Vec<PixelTile> = (0..STRIPE_TILES).map(|_| PixelTile::new()).collect();
    let params = DitherParamsV2 {
        mode: DitherModeV2::ModulatedDiffuseX,
        levels: 4,
        line_scale: 5,
        color_mode: DitherColorMode::Rgb,
        ..DitherParamsV2::default()
    };
    apply_ed1d_tile_column_strip(&strip, &params, pc, lc, doc, &mut outputs).unwrap();
    black_box(&outputs);
}

fn ed1d_x_vs_y(c: &mut Criterion) {
    let doc = Document::new(
        DocumentId::new(1),
        STRIPE_TILES as u32 * TILE_SIZE,
        STRIPE_TILES as u32 * TILE_SIZE,
    );
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();
    let pixels = (STRIPE_TILES as u64) * (TILE_SIZE as u64) * (TILE_SIZE as u64);

    for _ in 0..2 {
        run_y(&doc, &pc, &lc);
        run_x(&doc, &pc, &lc);
    }

    let median = |s: &mut [f64]| {
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s[s.len() / 2]
    };
    let mut ys = Vec::new();
    let mut xs = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        run_y(&doc, &pc, &lc);
        ys.push(t0.elapsed().as_secs_f64());
        let t1 = Instant::now();
        run_x(&doc, &pc, &lc);
        xs.push(t1.elapsed().as_secs_f64());
    }
    let ym = median(&mut ys);
    let xm = median(&mut xs);
    eprintln!(
        "ed1d_xy_bench: pixels={pixels} y_median_s={ym:.4} ({:.2} MPix/s) \
         x_median_s={xm:.4} ({:.2} MPix/s) x/y={:.2}",
        pixels as f64 / ym / 1e6,
        pixels as f64 / xm / 1e6,
        xm / ym
    );
    assert!(
        xm / ym <= 1.5,
        "modulated_diffuse_x slower than 1.5× y: ratio={:.2}",
        xm / ym
    );

    c.bench_function("ed1d_y_5984_row", |b| b.iter(|| run_y(&doc, &pc, &lc)));
    c.bench_function("ed1d_x_5984_col", |b| b.iter(|| run_x(&doc, &pc, &lc)));
}

criterion_group!(benches, ed1d_x_vs_y);
criterion_main!(benches);
