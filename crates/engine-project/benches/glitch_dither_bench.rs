//! `glitch_dither` vs `modulated_diffuse_y` on a ~6K-wide strip.
//!
//! ```text
//! cargo bench -p engine-project --bench glitch_dither_bench -- --sample-size 10
//! ```
//! Acceptance: glitch_dither ≤ 1.3× modulated_diffuse_y.

use std::hint::black_box;
use std::time::Instant;

use criterion::{criterion_group, criterion_main, Criterion};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::apply_ed1d_tile_row_strip;
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

fn run_mode(doc: &Document, pc: &PaletteKdCache, lc: &PaletteLutCache, mode: DitherModeV2) {
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
    let params = match mode {
        DitherModeV2::GlitchDither => DitherParamsV2 {
            mode,
            levels: 4,
            glitch_intensity: 12,
            glitch_seed: 42,
            color_mode: DitherColorMode::Rgb,
            ..DitherParamsV2::default()
        },
        _ => DitherParamsV2 {
            mode,
            levels: 4,
            line_scale: 5,
            color_mode: DitherColorMode::Rgb,
            ..DitherParamsV2::default()
        },
    };
    apply_ed1d_tile_row_strip(&strip, &params, pc, lc, doc, &mut outputs).unwrap();
    black_box(&outputs);
}

fn glitch_vs_mdy(c: &mut Criterion) {
    let doc = Document::new(
        DocumentId::new(1),
        STRIPE_TILES as u32 * TILE_SIZE,
        TILE_SIZE,
    );
    let pc = PaletteKdCache::new();
    let lc = PaletteLutCache::new();
    let pixels = (STRIPE_TILES as u64) * (TILE_SIZE as u64) * (TILE_SIZE as u64);

    for _ in 0..2 {
        run_mode(&doc, &pc, &lc, DitherModeV2::ModulatedDiffuseY);
        run_mode(&doc, &pc, &lc, DitherModeV2::GlitchDither);
    }

    let median = |s: &mut [f64]| {
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        s[s.len() / 2]
    };
    let mut ys = Vec::new();
    let mut gs = Vec::new();
    for _ in 0..5 {
        let t0 = Instant::now();
        run_mode(&doc, &pc, &lc, DitherModeV2::ModulatedDiffuseY);
        ys.push(t0.elapsed().as_secs_f64());
        let t1 = Instant::now();
        run_mode(&doc, &pc, &lc, DitherModeV2::GlitchDither);
        gs.push(t1.elapsed().as_secs_f64());
    }
    let ym = median(&mut ys);
    let gm = median(&mut gs);
    eprintln!(
        "glitch_dither_bench: pixels={pixels} y_median_s={ym:.4} g_median_s={gm:.4} g/y={:.2}",
        gm / ym
    );
    assert!(
        gm / ym <= 1.3,
        "glitch_dither slower than 1.3× modulated_diffuse_y: ratio={:.2}",
        gm / ym
    );

    c.bench_function("modulated_diffuse_y_6k", |b| {
        b.iter(|| run_mode(&doc, &pc, &lc, DitherModeV2::ModulatedDiffuseY))
    });
    c.bench_function("glitch_dither_6k", |b| {
        b.iter(|| run_mode(&doc, &pc, &lc, DitherModeV2::GlitchDither))
    });
}

criterion_group!(benches, glitch_vs_mdy);
criterion_main!(benches);
