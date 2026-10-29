//! P0 counters for Post/barrier invalidation (`.local-doc/post.md`).
//!
//! Always-on atomics + info logs. No behavior change.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::filter::FilterInstance;
use crate::post_barrier::{
    barrier_index, classify_change, prefix_hash_through_barrier, ChangeClass, PrefixExternals,
};

static CLASSIFY_POST: AtomicU64 = AtomicU64::new(0);
static CLASSIFY_BARRIER: AtomicU64 = AtomicU64::new(0);
static CLASSIFY_PRE: AtomicU64 = AtomicU64::new(0);
static CLEAR_RESIDUALS: AtomicU64 = AtomicU64::new(0);
static CLEAR_SKIPPED: AtomicU64 = AtomicU64::new(0);
static NEUTRAL_APPLY_SKIPPED: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy)]
pub struct PostBarrierCounters {
    pub classify_post: u64,
    pub classify_barrier: u64,
    pub classify_pre: u64,
    pub clear_residuals: u64,
    pub clear_skipped: u64,
    pub neutral_apply_skipped: u64,
}

pub fn snapshot() -> PostBarrierCounters {
    PostBarrierCounters {
        classify_post: CLASSIFY_POST.load(Ordering::Relaxed),
        classify_barrier: CLASSIFY_BARRIER.load(Ordering::Relaxed),
        classify_pre: CLASSIFY_PRE.load(Ordering::Relaxed),
        clear_residuals: CLEAR_RESIDUALS.load(Ordering::Relaxed),
        clear_skipped: CLEAR_SKIPPED.load(Ordering::Relaxed),
        neutral_apply_skipped: NEUTRAL_APPLY_SKIPPED.load(Ordering::Relaxed),
    }
}

pub fn record_neutral_apply_skip() {
    NEUTRAL_APPLY_SKIPPED.fetch_add(1, Ordering::Relaxed);
}

pub fn record_clear(did_clear: bool) {
    if did_clear {
        CLEAR_RESIDUALS.fetch_add(1, Ordering::Relaxed);
    } else {
        CLEAR_SKIPPED.fetch_add(1, Ordering::Relaxed);
    }
}

/// Index of `filter_id` in panel order, if present.
pub fn filter_index(
    filters: &[FilterInstance],
    filter_id: crate::types::FilterInstanceId,
) -> Option<usize> {
    filters.iter().position(|f| f.id == filter_id)
}

/// Classify + log a stack edit. Returns `(ChangeClass, prefix_hash, barrier_index)`.
pub fn note_stack_change(
    layer_id: u32,
    old: &[FilterInstance],
    new: &[FilterInstance],
    changed_index: Option<usize>,
    clear_residuals: bool,
    externals: &PrefixExternals,
) -> ChangeClass {
    let class = classify_change(old, new);
    match class {
        ChangeClass::Post => {
            CLASSIFY_POST.fetch_add(1, Ordering::Relaxed);
        }
        ChangeClass::Barrier => {
            CLASSIFY_BARRIER.fetch_add(1, Ordering::Relaxed);
        }
        ChangeClass::Pre => {
            CLASSIFY_PRE.fetch_add(1, Ordering::Relaxed);
        }
    }
    let prefix = prefix_hash_through_barrier(new, externals);
    let barrier = barrier_index(new);
    log::info!(
        target: "post_barrier",
        "layer={} classify={:?} changed_index={:?} barrier_index={:?} prefix={:016x} clear_residuals={} (P1: clear rule unchanged)",
        layer_id,
        class,
        changed_index,
        barrier,
        prefix,
        clear_residuals
    );
    class
}
