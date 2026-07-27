use criterion::{black_box, criterion_group, criterion_main, Criterion};
use engine_tiles::{downsample_tile, PixelTile, HALO, TILE_SIZE};

/// Benchmark the downsample_tile throughput with a parent tile.
/// Creates a parent PixelTile and measures the time to downsample it.
/// Target: ≤5ms per 256×256 tile (TILE_SIZE=256, output size=(TILE_SIZE+2*HALO)²)
fn bench_downsample_tile(c: &mut Criterion) {
    c.bench_function("downsample_tile_256x256", |b| {
        // Create parent tile with sample data
        let mut parent = PixelTile::new();

        // Populate parent main region with sample values
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for ch in 0..4 {
                    parent.set(x, y, ch, 0.5);
                }
            }
        }

        // Benchmark the downsampling operation
        b.iter(|| {
            // Use black_box to prevent the compiler from optimizing away the computation
            let parent_ref = black_box(&parent);
            let _child = downsample_tile(parent_ref);
            _child
        });
    });
}

criterion_group!(benches, bench_downsample_tile);
criterion_main!(benches);
