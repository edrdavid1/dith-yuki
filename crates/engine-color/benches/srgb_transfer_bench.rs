//! Benchmark previous direct sRGB↔linear transfer vs the production LUT path.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use engine_color::palette::{
    linear_to_srgb, linear_to_srgb_reference, srgb_to_linear, srgb_to_linear_reference,
};

fn bench_srgb_transfer(c: &mut Criterion) {
    let mut group = c.benchmark_group("srgb_transfer");
    const N: usize = 1 << 20;
    group.throughput(Throughput::Elements(N as u64));

    group.bench_function("srgb_to_linear_reference_powf", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for i in 0..N {
                acc ^= srgb_to_linear_reference((i % 256) as u8).to_bits();
            }
            black_box(acc);
        });
    });

    group.bench_function("srgb_to_linear_lut", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for i in 0..N {
                acc ^= srgb_to_linear((i % 256) as u8).to_bits();
            }
            black_box(acc);
        });
    });

    // Warm the LUT so the first production call is not attributed to init.
    let _ = srgb_to_linear(0);

    group.bench_function("linear_to_srgb_reference", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for i in 0..N {
                let t = (i as f32) / (N as f32);
                acc += linear_to_srgb_reference(t) as u32;
            }
            black_box(acc);
        });
    });

    group.bench_function("linear_to_srgb_production", |b| {
        b.iter(|| {
            let mut acc = 0u32;
            for i in 0..N {
                let t = (i as f32) / (N as f32);
                acc += linear_to_srgb(t) as u32;
            }
            black_box(acc);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_srgb_transfer);
criterion_main!(benches);
