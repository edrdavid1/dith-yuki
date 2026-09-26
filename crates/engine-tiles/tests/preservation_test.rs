//! Property tests for scheduler invariants after the DashMap / lane-hint redesign.
//!
//! The scheduler keeps **one pending task per [`TileKey`]**. Lane SegQueues hold
//! key hints only; duplicate enqueues merge via `enqueue_or_bump`.
//!
//! Preserved product invariants:
//! 1. Cross-bucket priority: Immediate > ViewportCenter > ViewportEdge > Prefetch
//! 2. FIFO among **distinct** keys in the same priority lane

use engine_tiles::{CacheStage, Priority, RecomputeTask, Scheduler, TileCoord, TileKey};
use proptest::prelude::*;
use std::collections::HashSet;

fn arb_priority() -> impl Strategy<Value = Priority> {
    prop_oneof![
        Just(Priority::Immediate),
        Just(Priority::ViewportCenter),
        Just(Priority::ViewportEdge),
        Just(Priority::Prefetch),
    ]
}

fn arb_task_with_priority(priority: Priority) -> impl Strategy<Value = RecomputeTask> {
    (0..100u32, 0..16u32, 0..16u32, 0..4u8).prop_map(move |(layer, x, y, level)| RecomputeTask {
        key: TileKey {
            doc: 1,
            layer,
            coord: TileCoord { level, x, y },
            stage: CacheStage::Composite,
        },
        generation: 0,
        layer_generation: 0,
        priority,
    })
}

fn arb_task() -> impl Strategy<Value = RecomputeTask> {
    arb_priority().prop_flat_map(arb_task_with_priority)
}

fn priority_value(p: Priority) -> u8 {
    match p {
        Priority::Prefetch => 0,
        Priority::ViewportEdge => 1,
        Priority::ViewportCenter => 2,
        Priority::Immediate => 3,
    }
}

proptest! {
    /// Dequeue order respects priority buckets for the **deduped** pending set.
    #[test]
    fn cross_bucket_priority_ordering(tasks in prop::collection::vec(arb_task(), 1..50)) {
        let scheduler = Scheduler::new();
        let mut unique_keys = HashSet::new();
        for task in &tasks {
            unique_keys.insert(task.key);
            scheduler.enqueue(*task);
        }

        let mut dequeued: Vec<RecomputeTask> = Vec::new();
        while let Some(task) = scheduler.dequeue() {
            dequeued.push(task);
        }

        prop_assert_eq!(dequeued.len(), unique_keys.len());

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

    /// Distinct keys at the same priority dequeue in enqueue order (SegQueue FIFO).
    #[test]
    fn fifo_within_same_priority_bucket(
        priority in arb_priority(),
        coords in prop::collection::vec((0..16u32, 0..16u32), 3..=3)
            .prop_filter("distinct keys", |c| {
                c[0] != c[1] && c[0] != c[2] && c[1] != c[2]
            }),
    ) {
        let scheduler = Scheduler::new();
        let mut expected_gens = Vec::new();

        for (i, (x, y)) in coords.iter().enumerate() {
            let gen = (i as u64) + 1;
            expected_gens.push(gen);
            scheduler.enqueue(RecomputeTask {
                key: TileKey {
                    doc: 1,
                    layer: 0,
                    coord: TileCoord { level: 0, x: *x, y: *y },
                    stage: CacheStage::Composite,
                },
                generation: gen,
                layer_generation: 0,
                priority,
            });
        }

        let d1 = scheduler.dequeue().expect("first");
        let d2 = scheduler.dequeue().expect("second");
        let d3 = scheduler.dequeue().expect("third");

        prop_assert_eq!(d1.generation, expected_gens[0]);
        prop_assert_eq!(d2.generation, expected_gens[1]);
        prop_assert_eq!(d3.generation, expected_gens[2]);
        prop_assert!(scheduler.dequeue().is_none());
    }
}
