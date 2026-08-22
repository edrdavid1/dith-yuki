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

use crate::{CacheStage, TileCoord, TileKey, PixelTile};
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;
use dashmap::DashMap;
use crossbeam::queue::SegQueue;

/// Context for doc-aware budget pressure eviction.
///
/// - `active_doc` + `viewport_coords`: viewport protect set for the visible doc
///   (`None` active → legacy doc-blind coord protect).
/// - `open_docs`: runtime sessions whose **Raw** tiles are hard-excluded from
///   pressure (source of truth until close / `evict_document`).
pub struct EvictContext<'a> {
    pub active_doc: Option<u32>,
    pub open_docs: &'a HashSet<u32>,
    pub viewport_coords: &'a HashSet<TileCoord>,
}

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

    /// Drop every cache entry. LRU leftovers are ignored on later pop, as with `evict_layer`.
    pub fn clear(&self) {
        let n = self.entries.len();
        self.entries.clear();
        if n > 0 {
            self.used_bytes.store(0, Ordering::Relaxed);
        }
    }

    /// Highest `generation` among cached entries, or 0 if empty.
    pub fn max_generation(&self) -> u64 {
        self.entries
            .iter()
            .map(|e| e.generation)
            .max()
            .unwrap_or(0)
    }

    /// Remove every cached stage for `layer` on `doc`.
    pub fn evict_layer(&self, doc: u32, layer: crate::LayerId) {
        self.retain_removed(|key| key.doc == doc && key.layer == layer);
    }

    /// Remove every cached tile for a document session.
    pub fn evict_document(&self, doc: u32) {
        self.retain_removed(|key| key.doc == doc);
    }

    fn retain_removed(&self, mut drop_key: impl FnMut(&TileKey) -> bool) {
        let mut removed = 0usize;
        self.entries.retain(|key, _| {
            if drop_key(key) {
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
    /// Doc-blind: no viewport protection. Prefer [`Self::evict_for_pressure`] when
    /// an active document / viewport is known.
    pub fn evict_if_over_budget(&self) {
        let empty_vp = HashSet::new();
        let empty_open = HashSet::new();
        self.evict_for_pressure(&EvictContext {
            active_doc: None,
            open_docs: &empty_open,
            viewport_coords: &empty_vp,
        });
    }

    /// Doc-aware pressure eviction.
    ///
    /// Cheap no-op when `used ≤ budget`. Drop order:
    /// 1. inactive docs, Composite → Processed → (orphan) Raw
    /// 2. active off-viewport, same stage order
    /// 3. stop — allow over-budget if only protected / pinned remain
    ///
    /// **Pinned (never pressure-evicted):** `stage == Raw` and `doc ∈ open_docs`.
    /// Close path uses [`Self::evict_document`], not pressure.
    ///
    /// Viewport-protected: `coord ∈ viewport_coords` and (`doc == active_doc` or
    /// `active_doc` is `None`).
    ///
    /// If `viewport_coords` is empty and `active_doc` is set, only inactive-doc
    /// non-pinned tiles are dropped (activate/open before `set_viewport`).
    pub fn evict_for_pressure(&self, ctx: &EvictContext<'_>) {
        let budget = self.budget_bytes.load(Ordering::Relaxed);
        if self.used_bytes.load(Ordering::Relaxed) <= budget {
            return;
        }

        // Raw of open docs is pinned; still scan Raw for orphans (not in open_docs).
        let stage_order = [
            Some(CacheStage::Composite),
            Some(CacheStage::Processed),
            Some(CacheStage::Raw),
        ];

        if ctx.active_doc.is_some() {
            for stage in stage_order {
                self.evict_pass(ctx, budget, true, stage);
                if self.used_bytes.load(Ordering::Relaxed) <= budget {
                    return;
                }
            }
        }

        if ctx.active_doc.is_some() && ctx.viewport_coords.is_empty() {
            return;
        }

        for stage in stage_order {
            self.evict_pass(ctx, budget, false, stage);
            if self.used_bytes.load(Ordering::Relaxed) <= budget {
                return;
            }
        }
    }

    fn is_viewport_protected(key: &TileKey, ctx: &EvictContext<'_>) -> bool {
        if !ctx.viewport_coords.contains(&key.coord) {
            return false;
        }
        match ctx.active_doc {
            Some(active) => key.doc == active,
            None => true,
        }
    }

    /// Raw tiles belonging to a still-open session — never dropped by pressure.
    fn is_pinned_open_raw(key: &TileKey, ctx: &EvictContext<'_>) -> bool {
        key.stage == CacheStage::Raw && ctx.open_docs.contains(&key.doc)
    }

    fn evict_pass(
        &self,
        ctx: &EvictContext<'_>,
        budget: usize,
        inactive_only: bool,
        stage_filter: Option<CacheStage>,
    ) {
        let mut skipped: Vec<TileKey> = Vec::new();
        let max_iterations = self.entries.len().saturating_mul(2).max(1);
        let mut iterations = 0;
        let mut removed_this_pass = 0usize;

        while self.used_bytes.load(Ordering::Relaxed) > budget {
            iterations += 1;
            if iterations > max_iterations {
                break;
            }

            match self.lru_queue.pop() {
                Some(key) => {
                    let pinned = Self::is_pinned_open_raw(&key, ctx);
                    let protected = Self::is_viewport_protected(&key, ctx);
                    let inactive = ctx
                        .active_doc
                        .map(|active| key.doc != active)
                        .unwrap_or(false);
                    let stage_ok = stage_filter
                        .map(|s| key.stage == s)
                        .unwrap_or(true);

                    if pinned || !stage_ok {
                        skipped.push(key);
                        if skipped.len() >= self.entries.len() && removed_this_pass == 0 {
                            break;
                        }
                        continue;
                    }

                    if inactive_only {
                        if !inactive {
                            skipped.push(key);
                            if skipped.len() >= self.entries.len() && removed_this_pass == 0 {
                                break;
                            }
                            continue;
                        }
                    } else if protected {
                        skipped.push(key);
                        if skipped.len() >= self.entries.len() && removed_this_pass == 0 {
                            break;
                        }
                        continue;
                    }

                    if self.entries.remove(&key).is_some() {
                        self.used_bytes.fetch_sub(TILE_BYTES, Ordering::Relaxed);
                        removed_this_pass += 1;
                    }
                }
                None => break,
            }
        }

        for key in skipped {
            self.lru_queue.push(key);
        }
    }

    /// Legacy doc-blind viewport preserve. Prefer [`Self::evict_for_pressure`].
    pub fn evict_preserving_viewport(&self, viewport_tiles: &HashSet<TileCoord>) {
        let empty_open = HashSet::new();
        self.evict_for_pressure(&EvictContext {
            active_doc: None,
            open_docs: &empty_open,
            viewport_coords: viewport_tiles,
        });
    }

    /// Drop selected stages for one document (soft trim on deactivate).
    pub fn evict_stages(&self, doc: u32, stages: &[CacheStage]) {
        self.retain_removed(|key| key.doc == doc && stages.iter().any(|s| *s == key.stage));
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
        let _ = self.insert_fresh_gen(key, tile, 0);
    }

    /// Insert a freshly computed tile at `generation`.
    ///
    /// Returns `false` (and leaves the cache unchanged) if the key already holds
    /// a **newer** generation — so a slow stale Composite cannot overwrite a
    /// newer result. Same or older cached generation is replaced. The per-key
    /// DashMap entry lock makes the compare-and-swap atomic.
    pub fn insert_fresh_gen(&self, key: TileKey, tile: Arc<PixelTile>, generation: u64) -> bool {
        use dashmap::mapref::entry::Entry;
        match self.entries.entry(key) {
            Entry::Occupied(mut occ) => {
                if occ.get().generation > generation {
                    return false;
                }
                occ.insert(CacheEntry {
                    tile,
                    generation,
                    last_touched: Instant::now(),
                    dirty: AtomicBool::new(false),
                });
                true
            }
            Entry::Vacant(v) => {
                v.insert(CacheEntry {
                    tile,
                    generation,
                    last_touched: Instant::now(),
                    dirty: AtomicBool::new(false),
                });
                self.used_bytes.fetch_add(TILE_BYTES, Ordering::Relaxed);
                self.lru_queue.push(key);
                true
            }
        }
    }

    /// Protocol Ready: 200 only when the entry is clean and not behind `doc_gen`.
    pub fn tile_entry_is_ready(dirty: bool, entry_generation: u64, doc_gen: u64) -> bool {
        !dirty && entry_generation >= doc_gen
    }

    /// If `live_gen` is ahead of the cached generation, mark dirty and return true
    /// so the caller can enqueue the current generation. No-op when cache already
    /// matches live (stale insert lost the race against a still-current frame).
    pub fn mark_dirty_if_generation_behind(&self, key: TileKey, live_gen: u64) -> bool {
        if let Some(entry) = self.entries.get(&key) {
            if live_gen > entry.generation {
                entry.dirty.store(true, Ordering::Release);
                return true;
            }
        }
        false
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
            doc: 1,
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

        let key_raw = TileKey { doc: 1, layer: 0, coord, stage: CacheStage::Raw };
        let key_processed = TileKey { doc: 1, layer: 0, coord, stage: CacheStage::Processed };
        let key_other = TileKey {
            doc: 1,
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
                    doc: 1,
                    layer: 1,
                    coord,
                    stage,
                },
                tile.clone(),
            );
            cache.get_or_insert(
                TileKey {
                    doc: 1,
                    layer: 2,
                    coord,
                    stage,
                },
                tile.clone(),
            );
        }
        assert_eq!(cache.entry_count(), 6);

        cache.evict_layer(1, 1);

        for stage in stages {
            assert!(!cache.entries.contains_key(&TileKey {
                doc: 1,
                layer: 1,
                coord,
                stage,
            }));
            assert!(cache.entries.contains_key(&TileKey {
                doc: 1,
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
        cache.evict_layer(1, 1);
        assert_eq!(cache.entry_count(), 1);
    }

    #[test]
    fn evict_document_keeps_other_doc_same_layer_coord() {
        let cache = TileCache::new(10_000_000);
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let a = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Raw,
        };
        let b = TileKey {
            doc: 2,
            layer: 1,
            coord,
            stage: CacheStage::Raw,
        };
        cache.get_or_insert(a, Arc::new(PixelTile::new()));
        cache.get_or_insert(b, Arc::new(PixelTile::new()));
        cache.evict_document(1);
        assert!(!cache.entries.contains_key(&a));
        assert!(cache.entries.contains_key(&b));
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

    fn composite_key() -> TileKey {
        TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        }
    }

    fn marked_tile(r: f32) -> Arc<PixelTile> {
        let mut tile = PixelTile::new();
        tile.set(0, 0, 0, r);
        Arc::new(tile)
    }

    #[test]
    fn insert_fresh_gen_keeps_newer_generation() {
        let cache = TileCache::new(10_000_000);
        let key = composite_key();

        assert!(cache.insert_fresh_gen(key, marked_tile(0.1), 1));
        assert!(cache.insert_fresh_gen(key, marked_tile(0.2), 2));
        assert!(
            !cache.insert_fresh_gen(key, marked_tile(0.9), 1),
            "stale gen 1 must not overwrite gen 2"
        );

        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(entry.generation, 2);
        assert!((entry.tile.at(0, 0, 0) - 0.2).abs() < 1e-6);
        assert!(!entry.dirty.load(Ordering::Acquire));
        drop(entry);

        // Live doc_gen advanced to 3: rejected stale write must not leave a
        // silently-ready (clean, behind) entry.
        assert!(cache.mark_dirty_if_generation_behind(key, 3));
        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(entry.generation, 2);
        assert!(entry.dirty.load(Ordering::Acquire));
        assert!(!TileCache::tile_entry_is_ready(
            entry.dirty.load(Ordering::Acquire),
            entry.generation,
            3
        ));
    }

    #[test]
    fn mark_dirty_if_behind_noop_when_cache_matches_live() {
        let cache = TileCache::new(10_000_000);
        let key = composite_key();
        assert!(cache.insert_fresh_gen(key, marked_tile(0.2), 2));
        assert!(!cache.insert_fresh_gen(key, marked_tile(0.9), 1));
        assert!(!cache.mark_dirty_if_generation_behind(key, 2));
        assert!(!cache.entries.get(&key).unwrap().dirty.load(Ordering::Acquire));
        assert!(TileCache::tile_entry_is_ready(false, 2, 2));
        assert!(!TileCache::tile_entry_is_ready(false, 1, 2));
        assert!(!TileCache::tile_entry_is_ready(true, 2, 2));
    }

    #[test]
    fn insert_fresh_gen_replaces_equal_generation() {
        let cache = TileCache::new(10_000_000);
        let key = composite_key();
        assert!(cache.insert_fresh_gen(key, marked_tile(0.1), 5));
        assert!(cache.insert_fresh_gen(key, marked_tile(0.5), 5));
        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(entry.generation, 5);
        assert!((entry.tile.at(0, 0, 0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn insert_fresh_gen_zero_loses_to_high_generation() {
        let cache = TileCache::new(10_000_000);
        let key = make_key(1, 0, 0);
        assert!(cache.insert_fresh_gen(key, marked_tile(0.2), 50));
        assert!(
            !cache.insert_fresh_gen(key, marked_tile(0.9), 0),
            "decompose insert_fresh (gen 0) must not overwrite gen 50"
        );
        let entry = cache.entries.get(&key).unwrap();
        assert_eq!(entry.generation, 50);
        assert!((entry.tile.at(0, 0, 0) - 0.2).abs() < 1e-6);
    }

    #[test]
    fn clear_drops_all_entries_and_used_bytes() {
        let cache = TileCache::new(10_000_000);
        cache.insert_fresh_gen(make_key(1, 0, 0), marked_tile(0.1), 50);
        cache.insert_fresh_gen(composite_key(), marked_tile(0.2), 50);
        assert!(cache.entry_count() >= 2);
        cache.clear();
        assert_eq!(cache.entry_count(), 0);
        assert_eq!(cache.used_bytes_count(), 0);
        assert_eq!(cache.max_generation(), 0);
        assert!(cache.insert_fresh_gen(make_key(1, 0, 0), marked_tile(0.9), 51));
    }

    fn key_doc(doc: u32, x: u32, y: u32, stage: CacheStage) -> TileKey {
        TileKey {
            doc,
            layer: 0,
            coord: TileCoord { level: 0, x, y },
            stage,
        }
    }

    fn open_set(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn evict_for_pressure_single_doc_keeps_viewport_drops_outside_composite() {
        // Open Raw is pinned — off-viewport pressure drops Composite, not Raw.
        let cache = TileCache::new(TILE_BYTES);
        let vp = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        cache.get_or_insert(
            key_doc(2, 0, 0, CacheStage::Composite),
            Arc::new(PixelTile::new()),
        );
        cache.get_or_insert(
            key_doc(2, 5, 5, CacheStage::Composite),
            Arc::new(PixelTile::new()),
        );
        let mut viewport = HashSet::new();
        viewport.insert(vp);
        let open = open_set(&[2]);
        cache.evict_for_pressure(&EvictContext {
            active_doc: Some(2),
            open_docs: &open,
            viewport_coords: &viewport,
        });
        assert!(cache
            .entries
            .contains_key(&key_doc(2, 0, 0, CacheStage::Composite)));
        assert!(!cache
            .entries
            .contains_key(&key_doc(2, 5, 5, CacheStage::Composite)));
    }

    #[test]
    fn evict_for_pressure_pins_open_raw_drops_inactive_composite() {
        let cache = TileCache::new(TILE_BYTES);
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let inactive_raw = key_doc(1, 0, 0, CacheStage::Raw);
        let inactive_comp = key_doc(1, 0, 0, CacheStage::Composite);
        let active_raw = key_doc(2, 0, 0, CacheStage::Raw);
        cache.get_or_insert(inactive_raw, Arc::new(PixelTile::new()));
        cache.get_or_insert(inactive_comp, Arc::new(PixelTile::new()));
        cache.get_or_insert(active_raw, Arc::new(PixelTile::new()));
        let mut viewport = HashSet::new();
        viewport.insert(coord);
        let open = open_set(&[1, 2]);
        cache.evict_for_pressure(&EvictContext {
            active_doc: Some(2),
            open_docs: &open,
            viewport_coords: &viewport,
        });
        assert!(
            !cache.entries.contains_key(&inactive_comp),
            "inactive Composite must go under pressure"
        );
        assert!(
            cache.entries.contains_key(&inactive_raw),
            "open-session Raw must stay pinned"
        );
        assert!(cache.entries.contains_key(&active_raw));
    }

    #[test]
    fn evict_for_pressure_allows_over_budget_when_only_pinned_raw_remain() {
        let cache = TileCache::new(TILE_BYTES);
        cache.get_or_insert(key_doc(2, 0, 0, CacheStage::Raw), Arc::new(PixelTile::new()));
        cache.get_or_insert(key_doc(2, 1, 0, CacheStage::Raw), Arc::new(PixelTile::new()));
        let mut viewport = HashSet::new();
        viewport.insert(TileCoord {
            level: 0,
            x: 0,
            y: 0,
        });
        let open = open_set(&[2]);
        cache.evict_for_pressure(&EvictContext {
            active_doc: Some(2),
            open_docs: &open,
            viewport_coords: &viewport,
        });
        assert_eq!(cache.entry_count(), 2);
        assert!(cache.used_bytes_count() > cache.budget_bytes_count());
    }

    #[test]
    fn insert_then_pressure_evict_leaves_miss_for_reschedule() {
        let cache = TileCache::new(TILE_BYTES);
        let protected = key_doc(2, 0, 0, CacheStage::Composite);
        let victim = key_doc(2, 9, 9, CacheStage::Composite);
        assert!(cache.insert_fresh_gen(protected, marked_tile(0.1), 1));
        assert!(cache.insert_fresh_gen(victim, marked_tile(0.2), 1));
        let mut viewport = HashSet::new();
        viewport.insert(TileCoord {
            level: 0,
            x: 0,
            y: 0,
        });
        let open = open_set(&[2]);
        cache.evict_for_pressure(&EvictContext {
            active_doc: Some(2),
            open_docs: &open,
            viewport_coords: &viewport,
        });
        assert!(cache.entries.contains_key(&protected));
        assert!(
            !cache.entries.contains_key(&victim),
            "evicted key must miss so callers reschedule"
        );
    }

    #[test]
    fn evict_for_pressure_prefers_composite_before_raw_on_inactive() {
        let cache = TileCache::new(2 * TILE_BYTES);
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        cache.get_or_insert(
            key_doc(1, 0, 0, CacheStage::Raw),
            Arc::new(PixelTile::new()),
        );
        cache.get_or_insert(
            key_doc(1, 0, 0, CacheStage::Composite),
            Arc::new(PixelTile::new()),
        );
        cache.get_or_insert(
            key_doc(2, 0, 0, CacheStage::Raw),
            Arc::new(PixelTile::new()),
        );
        let mut viewport = HashSet::new();
        viewport.insert(coord);
        let open = open_set(&[1, 2]);
        cache.evict_for_pressure(&EvictContext {
            active_doc: Some(2),
            open_docs: &open,
            viewport_coords: &viewport,
        });
        assert!(
            !cache
                .entries
                .contains_key(&key_doc(1, 0, 0, CacheStage::Composite)),
            "inactive Composite should go before touching Raw"
        );
        assert!(cache
            .entries
            .contains_key(&key_doc(1, 0, 0, CacheStage::Raw)));
        assert!(cache
            .entries
            .contains_key(&key_doc(2, 0, 0, CacheStage::Raw)));
    }

    #[test]
    fn evict_stages_drops_processed_composite_keeps_raw() {
        let cache = TileCache::new(10_000_000);
        for stage in [
            CacheStage::Raw,
            CacheStage::Processed,
            CacheStage::Composite,
        ] {
            cache.get_or_insert(key_doc(1, 0, 0, stage), Arc::new(PixelTile::new()));
        }
        cache.evict_stages(1, &[CacheStage::Processed, CacheStage::Composite]);
        assert!(cache
            .entries
            .contains_key(&key_doc(1, 0, 0, CacheStage::Raw)));
        assert!(!cache
            .entries
            .contains_key(&key_doc(1, 0, 0, CacheStage::Processed)));
        assert!(!cache
            .entries
            .contains_key(&key_doc(1, 0, 0, CacheStage::Composite)));
        assert_eq!(cache.used_bytes_count(), TILE_BYTES);
    }
}
