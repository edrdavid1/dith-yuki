//! Tile caching layer with LRU eviction and dirty marking.
//!
//! This module implements `TileCache`, a concurrent tile storage system with:
//! - Lock-free concurrent reads via `DashMap`
//! - Least-Recently-Used (LRU) eviction policy
//! - Dirty marking without deletion (stale tiles stay in cache until recomputed)
//! - Configurable memory budget enforcement
//!
//! For architecture details, see `tile-engine-architecture.md` §3 (TileCache).
//!
//! # Overview
//!
//! The cache stores tiles by `TileKey` with associated metadata:
//! - `tile`: The actual pixel data (Arc-wrapped for sharing)
//! - `generation`: Version counter for invalidation tracking
//! - `last_touched`: Timestamp for LRU ordering
//! - `dirty`: Flag indicating tile needs recomputation
//!
//! When the cache exceeds its memory budget, the least-recently-used tile is evicted.
//! Dirty tiles remain in the cache (marked but not deleted) for instant feedback.

use crate::{TileCoord, TileKey, PixelTile};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;
use dashmap::DashMap;
use crossbeam::queue::SegQueue;

/// Estimated size in bytes of a single PixelTile.
///
/// Calculated as: (TILE_SIZE + 2×HALO)² × 4 channels × 4 bytes per f32
/// = 260² × 4 × 4 = 1,081,600 bytes
pub const TILE_BYTES: usize = ((256 + 2 * 2) as usize) * ((256 + 2 * 2) as usize) * 4 * 4;

/// Metadata for a cached tile entry.
///
/// Tracks the tile data, its version, access time, and dirty status.
pub struct CacheEntry {
    /// The pixel data, wrapped in Arc for efficient sharing across threads.
    pub tile: Arc<PixelTile>,
    /// Version counter for invalidation tracking.
    /// Used to detect stale tasks and enable selective recomputation.
    pub generation: u64,
    /// Timestamp of last access (used for LRU ordering).
    pub last_touched: Instant,
    /// Flag indicating whether this tile is marked dirty (stale, needs recomputation).
    /// Dirty tiles remain in cache for instant feedback; not deleted.
    pub dirty: AtomicBool,
}

/// Concurrent tile cache with LRU eviction and dirty marking.
///
/// Manages a collection of tiles with:
/// - O(1) average-case lookups via DashMap
/// - LRU eviction when memory budget is exceeded
/// - Dirty marking (not deletion) for stale tiles
/// - Thread-safe concurrent access
///
/// # Examples
///
/// ```ignore
/// let cache = TileCache::new(100_000_000); // 100 MB budget
/// let tile = Arc::new(PixelTile::new());
/// let retrieved = cache.get_or_insert(key, tile.clone());
///
/// // Mark tile dirty without deleting it
/// cache.mark_dirty(key);
///
/// // Evict LRU tiles when over budget
/// cache.evict_if_over_budget();
/// ```
pub struct TileCache {
    /// DashMap for lock-free concurrent reads/writes of cache entries.
    pub entries: DashMap<TileKey, CacheEntry>,
    /// SegQueue maintaining insertion order (approximates LRU).
    /// When eviction is needed, we pop from this queue.
    lru_queue: SegQueue<TileKey>,
    /// Memory budget in bytes.
    budget_bytes: AtomicUsize,
    /// Current memory used by cached tiles in bytes.
    used_bytes: AtomicUsize,
}

impl TileCache {
    /// Create a new cache with the specified memory budget.
    ///
    /// # Arguments
    ///
    /// - `budget_bytes`: Maximum bytes the cache is allowed to use
    ///
    /// # Returns
    ///
    /// An empty cache with `used_bytes = 0`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let cache = TileCache::new(50_000_000); // 50 MB budget
    /// assert_eq!(cache.used_bytes.load(Ordering::Relaxed), 0);
    /// ```
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            entries: DashMap::new(),
            lru_queue: SegQueue::new(),
            budget_bytes: AtomicUsize::new(budget_bytes),
            used_bytes: AtomicUsize::new(0),
        }
    }

    /// Retrieve a cached tile or insert it if not present.
    ///
    /// If the tile already exists in the cache, returns the cached version.
    /// Otherwise, inserts the provided tile with initial state:
    /// - `generation = 0`
    /// - `last_touched = now()`
    /// - `dirty = false`
    ///
    /// Updates `used_bytes` when inserting a new tile.
    /// Enqueues the key to the LRU queue for eviction ordering.
    ///
    /// # Arguments
    ///
    /// - `key`: The TileKey identifying this tile
    /// - `tile`: The pixel data to cache
    ///
    /// # Returns
    ///
    /// The cached tile (either previously cached or newly inserted).
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let cache = TileCache::new(1_000_000);
    /// let key = TileKey { /* ... */ };
    /// let tile = Arc::new(PixelTile::new());
    /// let retrieved = cache.get_or_insert(key, tile.clone());
    /// assert_eq!(Arc::ptr_eq(&retrieved, &tile));
    /// ```
    pub fn get_or_insert(&self, key: TileKey, tile: Arc<PixelTile>) -> Arc<PixelTile> {
        if let Some(entry) = self.entries.get(&key) {
            entry.value().tile.clone()
        } else {
            self.entries.insert(
                key,
                CacheEntry {
                    tile: tile.clone(),
                    generation: 0,
                    last_touched: Instant::now(),
                    dirty: AtomicBool::new(false),
                },
            );
            self.used_bytes.fetch_add(TILE_BYTES, Ordering::Relaxed);
            self.lru_queue.push(key);
            tile
        }
    }

    /// Mark a tile as dirty (stale, needs recomputation).
    ///
    /// Sets the tile's dirty flag without removing it from the cache.
    /// The tile remains available for reads until recomputed and reinserted.
    ///
    /// # Arguments
    ///
    /// - `key`: The TileKey of the tile to mark dirty
    ///
    /// # Notes
    ///
    /// If the key does not exist in the cache, this is a no-op (silently ignored).
    /// Multiple marks to the same tile are idempotent.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// cache.mark_dirty(key);
    /// // Tile remains in cache but is marked dirty
    /// assert_eq!(cache.entries.get(&key).unwrap().dirty.load(Ordering::Relaxed), true);
    /// ```
    pub fn mark_dirty(&self, key: TileKey) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.dirty.store(true, Ordering::Release);
        }
    }

    /// Remove every cached stage for `layer` (Raw / Processed / Composite).
    ///
    /// Stale LRU-queue entries for the removed keys are ignored on pop,
    /// matching existing eviction. Missing keys are a no-op.
    pub fn evict_layer(&self, layer: crate::LayerId) {
        let mut removed = 0usize;
        self.entries.retain(|key, _| {
            if key.layer == layer {
                removed += 1;
                false
            } else {
                true
            }
        });
        if removed > 0 {
            self.used_bytes
                .fetch_sub(removed * TILE_BYTES, Ordering::Relaxed);
        }
    }

    /// Evict least-recently-used tiles if cache exceeds budget.
    ///
    /// Compares current `used_bytes` against `budget_bytes`.
    /// If over budget, pops tiles from the LRU queue and removes them from the cache
    /// until `used_bytes ≤ budget_bytes`.
    ///
    /// Tiles are removed from the DashMap; their memory is freed when the last
    /// Arc reference is dropped (may not be immediate if tiles are in-flight).
    ///
    /// # Notes
    ///
    /// - Uses relaxed atomic ordering for performance (cache-local state)
    /// - Approximates true LRU due to SegQueue FIFO semantics (first-in, first-out)
    /// - Does not guarantee perfect LRU; is a best-effort eviction
    /// - May remove more tiles than necessary to return under budget
    ///
    /// # Examples
    ///
    /// ```ignore
    /// cache.get_or_insert(key1, tile1);
    /// cache.get_or_insert(key2, tile2);
    /// cache.evict_if_over_budget(); // Removes least-recently-used tiles
    /// ```
    pub fn evict_if_over_budget(&self) {
        let used = self.used_bytes.load(Ordering::Relaxed);
        let budget = self.budget_bytes.load(Ordering::Relaxed);

        if used > budget {
            while let Some(key) = self.lru_queue.pop() {
                if self.entries.remove(&key).is_some() {
                    self.used_bytes.fetch_sub(TILE_BYTES, Ordering::Relaxed);
                    if self.used_bytes.load(Ordering::Relaxed) <= budget {
                        break;
                    }
                }
            }
        }
    }

    /// Evict least-recently-used tiles while preserving viewport tiles.
    ///
    /// Like `evict_if_over_budget`, but skips any tile whose `TileCoord` is in the
    /// provided viewport set. Tiles at different stages (Raw, Processed, Composite)
    /// sharing a coord that overlaps the viewport are all preserved.
    ///
    /// If the budget is exceeded but all remaining tiles are viewport tiles,
    /// eviction stops and the cache is allowed to remain over-budget.
    ///
    /// # Arguments
    ///
    /// - `viewport_tiles`: The set of TileCoords that must be preserved (visible in the viewport
    ///   at the active pyramid level)
    ///
    /// # Notes
    ///
    /// - Viewport tiles that are popped from the LRU queue are re-enqueued to maintain
    ///   their presence in future eviction runs.
    /// - If a key popped from the LRU queue is no longer in the cache (already removed),
    ///   it is simply discarded without affecting `used_bytes`.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// use std::collections::HashSet;
    /// let mut viewport = HashSet::new();
    /// viewport.insert(TileCoord { level: 0, x: 0, y: 0 });
    /// cache.evict_preserving_viewport(&viewport);
    /// ```
    pub fn evict_preserving_viewport(&self, viewport_tiles: &HashSet<TileCoord>) {
        let used = self.used_bytes.load(Ordering::Relaxed);
        let budget = self.budget_bytes.load(Ordering::Relaxed);

        if used <= budget {
            return;
        }

        // Track viewport tiles we skip so we can re-enqueue them.
        let mut skipped: Vec<TileKey> = Vec::new();
        // Limit iterations to prevent infinite looping if all tiles are viewport tiles.
        let max_iterations = self.entries.len();
        let mut iterations = 0;

        while self.used_bytes.load(Ordering::Relaxed) > budget {
            iterations += 1;
            if iterations > max_iterations {
                // All remaining tiles are viewport tiles; allow over-budget.
                break;
            }

            match self.lru_queue.pop() {
                Some(key) => {
                    if viewport_tiles.contains(&key.coord) {
                        // This tile overlaps the viewport — skip eviction, re-enqueue later.
                        skipped.push(key);
                    } else if self.entries.remove(&key).is_some() {
                        self.used_bytes.fetch_sub(TILE_BYTES, Ordering::Relaxed);
                    }
                    // If the key wasn't in the cache (already removed), just skip it.
                }
                None => {
                    // LRU queue is empty; nothing left to evict.
                    break;
                }
            }
        }

        // Re-enqueue skipped viewport tiles so they remain in the LRU queue.
        for key in skipped {
            self.lru_queue.push(key);
        }
    }

    /// Get the current memory usage in bytes.
    ///
    /// # Returns
    ///
    /// Current number of bytes used by tiles in the cache.
    pub fn used_bytes_count(&self) -> usize {
        self.used_bytes.load(Ordering::Relaxed)
    }

    /// Get the memory budget in bytes.
    ///
    /// # Returns
    ///
    /// Maximum bytes allowed for this cache.
    pub fn budget_bytes_count(&self) -> usize {
        self.budget_bytes.load(Ordering::Relaxed)
    }

    /// Get the number of tiles currently in the cache.
    ///
    /// # Returns
    ///
    /// Count of entries in the cache.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Insert or replace a tile entry, marking it as not dirty.
    ///
    /// Unlike `get_or_insert`, this always overwrites any existing entry with the new tile.
    /// Used by the worker loop after computing a fresh tile to replace a stale/dirty entry.
    ///
    /// # Arguments
    ///
    /// - `key`: The TileKey identifying this tile
    /// - `tile`: The freshly computed pixel data to cache
    ///
    /// # Notes
    ///
    /// If the key already exists, the existing entry is replaced in-place (no net memory change).
    /// If the key is new, `used_bytes` is incremented and the key is added to the LRU queue.
    pub fn insert_fresh(&self, key: TileKey, tile: Arc<PixelTile>) {
        if self.entries.contains_key(&key) {
            // Replace existing entry in-place
            self.entries.insert(
                key,
                CacheEntry {
                    tile,
                    generation: 0,
                    last_touched: Instant::now(),
                    dirty: AtomicBool::new(false),
                },
            );
        } else {
            // New entry
            self.entries.insert(
                key,
                CacheEntry {
                    tile,
                    generation: 0,
                    last_touched: Instant::now(),
                    dirty: AtomicBool::new(false),
                },
            );
            self.used_bytes.fetch_add(TILE_BYTES, Ordering::Relaxed);
            self.lru_queue.push(key);
        }
    }

    /// Retrieve a tile entry without modifying it.
    ///
    /// Useful for testing and inspection. Returns a reference to the entry if it exists.
    ///
    /// # Arguments
    ///
    /// - `key`: The TileKey to look up
    ///
    /// # Returns
    ///
    /// An Option containing a reference to the entry if found.
    #[allow(dead_code)]
    pub fn get_entry(&self, key: TileKey) -> Option<Arc<PixelTile>> {
        self.entries.get(&key).map(|e| e.tile.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TileCoord, CacheStage};

    fn make_key(layer: u32, x: u32, y: u32) -> TileKey {
        TileKey {
            layer,
            coord: TileCoord {
                level: 0,
                x,
                y,
            },
            stage: CacheStage::Raw,
        }
    }

    #[test]
    fn tile_cache_new_creates_empty_cache() {
        let cache = TileCache::new(1_000_000);
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.used_bytes_count(), 0);
        assert_eq!(cache.budget_bytes_count(), 1_000_000);
    }

    #[test]
    fn get_or_insert_inserts_new_tile() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(0, 0, 0);
        let tile = Arc::new(PixelTile::new());

        let retrieved = cache.get_or_insert(key, tile.clone());

        assert_eq!(cache.entry_count(), 1);
        assert!(Arc::ptr_eq(&retrieved, &tile));
        assert_eq!(cache.used_bytes_count(), TILE_BYTES);
    }

    #[test]
    fn get_or_insert_returns_existing_tile() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(0, 0, 0);
        let tile1 = Arc::new(PixelTile::new());
        let tile2 = Arc::new(PixelTile::new());

        let retrieved1 = cache.get_or_insert(key, tile1.clone());
        let retrieved2 = cache.get_or_insert(key, tile2.clone());

        assert_eq!(cache.entry_count(), 1);
        assert!(Arc::ptr_eq(&retrieved1, &tile1));
        assert!(Arc::ptr_eq(&retrieved2, &tile1));
        assert!(!Arc::ptr_eq(&retrieved2, &tile2));
        assert_eq!(cache.used_bytes_count(), TILE_BYTES);
    }

    #[test]
    fn mark_dirty_sets_dirty_flag() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(0, 0, 0);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key, tile);
        cache.mark_dirty(key);

        let entry = cache.entries.get(&key).unwrap();
        assert!(entry.dirty.load(Ordering::Relaxed));
    }

    #[test]
    fn mark_dirty_does_not_delete_tile() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(0, 0, 0);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key, tile);
        cache.mark_dirty(key);

        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.used_bytes_count(), TILE_BYTES);
    }

    #[test]
    fn evict_if_over_budget_removes_lru_tiles() {
        let cache = TileCache::new(TILE_BYTES); // Budget for exactly 1 tile
        let key1 = make_key(0, 0, 0);
        let key2 = make_key(0, 1, 0);
        let tile1 = Arc::new(PixelTile::new());
        let tile2 = Arc::new(PixelTile::new());

        cache.get_or_insert(key1, tile1);
        assert_eq!(cache.entry_count(), 1);

        cache.get_or_insert(key2, tile2);
        assert_eq!(cache.entry_count(), 2); // Both inserted; used_bytes now 2×TILE_BYTES
        assert_eq!(cache.used_bytes_count(), 2 * TILE_BYTES);

        cache.evict_if_over_budget();

        // Should evict key1 (least recently used)
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.used_bytes_count(), TILE_BYTES);
        assert!(cache.entries.contains_key(&key2));
        assert!(!cache.entries.contains_key(&key1));
    }

    #[test]
    fn evict_if_over_budget_does_not_evict_when_under_budget() {
        let cache = TileCache::new(10_000_000); // Large budget
        let key = make_key(0, 0, 0);
        let tile = Arc::new(PixelTile::new());

        cache.get_or_insert(key, tile);
        let before = cache.entry_count();
        cache.evict_if_over_budget();
        let after = cache.entry_count();

        assert_eq!(before, after);
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn multiple_tiles_in_cache() {
        let cache = TileCache::new(10_000_000);
        let key1 = make_key(0, 0, 0);
        let key2 = make_key(0, 1, 0);
        let key3 = make_key(0, 0, 1);

        let tile1 = Arc::new(PixelTile::new());
        let tile2 = Arc::new(PixelTile::new());
        let tile3 = Arc::new(PixelTile::new());

        cache.get_or_insert(key1, tile1);
        cache.get_or_insert(key2, tile2);
        cache.get_or_insert(key3, tile3);

        assert_eq!(cache.entry_count(), 3);
        assert_eq!(cache.used_bytes_count(), 3 * TILE_BYTES);
    }

    #[test]
    fn mark_dirty_on_nonexistent_key_is_noop() {
        let cache = TileCache::new(1_000_000);
        let key = make_key(99, 99, 99);

        // Should not panic; is a no-op
        cache.mark_dirty(key);
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn tile_bytes_constant_is_correct() {
        // (256 + 2*2)^2 * 4 * 4 = 260^2 * 16 = 67,600 * 16 = 1,081,600
        assert_eq!(TILE_BYTES, 260 * 260 * 16);
        assert_eq!(TILE_BYTES, 1_081_600);
    }

    // --- evict_preserving_viewport tests ---

    #[test]
    fn evict_preserving_viewport_skips_viewport_tiles() {
        // Budget for 1 tile, insert 2. The viewport tile should be preserved.
        let cache = TileCache::new(TILE_BYTES);
        let key1 = make_key(0, 0, 0); // Will be in viewport
        let key2 = make_key(0, 1, 0); // Not in viewport
        let tile1 = Arc::new(PixelTile::new());
        let tile2 = Arc::new(PixelTile::new());

        cache.get_or_insert(key1, tile1);
        cache.get_or_insert(key2, tile2);
        assert_eq!(cache.entry_count(), 2);

        let mut viewport = std::collections::HashSet::new();
        viewport.insert(TileCoord { level: 0, x: 0, y: 0 });

        cache.evict_preserving_viewport(&viewport);

        // key1 (viewport tile) should be preserved, key2 evicted
        assert!(cache.entries.contains_key(&key1));
        assert!(!cache.entries.contains_key(&key2));
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.used_bytes_count(), TILE_BYTES);
    }

    #[test]
    fn evict_preserving_viewport_allows_over_budget_when_all_viewport() {
        // Budget for 1 tile, insert 2 tiles both in viewport.
        // Should allow over-budget since all are viewport tiles.
        let cache = TileCache::new(TILE_BYTES);
        let key1 = make_key(0, 0, 0);
        let key2 = make_key(0, 1, 0);
        let tile1 = Arc::new(PixelTile::new());
        let tile2 = Arc::new(PixelTile::new());

        cache.get_or_insert(key1, tile1);
        cache.get_or_insert(key2, tile2);

        let mut viewport = std::collections::HashSet::new();
        viewport.insert(TileCoord { level: 0, x: 0, y: 0 });
        viewport.insert(TileCoord { level: 0, x: 1, y: 0 });

        cache.evict_preserving_viewport(&viewport);

        // Both tiles should be preserved (over-budget allowed)
        assert_eq!(cache.entry_count(), 2);
        assert_eq!(cache.used_bytes_count(), 2 * TILE_BYTES);
        assert!(cache.entries.contains_key(&key1));
        assert!(cache.entries.contains_key(&key2));
    }

    #[test]
    fn evict_preserving_viewport_preserves_all_stages_of_viewport_coord() {
        // Budget for 2 tiles, insert 3: two are different stages of the same viewport coord.
        let cache = TileCache::new(2 * TILE_BYTES);
        let coord = TileCoord { level: 0, x: 0, y: 0 };

        let key_raw = TileKey { layer: 0, coord, stage: CacheStage::Raw };
        let key_processed = TileKey { layer: 0, coord, stage: CacheStage::Processed };
        let key_other = TileKey {
            layer: 0,
            coord: TileCoord { level: 0, x: 5, y: 5 },
            stage: CacheStage::Raw,
        };

        cache.get_or_insert(key_raw, Arc::new(PixelTile::new()));
        cache.get_or_insert(key_processed, Arc::new(PixelTile::new()));
        cache.get_or_insert(key_other, Arc::new(PixelTile::new()));
        assert_eq!(cache.entry_count(), 3);

        let mut viewport = std::collections::HashSet::new();
        viewport.insert(coord);

        cache.evict_preserving_viewport(&viewport);

        // Both stages of the viewport coord should be preserved
        assert!(cache.entries.contains_key(&key_raw));
        assert!(cache.entries.contains_key(&key_processed));
        // The other tile should be evicted
        assert!(!cache.entries.contains_key(&key_other));
        assert_eq!(cache.entry_count(), 2);
    }

    #[test]
    fn evict_preserving_viewport_no_eviction_when_under_budget() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(0, 0, 0);
        cache.get_or_insert(key, Arc::new(PixelTile::new()));

        let viewport = std::collections::HashSet::new();
        cache.evict_preserving_viewport(&viewport);

        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn evict_layer_removes_all_stages_keeps_other_layer() {
        let cache = TileCache::new(10_000_000);
        let tile = Arc::new(PixelTile::new());
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let stages = [CacheStage::Raw, CacheStage::Processed, CacheStage::Composite];
        for stage in stages {
            cache.get_or_insert(
                TileKey {
                    layer: 1,
                    coord,
                    stage,
                },
                tile.clone(),
            );
            cache.get_or_insert(
                TileKey {
                    layer: 2,
                    coord,
                    stage,
                },
                tile.clone(),
            );
        }
        assert_eq!(cache.entry_count(), 6);

        cache.evict_layer(1);

        for stage in stages {
            assert!(!cache.entries.contains_key(&TileKey {
                layer: 1,
                coord,
                stage,
            }));
            assert!(cache.entries.contains_key(&TileKey {
                layer: 2,
                coord,
                stage,
            }));
        }
        assert_eq!(cache.entry_count(), 3);
        assert_eq!(cache.used_bytes_count(), 3 * TILE_BYTES);
    }

    #[test]
    fn evict_layer_missing_keys_is_noop() {
        let cache = TileCache::new(10_000_000);
        cache.get_or_insert(make_key(2, 0, 0), Arc::new(PixelTile::new()));
        cache.evict_layer(1);
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn evict_preserving_viewport_empty_viewport_evicts_normally() {
        // No viewport tiles means everything is evictable.
        let cache = TileCache::new(TILE_BYTES);
        let key1 = make_key(0, 0, 0);
        let key2 = make_key(0, 1, 0);

        cache.get_or_insert(key1, Arc::new(PixelTile::new()));
        cache.get_or_insert(key2, Arc::new(PixelTile::new()));

        let viewport = std::collections::HashSet::new();
        cache.evict_preserving_viewport(&viewport);

        // Should evict until under budget (1 tile remains)
        assert_eq!(cache.entry_count(), 1);
        assert_eq!(cache.used_bytes_count(), TILE_BYTES);
    }
}
