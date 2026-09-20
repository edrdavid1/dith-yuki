//! Track C Phase 2: what `tile://` returns when L0 is dirty or evicted.

use std::sync::atomic::Ordering;

use engine_tiles::{find_cached_ancestor, upsample_from_ancestor, TileCache, TileKey, TILE_SIZE};

use crate::tile_protocol::f32_tile_to_rgba8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewServe {
    Fresh { rgba8: Vec<u8>, generation: u64 },
    Stale { rgba8: Vec<u8>, generation: u64 },
    PyramidFallback { rgba8: Vec<u8> },
    Pending,
}

pub fn resolve_preview_tile(cache: &TileCache, key: TileKey, doc_gen: u64) -> PreviewServe {
    if let Some(entry) = cache.entries.get(&key) {
        let dirty = entry.dirty.load(Ordering::Acquire);
        let gen = entry.generation;
        let rgba8 = f32_tile_to_rgba8(&entry.tile);
        if TileCache::tile_entry_is_ready(dirty, gen, doc_gen) {
            return PreviewServe::Fresh {
                rgba8,
                generation: gen,
            };
        }
        return PreviewServe::Stale {
            rgba8,
            generation: gen,
        };
    }
    if let Some((ancestor_c, tile)) = find_cached_ancestor(cache, key) {
        let up = upsample_from_ancestor(&tile, key.coord, ancestor_c);
        return PreviewServe::PyramidFallback {
            rgba8: f32_tile_to_rgba8(&up),
        };
    }
    PreviewServe::Pending
}

pub fn rgba8_len() -> usize {
    (TILE_SIZE * TILE_SIZE * 4) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_tiles::{CacheStage, PixelTile, TileCoord, HALO};
    use std::sync::Arc;

    fn key(level: u8, x: u32, y: u32) -> TileKey {
        TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord { level, x, y },
            stage: CacheStage::Composite,
        }
    }

    fn solid(r: f32) -> Arc<PixelTile> {
        let mut t = PixelTile::new();
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                t.set(x, y, 0, r);
            }
        }
        Arc::new(t)
    }

    #[test]
    fn dirty_entry_is_stale_not_pending() {
        let cache = TileCache::new(50_000_000);
        cache.insert_fresh_gen(key(0, 0, 0), solid(0.4), 1);
        cache.mark_dirty(key(0, 0, 0));
        match resolve_preview_tile(&cache, key(0, 0, 0), 2) {
            PreviewServe::Stale { rgba8, generation } => {
                assert_eq!(generation, 1);
                assert_eq!(rgba8.len(), rgba8_len());
            }
            other => panic!("expected stale, got {other:?}"),
        }
    }

    #[test]
    fn ready_entry_is_fresh() {
        let cache = TileCache::new(50_000_000);
        cache.insert_fresh_gen(key(0, 0, 0), solid(0.2), 3);
        assert!(matches!(
            resolve_preview_tile(&cache, key(0, 0, 0), 3),
            PreviewServe::Fresh { generation: 3, .. }
        ));
    }

    #[test]
    fn evicted_l0_uses_l1_pyramid() {
        let cache = TileCache::new(50_000_000);
        cache.insert_fresh(key(1, 0, 0), solid(0.8));
        match resolve_preview_tile(&cache, key(0, 0, 0), 1) {
            PreviewServe::PyramidFallback { rgba8 } => {
                assert_eq!(rgba8.len(), rgba8_len());
                assert!(rgba8[0] > 180, "upsampled red channel from 0.8");
            }
            other => panic!("expected pyramid fallback, got {other:?}"),
        }
    }

    #[test]
    fn miss_without_ancestor_is_pending() {
        let cache = TileCache::new(50_000_000);
        assert_eq!(
            resolve_preview_tile(&cache, key(0, 3, 3), 1),
            PreviewServe::Pending
        );
    }
}
