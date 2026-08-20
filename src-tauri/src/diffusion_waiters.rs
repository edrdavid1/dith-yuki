//! Pending diffusion waiters — contract for silent-skip re-invalidation.
//!
//! When Dependency_Enforcement would skip left/top recursion because a neighbor
//! raw tile is absent, the current Processed key can be registered under that
//! raw key. When the raw later appears, waiters are woken (marked dirty /
//! rescheduled).
//!
//! Production wiring is optional (see Track A diagnosis); these helpers lock the
//! contract with unit tests even if the skip branch is currently unreachable.

use std::collections::HashMap;
use std::sync::Mutex;

use engine_tiles::TileKey;

/// Registry: missing raw key → Processed keys that computed with zero seed.
///
/// Mutex+HashMap (not DashMap) — src-tauri does not depend on dashmap directly;
/// contention is rare (only on silent-skip / raw load).
#[derive(Debug, Default)]
pub struct PendingDiffusionWaiters {
    map: Mutex<HashMap<TileKey, Vec<TileKey>>>,
}

impl PendingDiffusionWaiters {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    /// Register `waiter_processed` as waiting on `missing_raw`.
    /// Duplicate registrations are allowed (idempotent wake).
    pub fn register(&self, missing_raw: TileKey, waiter_processed: TileKey) {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(missing_raw).or_default().push(waiter_processed);
    }

    /// Remove and return all waiters for a newly loaded raw key.
    pub fn wake(&self, loaded_raw: &TileKey) -> Vec<TileKey> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        map.remove(loaded_raw).unwrap_or_default()
    }

    /// Drop all waiters (full document replace).
    pub fn clear(&self) {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        map.clear();
    }

    /// Number of distinct missing-raw keys with waiters (diagnostics / tests).
    pub fn pending_key_count(&self) -> usize {
        self.map.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

/// Diagnostic counter: times Dependency_Enforcement skipped recursion because
/// neighbor raw was absent from `tile_cache`.
#[derive(Debug, Default)]
pub struct DiffusionSkipCounter {
    inner: std::sync::atomic::AtomicU64,
}

impl DiffusionSkipCounter {
    pub fn new() -> Self {
        Self {
            inner: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn increment(&self) {
        self.inner
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.inner.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn reset(&self) {
        self.inner.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_tiles::{CacheStage, TileCoord};

    fn raw_key(x: u32, y: u32) -> TileKey {
        TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x, y },
            stage: CacheStage::Raw,
        }
    }

    fn processed_key(x: u32, y: u32) -> TileKey {
        TileKey {
            layer: 1,
            coord: TileCoord { level: 0, x, y },
            stage: CacheStage::Processed,
        }
    }

    #[test]
    fn register_then_wake_returns_waiter() {
        let waiters = PendingDiffusionWaiters::new();
        let missing = raw_key(0, 0);
        let waiter = processed_key(1, 0);

        waiters.register(missing, waiter);
        assert_eq!(waiters.pending_key_count(), 1);

        let woken = waiters.wake(&missing);
        assert_eq!(woken, vec![waiter]);
        assert_eq!(waiters.pending_key_count(), 0);

        // Second wake is empty
        assert!(waiters.wake(&missing).is_empty());
    }

    #[test]
    fn multiple_waiters_and_neighbors() {
        let waiters = PendingDiffusionWaiters::new();
        let left_raw = raw_key(0, 1);
        let top_raw = raw_key(1, 0);
        let a = processed_key(1, 1);

        waiters.register(left_raw, a);
        waiters.register(top_raw, a);

        let from_left = waiters.wake(&left_raw);
        assert_eq!(from_left, vec![a]);
        // Still waiting on top
        assert_eq!(waiters.pending_key_count(), 1);

        let from_top = waiters.wake(&top_raw);
        assert_eq!(from_top, vec![a]);
        assert_eq!(waiters.pending_key_count(), 0);
    }

    #[test]
    fn skip_counter_increments() {
        let c = DiffusionSkipCounter::new();
        assert_eq!(c.get(), 0);
        c.increment();
        c.increment();
        assert_eq!(c.get(), 2);
        c.reset();
        assert_eq!(c.get(), 0);
    }
}
