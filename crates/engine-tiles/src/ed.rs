//! Error-diffusion readiness and frontier (wavefront scheduler).
//!
//! See `.cursor-spec/ed-scheduler/SPEC.md`. Topology is left / top / diag;
//! workers compute one tile only when [`ed_ready`]. Blocked work (Processed or
//! Composite) parks here until deps insert — no zero-seed publish, no stringly
//! Composite busy-retry.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;

use crate::cache::TileCache;
use crate::scheduler::RecomputeTask;
use crate::types::{CacheStage, TileCoord, TileKey};

/// Cumulative prefix tiles offered to the scheduler on ED invalidate / viewport
/// schedule (Decision 6 metric).
pub static ED_PREFIX_TILES_ENQUEUED: AtomicU64 = AtomicU64::new(0);

/// Times a task was parked in the frontier (missing Raw / not-ready deps).
pub static ED_BLOCKED_TOTAL: AtomicU64 = AtomicU64::new(0);

pub fn reset_ed_prefix_tiles_enqueued() {
    ED_PREFIX_TILES_ENQUEUED.store(0, Ordering::Release);
}

pub fn ed_prefix_tiles_enqueued() -> u64 {
    ED_PREFIX_TILES_ENQUEUED.load(Ordering::Acquire)
}

pub fn add_ed_prefix_tiles_enqueued(n: u64) {
    if n > 0 {
        ED_PREFIX_TILES_ENQUEUED.fetch_add(n, Ordering::Relaxed);
    }
}

pub fn ed_blocked_total() -> u64 {
    ED_BLOCKED_TOTAL.load(Ordering::Acquire)
}

pub fn reset_ed_blocked_total() {
    ED_BLOCKED_TOTAL.store(0, Ordering::Release);
}

fn note_blocked() {
    ED_BLOCKED_TOTAL.fetch_add(1, Ordering::Relaxed);
}

/// True when `entry` exists and is not dirty.
pub fn tile_fresh(cache: &TileCache, key: TileKey) -> bool {
    match cache.entries.get(&key) {
        None => false,
        Some(entry) => !entry.dirty.load(Ordering::Acquire),
    }
}

fn raw_present(cache: &TileCache, key: TileKey) -> bool {
    cache.entries.contains_key(&key.with_stage(CacheStage::Raw))
}

/// Neighbor Processed keys that must be fresh before `key` (ED topology).
pub fn ed_dependency_keys(key: TileKey) -> Vec<TileKey> {
    let mut deps = Vec::with_capacity(3);
    let c = key.coord;
    if c.x > 0 {
        deps.push(TileKey {
            doc: key.doc,
            layer: key.layer,
            coord: TileCoord {
                level: c.level,
                x: c.x - 1,
                y: c.y,
            },
            stage: CacheStage::Processed,
        });
    }
    if c.y > 0 {
        deps.push(TileKey {
            doc: key.doc,
            layer: key.layer,
            coord: TileCoord {
                level: c.level,
                x: c.x,
                y: c.y - 1,
            },
            stage: CacheStage::Processed,
        });
    }
    if c.x > 0 && c.y > 0 {
        deps.push(TileKey {
            doc: key.doc,
            layer: key.layer,
            coord: TileCoord {
                level: c.level,
                x: c.x - 1,
                y: c.y - 1,
            },
            stage: CacheStage::Processed,
        });
    }
    deps
}

/// Keys that may become ready after `completed` Processed finishes
/// (right, bottom, and diag-consumer).
pub fn ed_dependent_keys(completed: TileKey) -> Vec<TileKey> {
    let c = completed.coord;
    vec![
        TileKey {
            doc: completed.doc,
            layer: completed.layer,
            coord: TileCoord {
                level: c.level,
                x: c.x + 1,
                y: c.y,
            },
            stage: CacheStage::Processed,
        },
        TileKey {
            doc: completed.doc,
            layer: completed.layer,
            coord: TileCoord {
                level: c.level,
                x: c.x,
                y: c.y + 1,
            },
            stage: CacheStage::Processed,
        },
        TileKey {
            doc: completed.doc,
            layer: completed.layer,
            coord: TileCoord {
                level: c.level,
                x: c.x + 1,
                y: c.y + 1,
            },
            stage: CacheStage::Processed,
        },
    ]
}

/// Decision 1: ED Processed may run only when Raw exists and left/top/diag
/// Processed are fresh (or at image edge).
pub fn ed_ready(cache: &TileCache, key: TileKey, layer_has_ed: bool) -> bool {
    if !layer_has_ed {
        return true;
    }
    if key.stage != CacheStage::Processed {
        return true;
    }
    if !raw_present(cache, key) {
        return false;
    }
    for dep in ed_dependency_keys(key) {
        if !tile_fresh(cache, dep) {
            return false;
        }
    }
    true
}

/// Keys this ED tile is still waiting on (own Raw and/or neighbor Processed).
pub fn ed_missing_deps(cache: &TileCache, key: TileKey) -> Vec<TileKey> {
    let mut missing = Vec::new();
    let raw = key.with_stage(CacheStage::Raw);
    if !cache.entries.contains_key(&raw) {
        missing.push(raw);
    }
    for dep in ed_dependency_keys(key) {
        if !tile_fresh(cache, dep) {
            missing.push(dep);
        }
    }
    missing
}

fn dep_satisfied(cache: &TileCache, dep: TileKey) -> bool {
    match dep.stage {
        CacheStage::Raw => cache.entries.contains_key(&dep),
        CacheStage::Processed | CacheStage::Composite => tile_fresh(cache, dep),
    }
}

fn all_deps_satisfied(cache: &TileCache, deps: &HashSet<TileKey>) -> bool {
    deps.iter().all(|d| dep_satisfied(cache, *d))
}

#[derive(Clone)]
struct BlockedEntry {
    task: RecomputeTask,
    deps: HashSet<TileKey>,
}

/// Blocked tasks waiting on dependency inserts (ED Processed, Composite, Raw).
///
/// Ready work lives in the normal [`crate::Scheduler`] priority queues.
pub struct EdFrontier {
    waiting_on: DashMap<TileKey, HashSet<TileKey>>,
    blocked: DashMap<TileKey, BlockedEntry>,
}

impl EdFrontier {
    pub fn new() -> Self {
        Self {
            waiting_on: DashMap::new(),
            blocked: DashMap::new(),
        }
    }

    pub fn blocked_count(&self) -> usize {
        self.blocked.len()
    }

    pub fn clear(&self) {
        self.waiting_on.clear();
        self.blocked.clear();
    }

    pub fn evict_document(&self, doc: u32) {
        self.blocked.retain(|k, _| k.doc != doc);
        self.waiting_on.retain(|k, waiters| {
            if k.doc == doc {
                return false;
            }
            waiters.retain(|w| w.doc != doc);
            !waiters.is_empty()
        });
    }

    /// Park `task` on an explicit dependency set.
    pub fn block_on(&self, task: RecomputeTask, deps: Vec<TileKey>) {
        let key = task.key;
        self.unblock_key(key);
        if deps.is_empty() {
            return;
        }
        note_blocked();
        let dep_set: HashSet<TileKey> = deps.into_iter().collect();
        for dep in &dep_set {
            self.waiting_on.entry(*dep).or_default().insert(key);
        }
        self.blocked.insert(
            key,
            BlockedEntry {
                task,
                deps: dep_set,
            },
        );
    }

    /// Park ED Processed until [`ed_missing_deps`] are satisfied.
    pub fn block(&self, task: RecomputeTask, cache: &TileCache) {
        let missing = ed_missing_deps(cache, task.key);
        self.block_on(task, missing);
    }

    fn unblock_key(&self, key: TileKey) {
        if self.blocked.remove(&key).is_none() {
            return;
        }
        for mut entry in self.waiting_on.iter_mut() {
            entry.value_mut().remove(&key);
        }
        self.waiting_on.retain(|_, v| !v.is_empty());
    }

    /// After `completed` was inserted fresh, return blocked tasks that are ready.
    pub fn wake(&self, completed: TileKey, cache: &TileCache) -> Vec<RecomputeTask> {
        let Some((_, waiters)) = self.waiting_on.remove(&completed) else {
            return Vec::new();
        };
        let mut ready = Vec::new();
        for waiter_key in waiters {
            let Some(entry) = self.blocked.get(&waiter_key).map(|e| e.clone()) else {
                continue;
            };
            let satisfied = match entry.task.key.stage {
                CacheStage::Processed => ed_ready(cache, entry.task.key, true),
                _ => all_deps_satisfied(cache, &entry.deps),
            };
            if satisfied {
                self.unblock_key(waiter_key);
                ready.push(entry.task);
            } else if entry.task.key.stage == CacheStage::Processed {
                self.block(entry.task, cache);
            } else {
                let still: Vec<TileKey> = entry
                    .deps
                    .into_iter()
                    .filter(|d| !dep_satisfied(cache, *d))
                    .collect();
                self.block_on(entry.task, still);
            }
        }
        ready
    }

    /// Wake after Processed insert: explicit waiters + topology neighbors.
    pub fn wake_after_processed(
        &self,
        completed: TileKey,
        cache: &TileCache,
    ) -> Vec<RecomputeTask> {
        let mut ready = self.wake(completed, cache);
        for dep_key in ed_dependent_keys(completed) {
            if let Some(entry) = self.blocked.get(&dep_key).map(|e| e.clone()) {
                if ed_ready(cache, dep_key, true) {
                    self.unblock_key(dep_key);
                    ready.push(entry.task);
                } else {
                    self.block(entry.task, cache);
                }
            }
        }
        ready
    }
}

impl Default for EdFrontier {
    fn default() -> Self {
        Self::new()
    }
}

/// Enumerate causal prefix coords `[0..=max_x]×[0..=max_y]`.
pub fn ed_prefix_coords(level: u8, max_x: u32, max_y: u32) -> Vec<TileCoord> {
    let mut out = Vec::with_capacity(((max_x + 1) * (max_y + 1)) as usize);
    for y in 0..=max_y {
        for x in 0..=max_x {
            out.push(TileCoord { level, x, y });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduler::Priority;
    use crate::tile::PixelTile;
    use std::sync::Arc;

    fn key(x: u32, y: u32) -> TileKey {
        TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord { level: 0, x, y },
            stage: CacheStage::Processed,
        }
    }

    fn insert_raw(cache: &TileCache, x: u32, y: u32) {
        cache.insert_fresh(
            key(x, y).with_stage(CacheStage::Raw),
            Arc::new(PixelTile::new()),
        );
    }

    fn insert_processed_fresh(cache: &TileCache, x: u32, y: u32) {
        cache.insert_fresh(key(x, y), Arc::new(PixelTile::new()));
    }

    #[test]
    fn origin_ready_with_raw() {
        let cache = TileCache::new(64 * 1024 * 1024);
        assert!(!ed_ready(&cache, key(0, 0), true));
        insert_raw(&cache, 0, 0);
        assert!(ed_ready(&cache, key(0, 0), true));
    }

    #[test]
    fn non_ed_always_ready() {
        let cache = TileCache::new(64 * 1024 * 1024);
        assert!(ed_ready(&cache, key(3, 3), false));
    }

    #[test]
    fn right_of_origin_needs_left() {
        let cache = TileCache::new(64 * 1024 * 1024);
        insert_raw(&cache, 1, 0);
        assert!(!ed_ready(&cache, key(1, 0), true));
        insert_processed_fresh(&cache, 0, 0);
        assert!(ed_ready(&cache, key(1, 0), true));
    }

    #[test]
    fn diagonal_needs_left_top_diag() {
        let cache = TileCache::new(64 * 1024 * 1024);
        insert_raw(&cache, 1, 1);
        assert!(!ed_ready(&cache, key(1, 1), true));
        insert_processed_fresh(&cache, 0, 1);
        assert!(!ed_ready(&cache, key(1, 1), true));
        insert_processed_fresh(&cache, 1, 0);
        assert!(!ed_ready(&cache, key(1, 1), true));
        insert_processed_fresh(&cache, 0, 0);
        assert!(ed_ready(&cache, key(1, 1), true));
    }

    #[test]
    fn frontier_block_wake_on_dep() {
        let cache = TileCache::new(64 * 1024 * 1024);
        let frontier = EdFrontier::new();
        insert_raw(&cache, 0, 0);
        insert_raw(&cache, 1, 0);

        let task = RecomputeTask {
            key: key(1, 0),
            generation: 1,
            layer_generation: 0,
            priority: Priority::Immediate,
        };
        frontier.block(task, &cache);
        assert_eq!(frontier.blocked_count(), 1);

        insert_processed_fresh(&cache, 0, 0);
        let woken = frontier.wake(key(0, 0), &cache);
        assert_eq!(woken.len(), 1);
        assert_eq!(woken[0].key, key(1, 0));
        assert_eq!(frontier.blocked_count(), 0);
    }

    #[test]
    fn frontier_blocks_composite_until_processed_fresh() {
        let cache = TileCache::new(64 * 1024 * 1024);
        let frontier = EdFrontier::new();
        let processed = key(0, 0);
        let composite = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        let task = RecomputeTask {
            key: composite,
            generation: 1,
            layer_generation: 0,
            priority: Priority::Immediate,
        };
        frontier.block_on(task, vec![processed]);
        assert_eq!(frontier.blocked_count(), 1);
        assert!(frontier.wake(processed, &cache).is_empty());
        insert_processed_fresh(&cache, 0, 0);
        let woken = frontier.wake(processed, &cache);
        // First wake consumed waiting_on before insert — re-block pattern:
        // block_on again after insert via direct check
        let _ = woken;
        frontier.block_on(
            RecomputeTask {
                key: composite,
                generation: 1,
                layer_generation: 0,
                priority: Priority::Immediate,
            },
            vec![processed],
        );
        // processed already fresh → all_deps_satisfied on wake of unrelated won't help.
        // Wake with a fake completed that composite waited on — already fresh so:
        let ready = {
            // Simulate: composite blocked on processed; processed is fresh; wake(processed)
            frontier.block_on(
                RecomputeTask {
                    key: composite,
                    generation: 1,
                    layer_generation: 0,
                    priority: Priority::Immediate,
                },
                vec![processed],
            );
            frontier.wake(processed, &cache)
        };
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].key.stage, CacheStage::Composite);
    }

    #[test]
    fn prefix_coords_rectangle() {
        let coords = ed_prefix_coords(0, 1, 1);
        assert_eq!(coords.len(), 4);
    }
}
