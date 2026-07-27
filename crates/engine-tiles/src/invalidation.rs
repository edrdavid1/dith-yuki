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
    /// Layer raw pixels changed at specific coordinates.
    ///
    /// Marks Raw + Processed for the affected tiles, and cascades to Composite for the layer
    /// and all layers above. This is the most disruptive change type.
    ///
    /// # Arguments
    /// - `layer`: The affected layer
    /// - `coords`: Specific tile coordinates that changed (may be empty = all tiles affected)
    LayerRawChanged {
        layer: LayerId,
        coords: Vec<TileCoord>,
    },

    /// Layer filters changed (affects all tiles of the layer).
    ///
    /// Marks all Processed tiles for the affected layer, and cascades to Composite for the layer
    /// and all layers above. Does not invalidate Raw tiles (pixels unchanged).
    ///
    /// # Arguments
    /// - `layer`: The affected layer
    LayerFilterChanged { layer: LayerId },

    /// Layer properties changed (opacity, blend mode, visibility).
    ///
    /// Only cascades Composite tiles for the affected layer and layers above.
    /// Raw and Processed tiles of this layer remain valid (their content didn't change,
    /// only how they're blended into the composite).
    ///
    /// # Arguments
    /// - `layer`: The affected layer
    LayerPropsChanged { layer: LayerId },

    /// Layer mask changed at specific coordinates.
    ///
    /// Marks Processed tiles at the affected coordinates, and cascades to Composite for the layer
    /// and all layers above. Affects pixel content of the layer, but only at masked coordinates.
    ///
    /// # Arguments
    /// - `layer`: The affected layer
    /// - `coords`: Specific tile coordinates where mask changed
    MaskChanged {
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
        InvalidationEvent::LayerRawChanged { layer, coords } => {
            for coord in coords {
                // Mark Raw stage dirty
                cache.mark_dirty(TileKey {
                    layer,
                    coord,
                    stage: CacheStage::Raw,
                });
                // Mark Processed stage dirty
                cache.mark_dirty(TileKey {
                    layer,
                    coord,
                    stage: CacheStage::Processed,
                });
                // Cascade: mark Composite for this layer and all above
                cascade_composite_invalidation(cache, layer, coord);
            }
        }

        InvalidationEvent::LayerFilterChanged { layer } => {
            // Mark all Processed tiles for this layer as dirty
            mark_all_processed_for_layer(cache, layer);
            // Cascade: mark Composite for this layer and all above
            cascade_composite_invalidation_all_coords(cache, layer);
        }

        InvalidationEvent::LayerPropsChanged { layer } => {
            // Only cascade Composite; Raw and Processed of this layer are still valid
            cascade_composite_invalidation_all_coords(cache, layer);
        }

        InvalidationEvent::MaskChanged { layer, coords } => {
            for coord in coords {
                // Mark Processed stage dirty (mask affects processed content)
                cache.mark_dirty(TileKey {
                    layer,
                    coord,
                    stage: CacheStage::Processed,
                });
                // Cascade: mark Composite for this layer and all above
                cascade_composite_invalidation(cache, layer, coord);
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
fn mark_all_processed_for_layer(cache: &TileCache, layer: LayerId) {
    let mut keys_to_mark = Vec::new();

    // Collect keys from cache entries
    for entry in cache.entries.iter() {
        let key = *entry.key();
        if key.layer == layer && key.stage == CacheStage::Processed {
            keys_to_mark.push(key);
        }
    }

    // Mark collected keys as dirty
    for key in keys_to_mark {
        cache.mark_dirty(key);
    }
}

/// Cascade Composite tile invalidation for a specific coordinate.
///
/// Marks all Composite tiles at the given coordinate for the specified layer
/// and all layers above (layer ≥ given layer) as dirty.
///
/// This maintains the invariant that Composite tiles depend on all Raw/Processed
/// tiles below them. When a lower layer changes, all Composite tiles that depend
/// on it must be recomputed.
///
/// # Arguments
///
/// - `cache`: The TileCache
/// - `layer`: The base layer; tiles at this layer and above will be cascaded
/// - `coord`: The tile coordinate to cascade
fn cascade_composite_invalidation(cache: &TileCache, layer: LayerId, coord: TileCoord) {
    let mut keys_to_mark = Vec::new();

    // Iterate cache entries and find Composite tiles at this coordinate
    // for the given layer and all layers above
    for entry in cache.entries.iter() {
        let key = *entry.key();
        if key.stage == CacheStage::Composite
            && key.coord == coord
            && key.layer >= layer
        {
            keys_to_mark.push(key);
        }
    }

    // Mark collected keys as dirty
    for key in keys_to_mark {
        cache.mark_dirty(key);
    }
}

/// Cascade Composite tile invalidation for all coordinates of a given layer.
///
/// Marks all Composite tiles for the specified layer and all layers above as dirty.
/// This is used when layer properties change (affecting all Composite tiles).
///
/// # Arguments
///
/// - `cache`: The TileCache
/// - `layer`: The base layer; tiles at this layer and above will be cascaded
fn cascade_composite_invalidation_all_coords(cache: &TileCache, layer: LayerId) {
    let mut keys_to_mark = Vec::new();

    // Iterate cache entries and find Composite tiles for this layer and above
    for entry in cache.entries.iter() {
        let key = *entry.key();
        if key.stage == CacheStage::Composite && key.layer >= layer {
            keys_to_mark.push(key);
        }
    }

    // Mark collected keys as dirty
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

        let event = InvalidationEvent::LayerRawChanged {
            layer: 0,
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

        let event = InvalidationEvent::LayerRawChanged {
            layer: 0,
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

        let event = InvalidationEvent::LayerFilterChanged { layer: 0 };
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

        let event = InvalidationEvent::LayerPropsChanged { layer: 0 };
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

        let event = InvalidationEvent::MaskChanged {
            layer: 0,
            coords: vec![make_coord(0, 0)],
        };
        invalidate(&cache, event);

        // Processed should be dirty
        assert!(cache.entries.get(&key_processed).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        // Raw should not be
        assert!(!cache.entries.get(&key_raw).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn cascade_respects_layer_boundaries() {
        let cache = TileCache::new(10_000_000);
        let key_composite_0 = make_key(0, 0, 0, CacheStage::Composite);
        let key_composite_1 = make_key(1, 0, 0, CacheStage::Composite);
        let key_composite_2 = make_key(2, 0, 0, CacheStage::Composite);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key_composite_0, tile.clone());
        cache.get_or_insert(key_composite_1, tile.clone());
        cache.get_or_insert(key_composite_2, tile);

        // Invalidate layer 1; should cascade to layer 2 but not layer 0
        let event = InvalidationEvent::LayerRawChanged {
            layer: 1,
            coords: vec![make_coord(0, 0)],
        };
        invalidate(&cache, event);

        // Layer 1 and above should be dirty
        assert!(!cache.entries.get(&key_composite_0).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key_composite_1).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
        assert!(cache.entries.get(&key_composite_2).unwrap().dirty.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn invalidate_empty_coords_does_nothing() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(0, 0, 0, CacheStage::Raw);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key, tile);

        let event = InvalidationEvent::LayerRawChanged {
            layer: 0,
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

        let event = InvalidationEvent::LayerRawChanged {
            layer: 0,
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

        let event = InvalidationEvent::LayerRawChanged {
            layer: 99,
            coords: vec![make_coord(99, 99)],
        };

        // Should not panic
        invalidate(&cache, event);
    }
}
