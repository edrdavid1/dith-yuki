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
            let snapshot = match state.session(task.key.doc) {
                Ok(s) => s.document_handle.snapshot(),
                Err(_) => {
                    state
                        .preview_pass_inflight
                        .fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
            };

            if task_is_stale(&task, &snapshot) {
                state
                    .preview_pass_inflight
                    .fetch_sub(1, Ordering::AcqRel);
                crate::commands::on_preview_task_finished(&state);
                continue;
            }

            let result: Result<Arc<PixelTile>, engine_project::error::EngineError> =
                match task.key.stage {
                CacheStage::Raw => load_raw_tile(task.key, &state),
                CacheStage::Processed => {
                    if crate::tile_pipeline::layer_has_error_diffusion(
                        &snapshot.root,
                        task.key.layer,
                    ) && !engine_tiles::ed_ready(&state.tile_cache, task.key, true)
                    {
                        state.ed_frontier.block(task, &state.tile_cache);
                        state
                            .preview_pass_inflight
                            .fetch_sub(1, Ordering::AcqRel);
                        crate::commands::on_preview_task_finished(&state);
                        continue;
                    }
                    compute_processed_tile(task.key, &state)
                }
                CacheStage::Composite => match compute_composite_tile(task.key, &state) {
                    Err(engine_project::error::EngineError::EdPrefixPending) => {
                        let deps = crate::tile_pipeline::pending_ed_processed_at_coord(
                            &state,
                            &snapshot.root,
                            task.key.doc,
                            task.key.coord,
                        );
                        if deps.is_empty() {
                            state.scheduler.enqueue_or_bump(task);
                            state.worker_wake.notify_one();
                        } else {
                            state.ed_frontier.block_on(task, deps);
                        }
                        state
                            .preview_pass_inflight
                            .fetch_sub(1, Ordering::AcqRel);
                        crate::commands::on_preview_task_finished(&state);
                        continue;
                    }
                    other => other,
                },
            };

            if let Ok(tile) = result {
                // Re-check after compute: gen may have advanced while we worked.
                let snapshot = match state.session(task.key.doc) {
                    Ok(s) => s.document_handle.snapshot(),
                    Err(_) => {
                        state
                            .preview_pass_inflight
                            .fetch_sub(1, Ordering::AcqRel);
                        crate::commands::on_preview_task_finished(&state);
                        continue;
                    }
                };
                if task_is_stale(&task, &snapshot) {
                    // Discarded; current gen should already be queued or pending refresh.
                } else {
                    match task.key.stage {
                        // Raw is already in cache; load_raw_tile returns the shared Arc.
                        CacheStage::Raw => {
                            let inserted = state.tile_cache.insert_fresh_gen(
                                task.key,
                                tile,
                                task.generation,
                            );
                            if inserted {
                                state.evict_for_pressure_if_needed();
                                crate::tile_pipeline::wake_ed_frontier_after_insert(
                                    &state, task.key,
                                );
                            } else {
                                crate::tile_pipeline::reschedule_if_insert_rejected(
                                    &state, task.key, false,
                                );
                            }
                        }
                        // Processed / Composite: pipeline already published a single Arc.
                        CacheStage::Processed | CacheStage::Composite => {
                            let viewport_level = state.viewport.lock().unwrap().level;
                            if task.key.stage == CacheStage::Composite
                                && task.key.coord.level == viewport_level
                                && state.tile_cache.get_entry(task.key).is_some()
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
                        }
                    }
                }
            } else if let Err(err) = result {
                if !matches!(
                    err,
                    engine_project::error::EngineError::PyramidChildrenPending
                        | engine_project::error::EngineError::EdPrefixPending
                        | engine_project::error::EngineError::EdDependenciesPending
                ) {
                    eprintln!(
                        "tile compute failed layer={} stage={:?} l={}/{}/{}: {err}",
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

fn load_raw_tile(
    key: TileKey,
    state: &AppState,
) -> Result<Arc<PixelTile>, engine_project::error::EngineError> {
    state.tile_cache.get_entry(key).ok_or_else(|| {
        engine_project::error::EngineError::invalid_state(format!(
            "Raw tile not found in cache: layer={}, level={}, ({}, {})",
            key.layer, key.coord.level, key.coord.x, key.coord.y
        ))
    })
}

fn compute_processed_tile(
    key: TileKey,
    state: &AppState,
) -> Result<Arc<PixelTile>, engine_project::error::EngineError> {
    crate::tile_pipeline::compute_processed_tile(key, state)
}

fn compute_composite_tile(
    key: TileKey,
    state: &AppState,
) -> Result<Arc<PixelTile>, engine_project::error::EngineError> {
    crate::tile_pipeline::compute_composite_tile(key, state)
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
                doc: 1,
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
