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

use crate::{TileKey, PixelTile};
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
    pub(crate) fn get_entry(&self, key: TileKey) -> Option<Arc<PixelTile>> {
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
}
