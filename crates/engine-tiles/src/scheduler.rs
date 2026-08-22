//! Priority-based task scheduler for tile recomputation.
//!
//! Canonical pending set is a [`DashMap`] keyed by [`TileKey`]. Four
//! [`SegQueue`] lanes hold **key hints** only — priority bumps update the map
//! in place and push a higher-lane hint, without duplicating task payloads.

use crate::TileKey;
use crossbeam::queue::SegQueue;
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Priority level for tile recomputation tasks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    Prefetch = 0,
    ViewportEdge = 1,
    ViewportCenter = 2,
    Immediate = 3,
}

/// A task to recompute a single tile with version checking.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecomputeTask {
    pub key: TileKey,
    pub generation: u64,
    pub layer_generation: u64,
    pub priority: Priority,
}

/// Priority-based scheduler: one pending task per key, lane hints for dequeue order.
pub struct Scheduler {
    pending: DashMap<TileKey, RecomputeTask>,
    immediate: SegQueue<TileKey>,
    viewport_center: SegQueue<TileKey>,
    viewport_edge: SegQueue<TileKey>,
    prefetch: SegQueue<TileKey>,
    /// Approximate pending count (Vacant insert +1, successful dequeue −1).
    queued_count: AtomicUsize,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            pending: DashMap::new(),
            immediate: SegQueue::new(),
            viewport_center: SegQueue::new(),
            viewport_edge: SegQueue::new(),
            prefetch: SegQueue::new(),
            queued_count: AtomicUsize::new(0),
        }
    }

    /// Best priority currently recorded for `key` (if pending).
    pub fn queued_priority_of(&self, key: &TileKey) -> Option<Priority> {
        self.pending.get(key).map(|t| t.priority)
    }

    pub fn enqueue(&self, task: RecomputeTask) {
        let _ = self.enqueue_or_bump(task);
    }

    /// Enqueue unless this key is already pending at the same or a newer generation.
    pub fn enqueue_dedup(&self, task: RecomputeTask) -> bool {
        if let Some(cur) = self.pending.get(&task.key) {
            if cur.generation >= task.generation {
                return false;
            }
        }
        self.upsert(task)
    }

    /// Insert or raise priority via `max` (ED inheritance). Never lowers priority.
    pub fn enqueue_or_bump(&self, task: RecomputeTask) -> bool {
        if let Some(cur) = self.pending.get(&task.key) {
            if task.generation < cur.generation {
                return false;
            }
            if task.generation == cur.generation && task.priority <= cur.priority {
                return false;
            }
        }
        self.upsert(task)
    }

    pub fn contains_key(&self, key: &TileKey) -> bool {
        self.pending.contains_key(key)
    }

    pub fn queued_len(&self) -> usize {
        self.queued_count.load(Ordering::Acquire)
    }

    fn push_hint(&self, key: TileKey, priority: Priority) {
        match priority {
            Priority::Immediate => self.immediate.push(key),
            Priority::ViewportCenter => self.viewport_center.push(key),
            Priority::ViewportEdge => self.viewport_edge.push(key),
            Priority::Prefetch => self.prefetch.push(key),
        }
    }

    /// Insert/replace pending and emit a lane hint when the key is new or priority rises.
    fn upsert(&self, task: RecomputeTask) -> bool {
        match self.pending.entry(task.key) {
            Entry::Vacant(v) => {
                v.insert(task);
                self.queued_count.fetch_add(1, Ordering::Release);
                self.push_hint(task.key, task.priority);
                true
            }
            Entry::Occupied(mut occ) => {
                let prev = *occ.get();
                let pri_raised = task.priority > prev.priority;
                let gen_newer = task.generation > prev.generation;
                let merged = RecomputeTask {
                    key: task.key,
                    generation: task.generation.max(prev.generation),
                    layer_generation: if gen_newer {
                        task.layer_generation
                    } else {
                        prev.layer_generation
                    },
                    priority: std::cmp::max(task.priority, prev.priority),
                };
                occ.insert(merged);
                if pri_raised || gen_newer {
                    self.push_hint(merged.key, merged.priority);
                }
                true
            }
        }
    }

    /// Dequeue the next pending task in priority order.
    ///
    /// Stale lane hints (after bump or prior dequeue) are skipped.
    pub fn dequeue(&self) -> Option<RecomputeTask> {
        loop {
            let key = self
                .immediate
                .pop()
                .or_else(|| self.viewport_center.pop())
                .or_else(|| self.viewport_edge.pop())
                .or_else(|| self.prefetch.pop())?;

            let Some((_, task)) = self.pending.remove(&key) else {
                // Hint for a key already taken or never inserted — ignore.
                continue;
            };
            self.queued_count.fetch_sub(1, Ordering::Release);
            return Some(task);
        }
    }

    pub fn clear_all(&self) {
        while self.immediate.pop().is_some() {}
        while self.viewport_center.pop().is_some() {}
        while self.viewport_edge.pop().is_some() {}
        while self.prefetch.pop().is_some() {}
        self.pending.clear();
        self.queued_count.store(0, Ordering::Release);
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CacheStage, TileCoord};

    fn make_task(priority: Priority, layer: u32, x: u32, y: u32) -> RecomputeTask {
        RecomputeTask {
            key: TileKey {
                doc: 1,
                layer,
                coord: TileCoord {
                    level: 0,
                    x,
                    y,
                },
                stage: CacheStage::Raw,
            },
            generation: 0,
            layer_generation: 0,
            priority,
        }
    }

    #[test]
    fn scheduler_new_creates_empty_scheduler() {
        let scheduler = Scheduler::new();
        assert_eq!(scheduler.dequeue(), None);
    }

    #[test]
    fn enqueue_adds_task_to_queue() {
        let scheduler = Scheduler::new();
        let task = make_task(Priority::Immediate, 0, 0, 0);
        scheduler.enqueue(task);
        let dequeued = scheduler.dequeue();
        assert!(dequeued.is_some());
        assert_eq!(dequeued.unwrap().priority, Priority::Immediate);
    }

    #[test]
    fn dequeue_respects_priority_order() {
        let scheduler = Scheduler::new();
        scheduler.enqueue(make_task(Priority::Prefetch, 0, 0, 0));
        scheduler.enqueue(make_task(Priority::Immediate, 0, 1, 0));
        scheduler.enqueue(make_task(Priority::ViewportEdge, 0, 2, 0));
        scheduler.enqueue(make_task(Priority::ViewportCenter, 0, 3, 0));

        assert_eq!(
            scheduler.dequeue().unwrap().priority,
            Priority::Immediate
        );
        assert_eq!(
            scheduler.dequeue().unwrap().priority,
            Priority::ViewportCenter
        );
        assert_eq!(
            scheduler.dequeue().unwrap().priority,
            Priority::ViewportEdge
        );
        assert_eq!(scheduler.dequeue().unwrap().priority, Priority::Prefetch);
        assert_eq!(scheduler.dequeue(), None);
    }

    #[test]
    fn enqueue_dedup_skips_same_key_same_or_older_generation() {
        let scheduler = Scheduler::new();
        let mut a = make_task(Priority::Immediate, 0, 0, 0);
        a.generation = 2;
        a.key.stage = CacheStage::Composite;
        let mut b = a;
        b.generation = 2;
        let mut older = a;
        older.generation = 1;

        assert!(scheduler.enqueue_dedup(a));
        assert!(!scheduler.enqueue_dedup(b));
        assert!(!scheduler.enqueue_dedup(older));
        assert_eq!(scheduler.queued_len(), 1);
        assert!(scheduler.contains_key(&a.key));

        let dequeued = scheduler.dequeue().unwrap();
        assert_eq!(dequeued.generation, 2);
        assert_eq!(scheduler.dequeue(), None);
        assert!(!scheduler.contains_key(&a.key));
    }

    #[test]
    fn enqueue_dedup_allows_newer_generation() {
        let scheduler = Scheduler::new();
        let mut a = make_task(Priority::Immediate, 0, 0, 0);
        a.generation = 1;
        a.key.stage = CacheStage::Composite;
        let mut newer = a;
        newer.generation = 2;

        assert!(scheduler.enqueue_dedup(a));
        assert!(scheduler.enqueue_dedup(newer));
        assert_eq!(scheduler.queued_len(), 1);

        let only = scheduler.dequeue().unwrap();
        assert_eq!(only.generation, 2);
        assert_eq!(scheduler.dequeue(), None);
    }

    #[test]
    fn enqueue_or_bump_raises_priority_without_duplicate_payloads() {
        let scheduler = Scheduler::new();
        let mut low = make_task(Priority::Prefetch, 0, 0, 0);
        low.generation = 1;
        low.key.stage = CacheStage::Processed;
        assert!(scheduler.enqueue_or_bump(low));
        assert_eq!(
            scheduler.queued_priority_of(&low.key),
            Some(Priority::Prefetch)
        );

        let mut high = low;
        high.priority = Priority::Immediate;
        assert!(scheduler.enqueue_or_bump(high));
        assert_eq!(
            scheduler.queued_priority_of(&low.key),
            Some(Priority::Immediate)
        );
        assert_eq!(scheduler.queued_len(), 1);

        let mut lower_again = low;
        lower_again.priority = Priority::ViewportEdge;
        assert!(!scheduler.enqueue_or_bump(lower_again));

        let first = scheduler.dequeue().unwrap();
        assert_eq!(first.priority, Priority::Immediate);
        // Stale Prefetch hint must not yield a second task.
        assert_eq!(scheduler.dequeue(), None);
        assert_eq!(scheduler.queued_len(), 0);
    }

    #[test]
    fn clear_all_drains() {
        let scheduler = Scheduler::new();
        scheduler.enqueue(make_task(Priority::Immediate, 0, 0, 0));
        scheduler.enqueue(make_task(Priority::Prefetch, 0, 1, 0));
        scheduler.clear_all();
        assert_eq!(scheduler.dequeue(), None);
        assert_eq!(scheduler.queued_len(), 0);
    }

    #[test]
    fn multiple_tasks_same_priority_fifo_by_hint() {
        let scheduler = Scheduler::new();
        scheduler.enqueue(make_task(Priority::ViewportCenter, 0, 0, 0));
        scheduler.enqueue(make_task(Priority::ViewportCenter, 0, 1, 0));
        scheduler.enqueue(make_task(Priority::ViewportCenter, 0, 2, 0));
        assert_eq!(scheduler.dequeue().unwrap().key.coord.x, 0);
        assert_eq!(scheduler.dequeue().unwrap().key.coord.x, 1);
        assert_eq!(scheduler.dequeue().unwrap().key.coord.x, 2);
    }
}
