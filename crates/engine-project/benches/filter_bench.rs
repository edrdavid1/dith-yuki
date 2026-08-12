use criterion::{criterion_group, criterion_main, Criterion};

fn single_tile_levels(c: &mut Criterion) {
    c.bench_function("single_tile_levels", |b| {
        b.iter(|| {
            // Placeholder: will be populated in Wave 6
        });
    });
}

criterion_group!(benches, single_tile_levels);
criterion_main!(benches);
