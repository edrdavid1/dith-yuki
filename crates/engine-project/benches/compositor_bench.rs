use criterion::{criterion_group, criterion_main, Criterion};

fn single_tile_no_filter(c: &mut Criterion) {
    c.bench_function("single_tile_no_filter", |b| {
        b.iter(|| {
            // Placeholder: will be populated in Wave 6
        });
    });
}

fn composite_5_layers(c: &mut Criterion) {
    c.bench_function("composite_5_layers", |b| {
        b.iter(|| {
            // Placeholder: will be populated in Wave 6
        });
    });
}

criterion_group!(benches, single_tile_no_filter, composite_5_layers);
criterion_main!(benches);
