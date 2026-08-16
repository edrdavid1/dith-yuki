//! Tile worker loop for processing recomputation tasks from the scheduler.
//!
//! This module implements the background worker that continuously dequeues tasks
//! from the Scheduler, performs staleness checks against the GenerationTracker,
//! executes tile computations (Raw/Processed/Composite), inserts fresh results
//! into the TileCache, and emits `tile-ready` events to the frontend.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::Emitter;

use engine_project::Document;
use engine_tiles::{CacheStage, PixelTile, RecomputeTask, TileKey};

use crate::commands::AppState;

/// Condition-variable based worker wake mechanism.
/// Replaces `park_timeout(1ms)` for immediate wake on task availability.
pub struct WorkerWake {
    mutex: Mutex<bool>,
    condvar: Condvar,
}

impl WorkerWake {
    pub fn new() -> Self {
        Self {
            mutex: Mutex::new(false),
            condvar: Condvar::new(),
        }
    }

    /// Signal workers that tasks are available.
    /// Called from Scheduler::enqueue() or wherever tasks are added.
    pub fn notify_one(&self) {
        let mut has_tasks = self.mutex.lock().unwrap();
        *has_tasks = true;
        self.condvar.notify_one();
    }

    /// Wait until tasks are available.
    /// Called from worker loop when dequeue returns None.
    /// If the mutex is poisoned, falls back to a 1ms sleep (degraded mode).
    pub fn wait(&self) {
        let lock_result = self.mutex.lock();
        match lock_result {
            Ok(mut guard) => {
                while !*guard {
                    match self.condvar.wait(guard) {
                        Ok(g) => guard = g,
                        Err(_) => {
                            // Condvar poisoned — fall back to park_timeout
                            std::thread::park_timeout(Duration::from_millis(1));
                            return;
                        }
                    }
                }
                *guard = false;
            }
            Err(_) => {
                // Mutex poisoned — degraded mode
                std::thread::park_timeout(Duration::from_millis(1));
            }
        }
    }
}

impl Default for WorkerWake {
    fn default() -> Self {
        Self::new()
    }
}

/// Payload emitted with the `tile-ready` event when a tile has been recomputed.
///
/// Contains sufficient identity information for the frontend to determine
/// which screen region to repaint (requirement 10.6).
#[derive(Serialize, Clone)]
pub struct TileReadyPayload {
    pub doc_id: u32,
    pub layer_id: u32,
    pub stage: String,
    pub level: u8,
    pub x: u32,
    pub y: u32,
}

/// Process recomputation tasks from the scheduler in a continuous loop.
///
/// Called on a dedicated thread; loops continuously, dequeueing tasks by priority.
/// Before executing each task, performs a staleness check against the document's
/// GenerationTracker. Stale tasks (where the document or layer generation has
/// advanced past the task's recorded generation) are discarded without computation.
///
/// On successful computation, the fresh tile is inserted into the cache and a
/// `tile-ready` event is emitted to notify the frontend.
///
/// When no tasks are available, the thread parks for 1ms to avoid busy-spinning.
///
/// # Arguments
///
/// * `state` - Shared application state (Arc-wrapped for thread safety)
/// * `app_handle` - Tauri AppHandle for emitting events to the frontend
pub fn tile_worker_loop(state: Arc<AppState>, app_handle: tauri::AppHandle) {
    loop {
        if let Some(task) = state.scheduler.dequeue() {
            state
                .preview_pass_inflight
                .fetch_add(1, Ordering::AcqRel);

            // Staleness check (requirement 10.5), including Composite layer 0.
            // Skipping Composite used to reuse in-flight work during slider
            // changes, but insert_fresh then overwrote a newer result with a
            // slower older one. Discard stale tasks; schedule_dirty_viewport_tiles
            // already enqueued the current generation.
            let snapshot = state.document_handle.snapshot();

            if task_is_stale(&task, &snapshot) {
                state
                    .preview_pass_inflight
                    .fetch_sub(1, Ordering::AcqRel);
                crate::commands::on_preview_task_finished(&state);
                continue;
            }

            // Execute task based on stage
            let result = match task.key.stage {
                CacheStage::Raw => load_raw_tile(task.key, &state),
                CacheStage::Processed => compute_processed_tile(task.key, &state),
                CacheStage::Composite => compute_composite_tile(task.key, &state),
            };

            if let Ok(tile) = result {
                // Re-check after compute: gen may have advanced while we worked.
                let snapshot = state.document_handle.snapshot();
                if task_is_stale(&task, &snapshot) {
                    // Discarded; current gen should already be queued or pending refresh.
                } else {
                    let inserted = state.tile_cache.insert_fresh_gen(
                        task.key,
                        Arc::new(tile),
                        task.generation,
                    );
                    if inserted {
                    // Track A: wake Processed tiles that computed with zero-seed
                    // because this raw neighbor was missing.
                    if task.key.stage == CacheStage::Raw {
                        let waiters = state.pending_diffusion_waiters.wake(&task.key);
                        if !waiters.is_empty() {
                            let doc_gen =
                                snapshot.generations.document_gen.load(Ordering::Acquire);
                            for processed_key in waiters {
                                state.tile_cache.mark_dirty(processed_key);
                                let layer_gen =
                                    snapshot.generations.get_layer_gen(processed_key.layer);
                                state.scheduler.enqueue(engine_tiles::RecomputeTask {
                                    key: processed_key,
                                    generation: doc_gen,
                                    layer_generation: layer_gen,
                                    priority: engine_tiles::Priority::Immediate,
                                });
                                state.worker_wake.notify_one();
                            }
                        }
                    }

                    // Only push tiles the canvas will actually blit (current pyramid level).
                    // Intermediate L0 children of a zoom-out display tile stay in cache.
                    let viewport_level = state.viewport.lock().unwrap().level;
                    if task.key.stage == CacheStage::Composite
                        && task.key.coord.level == viewport_level
                    {
                        let payload = TileReadyPayload {
                            doc_id: snapshot.id.0,
                            layer_id: task.key.layer,
                            stage: "composite".to_string(),
                            level: task.key.coord.level,
                            x: task.key.coord.x,
                            y: task.key.coord.y,
                        };
                        let _ = app_handle.emit_to(
                            tauri::EventTarget::Any,
                            "tile-ready",
                            payload,
                        );
                    }
                    } else {
                        crate::tile_pipeline::reschedule_if_insert_rejected(
                            &state,
                            task.key,
                            false,
                        );
                    }
                }
            } else if let Err(err) = result {
                let msg = err.to_string();
                if !msg.contains("Pyramid children not yet computed") {
                    eprintln!(
                        "tile compute failed layer={} stage={:?} l={}/{}/{}: {msg}",
                        task.key.layer,
                        task.key.stage,
                        task.key.coord.level,
                        task.key.coord.x,
                        task.key.coord.y
                    );
                }
            }

            state
                .preview_pass_inflight
                .fetch_sub(1, Ordering::AcqRel);
            crate::commands::on_preview_task_finished(&state);
        } else {
            // No tasks available — wait on Condvar for immediate wake when work arrives.
            // Falls back to park_timeout(1ms) if the mutex is poisoned (degraded mode).
            state.worker_wake.wait();
        }
    }
}

/// True when the task's recorded generations no longer match the live document.
pub(crate) fn task_is_stale(task: &RecomputeTask, snapshot: &Document) -> bool {
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let layer_gen = snapshot.generations.get_layer_gen(task.key.layer);
    task.generation != doc_gen || task.layer_generation != layer_gen
}

/// Load a Raw-stage tile from the cache.
///
/// Raw tiles are populated during image decomposition (`load_image` / `decompose_image_to_tiles`).
/// This function copies the existing tile data into a new PixelTile if present in the cache.
/// In the future, this could trigger on-demand loading from disk for tiles evicted from cache.
fn load_raw_tile(key: TileKey, state: &AppState) -> Result<PixelTile, String> {
    match state.tile_cache.get_entry(key) {
        Some(tile) => {
            // Copy the tile data into a new PixelTile
            let mut new_tile = PixelTile::new();
            new_tile.data.copy_from_slice(&tile.data);
            Ok(new_tile)
        }
        None => Err(format!(
            "Raw tile not found in cache: layer={}, level={}, ({}, {})",
            key.layer, key.coord.level, key.coord.x, key.coord.y
        )),
    }
}

/// Compute a Processed-stage tile (apply layer filters to the Raw tile).
///
/// Delegates to `tile_pipeline::compute_processed_tile` for the real implementation.
fn compute_processed_tile(key: TileKey, state: &AppState) -> Result<PixelTile, String> {
    crate::tile_pipeline::compute_processed_tile(key, state)
        .map_err(|e| format!("{}", e))
}

/// Compute a Composite-stage tile (blend all visible layers).
///
/// Delegates to `tile_pipeline::compute_composite_tile` for the real implementation.
fn compute_composite_tile(key: TileKey, state: &AppState) -> Result<PixelTile, String> {
    crate::tile_pipeline::compute_composite_tile(key, state)
        .map_err(|e| format!("{}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_project::Document;
    use engine_project::types::DocumentId;
    use engine_tiles::{Priority, TileCoord};

    fn composite_task(generation: u64) -> RecomputeTask {
        RecomputeTask {
            key: TileKey {
                layer: 0,
                coord: TileCoord {
                    level: 0,
                    x: 0,
                    y: 0,
                },
                stage: CacheStage::Composite,
            },
            generation,
            layer_generation: 0,
            priority: Priority::Immediate,
        }
    }

    #[test]
    fn composite_layer0_is_stale_when_document_gen_advances() {
        let doc = Document::new(DocumentId::new(1), 64, 64);
        assert_eq!(
            doc.generations
                .document_gen
                .load(std::sync::atomic::Ordering::Acquire),
            0
        );
        assert!(
            task_is_stale(&composite_task(1), &doc),
            "Composite layer 0 must not skip the generation check"
        );

        doc.generations.increment_document_gen();
        assert!(!task_is_stale(&composite_task(1), &doc));

        doc.generations.increment_document_gen();
        assert!(task_is_stale(&composite_task(1), &doc));
    }
}
