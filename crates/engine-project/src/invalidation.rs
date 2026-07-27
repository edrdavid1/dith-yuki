//! Document-level invalidation and cache coordination.
//!
//! This module handles document structure changes (add/remove/reorder layers)
//! and coordinates with the Phase 1 tile cache for invalidation.

use crate::document::Document;
use crate::types::LayerId;
use engine_tiles::invalidation::InvalidationEvent;
use engine_tiles::TileCache;

/// Handle invalidation for layer structure changes.
///
/// When layers are added, removed, or reordered, the Composite tiles for affected
/// layers must be marked dirty.
pub fn invalidate_layer_structure_changed(
    cache: &TileCache,
    _added: &[LayerId],
    _removed: &[LayerId],
) {
    // Mark all Composite tiles dirty (layer order changed, all need recomputation)
    // This is a conservative approach; Phase 3+ can optimize to only affected layers
    
    let mut keys_to_mark = Vec::new();

    // Collect all Composite tiles from cache
    for entry in cache.entries.iter() {
        let key = *entry.key();
        if key.stage == engine_tiles::types::CacheStage::Composite {
            keys_to_mark.push(key);
        }
    }

    // Mark all as dirty
    for key in keys_to_mark {
        cache.mark_dirty(key);
    }
}

/// Handle invalidation for layer property changes.
///
/// Updates to layer opacity, blend mode, visibility, or offset require Composite
/// recomputation.
pub fn invalidate_layer_props_changed(cache: &TileCache, layer_id: LayerId) {
    let event = InvalidationEvent::LayerPropsChanged { layer: layer_id.0 };
    engine_tiles::invalidation::invalidate(cache, event);
}

/// Handle invalidation for layer filter changes.
///
/// Updates to layer filter stack require Processed and Composite recomputation.
pub fn invalidate_layer_filter_changed(cache: &TileCache, layer_id: LayerId) {
    let event = InvalidationEvent::LayerFilterChanged { layer: layer_id.0 };
    engine_tiles::invalidation::invalidate(cache, event);
}

/// Handle invalidation for layer visibility change.
///
/// Visibility is a layer property, so only Composite needs invalidation.
pub fn invalidate_layer_visibility_changed(cache: &TileCache, layer_id: LayerId) {
    invalidate_layer_props_changed(cache, layer_id);
}

/// Validate document consistency before mutation.
///
/// Checks that layer IDs are valid and don't reference non-existent layers.
/// Returns Err if structure is invalid.
pub fn validate_document_consistency(doc: &Document, layer_id: LayerId) -> Result<(), String> {
    // Find layer in tree
    fn find_layer_recursive(nodes: &[crate::layer::LayerNode], id: LayerId) -> bool {
        for node in nodes {
            match node {
                crate::layer::LayerNode::Leaf(layer) => {
                    if layer.id == id {
                        return true;
                    }
                }
                crate::layer::LayerNode::Group(group) => {
                    if group.id == id {
                        return true;
                    }
                    if find_layer_recursive(&group.children, id) {
                        return true;
                    }
                }
            }
        }
        false
    }

    if find_layer_recursive(&doc.root, layer_id) {
        Ok(())
    } else {
        Err(format!("Layer {} not found in document", layer_id.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_tiles::TileCache;

    #[test]
    fn validate_document_consistency_finds_layer() {
        let mut doc = Document::default();
        let layer = crate::layer::Layer::new(LayerId::new(1), crate::types::LayerKind::Raster, 256, 256);
        doc.root.push(crate::layer::LayerNode::Leaf(layer));

        assert!(validate_document_consistency(&doc, LayerId::new(1)).is_ok());
    }

    #[test]
    fn validate_document_consistency_fails_for_missing_layer() {
        let doc = Document::default();
        assert!(validate_document_consistency(&doc, LayerId::new(999)).is_err());
    }

    #[test]
    fn invalidate_layer_props_marked_dirty() {
        let cache = TileCache::new(10_000_000);
        let key = engine_tiles::TileKey {
            layer: 0,
            coord: engine_tiles::types::TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: engine_tiles::types::CacheStage::Composite,
        };
        let tile = std::sync::Arc::new(engine_tiles::tile::PixelTile::new());
        cache.get_or_insert(key, tile);

        invalidate_layer_props_changed(&cache, LayerId::new(0));

        // Composite tile should be marked dirty
        assert!(cache
            .entries
            .get(&key)
            .unwrap()
            .dirty
            .load(std::sync::atomic::Ordering::Relaxed));
    }
}
