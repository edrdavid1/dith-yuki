//! Invalidation cascading for selective cache coherence.
//!
//! This module implements invalidation events and cascading logic to maintain cache coherence
//! when document state changes. For architecture details, see `tile-engine-architecture.md` §3.3 (Invalidation).
//!
//! # Overview
//!
//! The invalidation system selectively marks tiles dirty based on the type of change:
//! - **LayerRawChanged**: Layer pixels changed → mark Raw + Processed + cascade Composite
//! - **LayerFilterChanged**: Layer filters changed → mark Processed + cascade Composite
//! - **LayerPropsChanged**: Layer opacity/blend changed → cascade Composite only
//! - **MaskChanged**: Layer mask changed → mark Processed + cascade Composite
//!
//! # Cascade Semantics
//!
//! Composite tiles depend on all layers below. When a Raw or Processed tile changes,
//! all Composite tiles of that layer and layers above must be marked dirty.
//!
//! # Notes
//!
//! The cascade is performed by iterating cache entries (may be approximate for large caches).
//! Deleted tiles during cascading do not cause issues; only existing tiles are marked dirty.

use crate::{TileCache, TileKey, CacheStage, LayerId, TileCoord};

/// Describes a document change that may invalidate cached tiles.
///
/// Each variant represents a different type of change with different invalidation semantics.
#[derive(Clone, Debug)]
pub enum InvalidationEvent {
    LayerRawChanged {
        doc: u32,
        layer: LayerId,
        coords: Vec<TileCoord>,
    },
    LayerFilterChanged { doc: u32, layer: LayerId },
    LayerPropsChanged { doc: u32, layer: LayerId },
    MaskChanged {
        doc: u32,
        layer: LayerId,
        coords: Vec<TileCoord>,
    },
}

/// Apply an invalidation event to the cache, marking affected tiles dirty.
///
/// Routes to the appropriate invalidation logic based on the event type.
/// See `InvalidationEvent` for semantics of each type.
///
/// # Arguments
///
/// - `cache`: The TileCache to invalidate
/// - `event`: The invalidation event describing the change
///
/// # Examples
///
/// ```ignore
/// let cache = TileCache::new(10_000_000);
/// let cache.get_or_insert(/* ... */);
///
/// // Layer pixels changed
/// let event = InvalidationEvent::LayerRawChanged {
///     layer: 0,
///     coords: vec![TileCoord { level: 0, x: 1, y: 1 }],
/// };
/// invalidate(&cache, event);
///
/// // The affected tiles are now marked dirty
/// ```
pub fn invalidate(cache: &TileCache, event: InvalidationEvent) {
    match event {
        InvalidationEvent::LayerRawChanged { doc, layer, coords } => {
            for coord in coords {
                cache.mark_dirty(TileKey {
                    doc,
                    layer,
                    coord,
                    stage: CacheStage::Raw,
                });
                cache.mark_dirty(TileKey {
                    doc,
                    layer,
                    coord,
                    stage: CacheStage::Processed,
                });
                cascade_composite_invalidation(cache, doc, layer, coord);
            }
        }

        InvalidationEvent::LayerFilterChanged { doc, layer } => {
            mark_all_processed_for_layer(cache, doc, layer);
            cascade_composite_invalidation_all_coords(cache, doc, layer);
        }

        InvalidationEvent::LayerPropsChanged { doc, layer } => {
            cascade_composite_invalidation_all_coords(cache, doc, layer);
        }

        InvalidationEvent::MaskChanged { doc, layer, coords } => {
            for coord in coords {
                cache.mark_dirty(TileKey {
                    doc,
                    layer,
                    coord,
                    stage: CacheStage::Processed,
                });
                cascade_composite_invalidation(cache, doc, layer, coord);
            }
        }
    }
}

/// Mark all Processed tiles of a given layer as dirty.
///
/// Iterates through the cache entries and marks any Processed tile belonging to the layer.
/// This is used when layer filters change (affecting all Processed tiles).
///
/// # Arguments
///
/// - `cache`: The TileCache
/// - `layer`: The layer whose Processed tiles should be marked dirty
fn mark_all_processed_for_layer(cache: &TileCache, doc: u32, layer: LayerId) {
    let mut keys_to_mark = Vec::new();
    for entry in cache.entries.iter() {
        let key = *entry.key();
        if key.doc == doc && key.layer == layer && key.stage == CacheStage::Processed {
            keys_to_mark.push(key);
        }
    }
    for key in keys_to_mark {
        cache.mark_dirty(key);
    }
}

fn cascade_composite_invalidation(cache: &TileCache, doc: u32, _layer: LayerId, coord: TileCoord) {
    let mut keys_to_mark = Vec::new();
    for entry in cache.entries.iter() {
        let key = *entry.key();
        if key.doc == doc && key.stage == CacheStage::Composite && key.coord == coord {
            keys_to_mark.push(key);
        }
    }
    for key in keys_to_mark {
        cache.mark_dirty(key);
    }
}

fn cascade_composite_invalidation_all_coords(cache: &TileCache, doc: u32, _layer: LayerId) {
    let mut keys_to_mark = Vec::new();
    for entry in cache.entries.iter() {
        let key = *entry.key();
        if key.doc == doc && key.stage == CacheStage::Composite {
            keys_to_mark.push(key);
        }
    }
    for key in keys_to_mark {
        cache.mark_dirty(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PixelTile};
    use std::sync::Arc;

    fn make_key(layer: u32, x: u32, y: u32, stage: CacheStage) -> TileKey {
        TileKey {
            doc: 1,
            layer,
            coord: TileCoord {
                level: 0,
                x,
                y,
            },
            stage,
        }
    }

    fn make_coord(x: u32, y: u32) -> TileCoord {
        TileCoord {
            level: 0,
            x,
            y,
        }
    }

    #[test]
    fn invalidate_layer_raw_changed_marks_raw_and_processed() {
        let cache = TileCache::new(10_000_000);
        let key_raw = make_key(0, 0, 0, CacheStage::Raw);
        let key_processed = make_key(0, 0, 0, CacheStage::Processed);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key_raw, tile.clone());
        cache.get_or_insert(key_processed, tile);

        let event = InvalidationEvent::LayerRawChanged { doc: 1, layer: 0,
            coords: vec![make_coord(0, 0)],
        };
        invalidate(&cache, event);

        assert!(cache.entries.get(&key_raw).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key_processed).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_layer_raw_changed_cascades_composite() {
        let cache = TileCache::new(10_000_000);
        let key_composite_0 = make_key(0, 0, 0, CacheStage::Composite);
        let key_composite_1 = make_key(1, 0, 0, CacheStage::Composite);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key_composite_0, tile.clone());
        cache.get_or_insert(key_composite_1, tile);

        let event = InvalidationEvent::LayerRawChanged { doc: 1, layer: 0,
            coords: vec![make_coord(0, 0)],
        };
        invalidate(&cache, event);

        // Both composite tiles (layer 0 and 1) should be marked dirty
        assert!(cache.entries.get(&key_composite_0).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key_composite_1).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_layer_filter_changed_marks_processed() {
        let cache = TileCache::new(10_000_000);
        let key_processed_1 = make_key(0, 0, 0, CacheStage::Processed);
        let key_processed_2 = make_key(0, 1, 0, CacheStage::Processed);
        let key_raw = make_key(0, 0, 0, CacheStage::Raw);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key_processed_1, tile.clone());
        cache.get_or_insert(key_processed_2, tile.clone());
        cache.get_or_insert(key_raw, tile);

        let event = InvalidationEvent::LayerFilterChanged { doc: 1, layer: 0 };
        invalidate(&cache, event);

        // Processed tiles should be dirty
        assert!(cache.entries.get(&key_processed_1).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key_processed_2).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));

        // Raw tiles should not be marked dirty by filter change
        assert!(!cache.entries.get(&key_raw).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_layer_props_changed_cascades_only_composite() {
        let cache = TileCache::new(10_000_000);
        let key_composite = make_key(0, 0, 0, CacheStage::Composite);
        let key_processed = make_key(0, 0, 0, CacheStage::Processed);
        let key_raw = make_key(0, 0, 0, CacheStage::Raw);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key_composite, tile.clone());
        cache.get_or_insert(key_processed, tile.clone());
        cache.get_or_insert(key_raw, tile);

        let event = InvalidationEvent::LayerPropsChanged { doc: 1, layer: 0 };
        invalidate(&cache, event);

        // Only Composite should be dirty
        assert!(cache.entries.get(&key_composite).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!cache.entries.get(&key_processed).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(!cache.entries.get(&key_raw).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_mask_changed_marks_processed() {
        let cache = TileCache::new(10_000_000);
        let key_processed = make_key(0, 0, 0, CacheStage::Processed);
        let key_raw = make_key(0, 0, 0, CacheStage::Raw);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key_processed, tile.clone());
        cache.get_or_insert(key_raw, tile);

        let event = InvalidationEvent::MaskChanged { doc: 1, layer: 0,
            coords: vec![make_coord(0, 0)],
        };
        invalidate(&cache, event);

        // Processed should be dirty
        assert!(cache.entries.get(&key_processed).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        // Raw should not be
        assert!(!cache.entries.get(&key_raw).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn cascade_marks_all_composite_tiles_dirty() {
        let cache = TileCache::new(10_000_000);
        let key_composite_0 = make_key(0, 0, 0, CacheStage::Composite);
        let key_composite_1 = make_key(1, 0, 0, CacheStage::Composite);
        let key_composite_2 = make_key(2, 0, 0, CacheStage::Composite);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key_composite_0, tile.clone());
        cache.get_or_insert(key_composite_1, tile.clone());
        cache.get_or_insert(key_composite_2, tile);

        // Invalidate layer 1; should cascade to ALL composite tiles
        // because the global composite depends on all layers
        let event = InvalidationEvent::LayerRawChanged { doc: 1, layer: 1,
            coords: vec![make_coord(0, 0)],
        };
        invalidate(&cache, event);

        // All Composite tiles should be dirty since composite depends on all layers
        assert!(cache.entries.get(&key_composite_0).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key_composite_1).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key_composite_2).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_empty_coords_does_nothing() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(0, 0, 0, CacheStage::Raw);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key, tile);

        let event = InvalidationEvent::LayerRawChanged { doc: 1, layer: 0,
            coords: vec![],
        };
        invalidate(&cache, event);

        // Should be unaffected
        assert!(!cache.entries.get(&key).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_multiple_raw_changed_coords() {
        let cache = TileCache::new(10_000_000);
        let key1 = make_key(0, 0, 0, CacheStage::Raw);
        let key2 = make_key(0, 1, 0, CacheStage::Raw);
        let key3 = make_key(0, 2, 0, CacheStage::Raw);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key1, tile.clone());
        cache.get_or_insert(key2, tile.clone());
        cache.get_or_insert(key3, tile);

        let event = InvalidationEvent::LayerRawChanged { doc: 1, layer: 0,
            coords: vec![make_coord(0, 0), make_coord(1, 0), make_coord(2, 0)],
        };
        invalidate(&cache, event);

        assert!(cache.entries.get(&key1).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key2).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key3).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_nonexistent_key_is_safe() {
        let cache = TileCache::new(10_000_000);

        let event = InvalidationEvent::LayerRawChanged { doc: 1, layer: 99,
            coords: vec![make_coord(99, 99)],
        };

        // Should not panic
        invalidate(&cache, event);
    }
}
