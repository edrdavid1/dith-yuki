//! Preservation property tests for the Scheduler.
//!
//! These tests verify behavior that MUST NOT change after the bugfix:
//! 1. Cross-bucket priority ordering: Immediate > ViewportCenter > ViewportEdge > Prefetch
//! 2. FIFO within same priority bucket: tasks enqueued in order are dequeued in same order
//!
//! **Validates: Requirements 3.4, 3.5**
//!
//! These tests MUST PASS on unfixed code (scheduler behavior is already correct).

use engine_tiles::{CacheStage, Priority, RecomputeTask, Scheduler, TileCoord, TileKey};
use proptest::prelude::*;

/// Strategy to generate a random Priority value.
fn arb_priority() -> impl Strategy<Value = Priority> {
    prop_oneof![
        Just(Priority::Immediate),
        Just(Priority::ViewportCenter),
        Just(Priority::ViewportEdge),
        Just(Priority::Prefetch),
    ]
}

/// Strategy to generate a random RecomputeTask with a given priority.
fn arb_task_with_priority(priority: Priority) -> impl Strategy<Value = RecomputeTask> {
    (0..100u32, 0..16u32, 0..16u32, 0..4u8).prop_map(move |(layer, x, y, level)| {
        RecomputeTask {
            key: TileKey {
                doc: 1,
                layer,
                coord: TileCoord { level, x, y },
                stage: CacheStage::Composite,
            },
            generation: 0,
            layer_generation: 0,
            priority,
        }
    })
}

/// Strategy to generate a random RecomputeTask with random priority.
fn arb_task() -> impl Strategy<Value = RecomputeTask> {
    arb_priority().prop_flat_map(|p| arb_task_with_priority(p))
}

/// Helper: numeric priority value for ordering assertions.
fn priority_value(p: Priority) -> u8 {
    match p {
        Priority::Prefetch => 0,
        Priority::ViewportEdge => 1,
        Priority::ViewportCenter => 2,
        Priority::Immediate => 3,
    }
}

proptest! {
    /// Property: Cross-bucket priority ordering is always respected.
    ///
    /// For any random mix of 1–50 tasks across the 4 priority buckets,
    /// dequeue order always respects: Immediate > ViewportCenter > ViewportEdge > Prefetch.
    ///
    /// **Validates: Requirements 3.4**
    #[test]
    fn cross_bucket_priority_ordering(
        tasks in prop::collection::vec(arb_task(), 1..50)
    ) {
        let scheduler = Scheduler::new();

        for task in &tasks {
            scheduler.enqueue(*task);
        }

        // Dequeue all tasks and verify priority ordering is non-increasing
        let mut dequeued: Vec<RecomputeTask> = Vec::new();
        while let Some(task) = scheduler.dequeue() {
            dequeued.push(task);
        }

        // Verify all tasks were dequeued
        prop_assert_eq!(dequeued.len(), tasks.len());

        // Verify priority ordering: each task's priority must be >= the next task's priority
        for i in 0..dequeued.len().saturating_sub(1) {
            let current_prio = priority_value(dequeued[i].priority);
            let next_prio = priority_value(dequeued[i + 1].priority);
            prop_assert!(
                current_prio >= next_prio,
                "Priority ordering violated at position {}: {:?} (value {}) followed by {:?} (value {})",
                i,
                dequeued[i].priority,
                current_prio,
                dequeued[i + 1].priority,
                next_prio,
            );
        }
    }

    /// Property: FIFO within same priority bucket.
    ///
    /// For any 3 tasks with the same priority, dequeue order matches enqueue order.
    /// This verifies that intra-bucket ordering is preserved (SegQueue FIFO).
    ///
    /// **Validates: Requirements 3.4**
    #[test]
    fn fifo_within_same_priority_bucket(
        priority in arb_priority(),
        x1 in 0..16u32,
        y1 in 0..16u32,
        x2 in 0..16u32,
        y2 in 0..16u32,
        x3 in 0..16u32,
        y3 in 0..16u32,
    ) {
        let scheduler = Scheduler::new();

        let task1 = RecomputeTask {
            key: TileKey {
                doc: 1,
                layer: 0,
                coord: TileCoord { level: 0, x: x1, y: y1 },
                stage: CacheStage::Composite,
            },
            generation: 1,
            layer_generation: 0,
            priority,
        };
        let task2 = RecomputeTask {
            key: TileKey {
                doc: 1,
                layer: 0,
                coord: TileCoord { level: 0, x: x2, y: y2 },
                stage: CacheStage::Composite,
            },
            generation: 2,
            layer_generation: 0,
            priority,
        };
        let task3 = RecomputeTask {
            key: TileKey {
                doc: 1,
                layer: 0,
                coord: TileCoord { level: 0, x: x3, y: y3 },
                stage: CacheStage::Composite,
            },
            generation: 3,
            layer_generation: 0,
            priority,
        };

        scheduler.enqueue(task1);
        scheduler.enqueue(task2);
        scheduler.enqueue(task3);

        let d1 = scheduler.dequeue().unwrap();
        let d2 = scheduler.dequeue().unwrap();
        let d3 = scheduler.dequeue().unwrap();

        // FIFO: dequeue order matches enqueue order (using generation as discriminator)
        prop_assert_eq!(d1.generation, 1, "First dequeued should be generation 1");
        prop_assert_eq!(d2.generation, 2, "Second dequeued should be generation 2");
        prop_assert_eq!(d3.generation, 3, "Third dequeued should be generation 3");
    }
}
