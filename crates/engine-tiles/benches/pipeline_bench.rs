use criterion::{criterion_group, criterion_main, Criterion};

fn viewport_20_tiles_5_layers(c: &mut Criterion) {
    c.bench_function("viewport_20_tiles_5_layers", |b| {
        b.iter(|| {
            // TODO: Populate in Wave 6 — benchmark 20-tile × 5-layer viewport refresh
        });
    });
}

criterion_group!(benches, viewport_20_tiles_5_layers);
criterion_main!(benches);
