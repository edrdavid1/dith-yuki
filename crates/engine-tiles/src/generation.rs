//! Per-layer versioning system for selective invalidation.
//!
//! This module implements generation tracking for fine-grained cache invalidation.
//! For architecture details, see `tile-engine-architecture.md` §5.1 (GenerationTracker).
//!
//! # Overview
//!
//! The `GenerationTracker` maintains two-level versioning:
//! - **document_gen**: Global generation counter, incremented on any change
//! - **layer_gen**: Per-layer counters, incremented on layer-specific changes
//!
//! Tasks carry both generation values and are checked at execution time.
//! If either value is stale (doesn't match current tracker state), the task is discarded.
//!
//! This enables selective invalidation: only affected layers re-trigger recomputation,
//! while unaffected layers continue to use cached data.

use std::sync::atomic::{AtomicU64, Ordering};
use dashmap::DashMap;
use crate::LayerId;

/// Per-layer versioning system for selective cache invalidation.
///
/// Maintains two-level versioning to support efficient, fine-grained invalidation:
/// - `document_gen`: Incremented on any document-level change
/// - `layer_gen`: Map of per-layer counters, each incremented on layer-specific changes
///
/// # Concurrency
///
/// This type is thread-safe via atomic operations and concurrent DashMap.
/// Multiple threads may safely call these methods simultaneously without blocking.
///
/// # Example
///
/// ```ignore
/// let tracker = GenerationTracker::new();
///
/// // Initial state: all generations are 0
/// assert_eq!(tracker.get_layer_gen(0), 0);
/// assert_eq!(tracker.get_layer_gen(1), 0);
///
/// // Global change increments document generation
/// let old_doc_gen = tracker.increment_document_gen();
/// assert_eq!(old_doc_gen, 0);
/// assert_eq!(tracker.get_layer_gen(0), 0); // Layer 0 unchanged
///
/// // Layer-specific change increments layer generation
/// let new_layer_gen = tracker.increment_layer_gen(0);
/// assert_eq!(new_layer_gen, 1);
/// assert_eq!(tracker.get_layer_gen(1), 0); // Layer 1 unaffected
/// ```
pub struct GenerationTracker {
    /// Global generation counter, incremented on any document change.
    /// Used to detect any staleness at all.
    pub document_gen: AtomicU64,

    /// Per-layer generation counters, incremented on layer-specific changes.
    /// Enables selective invalidation: unchanged layers keep using cached data.
    pub layer_gen: DashMap<LayerId, u64>,
}

impl Clone for GenerationTracker {
    fn clone(&self) -> Self {
        GenerationTracker {
            document_gen: AtomicU64::new(self.document_gen.load(Ordering::SeqCst)),
            layer_gen: self.layer_gen.clone(),
        }
    }
}

impl std::fmt::Debug for GenerationTracker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GenerationTracker")
            .field("document_gen", &self.document_gen.load(Ordering::SeqCst))
            .field("layer_gen_count", &self.layer_gen.len())
            .finish()
    }
}

impl GenerationTracker {
    /// Creates a new generation tracker with all counters initialized to 0.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tracker = GenerationTracker::new();
    /// assert_eq!(tracker.get_layer_gen(0), 0);
    /// ```
    pub fn new() -> Self {
        Self {
            document_gen: AtomicU64::new(0),
            layer_gen: DashMap::new(),
        }
    }

    /// Atomically increments and returns the old document generation value.
    ///
    /// This is called when any document-level change occurs. The returned value
    /// is the generation that was active before the change.
    ///
    /// # Ordering
    ///
    /// Uses `Ordering::Release` to ensure prior stores are visible to readers
    /// before they observe the incremented value.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tracker = GenerationTracker::new();
    /// let old = tracker.increment_document_gen();
    /// assert_eq!(old, 0);
    /// let old2 = tracker.increment_document_gen();
    /// assert_eq!(old2, 1);
    /// ```
    pub fn increment_document_gen(&self) -> u64 {
        self.document_gen.fetch_add(1, Ordering::Release)
    }

    /// Atomically increments and returns the new per-layer generation value.
    ///
    /// If the layer has no prior generation, it is initialized to 0 then incremented to 1.
    /// On subsequent calls for the same layer, the counter continues incrementing.
    ///
    /// # Atomicity
    ///
    /// The get-or-insert and increment are atomic with respect to concurrent calls for
    /// the same layer (guaranteed by DashMap's internal locking).
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tracker = GenerationTracker::new();
    /// let gen1 = tracker.increment_layer_gen(0);
    /// assert_eq!(gen1, 1);
    /// let gen2 = tracker.increment_layer_gen(0);
    /// assert_eq!(gen2, 2);
    /// let gen3 = tracker.increment_layer_gen(1);  // Different layer
    /// assert_eq!(gen3, 1);
    /// ```
    pub fn increment_layer_gen(&self, layer: LayerId) -> u64 {
        let mut entry = self.layer_gen.entry(layer).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Returns the current generation for a layer, or 0 if the layer has not been tracked yet.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let tracker = GenerationTracker::new();
    /// assert_eq!(tracker.get_layer_gen(0), 0);  // Not tracked yet
    /// tracker.increment_layer_gen(0);
    /// assert_eq!(tracker.get_layer_gen(0), 1);  // Now tracked
    /// ```
    pub fn get_layer_gen(&self, layer: LayerId) -> u64 {
        self.layer_gen.get(&layer).map(|e| *e).unwrap_or(0)
    }
}

impl Default for GenerationTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn increment_document_gen_returns_previous_value() {
        let tracker = GenerationTracker::new();
        assert_eq!(tracker.increment_document_gen(), 0);
        assert_eq!(tracker.increment_document_gen(), 1);
        assert_eq!(tracker.increment_document_gen(), 2);
    }

    #[test]
    fn increment_layer_gen_increments_per_layer_counter() {
        let tracker = GenerationTracker::new();
        let gen1 = tracker.increment_layer_gen(0);
        assert_eq!(gen1, 1);
        let gen2 = tracker.increment_layer_gen(0);
        assert_eq!(gen2, 2);
        let gen3 = tracker.increment_layer_gen(0);
        assert_eq!(gen3, 3);
    }

    #[test]
    fn multiple_layers_have_independent_counters() {
        let tracker = GenerationTracker::new();
        let layer0_gen1 = tracker.increment_layer_gen(0);
        let layer1_gen1 = tracker.increment_layer_gen(1);
        assert_eq!(layer0_gen1, 1);
        assert_eq!(layer1_gen1, 1);

        let layer0_gen2 = tracker.increment_layer_gen(0);
        let layer2_gen1 = tracker.increment_layer_gen(2);
        assert_eq!(layer0_gen2, 2);
        assert_eq!(layer2_gen1, 1);

        // Layer 1 counter was not incremented again, still at 1
        assert_eq!(tracker.get_layer_gen(1), 1);
    }

    #[test]
    fn get_layer_gen_returns_correct_values() {
        let tracker = GenerationTracker::new();
        // Untracked layer returns 0
        assert_eq!(tracker.get_layer_gen(0), 0);
        assert_eq!(tracker.get_layer_gen(42), 0);

        // After increment, returns incremented value
        tracker.increment_layer_gen(0);
        assert_eq!(tracker.get_layer_gen(0), 1);

        // Different layers have independent values
        tracker.increment_layer_gen(1);
        tracker.increment_layer_gen(1);
        assert_eq!(tracker.get_layer_gen(1), 2);
        assert_eq!(tracker.get_layer_gen(0), 1);
    }
}
