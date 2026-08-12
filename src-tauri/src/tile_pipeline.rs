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
use engine_project::filters::apply::apply_filter_to_tile_with_caches;
use engine_project::filter::{DitherParamsV2, FilterParams};
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
    compute_processed_tile_inner(key, state, false)
}

/// Inner implementation with `is_dependency` flag.
/// When called for a dependency (neighbor tile needed for error diffusion),
/// we skip recomputation if the tile is already cached.
fn compute_processed_tile_inner(
    key: TileKey,
    state: &AppState,
    is_dependency: bool,
) -> Result<PixelTile, EngineError> {
    let processed_key = TileKey {
        layer: key.layer,
        coord: key.coord,
        stage: CacheStage::Processed,
    };

    // For dependency calls only: if already cached and NOT dirty, return without recomputing.
    // For primary calls (from worker/scheduler): always recompute to pick up changes.
    if is_dependency {
        let is_fresh = match state.tile_cache.entries.get(&processed_key) {
            None => false,
            Some(entry) => !entry.dirty.load(std::sync::atomic::Ordering::Acquire),
        };
        if is_fresh {
            if let Some(cached) = state.tile_cache.get_entry(processed_key) {
                return Ok(copy_tile(&cached));
            }
        }
    }

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

    // 2b. If the layer has error diffusion filters (requires_full_row),
    //     ensure neighbor tiles (left, top, and diagonal) are processed first so
    //     that cross-tile error residuals (incl. IncomingErrorBuffer corner) are
    //     available. Runs on all pyramid levels. On-demand row-major without a
    //     global sequential pass.
    let has_error_diffusion = layer.filters.iter().any(|f| f.enabled && f.requires_full_row);
    if has_error_diffusion {
        let processed_key_for_waiters = TileKey {
            layer: key.layer,
            coord: key.coord,
            stage: CacheStage::Processed,
        };

        // Ensure left neighbor is processed first
        if key.coord.x > 0 {
            let left_key = TileKey {
                layer: key.layer,
                coord: engine_tiles::TileCoord {
                    level: key.coord.level,
                    x: key.coord.x - 1,
                    y: key.coord.y,
                },
                stage: CacheStage::Processed,
            };
            let left_needs_compute = match state.tile_cache.entries.get(&left_key) {
                None => true,
                Some(entry) => entry.dirty.load(std::sync::atomic::Ordering::Acquire),
            };
            if left_needs_compute {
                let left_raw_key = TileKey {
                    layer: key.layer,
                    coord: left_key.coord,
                    stage: CacheStage::Raw,
                };
                if state.tile_cache.get_entry(left_raw_key).is_some() {
                    let _ = compute_processed_tile_inner(left_key, state, true);
                } else {
                    // Silent-skip zero-seed: diagnose + register waiter contract
                    state.diffusion_skip_counter.increment();
                    log::debug!(
                        target: "diffusion_skip",
                        "left neighbor raw missing {:?} — waiter {:?} zero-seed this pass",
                        left_raw_key,
                        processed_key_for_waiters
                    );
                    state
                        .pending_diffusion_waiters
                        .register(left_raw_key, processed_key_for_waiters);
                }
            }
        }
        // Ensure top neighbor is processed first
        if key.coord.y > 0 {
            let top_key = TileKey {
                layer: key.layer,
                coord: engine_tiles::TileCoord {
                    level: key.coord.level,
                    x: key.coord.x,
                    y: key.coord.y - 1,
                },
                stage: CacheStage::Processed,
            };
            let top_needs_compute = match state.tile_cache.entries.get(&top_key) {
                None => true,
                Some(entry) => entry.dirty.load(std::sync::atomic::Ordering::Acquire),
            };
            if top_needs_compute {
                let top_raw_key = TileKey {
                    layer: key.layer,
                    coord: top_key.coord,
                    stage: CacheStage::Raw,
                };
                if state.tile_cache.get_entry(top_raw_key).is_some() {
                    let _ = compute_processed_tile_inner(top_key, state, true);
                } else {
                    state.diffusion_skip_counter.increment();
                    log::debug!(
                        target: "diffusion_skip",
                        "top neighbor raw missing {:?} — waiter {:?} zero-seed this pass",
                        top_raw_key,
                        processed_key_for_waiters
                    );
                    state
                        .pending_diffusion_waiters
                        .register(top_raw_key, processed_key_for_waiters);
                }
            }
        }
        // Ensure diagonal neighbor (x-1, y-1) for IncomingErrorBuffer corner seed
        if key.coord.x > 0 && key.coord.y > 0 {
            let diag_key = TileKey {
                layer: key.layer,
                coord: engine_tiles::TileCoord {
                    level: key.coord.level,
                    x: key.coord.x - 1,
                    y: key.coord.y - 1,
                },
                stage: CacheStage::Processed,
            };
            let diag_needs_compute = match state.tile_cache.entries.get(&diag_key) {
                None => true,
                Some(entry) => entry.dirty.load(std::sync::atomic::Ordering::Acquire),
            };
            if diag_needs_compute {
                let diag_raw_key = TileKey {
                    layer: key.layer,
                    coord: diag_key.coord,
                    stage: CacheStage::Raw,
                };
                if state.tile_cache.get_entry(diag_raw_key).is_some() {
                    let _ = compute_processed_tile_inner(diag_key, state, true);
                } else {
                    state.diffusion_skip_counter.increment();
                    state
                        .pending_diffusion_waiters
                        .register(diag_raw_key, processed_key_for_waiters);
                }
            }
        }
    }

    // 3. Apply the layer's filter stack to the raw tile.
    //    If the layer has no filters, the result is a copy of the raw tile.
    let processed = if layer.filters.is_empty() {
        // No filters — copy raw tile data directly
        copy_tile(&raw_tile)
    } else {
        // Lazy-populate block representatives for any dither pixel_size > 1.
        for filter in &layer.filters {
            if !filter.enabled {
                continue;
            }
            let ps = match &filter.params {
                FilterParams::DitherV2(DitherParamsV2 { pixel_size, .. }) => *pixel_size,
                FilterParams::Dither { .. } => 1,
                _ => continue,
            };
            if ps > 1 {
                state.block_representatives.ensure_populated_from_tiles(
                    &state.tile_cache,
                    key.layer,
                    ps as u32,
                    snapshot.width,
                    snapshot.height,
                );
            }
        }

        apply_filter_to_tile_with_caches(
            &raw_tile,
            layer,
            key.coord,
            &state.palette_cache,
            &state.palette_lut_cache,
            &state.threshold_cache,
            &snapshot,
            &state.error_residuals,
            &state.block_representatives,
            state.gpu.as_deref(),
        )?
    };

    // 4. Store result in cache at Processed stage.
    //    We copy the tile for the return value and wrap the original in Arc for storage.
    //    insert_fresh always overwrites any existing (possibly dirty) entry.
    let return_tile = copy_tile(&processed);
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
    // If requesting a pyramid level > 0, try to generate from cached children
    if key.coord.level > 0 {
        if let Some(pyramid_tile) = engine_tiles::generate_pyramid_tile(
            key.coord.level,
            key.coord,
            key.layer,
            key.stage,
            &state.tile_cache,
        ) {
            // Store in cache and return
            let return_tile = copy_tile(&pyramid_tile);
            state.tile_cache.insert_fresh(key, Arc::new(pyramid_tile));
            return Ok(return_tile);
        }
        // Children not in cache yet — schedule level 0 composite computation for
        // all children, then return error so this task will be retried via tile-ready.
        let child_level = key.coord.level - 1;
        let children = [
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2,     y: key.coord.y * 2 },
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2 + 1, y: key.coord.y * 2 },
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2,     y: key.coord.y * 2 + 1 },
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2 + 1, y: key.coord.y * 2 + 1 },
        ];
        let snapshot = state.document_handle.snapshot();
        let doc_gen = snapshot.generations.document_gen.load(std::sync::atomic::Ordering::Acquire);
        for child_coord in &children {
            let child_key = TileKey {
                layer: key.layer,
                coord: *child_coord,
                stage: CacheStage::Composite,
            };
            // Only schedule if not already cached
            if state.tile_cache.entries.get(&child_key).is_none() {
                let task = engine_tiles::RecomputeTask {
                    key: child_key,
                    generation: doc_gen,
                    layer_generation: 0,
                    priority: engine_tiles::Priority::Immediate,
                };
                state.scheduler.enqueue(task);
                state.worker_wake.notify_one();
            }
        }
        // Also re-enqueue this same level > 0 tile with lower priority so it runs after children
        let retry_task = engine_tiles::RecomputeTask {
            key,
            generation: doc_gen,
            layer_generation: 0,
            priority: engine_tiles::Priority::ViewportCenter, // Lower than Immediate children
        };
        state.scheduler.enqueue(retry_task);
        state.worker_wake.notify_one();

        return Err(EngineError::invalid_state(
            "Pyramid children not yet computed; scheduled for computation".to_string()
        ));
    }

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
/// Uses rayon for parallel computation when multiple layers need work.
fn ensure_processed_tiles_fresh(
    nodes: &[LayerNode],
    coord: engine_tiles::TileCoord,
    state: &AppState,
) -> Result<(), EngineError> {
    // Collect all layers needing fresh Processed tiles
    let dirty_keys = collect_dirty_processed_keys(nodes, coord, state);

    if dirty_keys.is_empty() {
        return Ok(());
    }

    if dirty_keys.len() == 1 {
        // Single layer: compute inline (no rayon overhead)
        let _ = compute_processed_tile(dirty_keys[0], state);
        return Ok(());
    }

    // Parallel computation for multiple layers
    rayon::scope(|s| {
        for key in &dirty_keys {
            s.spawn(|_| {
                let _ = compute_processed_tile(*key, state);
            });
        }
    });

    Ok(())
}

/// Collect TileKeys for all visible leaf layers needing Processed tile recomputation.
fn collect_dirty_processed_keys(
    nodes: &[LayerNode],
    coord: engine_tiles::TileCoord,
    state: &AppState,
) -> Vec<TileKey> {
    let mut keys = Vec::new();
    collect_dirty_recursive(nodes, coord, state, &mut keys);
    keys
}

fn collect_dirty_recursive(
    nodes: &[LayerNode],
    coord: engine_tiles::TileCoord,
    state: &AppState,
    keys: &mut Vec<TileKey>,
) {
    use std::sync::atomic::Ordering;

    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if !layer.visible {
                    continue;
                }
                let key = TileKey {
                    layer: layer.id.0,
                    coord,
                    stage: CacheStage::Processed,
                };
                let needs_compute = match state.tile_cache.entries.get(&key) {
                    None => true,
                    Some(entry) => entry.dirty.load(Ordering::Acquire),
                };
                if needs_compute {
                    keys.push(key);
                }
            }
            LayerNode::Group(group) => {
                if !group.visible {
                    continue;
                }
                collect_dirty_recursive(&group.children, coord, state, keys);
            }
        }
    }
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
    dst.data.copy_from_slice(&src.data);
    dst
}

/// Reference implementation of `copy_tile` preserved for property-based testing.
/// This is an exact copy of the current triple-nested loop `copy_tile` implementation.
/// Used to verify that optimized versions (bulk `copy_from_slice`) produce bitwise-identical output.
#[cfg(test)]
pub fn reference_copy_tile(src: &PixelTile) -> PixelTile {
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
    use crate::worker::WorkerWake;

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
            worker_wake: WorkerWake::new(),
            palette_cache: engine_color::palette_cache::PaletteKdCache::new(),
            palette_lut_cache: engine_color::palette_lut::PaletteLutCache::new(),
            threshold_cache: engine_color::threshold_map::ThresholdMapCache::new(),
            error_residuals: engine_project::filters::ErrorResidualsStore::new(),
            block_representatives: engine_tiles::BlockRepresentativeCache::new(),
            diffusion_skip_counter: crate::diffusion_waiters::DiffusionSkipCounter::new(),
            pending_diffusion_waiters: crate::diffusion_waiters::PendingDiffusionWaiters::new(),
            gpu: None,
            panel_manager: Mutex::new(crate::panel_manager::PanelManager::new()),
            selection: Mutex::new(crate::commands::SelectionState::default()),
            dock_affinity: Mutex::new(crate::dock_affinity::DockAffinityController::new(true)),
            float_drag_mouseup_cancel: std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(true),
            ),
            float_drag_mouseup_hook: Mutex::new(None),
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
