//! Priority-based task scheduler for tile recomputation.
//!
//! This module implements a work-stealing scheduler with 4 priority tiers for tile recomputation tasks.
//! For architecture details, see `tile-engine-architecture.md` §5.2–§5.3 (Scheduler and Priority).
//!
//! # Overview
//!
//! The `Scheduler` maintains four queues (one per priority level) and routes tasks to the appropriate queue
//! based on their priority. Worker threads dequeue tasks in priority order, ensuring high-priority work
//! is processed before lower-priority work.
//!
//! # Priority Levels
//!
//! - **Immediate**: Coarse pyramid levels for current viewport (highest priority)
//! - **ViewportCenter**: Highest-priority visible tiles at viewport center
//! - **ViewportEdge**: Lower-priority visible tiles at viewport edges
//! - **Prefetch**: Out-of-viewport tiles for smooth panning (lowest priority)
//!
//! # Task Abandonment
//!
//! Before recomputing a task, the scheduler should check:
//! - `task.generation == current_document_gen`
//! - `task.layer_generation == current_layer_gen[task.key.layer]`
//!
//! If either check fails, the task is discarded (user changed parameters; result would be stale).

use crate::{TileKey};
use crossbeam::queue::SegQueue;

/// Priority level for tile recomputation tasks.
///
/// Determines which queue a task is enqueued to and affects dequeue ordering.
/// Higher priorities are processed first by worker threads.
///
/// # Examples
///
/// ```ignore
/// let immediate = Priority::Immediate;
/// let prefetch = Priority::Prefetch;
/// assert!(immediate > prefetch);  // Immediate is higher priority
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    /// Prefetch tiles: out-of-viewport, lowest urgency.
    /// Processed only when no higher-priority work is available.
    Prefetch = 0,

    /// Viewport edge tiles: lower visibility/urgency.
    /// Processed after Immediate and ViewportCenter.
    ViewportEdge = 1,

    /// Viewport center tiles: high-priority visible tiles.
    /// Processed after Immediate, before ViewportEdge and Prefetch.
    ViewportCenter = 2,

    /// Immediate priority: coarse pyramid levels for current viewport.
    /// Processed first by all worker threads.
    Immediate = 3,
}

/// A task to recompute a single tile with version checking.
///
/// Carries the tile to recompute and generation values for staleness detection.
/// Before execution, the scheduler should verify:
/// - `generation == current_document_gen`
/// - `layer_generation == current_layer_gen[key.layer]`
///
/// If either value is stale, the task should be discarded without recomputation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RecomputeTask {
    /// The tile to recompute.
    pub key: TileKey,

    /// Document generation at task creation time.
    /// If current document generation differs, task is stale (discard).
    pub generation: u64,

    /// Layer generation at task creation time.
    /// If current layer generation differs, task is stale (discard).
    pub layer_generation: u64,

    /// Priority level; determines which queue the task is routed to.
    pub priority: Priority,
}

/// Priority-based work-stealing scheduler for tile recomputation.
///
/// Maintains four SegQueues, one per priority level. Tasks are dequeued
/// in priority order: high-priority work is always preferred over lower-priority work.
///
/// # Concurrency
///
/// This type is thread-safe. Multiple worker threads may simultaneously enqueue and dequeue tasks.
/// SegQueue guarantees lock-free concurrent access.
///
/// # Example
///
/// ```ignore
/// let scheduler = Scheduler::new();
///
/// // High-priority task
/// let task1 = RecomputeTask {
///     key: TileKey { /* ... */ },
///     generation: 0,
///     layer_generation: 0,
///     priority: Priority::Immediate,
/// };
///
/// // Low-priority task
/// let task2 = RecomputeTask {
///     key: TileKey { /* ... */ },
///     generation: 0,
///     layer_generation: 0,
///     priority: Priority::Prefetch,
/// };
///
/// scheduler.enqueue(task1);
/// scheduler.enqueue(task2);
///
/// // Dequeues task1 first (higher priority)
/// let first = scheduler.dequeue();
/// assert_eq!(first.unwrap().priority, Priority::Immediate);
///
/// // Then dequeues task2
/// let second = scheduler.dequeue();
/// assert_eq!(second.unwrap().priority, Priority::Prefetch);
/// ```
pub struct Scheduler {
    /// Queue for Immediate priority tasks.
    immediate_queue: SegQueue<RecomputeTask>,
    /// Queue for ViewportCenter priority tasks.
    viewport_center_queue: SegQueue<RecomputeTask>,
    /// Queue for ViewportEdge priority tasks.
    viewport_edge_queue: SegQueue<RecomputeTask>,
    /// Queue for Prefetch priority tasks.
    prefetch_queue: SegQueue<RecomputeTask>,
}

impl Scheduler {
    /// Create a new empty scheduler with four queues.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let scheduler = Scheduler::new();
    /// assert_eq!(scheduler.dequeue(), None);
    /// ```
    pub fn new() -> Self {
        Self {
            immediate_queue: SegQueue::new(),
            viewport_center_queue: SegQueue::new(),
            viewport_edge_queue: SegQueue::new(),
            prefetch_queue: SegQueue::new(),
        }
    }

    /// Enqueue a task into the appropriate priority queue.
    ///
    /// Routes the task to its priority-specific queue based on `task.priority`.
    ///
    /// # Arguments
    ///
    /// - `task`: The RecomputeTask to enqueue
    ///
    /// # Example
    ///
    /// ```ignore
    /// let scheduler = Scheduler::new();
    /// let task = RecomputeTask {
    ///     key: TileKey { /* ... */ },
    ///     generation: 0,
    ///     layer_generation: 0,
    ///     priority: Priority::ViewportCenter,
    /// };
    /// scheduler.enqueue(task);
    /// assert_eq!(scheduler.dequeue().map(|t| t.priority), Some(Priority::ViewportCenter));
    /// ```
    pub fn enqueue(&self, task: RecomputeTask) {
        match task.priority {
            Priority::Immediate => self.immediate_queue.push(task),
            Priority::ViewportCenter => self.viewport_center_queue.push(task),
            Priority::ViewportEdge => self.viewport_edge_queue.push(task),
            Priority::Prefetch => self.prefetch_queue.push(task),
        }
    }

    /// Dequeue the next task in priority order.
    ///
    /// Attempts to pop from queues in order of descending priority:
    /// 1. Immediate
    /// 2. ViewportCenter
    /// 3. ViewportEdge
    /// 4. Prefetch
    ///
    /// Returns the first task found, or None if all queues are empty.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let scheduler = Scheduler::new();
    ///
    /// let low = RecomputeTask {
    ///     key: TileKey { /* ... */ },
    ///     generation: 0,
    ///     layer_generation: 0,
    ///     priority: Priority::Prefetch,
    /// };
    /// let high = RecomputeTask {
    ///     key: TileKey { /* ... */ },
    ///     generation: 0,
    ///     layer_generation: 0,
    ///     priority: Priority::Immediate,
    /// };
    ///
    /// scheduler.enqueue(low);
    /// scheduler.enqueue(high);
    ///
    /// // First dequeue gets high-priority task
    /// assert_eq!(scheduler.dequeue().map(|t| t.priority), Some(Priority::Immediate));
    /// // Second dequeue gets low-priority task
    /// assert_eq!(scheduler.dequeue().map(|t| t.priority), Some(Priority::Prefetch));
    /// // Third dequeue gets None (empty)
    /// assert_eq!(scheduler.dequeue(), None);
    /// ```
    pub fn dequeue(&self) -> Option<RecomputeTask> {
        self.immediate_queue
            .pop()
            .or_else(|| self.viewport_center_queue.pop())
            .or_else(|| self.viewport_edge_queue.pop())
            .or_else(|| self.prefetch_queue.pop())
    }

    /// Drain all queues, discarding all pending tasks.
    ///
    /// This is used when the viewport changes to cancel stale tasks before
    /// re-scheduling with updated priorities. Since SegQueue doesn't support
    /// selective removal, we clear everything and re-enqueue what's needed.
    ///
    /// # Example
    ///
    /// ```ignore
    /// scheduler.enqueue(task1);
    /// scheduler.enqueue(task2);
    /// scheduler.clear_all();
    /// assert_eq!(scheduler.dequeue(), None);
    /// ```
    pub fn clear_all(&self) {
        while self.immediate_queue.pop().is_some() {}
        while self.viewport_center_queue.pop().is_some() {}
        while self.viewport_edge_queue.pop().is_some() {}
        while self.prefetch_queue.pop().is_some() {}
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
    use crate::{TileCoord, CacheStage};

    fn make_task(priority: Priority, layer: u32, x: u32, y: u32) -> RecomputeTask {
        RecomputeTask {
            key: TileKey {
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

        // Enqueue in random order
        scheduler.enqueue(make_task(Priority::Prefetch, 0, 0, 0));
        scheduler.enqueue(make_task(Priority::Immediate, 0, 1, 0));
        scheduler.enqueue(make_task(Priority::ViewportEdge, 0, 2, 0));
        scheduler.enqueue(make_task(Priority::ViewportCenter, 0, 3, 0));

        // Dequeue in priority order
        let task1 = scheduler.dequeue().unwrap();
        assert_eq!(task1.priority, Priority::Immediate);

        let task2 = scheduler.dequeue().unwrap();
        assert_eq!(task2.priority, Priority::ViewportCenter);

        let task3 = scheduler.dequeue().unwrap();
        assert_eq!(task3.priority, Priority::ViewportEdge);

        let task4 = scheduler.dequeue().unwrap();
        assert_eq!(task4.priority, Priority::Prefetch);

        // Queue is empty
        let task5 = scheduler.dequeue();
        assert_eq!(task5, None);
    }

    #[test]
    fn dequeue_prefers_higher_priority_across_multiple_queues() {
        let scheduler = Scheduler::new();

        // Enqueue multiple prefetch tasks first
        scheduler.enqueue(make_task(Priority::Prefetch, 0, 0, 0));
        scheduler.enqueue(make_task(Priority::Prefetch, 0, 1, 0));

        // Enqueue a high-priority task
        scheduler.enqueue(make_task(Priority::Immediate, 0, 2, 0));

        // Should dequeue high-priority first
        let task1 = scheduler.dequeue().unwrap();
        assert_eq!(task1.priority, Priority::Immediate);

        // Then low-priority tasks
        let task2 = scheduler.dequeue().unwrap();
        assert_eq!(task2.priority, Priority::Prefetch);

        let task3 = scheduler.dequeue().unwrap();
        assert_eq!(task3.priority, Priority::Prefetch);
    }

    #[test]
    fn priority_enum_ordering() {
        assert!(Priority::Immediate > Priority::ViewportCenter);
        assert!(Priority::ViewportCenter > Priority::ViewportEdge);
        assert!(Priority::ViewportEdge > Priority::Prefetch);
        assert_eq!(Priority::Immediate, Priority::Immediate);
    }

    #[test]
    fn recompute_task_carries_generations() {
        let task = RecomputeTask {
            key: TileKey {
                layer: 5,
                coord: TileCoord {
                    level: 0,
                    x: 10,
                    y: 20,
                },
                stage: CacheStage::Processed,
            },
            generation: 42,
            layer_generation: 99,
            priority: Priority::ViewportCenter,
        };

        assert_eq!(task.generation, 42);
        assert_eq!(task.layer_generation, 99);
        assert_eq!(task.key.layer, 5);
    }

    #[test]
    fn multiple_tasks_same_priority() {
        let scheduler = Scheduler::new();

        let task1 = make_task(Priority::ViewportCenter, 0, 0, 0);
        let task2 = make_task(Priority::ViewportCenter, 0, 1, 0);
        let task3 = make_task(Priority::ViewportCenter, 0, 2, 0);

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);
        scheduler.enqueue(task3);

        let dequeue1 = scheduler.dequeue().unwrap();
        let dequeue2 = scheduler.dequeue().unwrap();
        let dequeue3 = scheduler.dequeue().unwrap();

        // All are ViewportCenter priority (order within same priority is FIFO)
        assert_eq!(dequeue1.priority, Priority::ViewportCenter);
        assert_eq!(dequeue2.priority, Priority::ViewportCenter);
        assert_eq!(dequeue3.priority, Priority::ViewportCenter);

        // Verify they're different tasks (FIFO order)
        assert_eq!(dequeue1.key.coord.x, 0);
        assert_eq!(dequeue2.key.coord.x, 1);
        assert_eq!(dequeue3.key.coord.x, 2);
    }

    #[test]
    fn empty_scheduler_returns_none() {
        let scheduler = Scheduler::new();
        assert_eq!(scheduler.dequeue(), None);
        assert_eq!(scheduler.dequeue(), None);
    }

    #[test]
    fn clear_all_drains_all_queues() {
        let scheduler = Scheduler::new();

        scheduler.enqueue(make_task(Priority::Immediate, 0, 0, 0));
        scheduler.enqueue(make_task(Priority::ViewportCenter, 0, 1, 0));
        scheduler.enqueue(make_task(Priority::ViewportEdge, 0, 2, 0));
        scheduler.enqueue(make_task(Priority::Prefetch, 0, 3, 0));

        scheduler.clear_all();

        assert_eq!(scheduler.dequeue(), None);
    }

    #[test]
    fn clear_all_on_empty_scheduler_is_noop() {
        let scheduler = Scheduler::new();
        scheduler.clear_all();
        assert_eq!(scheduler.dequeue(), None);
    }
}

