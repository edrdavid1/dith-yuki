//! Error residuals buffer for cross-tile error diffusion.
//!
//! Stores quantization error residuals at tile edges (right and bottom)
//! and the diagonal corner patch for IncomingErrorBuffer seeding of
//! tile `(tx+1, ty+1)`.
//!
//! **Requirements:** 3.3, 3.4, 10.4; Track A Req 4

use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use engine_tiles::{TileCoord, TILE_SIZE};

use crate::types::LayerId;

/// Default fixture margin (`pixel_size=1` × widest published kernel offset 2).
/// Production buffers use [`ErrorResiduals::with_margin`] =
/// `pixel_size × kernel.max_offset()`.
pub const CORNER_PATCH: usize = 2;

/// Quantization error residuals for cross-tile error diffusion.
///
/// Edge depth is `margin` columns / rows (and an `margin × margin` corner),
/// not a fixed 2. `margin = pixel_size × kernel_max_offset` so FS at
/// `pixel_size=3` keeps a 3-wide overflow (hop `dx × ps`), not a 2-wide
/// clip that repeats every TILE_SIZE pixels.
///
/// These residuals are produced after processing a tile and consumed
/// by the right, bottom, and diagonal-neighbor tiles to initialize their
/// error buffers.
#[derive(Debug, Clone)]
pub struct ErrorResiduals {
    /// Columns/rows stored past each edge (also the corner patch size).
    pub margin: usize,

    /// Right edge: `margin` columns of residual error.
    /// Layout: `[row * margin * 3 + col * 3 + channel]`
    /// Dimensions: TILE_SIZE rows × margin columns × 3 channels.
    pub right: Vec<f32>,

    /// Bottom edge: `margin` rows of residual error.
    /// Layout: `[row * TILE_SIZE * 3 + col * 3 + channel]`
    /// Dimensions: margin rows × TILE_SIZE columns × 3 channels.
    pub bottom: Vec<f32>,

    /// Diagonal overflow for tile `(tx+1, ty+1)` top-left.
    /// Layout: `[row * margin * 3 + col * 3 + channel]`
    pub corner: Vec<f32>,
}

impl ErrorResiduals {
    /// Zeroed buffer with the legacy 2-wide fixture margin.
    pub fn new() -> Self {
        Self::with_margin(CORNER_PATCH)
    }

    /// Zeroed buffer sized for a given edge margin.
    pub fn with_margin(margin: usize) -> Self {
        let m = margin.max(1);
        Self {
            margin: m,
            right: vec![0.0; TILE_SIZE as usize * m * 3],
            bottom: vec![0.0; m * TILE_SIZE as usize * 3],
            corner: vec![0.0; m * m * 3],
        }
    }
}

impl Default for ErrorResiduals {
    fn default() -> Self {
        Self::new()
    }
}

/// Concurrent store for error residuals, keyed by
/// `(doc, LayerId, filter_key, TileCoord)`.
///
/// `filter_key` is [`crate::types::FilterInstanceId::as_u128`] so stacked ED
/// filters on one layer keep independent wavefront streams (last-write-wins
/// on a layer-only key baked 256px seams into intermediate stages).
///
/// Uses `DashMap` for lock-free concurrent access from multiple worker threads.
/// Tiles store their edge residuals after processing; neighbor tiles read them
/// to seed error propagation at boundaries.
pub struct ErrorResidualsStore {
    entries: DashMap<(u32, LayerId, u128, TileCoord), ErrorResiduals>,
    clear_count: AtomicU64,
}

impl ErrorResidualsStore {
    /// Create a new empty residuals store.
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
            clear_count: AtomicU64::new(0),
        }
    }

    /// Get residuals from the left neighbor tile (coord.x - 1).
    ///
    /// Returns `None` if the current tile is at the left edge (x == 0)
    /// or if the left neighbor has not yet been processed.
    pub fn get_left(
        &self,
        doc: u32,
        layer_id: LayerId,
        filter_key: u128,
        coord: TileCoord,
    ) -> Option<ErrorResiduals> {
        if coord.x == 0 {
            return None;
        }
        let left_coord = TileCoord {
            level: coord.level,
            x: coord.x - 1,
            y: coord.y,
        };
        self.entries
            .get(&(doc, layer_id, filter_key, left_coord))
            .map(|r| r.clone())
    }

    pub fn get_top(
        &self,
        doc: u32,
        layer_id: LayerId,
        filter_key: u128,
        coord: TileCoord,
    ) -> Option<ErrorResiduals> {
        if coord.y == 0 {
            return None;
        }
        let top_coord = TileCoord {
            level: coord.level,
            x: coord.x,
            y: coord.y - 1,
        };
        self.entries
            .get(&(doc, layer_id, filter_key, top_coord))
            .map(|r| r.clone())
    }

    pub fn get_diag(
        &self,
        doc: u32,
        layer_id: LayerId,
        filter_key: u128,
        coord: TileCoord,
    ) -> Option<ErrorResiduals> {
        if coord.x == 0 || coord.y == 0 {
            return None;
        }
        let diag_coord = TileCoord {
            level: coord.level,
            x: coord.x - 1,
            y: coord.y - 1,
        };
        self.entries
            .get(&(doc, layer_id, filter_key, diag_coord))
            .map(|r| r.clone())
    }

    pub fn store(
        &self,
        doc: u32,
        layer_id: LayerId,
        filter_key: u128,
        coord: TileCoord,
        residuals: ErrorResiduals,
    ) {
        self.entries
            .insert((doc, layer_id, filter_key, coord), residuals);
    }

    pub fn cached_layer_ids(&self) -> std::collections::HashSet<u32> {
        self.entries.iter().map(|e| e.key().1 .0).collect()
    }

    pub fn evict_layer(&self, doc: u32, layer: LayerId) {
        self.entries
            .retain(|(d, l, _, _), _| *d != doc || l.0 != layer.0);
        self.clear_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Drop residuals for the ED causal cone at `level`: all tiles with
    /// `x >= origin_x && y >= origin_y` (dependents of an invalidated origin).
    pub fn evict_downstream_cone(
        &self,
        doc: u32,
        layer: LayerId,
        level: u8,
        origin_x: u32,
        origin_y: u32,
    ) {
        self.entries.retain(|(d, l, _, c), _| {
            !(*d == doc && l.0 == layer.0 && c.level == level && c.x >= origin_x && c.y >= origin_y)
        });
        self.clear_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn evict_document(&self, doc: u32) {
        self.entries.retain(|(d, _, _, _), _| *d != doc);
    }

    /// Clear all stored residuals (full document replace / welcome).
    pub fn clear(&self) {
        self.entries.clear();
        self.clear_count.fetch_add(1, Ordering::Relaxed);
    }

    /// How many residual invalidation ops (`clear` / `evict_layer` / cone) ran.
    pub fn clear_count(&self) -> u64 {
        self.clear_count.load(Ordering::Relaxed)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for ErrorResidualsStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(x: u32, y: u32) -> TileCoord {
        TileCoord { level: 0, x, y }
    }

    #[test]
    fn error_residuals_new_is_zeroed() {
        let r = ErrorResiduals::new();
        assert_eq!(r.margin, CORNER_PATCH);
        assert_eq!(r.right.len(), TILE_SIZE as usize * CORNER_PATCH * 3);
        assert_eq!(r.bottom.len(), CORNER_PATCH * TILE_SIZE as usize * 3);
        assert_eq!(r.corner.len(), CORNER_PATCH * CORNER_PATCH * 3);
        assert!(r.right.iter().all(|&v| v == 0.0));
        assert!(r.bottom.iter().all(|&v| v == 0.0));
        assert!(r.corner.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn with_margin_sizes_edges() {
        let r = ErrorResiduals::with_margin(3);
        assert_eq!(r.margin, 3);
        assert_eq!(r.right.len(), TILE_SIZE as usize * 3 * 3);
        assert_eq!(r.bottom.len(), 3 * TILE_SIZE as usize * 3);
        assert_eq!(r.corner.len(), 3 * 3 * 3);
    }

    #[test]
    fn store_and_get_diag() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);
        let mut residuals = ErrorResiduals::new();
        residuals.corner[0] = 0.25;

        store.store(1, layer, 0, tc(1, 1), residuals);

        let got = store.get_diag(1, layer, 0, tc(2, 2));
        assert!(got.is_some());
        assert_eq!(got.unwrap().corner[0], 0.25);
    }

    #[test]
    fn get_diag_returns_none_at_edge() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);
        assert!(store.get_diag(1, layer, 0, tc(0, 1)).is_none());
        assert!(store.get_diag(1, layer, 0, tc(1, 0)).is_none());
    }

    #[test]
    fn store_and_get_left() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);
        let mut residuals = ErrorResiduals::new();
        residuals.right[0] = 0.5;

        // Store residuals for tile (2, 3)
        store.store(1, layer, 0, tc(2, 3), residuals);

        // Get from the right neighbor's perspective (tile 3, 3 looking left)
        let got = store.get_left(1, layer, 0, tc(3, 3));
        assert!(got.is_some());
        assert_eq!(got.unwrap().right[0], 0.5);
    }

    #[test]
    fn get_left_returns_none_at_edge() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // x == 0 means no left neighbor
        let got = store.get_left(1, layer, 0, tc(0, 5));
        assert!(got.is_none());
    }

    #[test]
    fn get_left_returns_none_when_not_stored() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // Left neighbor (4, 3) not stored
        let got = store.get_left(1, layer, 0, tc(5, 3));
        assert!(got.is_none());
    }

    #[test]
    fn store_and_get_top() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(2);
        let mut residuals = ErrorResiduals::new();
        residuals.bottom[0] = 0.75;

        // Store residuals for tile (4, 1)
        store.store(1, layer, 0, tc(4, 1), residuals);

        // Get from the bottom neighbor's perspective (tile 4, 2 looking up)
        let got = store.get_top(1, layer, 0, tc(4, 2));
        assert!(got.is_some());
        assert_eq!(got.unwrap().bottom[0], 0.75);
    }

    #[test]
    fn get_top_returns_none_at_edge() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // y == 0 means no top neighbor
        let got = store.get_top(1, layer, 0, tc(5, 0));
        assert!(got.is_none());
    }

    #[test]
    fn get_top_returns_none_when_not_stored() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // Top neighbor (3, 4) not stored
        let got = store.get_top(1, layer, 0, tc(3, 5));
        assert!(got.is_none());
    }

    #[test]
    fn evict_downstream_cone_keeps_upstream_tiles() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);
        store.store(1, layer, 0, tc(0, 0), ErrorResiduals::new());
        store.store(1, layer, 0, tc(2, 0), ErrorResiduals::new());
        store.store(1, layer, 0, tc(0, 2), ErrorResiduals::new());
        store.store(1, layer, 0, tc(2, 2), ErrorResiduals::new());

        store.evict_downstream_cone(1, layer, 0, 1, 1);

        assert!(store.get_left(1, layer, 0, tc(1, 0)).is_some()); // (0,0) kept
        assert!(store.get_top(1, layer, 0, tc(0, 1)).is_some()); // (0,0)
                                                              // (2,2) in cone — gone (get_left of (3,2) would need (2,2))
        assert!(store.get_left(1, layer, 0, tc(3, 2)).is_none());
        // (2,0): x>=1 but y=0 < 1 — kept
        assert!(store.get_left(1, layer, 0, tc(3, 0)).is_some());
        // (0,2): y>=1 but x=0 < 1 — kept
        assert!(store.get_top(1, layer, 0, tc(0, 3)).is_some());
    }

    #[test]
    fn clear_removes_all_entries() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        store.store(1, layer, 0, tc(0, 0), ErrorResiduals::new());
        store.store(1, layer, 0, tc(1, 0), ErrorResiduals::new());
        store.store(1, layer, 0, tc(0, 1), ErrorResiduals::new());

        store.clear();

        assert!(store.get_left(1, layer, 0, tc(1, 0)).is_none());
        assert!(store.get_top(1, layer, 0, tc(0, 1)).is_none());
    }

    #[test]
    fn evict_layer_removes_target_keeps_other() {
        let store = ErrorResidualsStore::new();
        let layer_a = LayerId::new(1);
        let layer_b = LayerId::new(2);

        let mut a = ErrorResiduals::new();
        a.right[0] = 1.0;
        store.store(1, layer_a, 0, tc(3, 3), a);
        store.store(1, layer_b, 0, tc(3, 3), ErrorResiduals::new());

        store.evict_layer(1, layer_a);

        assert!(store.get_left(1, layer_a, 0, tc(4, 3)).is_none());
        assert!(store.get_left(1, layer_b, 0, tc(4, 3)).is_some());
    }

    #[test]
    fn different_filter_keys_are_independent() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        let mut a = ErrorResiduals::new();
        a.right[0] = 0.25;
        let mut b = ErrorResiduals::new();
        b.right[0] = 0.75;
        store.store(1, layer, 11, tc(0, 0), a);
        store.store(1, layer, 22, tc(0, 0), b);

        assert_eq!(
            store.get_left(1, layer, 11, tc(1, 0)).unwrap().right[0],
            0.25
        );
        assert_eq!(
            store.get_left(1, layer, 22, tc(1, 0)).unwrap().right[0],
            0.75
        );
    }
}
