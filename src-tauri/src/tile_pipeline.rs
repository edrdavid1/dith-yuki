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
use std::sync::atomic::Ordering;

use engine_project::composite_tile;
use engine_project::error::EngineError;
use engine_project::filters::apply::apply_filter_to_tile_with_caches;
use engine_project::filter::{DitherParamsV2, FilterParams};
use engine_project::layer::LayerNode;
use engine_tiles::{CacheStage, PixelTile, Priority, RecomputeTask, TileKey};

use crate::commands::AppState;

fn snapshot_for_key(
    state: &AppState,
    key: TileKey,
) -> Result<std::sync::Arc<engine_project::Document>, EngineError> {
    state
        .session(key.doc)
        .map(|s| s.document_handle.snapshot())
        .map_err(EngineError::invalid_state)
}

/// Compute a Processed-stage tile on demand.
///
/// Steps:
/// 1. Fetch the Raw tile from TileCache (same layer + coord, Raw stage)
/// 2. Get the layer's filter stack from the document snapshot
/// 3. Apply all enabled filters via `apply_filter_to_tile`
/// 4. Store the result in TileCache at the Processed stage (single `Arc`)
/// 5. Return that same `Arc` (no extra full-tile copy)
///
/// # Errors
///
/// Returns `EngineError::InvalidState` if the Raw tile is not in the cache,
/// or `EngineError::LayerNotFound` if the layer doesn't exist in the document.
pub fn compute_processed_tile(
    key: TileKey,
    state: &AppState,
) -> Result<Arc<PixelTile>, EngineError> {
    let processed_key = TileKey {
        doc: key.doc,
        layer: key.layer,
        coord: key.coord,
        stage: CacheStage::Processed,
    };

    // 1. Build the Raw-stage key and fetch from cache
    let raw_key = TileKey {
        doc: key.doc,
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
    let snapshot = snapshot_for_key(state, key)?;
    let layer = find_layer_by_id(&snapshot.root, key.layer)
        .ok_or_else(|| EngineError::layer_not_found(
            engine_project::types::LayerId::new(key.layer),
        ))?;

    let has_error_diffusion = layer.filters.iter().any(|f| f.enabled && f.requires_full_row);
    if has_error_diffusion
        && !engine_tiles::ed_ready(
            &state.tile_cache,
            key.with_stage(CacheStage::Processed),
            true,
        )
    {
        return Err(EngineError::EdDependenciesPending);
    }

    // 3. Apply the layer's filter stack to the raw tile.
    //    If the layer has no filters, the result is a copy of the raw tile.
    let processed = if layer.filters.is_empty() {
        engine_tiles::with_tile_buffer_park(|park| {
            park.ensure(2);
            let mut owned = park.take();
            debug_assert_ne!(
                owned.data.as_ptr(),
                raw_tile.data.as_ptr(),
                "Processed copy must not alias Raw Arc"
            );
            owned.copy_from(&raw_tile);
            owned
        })
    } else {
        // Lazy-populate block representatives for mega-pixel dither only when
        // dither is the first applied filter (otherwise Raw samples skip Adjust).
        if layer.dither_is_first_applied() {
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
                        key.doc,
                        key.layer,
                        ps as u32,
                        snapshot.width,
                        snapshot.height,
                    );
                }
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

    // 4. Single Arc: insert + return the same allocation (no return-path copy).
    let arc = Arc::new(processed);
    let compute_gen = snapshot
        .generations
        .document_gen
        .load(Ordering::Acquire);
    let now_gen = snapshot_for_key(state, key)?
        .generations
        .document_gen
        .load(Ordering::Acquire);
    if now_gen == compute_gen {
        let inserted = state.tile_cache.insert_fresh_gen(
            processed_key,
            Arc::clone(&arc),
            compute_gen,
        );
        if inserted {
            state.evict_for_pressure_if_needed();
            wake_ed_frontier_after_insert(state, processed_key);
        }
        reschedule_if_insert_rejected(state, processed_key, inserted);
    }

    Ok(arc)
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
/// 4. Store the result in TileCache at the Composite stage (single `Arc`)
/// 5. Return that same `Arc` (no extra full-tile copy)
///
/// # Errors
///
/// Returns `EngineError` if compositing fails.
pub fn compute_composite_tile(
    key: TileKey,
    state: &AppState,
) -> Result<Arc<PixelTile>, EngineError> {
    // 1. Get document snapshot for the layer tree
    let snapshot = snapshot_for_key(state, key)?;

    // Display pyramid only: zoom-out tiles are a 2×2 box-filter of *full-res*
    // Composite children. Filters always run at level 0 so Bayer/ED/pixel_size
    // match export. Never apply the stack on downsampled Raw.
    if key.coord.level > 0 {
        let child_level = key.coord.level - 1;
        let children = [
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2,     y: key.coord.y * 2 },
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2 + 1, y: key.coord.y * 2 },
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2,     y: key.coord.y * 2 + 1 },
            engine_tiles::TileCoord { level: child_level, x: key.coord.x * 2 + 1, y: key.coord.y * 2 + 1 },
        ];
        let doc_gen = snapshot.generations.document_gen.load(std::sync::atomic::Ordering::Acquire);
        let mut waiting = false;
        for child_coord in &children {
            if !coord_in_document(*child_coord, snapshot.width, snapshot.height) {
                continue;
            }
            let child_key = TileKey {
                doc: key.doc,
                layer: key.layer,
                coord: *child_coord,
                stage: CacheStage::Composite,
            };
            if composite_needs_compute(state, &child_key) {
                waiting = true;
                enqueue_composite_dedup(
                    state,
                    child_key,
                    doc_gen,
                    engine_tiles::Priority::Immediate,
                );
            }
        }
        if waiting {
            return retry_pyramid_parent(state, key, doc_gen);
        }
        if let Some(pyramid_tile) = engine_tiles::generate_pyramid_tile(
            key.coord.level,
            key.coord,
            key.doc,
            key.layer,
            key.stage,
            &state.tile_cache,
        ) {
            let arc = Arc::new(pyramid_tile);
            let now_gen = snapshot_for_key(state, key)?
                .generations
                .document_gen
                .load(Ordering::Acquire);
            if now_gen == doc_gen {
                let inserted = state.tile_cache.insert_fresh_gen(
                    key,
                    Arc::clone(&arc),
                    doc_gen,
                );
                if inserted {
                    state.evict_for_pressure_if_needed();
                    enqueue_coarser_parent(state, key);
                } else {
                    reschedule_if_insert_rejected(state, key, false);
                }
            }
            return Ok(arc);
        }
        // Children looked ready then vanished (zoom/pan eviction of finer
        // levels) — retry; never drop the display parent.
        for child_coord in &children {
            if !coord_in_document(*child_coord, snapshot.width, snapshot.height) {
                continue;
            }
            enqueue_composite_dedup(
                state,
                TileKey {
                    doc: key.doc,
                    layer: key.layer,
                    coord: *child_coord,
                    stage: CacheStage::Composite,
                },
                doc_gen,
                engine_tiles::Priority::Immediate,
            );
        }
        return retry_pyramid_parent(state, key, doc_gen);
    }

    // 2. Ensure all visible layers have fresh Processed tiles at this coordinate.
    //    ED layers schedule wavefront prefix; Composite parks in EdFrontier until ready.
    ensure_processed_tiles_fresh(&snapshot.root, key.doc, key.coord, state)?;

    // 3. Composite all visible layers at this tile coordinate.
    let composited = composite_tile(&snapshot.root, key.doc, key.coord, &state.tile_cache)?;

    // 4. Single Arc: insert + return the same allocation.
    let arc = Arc::new(composited);
    let composite_key = TileKey {
        doc: key.doc,
        layer: key.layer,
        coord: key.coord,
        stage: CacheStage::Composite,
    };
    let compute_gen = snapshot
        .generations
        .document_gen
        .load(Ordering::Acquire);
    let now_gen = snapshot_for_key(state, key)?
        .generations
        .document_gen
        .load(Ordering::Acquire);
    if now_gen == compute_gen {
        let inserted = state.tile_cache.insert_fresh_gen(
            composite_key,
            Arc::clone(&arc),
            compute_gen,
        );
        if inserted {
            state.evict_for_pressure_if_needed();
            enqueue_coarser_parent(state, composite_key);
        } else {
            reschedule_if_insert_rejected(state, composite_key, false);
        }
    }

    Ok(arc)
}

/// After `insert_fresh_gen` refuses a stale write, if live doc_gen is ahead of
/// the cached entry, mark dirty and enqueue the current generation.
pub(crate) fn reschedule_if_insert_rejected(state: &AppState, key: TileKey, inserted: bool) {
    if inserted {
        return;
    }
    let Ok(snapshot) = snapshot_for_key(state, key) else {
        return;
    };
    let live = snapshot.generations.document_gen.load(Ordering::Acquire);
    if !state.tile_cache.mark_dirty_if_generation_behind(key, live) {
        return;
    }
    let layer_gen = snapshot.generations.get_layer_gen(key.layer);
    state.scheduler.enqueue_dedup(RecomputeTask {
        key,
        generation: live,
        layer_generation: layer_gen,
        priority: Priority::Immediate,
    });
    state.worker_wake.notify_one();
}

/// After an L0/L1 Composite lands, wake the coarser display parent.
///
/// Zoom-out schedules the visible L2 first; that task only *retries* itself if
/// it already ran while children were missing. If that retry is lost or ran
/// too early, the canvas keeps an old-effect tile. Waking `level+1` here
/// closes the chain: L0 → L1 → L2 → tile-ready.
fn enqueue_coarser_parent(state: &AppState, child: TileKey) {
    if child.stage != CacheStage::Composite || child.layer != 0 {
        return;
    }
    let viewport_level = state.viewport.lock().unwrap().level;
    if child.coord.level >= viewport_level {
        return;
    }
    let parent_key = TileKey {
        doc: child.doc,
        layer: 0,
        coord: engine_tiles::TileCoord {
            level: child.coord.level + 1,
            x: child.coord.x / 2,
            y: child.coord.y / 2,
        },
        stage: CacheStage::Composite,
    };
    if !composite_needs_compute(state, &parent_key) {
        return;
    }
    let Ok(snapshot) = snapshot_for_key(state, parent_key) else {
        return;
    };
    let doc_gen = snapshot
        .generations
        .document_gen
        .load(Ordering::Acquire);
    enqueue_composite_dedup(state, parent_key, doc_gen, engine_tiles::Priority::Immediate);
}

fn enqueue_composite_dedup(
    state: &AppState,
    key: TileKey,
    generation: u64,
    priority: engine_tiles::Priority,
) {
    let enqueued = state.scheduler.enqueue_dedup(engine_tiles::RecomputeTask {
        key,
        generation,
        layer_generation: 0,
        priority,
    });
    if enqueued {
        state.worker_wake.notify_one();
    }
}

fn retry_pyramid_parent(
    state: &AppState,
    key: TileKey,
    doc_gen: u64,
) -> Result<Arc<PixelTile>, EngineError> {
    enqueue_composite_dedup(
        state,
        key,
        doc_gen,
        engine_tiles::Priority::ViewportCenter,
    );
    Err(EngineError::PyramidChildrenPending)
}

/// Enqueue ED Processed causal prefix for `(doc, layer, level)` up to `(max_x, max_y)`,
/// bumping each tile to at least `priority` (Decision 2 inheritance).
pub fn schedule_ed_prefix_closure(
    state: &AppState,
    doc: u32,
    layer: u32,
    level: u8,
    max_x: u32,
    max_y: u32,
    priority: Priority,
    doc_gen: u64,
    layer_gen: u64,
) -> usize {
    let mut enqueued = 0usize;
    for coord in engine_tiles::ed_prefix_coords(level, max_x, max_y) {
        let processed = TileKey {
            doc,
            layer,
            coord,
            stage: CacheStage::Processed,
        };
        let stale = match state.tile_cache.entries.get(&processed) {
            None => true,
            Some(entry) => entry.dirty.load(Ordering::Acquire),
        };
        if !stale {
            continue;
        }
        let raw = processed.with_stage(CacheStage::Raw);
        if !state.tile_cache.entries.contains_key(&raw) {
            // Park until Raw appears — do not zero-seed.
            let task = RecomputeTask {
                key: processed,
                generation: doc_gen,
                layer_generation: layer_gen,
                priority,
            };
            state.ed_frontier.block_on(task, vec![raw]);
            continue;
        }
        let task = RecomputeTask {
            key: processed,
            generation: doc_gen,
            layer_generation: layer_gen,
            priority,
        };
        if state.scheduler.enqueue_or_bump(task) {
            enqueued += 1;
        }
    }
    engine_tiles::add_ed_prefix_tiles_enqueued(enqueued as u64);
    if enqueued > 0 {
        state.worker_wake.notify_one();
    }
    enqueued
}

/// Schedule ED prefix for every visible ED leaf toward each viewport coord.
pub fn schedule_ed_for_viewport(state: &AppState) {
    let viewport = state.viewport.lock().unwrap().clone();
    let Ok(session) = state.active_session() else {
        return;
    };
    let snapshot = session.document_handle.snapshot();
    let doc = snapshot.id.0;
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

    collect_ed_layers(&snapshot.root, &mut |layer_id| {
        let layer_gen = snapshot.generations.get_layer_gen(layer_id);
        for coord in &viewport.visible_tiles {
            schedule_ed_prefix_closure(
                state,
                doc,
                layer_id,
                coord.level,
                coord.x,
                coord.y,
                Priority::Immediate,
                doc_gen,
                layer_gen,
            );
        }
    });
}

fn collect_ed_layers(nodes: &[LayerNode], f: &mut impl FnMut(u32)) {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if layer.visible
                    && layer
                        .filters
                        .iter()
                        .any(|filt| filt.enabled && filt.requires_full_row)
                {
                    f(layer.id.0);
                }
            }
            LayerNode::Group(group) if group.visible => collect_ed_layers(&group.children, f),
            _ => {}
        }
    }
}

pub(crate) fn layer_has_error_diffusion(nodes: &[LayerNode], layer_id: u32) -> bool {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                return layer
                    .filters
                    .iter()
                    .any(|f| f.enabled && f.requires_full_row);
            }
            LayerNode::Group(group) => {
                if layer_has_error_diffusion(&group.children, layer_id) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

/// ED Processed keys at `coord` that Composite must wait on (dirty/missing).
pub fn pending_ed_processed_at_coord(
    state: &AppState,
    nodes: &[LayerNode],
    doc: u32,
    coord: engine_tiles::TileCoord,
) -> Vec<TileKey> {
    let dirty = collect_dirty_processed_keys(nodes, doc, coord, state);
    dirty
        .into_iter()
        .filter(|k| layer_has_error_diffusion(nodes, k.layer))
        .filter(|k| !engine_tiles::tile_fresh(&state.tile_cache, *k))
        .collect()
}

/// Wake EdFrontier after a successful Processed/Raw insert and re-enqueue ready tasks.
pub fn wake_ed_frontier_after_insert(state: &AppState, completed: TileKey) {
    let ready = if completed.stage == CacheStage::Processed {
        state
            .ed_frontier
            .wake_after_processed(completed, &state.tile_cache)
    } else {
        state.ed_frontier.wake(completed, &state.tile_cache)
    };
    for task in ready {
        state.scheduler.enqueue_or_bump(task);
        state.worker_wake.notify_one();
    }
}

/// Ensure all visible layers have fresh (non-dirty) Processed tiles at the given coord.
/// Non-ED layers compute inline. ED layers are scheduled via prefix closure; if not
/// yet ready, returns [`EngineError::EdPrefixPending`] (Composite parks in frontier).
fn ensure_processed_tiles_fresh(
    nodes: &[LayerNode],
    doc: u32,
    coord: engine_tiles::TileCoord,
    state: &AppState,
) -> Result<(), EngineError> {
    let dirty_keys = collect_dirty_processed_keys(nodes, doc, coord, state);

    if dirty_keys.is_empty() {
        return Ok(());
    }

    let snapshot = state
        .session(doc)
        .map(|s| s.document_handle.snapshot())
        .map_err(EngineError::invalid_state)?;
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

    let mut ed_pending = false;
    let mut inline_keys = Vec::new();

    for key in dirty_keys {
        let has_ed = layer_has_error_diffusion(&snapshot.root, key.layer);
        if has_ed {
            let layer_gen = snapshot.generations.get_layer_gen(key.layer);
            schedule_ed_prefix_closure(
                state,
                doc,
                key.layer,
                coord.level,
                coord.x,
                coord.y,
                Priority::Immediate,
                doc_gen,
                layer_gen,
            );
            if !engine_tiles::ed_ready(&state.tile_cache, key, true) {
                ed_pending = true;
                continue;
            }
            if engine_tiles::tile_fresh(&state.tile_cache, key) {
                continue;
            }
            // Deps ready, still dirty — compute now (same as non-ED inline).
            inline_keys.push(key);
        } else {
            inline_keys.push(key);
        }
    }

    if inline_keys.len() == 1 {
        let _ = compute_processed_tile(inline_keys[0], state);
    } else if inline_keys.len() > 1 {
        rayon::scope(|s| {
            for key in &inline_keys {
                s.spawn(|_| {
                    let _ = compute_processed_tile(*key, state);
                });
            }
        });
    }

    if ed_pending {
        return Err(EngineError::EdPrefixPending);
    }

    Ok(())
}

/// Collect TileKeys for all visible leaf layers needing Processed tile recomputation.
fn collect_dirty_processed_keys(
    nodes: &[LayerNode],
    doc: u32,
    coord: engine_tiles::TileCoord,
    state: &AppState,
) -> Vec<TileKey> {
    let mut keys = Vec::new();
    collect_dirty_recursive(nodes, doc, coord, state, &mut keys);
    keys
}

fn collect_dirty_recursive(
    nodes: &[LayerNode],
    doc: u32,
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
                    doc,
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
                collect_dirty_recursive(&group.children, doc, coord, state, keys);
            }
        }
    }
}

fn composite_needs_compute(state: &AppState, key: &TileKey) -> bool {
    use std::sync::atomic::Ordering;
    match state.tile_cache.entries.get(key) {
        None => true,
        Some(entry) => entry.dirty.load(Ordering::Acquire),
    }
}

fn coord_in_document(coord: engine_tiles::TileCoord, doc_w: u32, doc_h: u32) -> bool {
    let (cols, rows) = engine_tiles::tile_grid_at_level(doc_w, doc_h, coord.level);
    coord.x < cols && coord.y < rows
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
    dst.copy_from(src);
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

        let state = AppState::empty_process(None, 512 * 1024 * 1024, true);
        state.spawn_session(doc);
        state
    }

    const ED_DOC: u32 = 1024;
    const ED_LAYER: u32 = 1;

    /// 4x4 L0 grid with an enabled Floyd-Steinberg layer and every Raw tile filled.
    fn make_ed_state() -> AppState {
        use engine_project::filter::{DitherModeV2, DitherParamsV2};

        let mut doc = Document::new(DocumentId::new(1), ED_DOC, ED_DOC);
        let mut layer = Layer::new(LayerId::new(ED_LAYER), LayerKind::Raster, ED_DOC, ED_DOC);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 4,
                pixel_size: 1,
                palette_id: None,
                ..DitherParamsV2::default()
            }),
        ));
        doc.root.push(LayerNode::Leaf(layer));

        let state = make_app_state_with_layer(ED_LAYER, false);
        state.must_active().document_handle.mutate(|d| *d = doc.clone());

        let full = engine_tiles::TILE_SIZE + 2 * engine_tiles::HALO;
        for cy in 0..4u32 {
            for cx in 0..4u32 {
                let coord = TileCoord { level: 0, x: cx, y: cy };
                let mut tile = PixelTile::new();
                for y in 0..full {
                    for x in 0..full {
                        let gx = cx as i32 * engine_tiles::TILE_SIZE as i32 + x as i32
                            - engine_tiles::HALO as i32;
                        let gy = cy as i32 * engine_tiles::TILE_SIZE as i32 + y as i32
                            - engine_tiles::HALO as i32;
                        tile.set(x, y, 0, gx.max(0) as f32 / ED_DOC as f32);
                        tile.set(x, y, 1, gy.max(0) as f32 / ED_DOC as f32);
                        tile.set(x, y, 2, 0.5);
                        tile.set(x, y, 3, 1.0);
                    }
                }
                state.tile_cache.insert_fresh(
                    TileKey { doc: 1, layer: ED_LAYER, coord, stage: CacheStage::Raw },
                    Arc::new(tile),
                );
            }
        }
        state
    }

    fn ed_key(x: u32, y: u32) -> TileKey {
        TileKey {
            doc: 1,
            layer: ED_LAYER,
            coord: TileCoord { level: 0, x, y },
            stage: CacheStage::Processed,
        }
    }

    fn reset_ed_processed(state: &AppState) {
        state.error_residuals.clear();
        engine_tiles::invalidation::invalidate(
            &state.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: ED_LAYER },
        );
    }

    /// Row-major is a valid topological order for the left/top/diagonal
    /// dependency, so a strictly sequential pass is the exactness reference.
    fn ed_reference_tile(state: &AppState, target: TileKey) -> PixelTile {
        for y in 0..=target.coord.y {
            for x in 0..=target.coord.x {
                compute_processed_tile(ed_key(x, y), state).expect("reference tile");
            }
        }
        copy_tile(&state.tile_cache.get_entry(target).expect("reference cached"))
    }

    /// Drain scheduled ED Processed tasks in a single thread (topo via ready-gate).
    fn drain_ed_scheduler(state: &AppState, max_steps: usize) {
        for _ in 0..max_steps {
            let Some(task) = state.scheduler.dequeue() else {
                if state.ed_frontier.blocked_count() == 0 {
                    break;
                }
                // Should not happen if Raw present; break to avoid hang.
                break;
            };
            if task.key.stage != CacheStage::Processed {
                continue;
            }
            if !engine_tiles::ed_ready(&state.tile_cache, task.key, true) {
                state.ed_frontier.block(task, &state.tile_cache);
                continue;
            }
            let _ = compute_processed_tile(task.key, state);
            wake_ed_frontier_after_insert(state, task.key);
        }
    }

    #[test]
    fn ed_wavefront_matches_row_major_reference() {
        let target = ed_key(3, 3);

        let reference = {
            let state = make_ed_state();
            ed_reference_tile(&state, target)
        };

        let state = make_ed_state();
        reset_ed_processed(&state);
        engine_tiles::reset_ed_prefix_tiles_enqueued();
        let n = schedule_ed_prefix_closure(
            &state,
            1,
            ED_LAYER,
            0,
            3,
            3,
            Priority::Immediate,
            0,
            0,
        );
        assert!(n >= 16, "expected full 4x4 prefix enqueue, got {n}");
        assert!(engine_tiles::ed_prefix_tiles_enqueued() >= 16);
        drain_ed_scheduler(&state, 256);

        let via_wave = copy_tile(
            &state
                .tile_cache
                .get_entry(target)
                .expect("wavefront target"),
        );
        assert_eq!(
            via_wave.data, reference.data,
            "wavefront schedule must be byte-identical to sequential row-major"
        );
    }

    #[test]
    fn ed_priority_inheritance_bumps_prefix_to_immediate() {
        let state = make_ed_state();
        reset_ed_processed(&state);
        // Seed prefix as Prefetch first.
        schedule_ed_prefix_closure(
            &state,
            1,
            ED_LAYER,
            0,
            3,
            3,
            Priority::Prefetch,
            0,
            0,
        );
        assert_eq!(
            state.scheduler.queued_priority_of(&ed_key(0, 0)),
            Some(Priority::Prefetch)
        );
        // Visible corner requests Immediate → inheritance bump.
        schedule_ed_prefix_closure(
            &state,
            1,
            ED_LAYER,
            0,
            3,
            3,
            Priority::Immediate,
            0,
            0,
        );
        assert_eq!(
            state.scheduler.queued_priority_of(&ed_key(0, 0)),
            Some(Priority::Immediate)
        );
        let first = state.scheduler.dequeue().unwrap();
        assert_eq!(first.priority, Priority::Immediate);
    }

    // Prefill helper removed with legacy path — wavefront coverage is
    // `ed_wavefront_matches_row_major_reference` + `ed_priority_inheritance_*`.

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
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        // Compute processed tile
        let processed_key = TileKey {
            doc: 1,
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
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
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
            doc: 1,
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
            doc: 1,
            layer: 999, // Non-existent layer
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
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
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
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

    #[test]
    fn compute_processed_returns_same_arc_as_cache() {
        let state = make_app_state_with_layer(1, false);

        let mut raw = PixelTile::new();
        raw.set(10, 10, 0, 0.42);
        let raw_key = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let returned = compute_processed_tile(processed_key, &state).expect("compute");
        let cached = state
            .tile_cache
            .get_entry(processed_key)
            .expect("cached processed");
        assert!(
            Arc::ptr_eq(&returned, &cached),
            "return path must not allocate a second PixelTile copy"
        );
        assert!((returned.at(10, 10, 0) - 0.42).abs() < 1e-6);
    }

    fn make_ed_layer_state() -> AppState {
        use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};

        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 512, 512);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: None,
                ..Default::default()
            }),
        ));
        doc.root.push(LayerNode::Leaf(layer));
        let mut state = make_app_state_with_layer(1, false);
        state.must_active().document_handle.store(std::sync::Arc::new(doc));
        state
    }

    #[test]
    fn skip_branch_does_not_publish_when_neighbor_raw_missing() {
        let state = make_ed_layer_state();
        let current_raw = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 1, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(current_raw, Arc::new(PixelTile::new()));

        let processed = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 1, y: 0 },
            stage: CacheStage::Processed,
        };
        let result = compute_processed_tile(processed, &state);
        assert!(
            matches!(result, Err(EngineError::EdDependenciesPending)),
            "must not publish zero-seed Processed when left Raw missing"
        );
        assert!(
            state.tile_cache.get_entry(processed).is_none()
                || state
                    .tile_cache
                    .entries
                    .get(&processed)
                    .map(|e| e.dirty.load(Ordering::Acquire))
                    .unwrap_or(true),
            "Processed must not be fresh after blocked ED"
        );
    }

    #[test]
    fn schedule_registers_frontier_when_prefix_raw_missing() {
        let state = make_ed_layer_state();
        let current_raw = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 1, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(current_raw, Arc::new(PixelTile::new()));
        // left Raw absent
        engine_tiles::reset_ed_blocked_total();
        schedule_ed_prefix_closure(
            &state,
            1,
            1,
            0,
            1,
            0,
            Priority::Immediate,
            0,
            0,
        );
        assert!(
            state.ed_frontier.blocked_count() > 0 || engine_tiles::ed_blocked_total() > 0,
            "missing left raw on schedule must park in EdFrontier"
        );
    }

    #[test]
    fn evict_layer_of_current_layer_errors_before_ed_ready() {
        let state = make_ed_layer_state();
        let raw = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 1, y: 0 },
            stage: CacheStage::Raw,
        };
        state.tile_cache.get_or_insert(raw, Arc::new(PixelTile::new()));
        state.tile_cache.evict_layer(1, 1);

        let processed = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x: 1, y: 0 },
            stage: CacheStage::Processed,
        };
        let result = compute_processed_tile(processed, &state);
        assert!(result.is_err(), "current raw gone after evict_layer");
    }

    #[test]
    fn composite_at_level_1_downsamples_full_res_children() {
        use engine_tiles::decompose::decompose_image_to_tiles;

        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 512, 512);
        doc.root.push(LayerNode::Leaf(layer));
        let mut state = make_app_state_with_layer(1, false);
        state.must_active().document_handle.store(std::sync::Arc::new(doc));

        let mut buffer = vec![0.0f32; (512 * 512 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[0] = 0.6;
            px[1] = 0.6;
            px[2] = 0.6;
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tile_cache).unwrap();

        let l1_key = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord { level: 1, x: 0, y: 0 },
            stage: CacheStage::Composite,
        };
        assert!(
            compute_composite_tile(l1_key, &state).is_err(),
            "L1 must wait for full-res children, not filter downsampled raw"
        );

        let mut l0_computed = 0usize;
        while let Some(task) = state.scheduler.dequeue() {
            if task.key.coord.level == 0 && task.key.stage == CacheStage::Composite {
                compute_composite_tile(task.key, &state).unwrap();
                l0_computed += 1;
            }
        }
        assert_eq!(l0_computed, 4, "512×512 → four L0 children");

        let tile = compute_composite_tile(l1_key, &state).unwrap();
        assert!((tile.at(engine_tiles::HALO, engine_tiles::HALO, 0) - 0.6).abs() < 1e-4);
    }

    #[test]
    fn composite_wakes_coarser_parent_when_viewport_is_zoomed_out() {
        use engine_tiles::decompose::decompose_image_to_tiles;

        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 512, 512);
        doc.root.push(LayerNode::Leaf(layer));
        let mut state = make_app_state_with_layer(1, false);
        state.must_active().document_handle.store(std::sync::Arc::new(doc));
        state.viewport.lock().unwrap().level = 1;

        let mut buffer = vec![0.0f32; (512 * 512 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[0] = 0.6;
            px[1] = 0.6;
            px[2] = 0.6;
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tile_cache).unwrap();

        let l0_key = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Composite,
        };
        compute_composite_tile(l0_key, &state).unwrap();

        let mut found_parent = false;
        while let Some(task) = state.scheduler.dequeue() {
            if task.key.stage == CacheStage::Composite
                && task.key.coord.level == 1
                && task.key.coord.x == 0
                && task.key.coord.y == 0
            {
                found_parent = true;
                break;
            }
        }
        assert!(found_parent, "L0 Composite must enqueue its L1 display parent");
    }

    #[test]
    fn composite_layer0_cache_keeps_latest_generation() {
        use engine_tiles::decompose::decompose_image_to_tiles;
        use std::sync::atomic::Ordering;

        let state = make_app_state_with_layer(1, false);
        let mut buffer = vec![0.0f32; (512 * 512 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[0] = 0.4;
            px[1] = 0.4;
            px[2] = 0.4;
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tile_cache).unwrap();

        let key = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Composite,
        };

        state.must_active().document_handle.mutate(|doc| {
            doc.increment_generation();
        });
        let gen1 = state.must_active().document_handle
            .snapshot()
            .generations
            .document_gen
            .load(Ordering::Acquire);
        compute_composite_tile(key, &state).unwrap();
        assert_eq!(state.tile_cache.entries.get(&key).unwrap().generation, gen1);

        state.must_active().document_handle.mutate(|doc| {
            doc.increment_generation();
        });
        let gen2 = state.must_active().document_handle
            .snapshot()
            .generations
            .document_gen
            .load(Ordering::Acquire);
        compute_composite_tile(key, &state).unwrap();
        assert_eq!(state.tile_cache.entries.get(&key).unwrap().generation, gen2);

        let mut stale = PixelTile::new();
        stale.set(0, 0, 0, 0.99);
        assert!(
            !state
                .tile_cache
                .insert_fresh_gen(key, Arc::new(stale), gen1),
            "older generation must not overwrite the latest Composite"
        );
        let entry = state.tile_cache.entries.get(&key).unwrap();
        assert_eq!(entry.generation, gen2);
        assert!((entry.tile.at(0, 0, 0) - 0.99).abs() > 0.5);
        drop(entry);

        state.must_active().document_handle.mutate(|doc| {
            doc.increment_generation();
        });
        let gen3 = state.must_active().document_handle
            .snapshot()
            .generations
            .document_gen
            .load(Ordering::Acquire);
        reschedule_if_insert_rejected(&state, key, false);
        assert!(state.tile_cache.entries.get(&key).unwrap().dirty.load(Ordering::Acquire));
        assert!(state.scheduler.contains_key(&key));
        assert!(gen3 > gen2);
    }

    #[test]
    fn enqueue_coarser_parent_dedups_same_generation() {
        use engine_tiles::decompose::decompose_image_to_tiles;

        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 512, 512);
        doc.root.push(LayerNode::Leaf(layer));
        let mut state = make_app_state_with_layer(1, false);
        state.must_active().document_handle.store(std::sync::Arc::new(doc));
        state.viewport.lock().unwrap().level = 1;

        let mut buffer = vec![0.0f32; (512 * 512 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tile_cache).unwrap();

        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let l0 = TileKey {
                doc: 1,
                layer: 0,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Composite,
            };
            compute_composite_tile(l0, &state).unwrap();
        }

        let parent = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord { level: 1, x: 0, y: 0 },
            stage: CacheStage::Composite,
        };
        let mut parent_tasks = 0u32;
        while let Some(task) = state.scheduler.dequeue() {
            if task.key == parent {
                parent_tasks += 1;
            }
        }
        assert_eq!(
            parent_tasks, 1,
            "four L0 children must enqueue the L1 parent once"
        );
    }

    #[test]
    fn pyramid_parent_retries_quietly_when_children_vanish() {
        use engine_tiles::decompose::decompose_image_to_tiles;

        let mut doc = Document::new(DocumentId::new(1), 1024, 1024);
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 1024, 1024);
        doc.root.push(LayerNode::Leaf(layer));
        let mut state = make_app_state_with_layer(1, false);
        state.must_active().document_handle.store(std::sync::Arc::new(doc));
        state.viewport.lock().unwrap().level = 2;

        let mut buffer = vec![0.0f32; (1024 * 1024 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 1024, 1024, 1, 1, &state.tile_cache).unwrap();

        let parent = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord { level: 2, x: 0, y: 0 },
            stage: CacheStage::Composite,
        };
        let err = match compute_composite_tile(parent, &state) {
            Err(e) => e,
            Ok(_) => panic!("L2 must wait for children"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("not yet computed"),
            "expected retry, got {msg}"
        );
        assert!(
            !msg.contains("missing after wait"),
            "parent must stay scheduled instead of failing closed"
        );
        assert!(state.scheduler.contains_key(&parent));
    }
}
