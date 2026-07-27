use criterion::{black_box, criterion_group, criterion_main, Criterion};
use engine_tiles::{TileCache, TileKey, TileCoord, CacheStage, PixelTile};
use std::sync::Arc;

/// Benchmark TileCache::get_or_insert() latency with 1000 tiles.
/// Measures the time to insert and retrieve tiles from the cache.
fn bench_cache_get_or_insert(c: &mut Criterion) {
    c.bench_function("cache_get_or_insert_1000_tiles", |b| {
        // Create a cache with sufficient budget for 1000 tiles
        // Each tile is approximately 1.03 MB, so budget for ~1.1 GB
        let budget_bytes = 1_100_000_000;
        let cache = black_box(TileCache::new(budget_bytes));

        // Pre-populate cache with tiles to simulate realistic scenario
        for i in 0..1000u32 {
            let key = TileKey {
                layer: 0,
                coord: TileCoord {
                    level: 0,
                    x: i,
                    y: i,
                },
                stage: CacheStage::Raw,
            };
            let tile = Arc::new(PixelTile::new());
            let _ = cache.get_or_insert(key, tile);
        }

        // Benchmark get_or_insert on existing tiles
        b.iter(|| {
            for i in 0..1000u32 {
                let key = black_box(TileKey {
                    layer: 0,
                    coord: TileCoord {
                        level: 0,
                        x: i,
                        y: i,
                    },
                    stage: CacheStage::Raw,
                });
                let tile = black_box(Arc::new(PixelTile::new()));
                let _result = cache.get_or_insert(key, tile);
            }
        });
    });
}

/// Benchmark TileCache::mark_dirty() latency.
/// Measures the time to mark tiles as dirty in the cache.
fn bench_cache_mark_dirty(c: &mut Criterion) {
    c.bench_function("cache_mark_dirty", |b| {
        // Create a cache with sufficient budget
        let budget_bytes = 1_100_000_000;
        let cache = black_box(TileCache::new(budget_bytes));

        // Pre-populate cache with tiles
        for i in 0..1000u32 {
            let key = TileKey {
                layer: 0,
                coord: TileCoord {
                    level: 0,
                    x: i,
                    y: i,
                },
                stage: CacheStage::Raw,
            };
            let tile = Arc::new(PixelTile::new());
            let _ = cache.get_or_insert(key, tile);
        }

        // Benchmark mark_dirty on existing tiles
        b.iter(|| {
            for i in 0..1000u32 {
                let key = black_box(TileKey {
                    layer: 0,
                    coord: TileCoord {
                        level: 0,
                        x: i,
                        y: i,
                    },
                    stage: CacheStage::Raw,
                });
                cache.mark_dirty(key);
            }
        });
    });
}

criterion_group!(benches, bench_cache_get_or_insert, bench_cache_mark_dirty);
criterion_main!(benches);
