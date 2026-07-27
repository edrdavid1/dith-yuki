//! On-demand tile pipeline for computing Processed-stage and Composite-stage tiles.
//!
//! This module provides:
//! - `compute_processed_tile`: derives a Processed tile from a Raw tile by applying
//!   the layer's filter stack.
//! - `compute_composite_tile`: blends all visible layers at a tile coordinate to
//!   produce a Composite tile.
//!
//! Both are called by the worker pool when a tile is needed (cache miss or dirty).

use std::sync::Arc;

use engine_project::composite_tile;
use engine_project::error::EngineError;
use engine_project::filters::apply::apply_filter_to_tile;
use engine_project::layer::LayerNode;
use engine_tiles::{CacheStage, PixelTile, TileKey};

use crate::commands::AppState;

/// Compute a Processed-stage tile on demand.
///
/// Steps:
/// 1. Fetch the Raw tile from TileCache (same layer + coord, Raw stage)
/// 2. Get the layer's filter stack from the document snapshot
/// 3. Apply all enabled filters via `apply_filter_to_tile`
/// 4. Store the result in TileCache at the Processed stage
/// 5. Return the processed tile
///
/// # Errors
///
/// Returns `EngineError::InvalidState` if the Raw tile is not in the cache,
/// or `EngineError::LayerNotFound` if the layer doesn't exist in the document.
pub fn compute_processed_tile(
    key: TileKey,
    state: &AppState,
) -> Result<PixelTile, EngineError> {
    // 1. Build the Raw-stage key and fetch from cache
    let raw_key = TileKey {
        layer: key.layer,
        coord: key.coord,
        stage: CacheStage::Raw,
    };

    let raw_tile = state
        .tile_cache
        .get_entry(raw_key)
        .ok_or_else(|| EngineError::invalid_state(format!(
            "Raw tile not found in cache for layer={}, coord=({},{}) level={}",
            key.layer, key.coord.x, key.coord.y, key.coord.level
        )))?;

    // 2. Get document snapshot and find the layer
    let snapshot = state.document_handle.snapshot();
    let layer = find_layer_by_id(&snapshot.root, key.layer)
        .ok_or_else(|| EngineError::layer_not_found(
            engine_project::types::LayerId::new(key.layer),
        ))?;

    // 3. Apply the layer's filter stack to the raw tile.
    //    If the layer has no filters, the result is a copy of the raw tile.
    let processed = if layer.filters.is_empty() {
        // No filters — copy raw tile data directly
        copy_tile(&raw_tile)
    } else {
        // apply_filter_to_tile applies all enabled filters in the layer's stack
        apply_filter_to_tile(&raw_tile, layer, key.coord)?
    };

    // 4. Store result in cache at Processed stage.
    //    We copy the tile for the return value and wrap the original in Arc for storage.
    //    insert_fresh always overwrites any existing (possibly dirty) entry.
    let return_tile = copy_tile(&processed);
    let processed_key = TileKey {
        layer: key.layer,
        coord: key.coord,
        stage: CacheStage::Processed,
    };
    state
        .tile_cache
        .insert_fresh(processed_key, Arc::new(processed));

    // 5. Return the processed tile
    Ok(return_tile)
}

/// Compute a Composite-stage tile on demand.
///
/// Ensures all visible layers have fresh Processed tiles before compositing.
/// Blends all visible layers bottom-to-top at the requested tile coordinate.
///
/// Steps:
/// 1. Take a document snapshot for the current layer tree
/// 2. Ensure all visible layers have Processed tiles (compute from Raw if needed)
/// 3. Call `composite_tile` with the root layer tree and tile coord
/// 4. Store the result in TileCache at the Composite stage
/// 5. Return the composite tile
///
/// # Errors
///
/// Returns `EngineError` if compositing fails.
pub fn compute_composite_tile(
    key: TileKey,
    state: &AppState,
) -> Result<PixelTile, EngineError> {
    // 1. Get document snapshot for the layer tree
    let snapshot = state.document_handle.snapshot();

    // 2. Ensure all visible layers have fresh Processed tiles at this coordinate.
    //    This handles the case where a filter was changed and the Processed tile
    //    is dirty or missing — we compute it from the Raw tile before compositing.
    ensure_processed_tiles_fresh(&snapshot.root, key.coord, state)?;

    // 3. Composite all visible layers at this tile coordinate.
    let composited = composite_tile(&snapshot.root, key.coord, &state.tile_cache)?;

    // 4. Store result in cache at Composite stage.
    let return_tile = copy_tile(&composited);
    let composite_key = TileKey {
        layer: key.layer,
        coord: key.coord,
        stage: CacheStage::Composite,
    };
    state
        .tile_cache
        .insert_fresh(composite_key, Arc::new(composited));

    // 5. Return the composite tile
    Ok(return_tile)
}

/// Ensure all visible layers have fresh (non-dirty) Processed tiles at the given coord.
/// If a Processed tile is missing or dirty, compute it from the Raw tile.
fn ensure_processed_tiles_fresh(
    nodes: &[LayerNode],
    coord: engine_tiles::TileCoord,
    state: &AppState,
) -> Result<(), EngineError> {
    use std::sync::atomic::Ordering;

    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if !layer.visible {
                    continue;
                }
                let processed_key = TileKey {
                    layer: layer.id.0,
                    coord,
                    stage: CacheStage::Processed,
                };
                // Check if Processed tile is missing or dirty
                let needs_compute = match state.tile_cache.entries.get(&processed_key) {
                    None => true,
                    Some(entry) => entry.dirty.load(Ordering::Acquire),
                };
                if needs_compute {
                    // Compute Processed tile from Raw tile (this stores it in cache)
                    let _ = compute_processed_tile(processed_key, state);
                    // Ignore errors — compositor will fall back to Raw if Processed fails
                }
            }
            LayerNode::Group(group) => {
                if !group.visible {
                    continue;
                }
                ensure_processed_tiles_fresh(&group.children, coord, state)?;
            }
        }
    }
    Ok(())
}

/// Find a layer by its numeric ID in the document tree (recursive).
fn find_layer_by_id(nodes: &[LayerNode], layer_id: u32) -> Option<&engine_project::layer::Layer> {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if layer.id.0 == layer_id {
                    return Some(layer);
                }
            }
            LayerNode::Group(group) => {
                if let Some(found) = find_layer_by_id(&group.children, layer_id) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Copy a PixelTile's data into a new PixelTile (full 260×260 region including halo).
fn copy_tile(src: &PixelTile) -> PixelTile {
    let mut dst = PixelTile::new();
    for y in 0u32..260 {
        for x in 0u32..260 {
            for c in 0..4 {
                dst.set(x, y, c, src.at(x, y, c));
            }
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_project::document::DocumentHandle;
    use engine_project::layer::{Layer, LayerNode};
    use engine_project::types::{DocumentId, LayerId, LayerKind};
    use engine_project::{Document, FilterInstance, FilterKind, FilterParams};
    use engine_project::filters::curves::CurveChannel;
    use engine_tiles::{Scheduler, TileCache, TileCoord, TileKey, CacheStage};
    use std::sync::Mutex;
    use crate::viewport::ViewportState;

    fn make_app_state_with_layer(layer_id: u32, add_filters: bool) -> AppState {
        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let mut layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, 512, 512);
        if add_filters {
            let filter = FilterInstance::new(
                FilterKind::Curves,
                FilterParams::Curves {
                    curve: vec![(0.0, 0.0), (1.0, 1.0)],
                    channel: CurveChannel::All,
                },
            );
            layer.filters.push(filter);
        }
        doc.root.push(LayerNode::Leaf(layer));

        let doc_handle = DocumentHandle::new(doc);
        let tile_cache = TileCache::new(256 * 1024 * 1024);
        let scheduler = Scheduler::new();

        AppState {
            document_handle: doc_handle,
            tile_cache,
            scheduler,
            viewport: Mutex::new(ViewportState::default()),
        }
    }

    #[test]
    fn compute_processed_tile_no_filters_copies_raw() {
        let state = make_app_state_with_layer(1, false);

        // Insert a raw tile with known pixel values
        let mut raw = PixelTile::new();
        raw.set(10, 10, 0, 0.5); // R
        raw.set(10, 10, 1, 0.3); // G
        raw.set(10, 10, 2, 0.7); // B
        raw.set(10, 10, 3, 1.0); // A

        let raw_key = TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        // Compute processed tile
        let processed_key = TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let result = compute_processed_tile(processed_key, &state);
        assert!(result.is_ok());

        let tile = result.unwrap();
        assert!((tile.at(10, 10, 0) - 0.5).abs() < 1e-6);
        assert!((tile.at(10, 10, 1) - 0.3).abs() < 1e-6);
        assert!((tile.at(10, 10, 2) - 0.7).abs() < 1e-6);
        assert!((tile.at(10, 10, 3) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn compute_processed_tile_with_filters_applies_them() {
        let state = make_app_state_with_layer(1, true);

        // Insert a raw tile
        let raw = PixelTile::new();
        let raw_key = TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let result = compute_processed_tile(processed_key, &state);
        assert!(result.is_ok());
    }

    #[test]
    fn compute_processed_tile_missing_raw_returns_error() {
        let state = make_app_state_with_layer(1, false);

        // Don't insert any raw tile
        let processed_key = TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let result = compute_processed_tile(processed_key, &state);
        assert!(result.is_err());
    }

    #[test]
    fn compute_processed_tile_missing_layer_returns_error() {
        let state = make_app_state_with_layer(1, false);

        // Insert a raw tile for a layer that doesn't exist in the document
        let raw = PixelTile::new();
        let raw_key = TileKey {
            layer: 999, // Non-existent layer
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            layer: 999,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let result = compute_processed_tile(processed_key, &state);
        assert!(result.is_err());
    }

    #[test]
    fn compute_processed_tile_stores_in_cache() {
        let state = make_app_state_with_layer(1, false);

        let raw = PixelTile::new();
        let raw_key = TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };

        // Cache should not have the processed tile yet
        assert!(state.tile_cache.get_entry(processed_key).is_none());

        let _ = compute_processed_tile(processed_key, &state);

        // After computation, cache should have the processed tile
        assert!(state.tile_cache.get_entry(processed_key).is_some());
    }
}
