//! On-demand tile pipeline for computing Processed-stage and Composite-stage tiles.
//!
//! This module provides:
//! - `compute_processed_tile`: derives a Processed tile from a Raw tile by applying
//!   the layer's filter stack.
//! - `compute_composite_tile`: blends all visible layers at a tile coordinate to
//!   produce a Composite tile.
//!
//! Both are called by the worker pool when a tile is needed (cache miss or dirty).

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(test)]
use std::thread::JoinHandle;
use std::time::Instant;

use engine_project::composite_tile;
use engine_project::error::EngineError;
use engine_project::filter::{DitherParamsV2, FilterParams};
use engine_project::filters::apply::apply_filter_to_tile_with_caches;
use engine_project::layer::LayerNode;
use engine_tiles::{CacheStage, PixelTile, Priority, RecomputeTask, TileKey};

use crate::commands::AppState;

/// Once-per-`(doc, layer, gen)` publish claim set (Option A).
///
/// Mass / SliceOnly experiment branches were removed after the 30-run matrix;
/// see ADR `docs/adr/gpu-dither-families.md` and `.local-doc/publish_ab_experiment.md`.
fn fd_published_set() -> &'static Mutex<HashSet<(u32, u32, u64)>> {
    static FD_PUBLISHED: OnceLock<Mutex<HashSet<(u32, u32, u64)>>> = OnceLock::new();
    FD_PUBLISHED.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Returns true iff this caller should perform the mass publish (first claim wins).
fn claim_fd_publish(doc: u32, layer: u32, gen: u64) -> bool {
    let Ok(mut set) = fd_published_set().lock() else {
        return true;
    };
    set.insert((doc, layer, gen))
}

/// Drop publish-once claims (tests / new document_gen invalidation).
#[cfg(test)]
pub fn reset_fd_publish_claims() {
    if let Ok(mut set) = fd_published_set().lock() {
        set.clear();
    }
}

#[cfg(test)]
fn fd_publish_joins() -> &'static Mutex<Vec<JoinHandle<()>>> {
    static JOINS: OnceLock<Mutex<Vec<JoinHandle<()>>>> = OnceLock::new();
    JOINS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Wait for background full-document publishes (harness / T-BYTES).
#[cfg(test)]
pub fn join_fd_publishes() {
    let handles = fd_publish_joins()
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default();
    for h in handles {
        let _ = h.join();
    }
}

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

    let raw_tile = state.tiles.tile_cache.get_entry(raw_key).ok_or_else(|| {
        EngineError::invalid_state(format!(
            "Raw tile not found in cache for layer={}, coord=({},{}) level={}",
            key.layer, key.coord.x, key.coord.y, key.coord.level
        ))
    })?;

    // 2. Get document snapshot and find the layer
    let snapshot = snapshot_for_key(state, key)?;
    let layer = find_layer_by_id(&snapshot.root, key.layer).ok_or_else(|| {
        EngineError::layer_not_found(engine_project::types::LayerId::new(key.layer))
    })?;

    let has_error_diffusion = layer
        .filters
        .iter()
        .any(|f| f.enabled && f.requires_full_row);
    if has_error_diffusion
        && !engine_tiles::ed_ready(
            &state.tiles.tile_cache,
            key.with_stage(CacheStage::Processed),
            true,
        )
    {
        return Err(EngineError::EdDependenciesPending);
    }

    // Full-document algorithms (Riemersma / ASCII): monolithic pass, then slice tiles.
    // Image preview mode omits Ascii so only non-ASCII full-doc filters force this path;
    // Ascii-only stacks fall through to the tiled apply (Ascii tile apply is identity).
    let include_ascii = state.ascii_preview.load(Ordering::Relaxed);
    if engine_project::filters::layer_has_full_document_filter_ex(layer, include_ascii) {
        let compute_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
        let diag = crate::preview_diag::level();
        crate::commands::begin_full_document_busy(state, key.doc);
        let mut trace = engine_project::filters::EnsureTrace::default();
        let result = if diag == 0 {
            engine_project::filters::ensure_full_document_ex(
                &state.tiles.full_document,
                &state.tiles.tile_cache,
                layer,
                &snapshot,
                compute_gen,
                include_ascii,
            )
        } else {
            crate::preview_diag::record_ensure(key.doc, compute_gen);
            engine_project::filters::ensure_full_document_ex_timed(
                &state.tiles.full_document,
                &state.tiles.tile_cache,
                layer,
                &snapshot,
                compute_gen,
                include_ascii,
                &mut trace,
            )
        };
        crate::commands::end_full_document_busy(state, key.doc);
        let result = result?;
        if diag != 0 {
            crate::preview_diag::record_ensure_class(
                key.doc,
                compute_gen,
                trace.hit,
                trace.waited,
                trace.rendered,
            );
            crate::preview_diag::add_task_phase(
                trace.wait,
                trace.render,
                std::time::Duration::ZERO,
            );
            if trace.rendered {
                if let Some(s) = engine_project::algorithms::gpu_dither::last_gpu_dither_status() {
                    crate::preview_diag::record_gpu_apply(key.doc, compute_gen, &s);
                }
            }
        }
        crate::commands::emit_gpu_dither_status(state, key.doc);
        let now_gen = snapshot_for_key(state, key)?
            .generations
            .document_gen
            .load(Ordering::Acquire);
        if now_gen != compute_gen {
            return Err(EngineError::invalid_state(
                "full-document result stale after compute",
            ));
        }
        // Option A: `store_ready` already ran inside ensure (waiters proceed).
        // First returner claims once-per-gen mass publish and runs it on a
        // background thread so slice/return is not blocked; harness joins via
        // [`join_fd_publishes`].
        if claim_fd_publish(key.doc, key.layer, compute_gen) {
            let cache = Arc::clone(&state.tiles.tile_cache);
            let published = result.clone();
            let doc = key.doc;
            let layer = key.layer;
            let gen = compute_gen;
            let handle = std::thread::Builder::new()
                .name("fd-publish".into())
                .spawn(move || {
                    let t_pub = Instant::now();
                    engine_project::filters::publish_processed_tiles(
                        &cache, &published, doc, layer, gen,
                    );
                    let pub_dt = t_pub.elapsed();
                    if crate::preview_diag::level() != 0 {
                        crate::preview_diag::record_publish(doc, gen, pub_dt);
                    }
                })
                .expect("spawn fd-publish");
            #[cfg(test)]
            if let Ok(mut joins) = fd_publish_joins().lock() {
                joins.push(handle);
            }
            #[cfg(not(test))]
            drop(handle); // detach — production never joins
            let _ = diag;
        }
        let tile = engine_project::filters::slice_processed_tile(&result, key.coord);
        let arc = Arc::new(tile);
        let was_missing = state.tiles.tile_cache.get_entry(processed_key).is_none();
        if was_missing && diag != 0 {
            crate::preview_diag::record_tile_fallback_insert(key.doc, compute_gen);
        }
        let inserted =
            state
                .tiles
                .tile_cache
                .insert_fresh_gen(processed_key, Arc::clone(&arc), compute_gen);
        if inserted {
            state.evict_for_pressure_if_needed();
            engine_project::dither_profile::mark("tiles_published");
        }
        reschedule_if_insert_rejected(state, processed_key, inserted);
        return Ok(arc);
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
                    state
                        .tiles
                        .block_representatives
                        .ensure_populated_from_tiles(
                            &state.tiles.tile_cache,
                            key.doc,
                            key.layer,
                            ps as u32,
                            snapshot.width,
                            snapshot.height,
                        );
                }
            }
        }

        // Any ED in the stack: process the full horizontal tile-row as one strip
        // so FS below-left taps work even when Glow/Glitch/ordered wrap the ED.
        //
        // B1 strip owner: ≤1 strip compute per `(doc, layer, level, ty)`.
        // Owner = first successful `try_claim` (typically `min_x` via causal
        // schedule). Waiters poll briefly then return `EdStripBusy` so the
        // worker re-parks — never block a worker thread permanently on the gate.
        if engine_project::filters::layer_needs_ed_strip(layer) {
            use crate::state::ed_strip_gate::{STRIP_WAIT_SLICE, STRIP_WAIT_SLICES};
            use crate::state::EdStripKey;
            use engine_project::filters::apply_filter_stack_tile_row_strip;

            engine_project::filters::fs_ed_prof::note_tile_task();

            let bounds = layer.bounds_l0;
            let ty = key.coord.y;
            let level = key.coord.level;
            let processed_key = TileKey {
                doc: key.doc,
                layer: key.layer,
                coord: key.coord,
                stage: CacheStage::Processed,
            };

            // Fast path: another owner already published this tile.
            if engine_tiles::tile_fresh(&state.tiles.tile_cache, processed_key) {
                if let Some(arc) = state.tiles.tile_cache.get_entry(processed_key) {
                    return Ok(arc);
                }
            }

            let strip_key = EdStripKey {
                doc: key.doc,
                layer: key.layer,
                level,
                ty,
            };
            let gate = state.tiles.strip_gate(strip_key);
            let start_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

            let _strip_owner = {
                let mut claimed = gate.try_claim();
                if claimed.is_none() {
                    for _ in 0..STRIP_WAIT_SLICES {
                        // Owner finished → our tile may already be fresh.
                        if engine_tiles::tile_fresh(&state.tiles.tile_cache, processed_key) {
                            if let Some(arc) = state.tiles.tile_cache.get_entry(processed_key) {
                                return Ok(arc);
                            }
                        }
                        let now_gen = snapshot_for_key(state, key)?
                            .generations
                            .document_gen
                            .load(Ordering::Acquire);
                        if now_gen != start_gen {
                            return Err(EngineError::invalid_state(
                                "ED strip stale while waiting for strip owner",
                            ));
                        }
                        let _ = gate.wait_free_timeout(STRIP_WAIT_SLICE);
                        claimed = gate.try_claim();
                        if claimed.is_some() {
                            break;
                        }
                    }
                }
                match claimed {
                    Some(g) => g,
                    None => {
                        // Still busy: re-park (worker parks on strip anchor).
                        return Err(EngineError::EdStripBusy);
                    }
                }
            };

            // Re-check after claiming: previous owner finished while we waited.
            if engine_tiles::tile_fresh(&state.tiles.tile_cache, processed_key) {
                if let Some(arc) = state.tiles.tile_cache.get_entry(processed_key) {
                    return Ok(arc);
                }
            }

            // Previous strip must be finalized so top residuals exist.
            // Time only freshness probes here — recursive compute is counted in
            // its own scopes (avoid double-counting in fs_ed_prof %).
            if ty > 0 {
                for tx in bounds.min_x..=bounds.max_x {
                    let prev = TileKey {
                        doc: key.doc,
                        layer: key.layer,
                        coord: engine_tiles::TileCoord {
                            level,
                            x: tx,
                            y: ty - 1,
                        },
                        stage: CacheStage::Processed,
                    };
                    let fresh = {
                        let _prev = engine_project::filters::fs_ed_prof::Scope::new(
                            engine_project::filters::fs_ed_prof::add_ns_prev_row,
                        );
                        engine_tiles::tile_fresh(&state.tiles.tile_cache, prev)
                    };
                    if !fresh {
                        let _ = compute_processed_tile(prev, state)?;
                    }
                }
            }

            // doc_gen check between stripes (after prev-row ensure, before ED).
            let compute_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
            let mid_gen = snapshot_for_key(state, key)?
                .generations
                .document_gen
                .load(Ordering::Acquire);
            if mid_gen != compute_gen {
                return Err(EngineError::invalid_state(
                    "ED strip stale: document_gen advanced between stripes",
                ));
            }

            let mut coords = Vec::new();
            let mut raw_arcs = Vec::new();
            {
                let _raw = engine_project::filters::fs_ed_prof::Scope::new(
                    engine_project::filters::fs_ed_prof::add_ns_raw,
                );
                for tx in bounds.min_x..=bounds.max_x {
                    let coord = engine_tiles::TileCoord {
                        level,
                        x: tx,
                        y: ty,
                    };
                    let raw_key = TileKey {
                        doc: key.doc,
                        layer: key.layer,
                        coord,
                        stage: CacheStage::Raw,
                    };
                    let raw = state.tiles.tile_cache.get_entry(raw_key).ok_or_else(|| {
                        EngineError::invalid_state(format!(
                            "Raw tile missing for ED strip layer={} ({tx},{ty})",
                            key.layer
                        ))
                    })?;
                    coords.push(coord);
                    raw_arcs.push(raw);
                }
            }
            let strip: Vec<(engine_tiles::TileCoord, &PixelTile)> = coords
                .iter()
                .zip(raw_arcs.iter())
                .map(|(c, t)| (*c, t.as_ref()))
                .collect();
            let mut outputs: Vec<PixelTile> = (0..strip.len()).map(|_| PixelTile::new()).collect();
            apply_filter_stack_tile_row_strip(
                &strip,
                layer,
                &state.tiles.palette_cache,
                &state.tiles.palette_lut_cache,
                &state.tiles.threshold_cache,
                &snapshot,
                &state.tiles.error_residuals,
                &state.tiles.block_representatives,
                state.gpu.as_deref(),
                &mut outputs,
            )?;

            let now_gen = snapshot_for_key(state, key)?
                .generations
                .document_gen
                .load(Ordering::Acquire);
            let mut requested = None;
            if now_gen == compute_gen {
                let mut inserted_keys = Vec::with_capacity(coords.len());
                for (i, coord) in coords.iter().enumerate() {
                    let pkey = TileKey {
                        doc: key.doc,
                        layer: key.layer,
                        coord: *coord,
                        stage: CacheStage::Processed,
                    };
                    let arc = Arc::new(std::mem::replace(&mut outputs[i], PixelTile::new()));
                    let inserted = {
                        let _cw = engine_project::filters::fs_ed_prof::Scope::new(
                            engine_project::filters::fs_ed_prof::add_ns_cache_write,
                        );
                        state
                            .tiles
                            .tile_cache
                            .insert_fresh_gen(pkey, Arc::clone(&arc), compute_gen)
                    };
                    if inserted {
                        inserted_keys.push(pkey);
                    }
                    reschedule_if_insert_rejected(state, pkey, inserted);
                    if coord.x == key.coord.x && coord.y == key.coord.y {
                        requested = Some(arc);
                    }
                }
                if !inserted_keys.is_empty() {
                    state.evict_for_pressure_if_needed();
                    let _wk = engine_project::filters::fs_ed_prof::Scope::new(
                        engine_project::filters::fs_ed_prof::add_ns_wake,
                    );
                    wake_ed_frontier_after_strip_batch(state, &inserted_keys);
                }
            } else {
                for (i, coord) in coords.iter().enumerate() {
                    if coord.x == key.coord.x && coord.y == key.coord.y {
                        requested = Some(Arc::new(std::mem::replace(
                            &mut outputs[i],
                            PixelTile::new(),
                        )));
                        break;
                    }
                }
            }
            return requested
                .ok_or_else(|| EngineError::invalid_state("ED strip missing requested tile"));
        }

        apply_filter_to_tile_with_caches(
            &raw_tile,
            layer,
            key.coord,
            &state.tiles.palette_cache,
            &state.tiles.palette_lut_cache,
            &state.tiles.threshold_cache,
            &snapshot,
            &state.tiles.error_residuals,
            &state.tiles.block_representatives,
            state.gpu.as_deref(),
        )?
    };

    // 4. Single Arc: insert + return the same allocation (no return-path copy).
    let arc = Arc::new(processed);
    let compute_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let now_gen = snapshot_for_key(state, key)?
        .generations
        .document_gen
        .load(Ordering::Acquire);
    if now_gen == compute_gen {
        let inserted =
            state
                .tiles
                .tile_cache
                .insert_fresh_gen(processed_key, Arc::clone(&arc), compute_gen);
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
            engine_tiles::TileCoord {
                level: child_level,
                x: key.coord.x * 2,
                y: key.coord.y * 2,
            },
            engine_tiles::TileCoord {
                level: child_level,
                x: key.coord.x * 2 + 1,
                y: key.coord.y * 2,
            },
            engine_tiles::TileCoord {
                level: child_level,
                x: key.coord.x * 2,
                y: key.coord.y * 2 + 1,
            },
            engine_tiles::TileCoord {
                level: child_level,
                x: key.coord.x * 2 + 1,
                y: key.coord.y * 2 + 1,
            },
        ];
        let doc_gen = snapshot
            .generations
            .document_gen
            .load(std::sync::atomic::Ordering::Acquire);
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
            &state.tiles.tile_cache,
        ) {
            let arc = Arc::new(pyramid_tile);
            let now_gen = snapshot_for_key(state, key)?
                .generations
                .document_gen
                .load(Ordering::Acquire);
            if now_gen == doc_gen {
                let inserted =
                    state
                        .tiles
                        .tile_cache
                        .insert_fresh_gen(key, Arc::clone(&arc), doc_gen);
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
    let diag = crate::preview_diag::level();
    let t_comp = (diag != 0).then(Instant::now);
    let composited = composite_tile(&snapshot.root, key.doc, key.coord, &state.tiles.tile_cache)?;
    if let Some(t0) = t_comp {
        let gen = snapshot.generations.document_gen.load(Ordering::Acquire);
        crate::preview_diag::record_composite(key.doc, gen, t0.elapsed());
    }

    // 4. Single Arc: insert + return the same allocation.
    let arc = Arc::new(composited);
    let composite_key = TileKey {
        doc: key.doc,
        layer: key.layer,
        coord: key.coord,
        stage: CacheStage::Composite,
    };
    let compute_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let now_gen = snapshot_for_key(state, key)?
        .generations
        .document_gen
        .load(Ordering::Acquire);
    if now_gen == compute_gen {
        let inserted =
            state
                .tiles
                .tile_cache
                .insert_fresh_gen(composite_key, Arc::clone(&arc), compute_gen);
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
    if !state
        .tiles
        .tile_cache
        .mark_dirty_if_generation_behind(key, live)
    {
        return;
    }
    let layer_gen = snapshot.generations.get_layer_gen(key.layer);
    state.tiles.scheduler.enqueue_dedup(RecomputeTask {
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
    let viewport_level = state.ui.viewport.lock().unwrap().level;
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
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    enqueue_composite_dedup(
        state,
        parent_key,
        doc_gen,
        engine_tiles::Priority::Immediate,
    );
}

fn enqueue_composite_dedup(
    state: &AppState,
    key: TileKey,
    generation: u64,
    priority: engine_tiles::Priority,
) {
    let enqueued = state
        .tiles
        .scheduler
        .enqueue_dedup(engine_tiles::RecomputeTask {
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
    // Children were already enqueued by the caller. Do **not** requeue the parent
    // here: under zoom-out load that creates a dequeue→pending→requeue spin that
    // starves L0 Composite (Z3 harness: 320k pending, 0 L2 tile-ready). Parent
    // wakes via [`enqueue_coarser_parent`] when a child inserts. Keep `doc_gen`
    // in the signature for call-site symmetry / future trace.
    let _ = (state, key, doc_gen);
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
        let stale = match state.tiles.tile_cache.entries.get(&processed) {
            None => true,
            Some(entry) => entry.dirty.load(Ordering::Acquire),
        };
        if !stale {
            continue;
        }
        let raw = processed.with_stage(CacheStage::Raw);
        if !state.tiles.tile_cache.entries.contains_key(&raw) {
            // Park until Raw appears — do not zero-seed.
            let task = RecomputeTask {
                key: processed,
                generation: doc_gen,
                layer_generation: layer_gen,
                priority,
            };
            if let Some(ready) =
                state
                    .tiles
                    .ed_frontier
                    .block_on(task, vec![raw], &state.tiles.tile_cache)
            {
                if state.tiles.scheduler.enqueue_or_bump(ready) {
                    enqueued += 1;
                }
            }
            continue;
        }
        let task = RecomputeTask {
            key: processed,
            generation: doc_gen,
            layer_generation: layer_gen,
            priority,
        };
        if state.tiles.scheduler.enqueue_or_bump(task) {
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
///
/// CPU ED Processed exists only at **L0**. Visible L>0 tiles must expand to their
/// L0 footprint (`[0..=x1]×[0..=y1]` of the covering L0 max corner) — scheduling
/// at `coord.level` parks forever on missing L>0 Raw (harness Z3 holes=12).
pub fn schedule_ed_for_viewport(state: &AppState) {
    let viewport = state.ui.viewport.lock().unwrap().clone();
    let Ok(session) = state.active_session() else {
        return;
    };
    let snapshot = session.document_handle.snapshot();
    let doc = snapshot.id.0;
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

    collect_ed_layers(&snapshot.root, &mut |layer_id| {
        let layer_gen = snapshot.generations.get_layer_gen(layer_id);
        for coord in &viewport.visible_tiles {
            let (level, max_x, max_y) = if coord.level == 0 {
                (0u8, coord.x, coord.y)
            } else {
                let shift = coord.level;
                let x1 = ((coord.x + 1) << shift).saturating_sub(1);
                let y1 = ((coord.y + 1) << shift).saturating_sub(1);
                (0u8, x1, y1)
            };
            schedule_ed_prefix_closure(
                state,
                doc,
                layer_id,
                level,
                max_x,
                max_y,
                Priority::Immediate,
                doc_gen,
                layer_gen,
            );
        }
    });
}

pub fn collect_ed_layer_ids(nodes: &[LayerNode], out: &mut Vec<u32>) {
    collect_ed_layers(nodes, &mut |id| out.push(id));
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
            LayerNode::Group(group) if layer_has_error_diffusion(&group.children, layer_id) => {
                return true;
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
        .filter(|k| !engine_tiles::tile_fresh(&state.tiles.tile_cache, *k))
        .collect()
}

/// Deps for parking a Composite task after [`EngineError::EdPrefixPending`].
///
/// `pending_ed_processed_at_coord` only lists dirty keys **at this coord**. ED
/// readiness often fails because **prefix neighbors** are not fresh yet, which
/// left the worker with an empty dep list and a tight `enqueue_or_bump` spin.
/// Expand to every non-fresh Processed tile in the causal prefix `[0..=x]×[0..=y]`
/// for each visible ED layer.
pub fn composite_ed_park_deps(
    state: &AppState,
    nodes: &[LayerNode],
    doc: u32,
    coord: engine_tiles::TileCoord,
) -> Vec<TileKey> {
    let cache = &state.tiles.tile_cache;
    // CPU ED Processed exists only at L0. Do not seed deps from Processed at
    // `coord.level` when L>0 (phantom keys → forever blocked; harness holes=12).
    let mut deps = if coord.level == 0 {
        pending_ed_processed_at_coord(state, nodes, doc, coord)
    } else {
        Vec::new()
    };
    let (max_x, max_y) = if coord.level == 0 {
        (coord.x, coord.y)
    } else {
        let shift = coord.level;
        let x1 = ((coord.x + 1) << shift).saturating_sub(1);
        let y1 = ((coord.y + 1) << shift).saturating_sub(1);
        (x1, y1)
    };
    collect_ed_layers(nodes, &mut |layer_id| {
        for py in 0..=max_y {
            for px in 0..=max_x {
                let key = TileKey {
                    doc,
                    layer: layer_id,
                    coord: engine_tiles::TileCoord {
                        level: 0,
                        x: px,
                        y: py,
                    },
                    stage: CacheStage::Processed,
                };
                if !engine_tiles::tile_fresh(cache, key) {
                    deps.push(key);
                    for miss in engine_tiles::ed_missing_deps(cache, key) {
                        deps.push(miss);
                    }
                }
            }
        }
    });
    deps.sort_by(|a, b| {
        let stage_ord = |s: CacheStage| match s {
            CacheStage::Raw => 0u8,
            CacheStage::Processed => 1,
            CacheStage::Composite => 2,
        };
        (
            a.layer,
            a.coord.level,
            a.coord.y,
            a.coord.x,
            stage_ord(a.stage),
        )
            .cmp(&(
                b.layer,
                b.coord.level,
                b.coord.y,
                b.coord.x,
                stage_ord(b.stage),
            ))
    });
    deps.dedup();
    // Drop deps that are already satisfied — otherwise block_on waits for an
    // insert wake that will never come (race: L0 finished before Composite parked).
    deps.retain(|d| match d.stage {
        CacheStage::Raw => !cache.entries.contains_key(d),
        _ => !engine_tiles::tile_fresh(cache, *d),
    });
    deps
}

/// Wake EdFrontier after a successful Processed/Raw insert and re-enqueue ready tasks.
///
/// Composite tasks parked via [`EdFrontier::block_on`] on Processed deps are returned
/// here when those deps insert — without this wake, park would become a silent hang.
pub fn wake_ed_frontier_after_insert(state: &AppState, completed: TileKey) {
    let ready = if completed.stage == CacheStage::Processed {
        state
            .tiles
            .ed_frontier
            .wake_after_processed(completed, &state.tiles.tile_cache)
    } else {
        state
            .tiles
            .ed_frontier
            .wake(completed, &state.tiles.tile_cache)
    };
    for task in ready {
        state.tiles.scheduler.enqueue_or_bump(task);
        state.worker_wake.notify_one();
    }
}

/// Deps to park on when [`EngineError::EdStripBusy`]: other Processed tiles in
/// the same strip row (never `self` — that would self-deadlock). Owner inserts
/// the whole row before batch-wake, so any sibling becoming fresh unblocks us.
pub fn ed_strip_busy_deps(key: TileKey, min_x: u32, max_x: u32) -> Vec<TileKey> {
    let mut deps = Vec::new();
    for tx in min_x..=max_x {
        if tx == key.coord.x {
            continue;
        }
        deps.push(TileKey {
            doc: key.doc,
            layer: key.layer,
            coord: engine_tiles::TileCoord {
                level: key.coord.level,
                x: tx,
                y: key.coord.y,
            },
            stage: CacheStage::Processed,
        });
    }
    deps
}

/// Resolve layer bounds and return strip-busy park deps for `key`.
pub fn ed_strip_busy_park_deps(state: &AppState, key: TileKey) -> Vec<TileKey> {
    let Ok(snapshot) = snapshot_for_key(state, key) else {
        return Vec::new();
    };
    let Some(layer) = find_layer_by_id(&snapshot.root, key.layer) else {
        return Vec::new();
    };
    let b = layer.bounds_l0;
    ed_strip_busy_deps(key, b.min_x, b.max_x)
}

/// B1: after a full ED strip row is inserted, wake once per completed key and
/// dedupe ready tasks so the next ty is scheduled without mid-row churn.
pub fn wake_ed_frontier_after_strip_batch(state: &AppState, completed: &[TileKey]) {
    let mut ready_keys = HashSet::new();
    let mut ready = Vec::new();
    for &key in completed {
        let tasks = if key.stage == CacheStage::Processed {
            state
                .tiles
                .ed_frontier
                .wake_after_processed(key, &state.tiles.tile_cache)
        } else {
            state.tiles.ed_frontier.wake(key, &state.tiles.tile_cache)
        };
        for task in tasks {
            if ready_keys.insert(task.key) {
                ready.push(task);
            }
        }
    }
    for task in ready {
        state.tiles.scheduler.enqueue_or_bump(task);
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
            if !engine_tiles::ed_ready(&state.tiles.tile_cache, key, true) {
                ed_pending = true;
                continue;
            }
            if engine_tiles::tile_fresh(&state.tiles.tile_cache, key) {
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
                let needs_compute = match state.tiles.tile_cache.entries.get(&key) {
                    None => {
                        if crate::preview_diag::level() != 0 {
                            let gen = state
                                .session(doc)
                                .ok()
                                .map(|s| {
                                    s.document_handle
                                        .snapshot()
                                        .generations
                                        .document_gen
                                        .load(Ordering::Acquire)
                                })
                                .unwrap_or(0);
                            crate::preview_diag::record_composer_tile_miss(doc, gen);
                        }
                        true
                    }
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
    match state.tiles.tile_cache.entries.get(key) {
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
#[allow(dead_code)] // exercised in tile_pipeline tests
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
    use crate::viewport::ViewportState;
    use crate::worker::WorkerWake;
    use engine_project::document::DocumentHandle;
    use engine_project::filters::curves::CurveChannel;
    use engine_project::layer::{Layer, LayerNode};
    use engine_project::types::{DocumentId, LayerId, LayerKind};
    use engine_project::{Document, FilterInstance, FilterKind, FilterParams};
    use engine_tiles::{CacheStage, Scheduler, TileCache, TileCoord, TileKey};
    use std::sync::Mutex;
    use std::time::Duration;

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
        state
            .must_active()
            .document_handle
            .mutate(|d| *d = doc.clone());

        let full = engine_tiles::TILE_SIZE + 2 * engine_tiles::HALO;
        for cy in 0..4u32 {
            for cx in 0..4u32 {
                let coord = TileCoord {
                    level: 0,
                    x: cx,
                    y: cy,
                };
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
                state.tiles.tile_cache.insert_fresh(
                    TileKey {
                        doc: 1,
                        layer: ED_LAYER,
                        coord,
                        stage: CacheStage::Raw,
                    },
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
        state.tiles.error_residuals.clear();
        state.tiles.clear_strip_gates();
        engine_tiles::invalidation::invalidate(
            &state.tiles.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
                doc: 1,
                layer: ED_LAYER,
            },
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
        copy_tile(
            &state
                .tiles
                .tile_cache
                .get_entry(target)
                .expect("reference cached"),
        )
    }

    /// Drain scheduled ED Processed tasks in a single thread (topo via ready-gate).
    fn drain_ed_scheduler(state: &AppState, max_steps: usize) {
        for _ in 0..max_steps {
            let Some(task) = state.tiles.scheduler.dequeue() else {
                if state.tiles.ed_frontier.blocked_count() == 0 {
                    break;
                }
                // Should not happen if Raw present; break to avoid hang.
                break;
            };
            if task.key.stage != CacheStage::Processed {
                continue;
            }
            if !engine_tiles::ed_ready(&state.tiles.tile_cache, task.key, true) {
                if let Some(ready) = state.tiles.ed_frontier.block(task, &state.tiles.tile_cache) {
                    state.tiles.scheduler.enqueue_or_bump(ready);
                }
                continue;
            }
            match compute_processed_tile(task.key, state) {
                Ok(_) => wake_ed_frontier_after_insert(state, task.key),
                Err(EngineError::EdStripBusy) => {
                    let deps = ed_strip_busy_park_deps(state, task.key);
                    if deps.is_empty() {
                        state.tiles.scheduler.enqueue_or_bump(task);
                    } else {
                        let _ =
                            state
                                .tiles
                                .ed_frontier
                                .block_on(task, deps, &state.tiles.tile_cache);
                    }
                }
                Err(_) => {}
            }
        }
    }

    fn drain_ed_scheduler_n_workers(
        state: &Arc<AppState>,
        n_workers: usize,
        max_x: u32,
        max_y: u32,
        timeout: Duration,
    ) {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let t0 = Instant::now();
        let parent_session = engine_project::filters::fs_ed_prof::thread_session();
        std::thread::scope(|scope| {
            for _ in 0..n_workers {
                let state = Arc::clone(state);
                let stop = Arc::clone(&stop);
                scope.spawn(move || {
                    engine_project::filters::fs_ed_prof::set_thread_session(parent_session);
                    while !stop.load(Ordering::Relaxed) {
                        let all_fresh = (0..=max_y).all(|y| {
                            (0..=max_x).all(|x| {
                                engine_tiles::tile_fresh(&state.tiles.tile_cache, ed_key(x, y))
                            })
                        });
                        if all_fresh {
                            stop.store(true, Ordering::Relaxed);
                            break;
                        }
                        if t0.elapsed() > timeout {
                            stop.store(true, Ordering::Relaxed);
                            break;
                        }
                        let Some(task) = state.tiles.scheduler.dequeue() else {
                            std::thread::yield_now();
                            continue;
                        };
                        if task.key.stage != CacheStage::Processed {
                            continue;
                        }
                        if !engine_tiles::ed_ready(&state.tiles.tile_cache, task.key, true) {
                            state.tiles.ed_frontier.block(task, &state.tiles.tile_cache);
                            continue;
                        }
                        match compute_processed_tile(task.key, &state) {
                            Ok(_) => wake_ed_frontier_after_insert(&state, task.key),
                            Err(EngineError::EdStripBusy) => {
                                let deps = ed_strip_busy_park_deps(&state, task.key);
                                if deps.is_empty() {
                                    state.tiles.scheduler.enqueue_or_bump(task);
                                } else {
                                    let _ = state.tiles.ed_frontier.block_on(
                                        task,
                                        deps,
                                        &state.tiles.tile_cache,
                                    );
                                }
                            }
                            Err(_) => {}
                        }
                    }
                });
            }
        });
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
        let n = schedule_ed_prefix_closure(&state, 1, ED_LAYER, 0, 3, 3, Priority::Immediate, 0, 0);
        assert!(n >= 16, "expected full 4x4 prefix enqueue, got {n}");
        assert!(engine_tiles::ed_prefix_tiles_enqueued() >= 16);
        drain_ed_scheduler(&state, 256);

        let via_wave = copy_tile(
            &state
                .tiles
                .tile_cache
                .get_entry(target)
                .expect("wavefront target"),
        );
        assert_eq!(
            via_wave.data, reference.data,
            "wavefront schedule must be byte-identical to sequential row-major"
        );
    }

    /// B1: concurrent workers must compute each ty strip at most once.
    #[test]
    fn ed_strip_owner_one_calc_per_ty() {
        let _measure = engine_project::filters::fs_ed_prof::MeasureGuard::enter();

        let state = Arc::new(make_ed_state());
        reset_ed_processed(&state);
        schedule_ed_prefix_closure(&state, 1, ED_LAYER, 0, 3, 3, Priority::Immediate, 0, 0);
        drain_ed_scheduler_n_workers(&state, 4, 3, 3, Duration::from_secs(30));

        for y in 0..4u32 {
            for x in 0..4u32 {
                assert!(
                    engine_tiles::tile_fresh(&state.tiles.tile_cache, ed_key(x, y)),
                    "missing fresh Processed ({x},{y})"
                );
            }
        }
        assert!(
            !state.tiles.ed_strip_gates.any_owned(),
            "strip gates must be released after drain"
        );

        let snap = engine_project::filters::fs_ed_prof::Snapshot::capture();
        assert_eq!(
            snap.unique_ty_strips, 4,
            "expected 4 unique ty strips, got {}",
            snap.unique_ty_strips
        );
        assert_eq!(
            snap.strip_runs, snap.unique_ty_strips,
            "B1 target: strip_runs == unique_ty (recalcs/line=1.0); runs={} unique={}",
            snap.strip_runs, snap.unique_ty_strips
        );
        assert_eq!(snap.strip_recalc_extra, 0);
    }

    #[test]
    fn ed_strip_gate_workers_1_2_8_single_and_multi_row() {
        for n_workers in [1usize, 2, 8] {
            // Multi-row 4×4
            let state = Arc::new(make_ed_state());
            reset_ed_processed(&state);
            schedule_ed_prefix_closure(&state, 1, ED_LAYER, 0, 3, 3, Priority::Immediate, 0, 0);
            drain_ed_scheduler_n_workers(&state, n_workers, 3, 3, Duration::from_secs(30));
            for y in 0..4u32 {
                for x in 0..4u32 {
                    assert!(
                        engine_tiles::tile_fresh(&state.tiles.tile_cache, ed_key(x, y)),
                        "workers={n_workers} missing ({x},{y})"
                    );
                }
            }
            assert!(!state.tiles.ed_strip_gates.any_owned());

            // Single-row: only y=0 (1×4)
            let state = Arc::new(make_ed_state());
            reset_ed_processed(&state);
            schedule_ed_prefix_closure(&state, 1, ED_LAYER, 0, 3, 0, Priority::Immediate, 0, 0);
            drain_ed_scheduler_n_workers(&state, n_workers, 3, 0, Duration::from_secs(15));
            for x in 0..4u32 {
                assert!(
                    engine_tiles::tile_fresh(&state.tiles.tile_cache, ed_key(x, 0)),
                    "single-row workers={n_workers} missing ({x},0)"
                );
            }
            assert!(!state.tiles.ed_strip_gates.any_owned());
        }
    }

    #[test]
    fn ed_strip_gate_mid_cancel_releases_waiters() {
        use crate::state::EdStripKey;
        use std::sync::Barrier;

        let state = Arc::new(make_ed_state());
        reset_ed_processed(&state);
        // Finish row 0 so ty=1's min_x is ed_ready.
        compute_processed_tile(ed_key(0, 0), &state).expect("row0");

        let strip_key = EdStripKey {
            doc: 1,
            layer: ED_LAYER,
            level: 0,
            ty: 1,
        };
        let gate = state.tiles.strip_gate(strip_key);
        let barrier = Arc::new(Barrier::new(3));
        let owner_hold = Arc::new(Barrier::new(2));

        let owner_gate = Arc::clone(&gate);
        let b1 = Arc::clone(&barrier);
        let hold = Arc::clone(&owner_hold);
        let owner = std::thread::spawn(move || {
            let guard = owner_gate.try_claim().expect("owner claim");
            b1.wait();
            hold.wait();
            drop(guard);
        });

        let mut waiter_handles = Vec::new();
        for _ in 0..2 {
            let state = Arc::clone(&state);
            let b = Arc::clone(&barrier);
            waiter_handles.push(std::thread::spawn(move || {
                b.wait();
                let t0 = Instant::now();
                // Both call min_x of ty=1 — ed_ready, contend for strip gate.
                let res = compute_processed_tile(ed_key(0, 1), &state);
                let elapsed = t0.elapsed();
                assert!(
                    elapsed < Duration::from_secs(2),
                    "waiter hung for {elapsed:?}"
                );
                matches!(
                    res,
                    Ok(_) | Err(EngineError::EdStripBusy) | Err(EngineError::InvalidState { .. })
                )
            }));
        }

        std::thread::sleep(Duration::from_millis(30));
        state.must_active().document_handle.mutate(|d| {
            d.generations.increment_document_gen();
        });
        reset_ed_processed(&state);
        owner_hold.wait();
        owner.join().unwrap();

        for h in waiter_handles {
            assert!(h.join().unwrap(), "waiter must exit cleanly");
        }
        assert!(!gate.is_owned(), "gate must be released after owner drop");
        assert!(!state.tiles.ed_strip_gates.any_owned());
    }

    #[test]
    fn ed_priority_inheritance_bumps_prefix_to_immediate() {
        let state = make_ed_state();
        reset_ed_processed(&state);
        // Seed prefix as Prefetch first.
        schedule_ed_prefix_closure(&state, 1, ED_LAYER, 0, 3, 3, Priority::Prefetch, 0, 0);
        assert_eq!(
            state.tiles.scheduler.queued_priority_of(&ed_key(0, 0)),
            Some(Priority::Prefetch)
        );
        // Visible corner requests Immediate → inheritance bump.
        schedule_ed_prefix_closure(&state, 1, ED_LAYER, 0, 3, 3, Priority::Immediate, 0, 0);
        assert_eq!(
            state.tiles.scheduler.queued_priority_of(&ed_key(0, 0)),
            Some(Priority::Immediate)
        );
        let first = state.tiles.scheduler.dequeue().unwrap();
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
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state.tiles.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        // Compute processed tile
        let processed_key = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
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
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state.tiles.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
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
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
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
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state.tiles.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
            layer: 999,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
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
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state.tiles.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Processed,
        };

        // Cache should not have the processed tile yet
        assert!(state.tiles.tile_cache.get_entry(processed_key).is_none());

        let _ = compute_processed_tile(processed_key, &state);

        // After computation, cache should have the processed tile
        assert!(state.tiles.tile_cache.get_entry(processed_key).is_some());
    }

    #[test]
    fn compute_processed_returns_same_arc_as_cache() {
        let state = make_app_state_with_layer(1, false);

        let mut raw = PixelTile::new();
        raw.set(10, 10, 0, 0.42);
        let raw_key = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state.tiles.tile_cache.get_or_insert(raw_key, Arc::new(raw));

        let processed_key = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Processed,
        };
        let returned = compute_processed_tile(processed_key, &state).expect("compute");
        let cached = state
            .tiles
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
        state
            .must_active()
            .document_handle
            .store(std::sync::Arc::new(doc));
        state
    }

    #[test]
    fn composite_ed_park_deps_includes_prefix_not_only_this_coord() {
        // When left Processed is missing, park deps must be non-empty so the
        // worker can block_on instead of tight-loop enqueue_or_bump.
        let state = make_ed_layer_state();
        let coord = TileCoord {
            level: 0,
            x: 1,
            y: 0,
        };
        state.tiles.tile_cache.get_or_insert(
            TileKey {
                doc: 1,
                layer: 1,
                coord,
                stage: CacheStage::Raw,
            },
            Arc::new(PixelTile::new()),
        );
        let snap = state.must_active().document_handle.snapshot();
        let deps = composite_ed_park_deps(&state, &snap.root, 1, coord);
        assert!(
            !deps.is_empty(),
            "park deps must be non-empty when ED prefix is incomplete"
        );
        assert!(
            deps.iter()
                .any(|k| { k.stage == CacheStage::Processed && k.coord.x == 0 && k.coord.y == 0 }),
            "must wait on left neighbor Processed (0,0), got {deps:?}"
        );
    }

    #[test]
    fn composite_ed_park_deps_l2_uses_l0_and_empty_when_fresh() {
        // Zoomed-out Composite must not invent Processed deps at L2.
        let state = make_ed_layer_state();
        for y in 0..4u32 {
            for x in 0..4u32 {
                let coord = TileCoord { level: 0, x, y };
                state.tiles.tile_cache.insert_fresh(
                    TileKey {
                        doc: 1,
                        layer: 1,
                        coord,
                        stage: CacheStage::Raw,
                    },
                    Arc::new(PixelTile::new()),
                );
                state.tiles.tile_cache.insert_fresh(
                    TileKey {
                        doc: 1,
                        layer: 1,
                        coord,
                        stage: CacheStage::Processed,
                    },
                    Arc::new(PixelTile::new()),
                );
            }
        }
        let snap = state.must_active().document_handle.snapshot();
        let l2 = TileCoord {
            level: 2,
            x: 0,
            y: 0,
        };
        let deps = composite_ed_park_deps(&state, &snap.root, 1, l2);
        assert!(
            deps.iter().all(|k| k.coord.level == 0),
            "ED park deps must be L0, got {deps:?}"
        );
        assert!(
            deps.is_empty(),
            "when L0 Processed footprint is fresh, park deps must be empty (else lost-wake), got {deps:?}"
        );
    }

    #[test]
    fn schedule_ed_for_viewport_l2_enqueues_l0_only() {
        let state = make_ed_layer_state();
        // Seed Raw at L0 so prefix enqueue does not park on missing Raw.
        for y in 0..4u32 {
            for x in 0..4u32 {
                state.tiles.tile_cache.insert_fresh(
                    TileKey {
                        doc: 1,
                        layer: 1,
                        coord: TileCoord { level: 0, x, y },
                        stage: CacheStage::Raw,
                    },
                    Arc::new(PixelTile::new()),
                );
            }
        }
        {
            let mut vp = state.ui.viewport.lock().unwrap();
            vp.level = 2;
            vp.visible_tiles = vec![TileCoord {
                level: 2,
                x: 0,
                y: 0,
            }];
        }
        schedule_ed_for_viewport(&state);
        let mut saw_l0 = false;
        while let Some(task) = state.tiles.scheduler.dequeue() {
            assert_eq!(
                task.key.stage,
                CacheStage::Processed,
                "ED viewport schedule should only enqueue Processed"
            );
            assert_eq!(
                task.key.coord.level, 0,
                "ED must not enqueue L>0 Processed (got {:?})",
                task.key.coord
            );
            saw_l0 = true;
        }
        assert!(saw_l0, "expected L0 Processed prefix for L2 viewport tile");
    }

    #[test]
    fn skip_branch_does_not_publish_when_neighbor_raw_missing() {
        let state = make_ed_layer_state();
        let current_raw = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state
            .tiles
            .tile_cache
            .get_or_insert(current_raw, Arc::new(PixelTile::new()));

        let processed = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
            stage: CacheStage::Processed,
        };
        let result = compute_processed_tile(processed, &state);
        assert!(
            matches!(result, Err(EngineError::EdDependenciesPending)),
            "must not publish zero-seed Processed when left Raw missing"
        );
        assert!(
            state.tiles.tile_cache.get_entry(processed).is_none()
                || state
                    .tiles
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
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state
            .tiles
            .tile_cache
            .get_or_insert(current_raw, Arc::new(PixelTile::new()));
        // left Raw absent
        engine_tiles::reset_ed_blocked_total();
        schedule_ed_prefix_closure(&state, 1, 1, 0, 1, 0, Priority::Immediate, 0, 0);
        assert!(
            state.tiles.ed_frontier.blocked_count() > 0 || engine_tiles::ed_blocked_total() > 0,
            "missing left raw on schedule must park in EdFrontier"
        );
    }

    #[test]
    fn evict_layer_of_current_layer_errors_before_ed_ready() {
        let state = make_ed_layer_state();
        let raw = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state
            .tiles
            .tile_cache
            .get_or_insert(raw, Arc::new(PixelTile::new()));
        state.tiles.tile_cache.evict_layer(1, 1);

        let processed = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
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
        state
            .must_active()
            .document_handle
            .store(std::sync::Arc::new(doc));

        let mut buffer = vec![0.0f32; (512 * 512 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[0] = 0.6;
            px[1] = 0.6;
            px[2] = 0.6;
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tiles.tile_cache).unwrap();

        let l1_key = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 1,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        assert!(
            compute_composite_tile(l1_key, &state).is_err(),
            "L1 must wait for full-res children, not filter downsampled raw"
        );

        let mut l0_computed = 0usize;
        while let Some(task) = state.tiles.scheduler.dequeue() {
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
        state
            .must_active()
            .document_handle
            .store(std::sync::Arc::new(doc));
        state.ui.viewport.lock().unwrap().level = 1;

        let mut buffer = vec![0.0f32; (512 * 512 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[0] = 0.6;
            px[1] = 0.6;
            px[2] = 0.6;
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tiles.tile_cache).unwrap();

        let l0_key = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        compute_composite_tile(l0_key, &state).unwrap();

        let mut found_parent = false;
        while let Some(task) = state.tiles.scheduler.dequeue() {
            if task.key.stage == CacheStage::Composite
                && task.key.coord.level == 1
                && task.key.coord.x == 0
                && task.key.coord.y == 0
            {
                found_parent = true;
                break;
            }
        }
        assert!(
            found_parent,
            "L0 Composite must enqueue its L1 display parent"
        );
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
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tiles.tile_cache).unwrap();

        let key = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };

        state.must_active().document_handle.mutate(|doc| {
            doc.increment_generation();
        });
        let gen1 = state
            .must_active()
            .document_handle
            .snapshot()
            .generations
            .document_gen
            .load(Ordering::Acquire);
        compute_composite_tile(key, &state).unwrap();
        assert_eq!(
            state.tiles.tile_cache.entries.get(&key).unwrap().generation,
            gen1
        );

        state.must_active().document_handle.mutate(|doc| {
            doc.increment_generation();
        });
        let gen2 = state
            .must_active()
            .document_handle
            .snapshot()
            .generations
            .document_gen
            .load(Ordering::Acquire);
        compute_composite_tile(key, &state).unwrap();
        assert_eq!(
            state.tiles.tile_cache.entries.get(&key).unwrap().generation,
            gen2
        );

        let mut stale = PixelTile::new();
        stale.set(0, 0, 0, 0.99);
        assert!(
            !state
                .tiles
                .tile_cache
                .insert_fresh_gen(key, Arc::new(stale), gen1),
            "older generation must not overwrite the latest Composite"
        );
        let entry = state.tiles.tile_cache.entries.get(&key).unwrap();
        assert_eq!(entry.generation, gen2);
        assert!((entry.tile.at(0, 0, 0) - 0.99).abs() > 0.5);
        drop(entry);

        state.must_active().document_handle.mutate(|doc| {
            doc.increment_generation();
        });
        let gen3 = state
            .must_active()
            .document_handle
            .snapshot()
            .generations
            .document_gen
            .load(Ordering::Acquire);
        reschedule_if_insert_rejected(&state, key, false);
        assert!(state
            .tiles
            .tile_cache
            .entries
            .get(&key)
            .unwrap()
            .dirty
            .load(Ordering::Acquire));
        assert!(state.tiles.scheduler.contains_key(&key));
        assert!(gen3 > gen2);
    }

    #[test]
    fn enqueue_coarser_parent_dedups_same_generation() {
        use engine_tiles::decompose::decompose_image_to_tiles;

        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 512, 512);
        doc.root.push(LayerNode::Leaf(layer));
        let mut state = make_app_state_with_layer(1, false);
        state
            .must_active()
            .document_handle
            .store(std::sync::Arc::new(doc));
        state.ui.viewport.lock().unwrap().level = 1;

        let mut buffer = vec![0.0f32; (512 * 512 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 512, 512, 1, 1, &state.tiles.tile_cache).unwrap();

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
            coord: TileCoord {
                level: 1,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        let mut parent_tasks = 0u32;
        while let Some(task) = state.tiles.scheduler.dequeue() {
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
        state
            .must_active()
            .document_handle
            .store(std::sync::Arc::new(doc));
        state.ui.viewport.lock().unwrap().level = 2;

        let mut buffer = vec![0.0f32; (1024 * 1024 * 4) as usize];
        for px in buffer.chunks_exact_mut(4) {
            px[3] = 1.0;
        }
        decompose_image_to_tiles(&buffer, 1024, 1024, 1, 1, &state.tiles.tile_cache).unwrap();

        let parent = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 2,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        let err = match compute_composite_tile(parent, &state) {
            Err(e) => e,
            Ok(_) => panic!("L2 must wait for children"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("not yet computed"),
            "expected pending children, got {msg}"
        );
        assert!(
            !msg.contains("missing after wait"),
            "must not fail closed when children are still missing"
        );
        // H8: parent must NOT self-requeue (that starved L0 under Z3). Children
        // are enqueued; parent wakes later via enqueue_coarser_parent on insert.
        assert!(
            !state.tiles.scheduler.contains_key(&parent),
            "parent must not self-requeue on PyramidChildrenPending"
        );
        let child = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 1,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        assert!(
            state.tiles.scheduler.contains_key(&child),
            "missing children must be enqueued so parent can wake on insert"
        );
    }
}
