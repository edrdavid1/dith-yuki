//! Benchmark PaletteLut3D build / lookup vs KdTree nearest (Track B1 §1.5).

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use engine_color::kdtree::KdTree;
use engine_color::oklab::{linear_to_oklab, LinRgb, Oklab};
use engine_color::palette::{LinearColor, Palette};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::{PaletteLut3D, PaletteLutCache, DEFAULT_LUT_SIZE};

fn make_palette(n: usize) -> Palette {
    let colors: Vec<LinearColor> = (0..n)
        .map(|i| {
            let t = i as f32 / (n as f32).max(1.0);
            LinearColor {
                r: t,
                g: 1.0 - t,
                b: (t * 2.0) % 1.0,
            }
        })
        .collect();
    Palette {
        id: 1,
        name: format!("bench-{n}"),
        colors,
        revision: 1,
    }
}

fn sample_oklab(i: usize) -> Oklab {
    let t = (i as f32 * 0.6180339887) % 1.0;
    Oklab {
        l: t,
        a: -0.35 + t * 0.7,
        b: 0.35 - t * 0.7,
    }
}

fn bench_lut(c: &mut Criterion) {
    let palette = make_palette(16);
    let kd = PaletteKdCache::new();
    let tree = kd.get_or_build(1, &palette).unwrap();

    let mut group = c.benchmark_group("palette_lut");

    for &size in &[32u32, 64u32] {
        group.bench_with_input(BenchmarkId::new("build", size), &size, |b, &size| {
            b.iter(|| {
                black_box(PaletteLut3D::build(&palette, size, &tree).unwrap());
            });
        });
    }

    let lut32 = PaletteLut3D::build(&palette, 32, &tree).unwrap();
    let lut64 = PaletteLut3D::build(&palette, 64, &tree).unwrap();
    eprintln!(
        "LUT memory: size=32 → {} KiB, size=64 → {} KiB",
        lut32.grid_bytes() / 1024,
        lut64.grid_bytes() / 1024
    );

    const N: usize = 50_000;
    group.throughput(Throughput::Elements(N as u64));

    group.bench_function("nearest_kd", |b| {
        b.iter(|| {
            let mut acc = 0usize;
            for i in 0..N {
                acc ^= tree.nearest(sample_oklab(i));
            }
            black_box(acc);
        });
    });

    group.bench_function("nearest_lut32", |b| {
        b.iter(|| {
            let mut acc = 0u16;
            for i in 0..N {
                acc ^= lut32.nearest_index(sample_oklab(i));
            }
            black_box(acc);
        });
    });

    group.bench_function("nearest_lut64", |b| {
        b.iter(|| {
            let mut acc = 0u16;
            for i in 0..N {
                acc ^= lut64.nearest_index(sample_oklab(i));
            }
            black_box(acc);
        });
    });

    // Cache hit path (should be Arc clone only)
    let lut_cache = PaletteLutCache::new();
    let _ = lut_cache
        .get_or_build(1, &palette, &kd, DEFAULT_LUT_SIZE)
        .unwrap();
    group.bench_function("cache_hit", |b| {
        b.iter(|| {
            black_box(
                lut_cache
                    .get_or_build(1, &palette, &kd, DEFAULT_LUT_SIZE)
                    .unwrap(),
            );
        });
    });

    // Dense close-colors disagreement sample (metric, not timed heavily)
    let dense = make_palette(64);
    let tree_d = KdTree::build(
        &dense
            .colors
            .iter()
            .map(|c| linear_to_oklab(LinRgb { r: c.r, g: c.g, b: c.b }))
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let lut_d = PaletteLut3D::build(&dense, 32, &tree_d).unwrap();
    let mut disagree32 = 0usize;
    let mut disagree64 = 0usize;
    let lut_d64 = PaletteLut3D::build(&dense, 64, &tree_d).unwrap();
    for i in 0..10_000 {
        let lab = sample_oklab(i);
        if lut_d.nearest_index(lab) as usize != tree_d.nearest(lab) {
            disagree32 += 1;
        }
        if lut_d64.nearest_index(lab) as usize != tree_d.nearest(lab) {
            disagree64 += 1;
        }
    }
    eprintln!(
        "Dense K=64 disagreement: size=32 → {disagree32}/10000 ({:.2}%), size=64 → {disagree64}/10000 ({:.2}%)",
        disagree32 as f64 / 100.0,
        disagree64 as f64 / 100.0
    );

    group.finish();
}

criterion_group!(benches, bench_lut);
criterion_main!(benches);
