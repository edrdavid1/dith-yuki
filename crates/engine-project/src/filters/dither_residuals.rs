//! Error residuals buffer for cross-tile error diffusion.
//!
//! Stores quantization error residuals at tile edges (right and bottom)
//! and the diagonal corner patch for IncomingErrorBuffer seeding of
//! tile `(tx+1, ty+1)`.
//!
//! **Requirements:** 3.3, 3.4, 10.4; Track A Req 4

use dashmap::DashMap;
use engine_tiles::{TileCoord, TILE_SIZE};

use crate::types::LayerId;

/// Patch size for diagonal overflow into the top-left of tile `(tx+1, ty+1)`.
/// Covers FS `(+1,+1)` and Atkinson kernel reach past both edges (up to 2 px).
pub const CORNER_PATCH: usize = 2;

/// Quantization error residuals for cross-tile error diffusion.
///
/// Stores right-edge (2 columns × TILE_SIZE rows × 3 channels),
/// bottom-edge (TILE_SIZE columns × 2 rows × 3 channels), and
/// corner patch (`CORNER_PATCH` × `CORNER_PATCH` × 3) for diagonal overflow.
///
/// These residuals are produced after processing a tile and consumed
/// by the right, bottom, and diagonal-neighbor tiles to initialize their
/// error buffers.
#[derive(Debug, Clone)]
pub struct ErrorResiduals {
    /// Right edge: 2 columns of residual error.
    /// Layout: `[row * 2 * 3 + col * 3 + channel]`
    /// Dimensions: TILE_SIZE rows × 2 columns × 3 channels.
    pub right: Vec<f32>,

    /// Bottom edge: 2 rows of residual error.
    /// Layout: `[row * TILE_SIZE * 3 + col * 3 + channel]`
    /// Dimensions: 2 rows × TILE_SIZE columns × 3 channels.
    pub bottom: Vec<f32>,

    /// Diagonal overflow for tile `(tx+1, ty+1)` top-left.
    /// Layout: `[row * CORNER_PATCH * 3 + col * 3 + channel]`
    /// Dimensions: CORNER_PATCH rows × CORNER_PATCH cols × 3 channels.
    pub corner: Vec<f32>,
}

impl ErrorResiduals {
    /// Create a new zeroed `ErrorResiduals` buffer.
    pub fn new() -> Self {
        Self {
            right: vec![0.0; TILE_SIZE as usize * 2 * 3],
            bottom: vec![0.0; 2 * TILE_SIZE as usize * 3],
            corner: vec![0.0; CORNER_PATCH * CORNER_PATCH * 3],
        }
    }
}

impl Default for ErrorResiduals {
    fn default() -> Self {
        Self::new()
    }
}

/// Concurrent store for error residuals, keyed by `(LayerId, TileCoord)`.
///
/// Uses `DashMap` for lock-free concurrent access from multiple worker threads.
/// Tiles store their edge residuals after processing; neighbor tiles read them
/// to seed error propagation at boundaries.
pub struct ErrorResidualsStore {
    entries: DashMap<(LayerId, TileCoord), ErrorResiduals>,
}

impl ErrorResidualsStore {
    /// Create a new empty residuals store.
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Get residuals from the left neighbor tile (coord.x - 1).
    ///
    /// Returns `None` if the current tile is at the left edge (x == 0)
    /// or if the left neighbor has not yet been processed.
    pub fn get_left(&self, layer_id: LayerId, coord: TileCoord) -> Option<ErrorResiduals> {
        if coord.x == 0 {
            return None;
        }
        let left_coord = TileCoord {
            level: coord.level,
            x: coord.x - 1,
            y: coord.y,
        };
        self.entries.get(&(layer_id, left_coord)).map(|r| r.clone())
    }

    /// Get residuals from the top neighbor tile (coord.y - 1).
    ///
    /// Returns `None` if the current tile is at the top edge (y == 0)
    /// or if the top neighbor has not yet been processed.
    pub fn get_top(&self, layer_id: LayerId, coord: TileCoord) -> Option<ErrorResiduals> {
        if coord.y == 0 {
            return None;
        }
        let top_coord = TileCoord {
            level: coord.level,
            x: coord.x,
            y: coord.y - 1,
        };
        self.entries.get(&(layer_id, top_coord)).map(|r| r.clone())
    }

    /// Get residuals from the diagonal neighbor tile `(coord.x - 1, coord.y - 1)`.
    ///
    /// Used to seed the IncomingErrorBuffer corner channel into the top-left
    /// of the current tile. Returns `None` at the top or left document edge,
    /// or if that neighbor has not been processed yet.
    pub fn get_diag(&self, layer_id: LayerId, coord: TileCoord) -> Option<ErrorResiduals> {
        if coord.x == 0 || coord.y == 0 {
            return None;
        }
        let diag_coord = TileCoord {
            level: coord.level,
            x: coord.x - 1,
            y: coord.y - 1,
        };
        self.entries.get(&(layer_id, diag_coord)).map(|r| r.clone())
    }

    /// Store residuals after processing a tile.
    ///
    /// Overwrites any previously stored residuals for this (layer, coord) pair.
    pub fn store(&self, layer_id: LayerId, coord: TileCoord, residuals: ErrorResiduals) {
        self.entries.insert((layer_id, coord), residuals);
    }

    /// Layer ids that currently have residual entries.
    pub fn cached_layer_ids(&self) -> std::collections::HashSet<u32> {
        self.entries.iter().map(|e| e.key().0.0).collect()
    }

    /// Drop every residual entry for `layer`. Missing keys are a no-op.
    pub fn evict_layer(&self, layer: LayerId) {
        self.entries.retain(|(l, _), _| l.0 != layer.0);
    }

    /// Clear all stored residuals (on document change or invalidation).
    pub fn clear(&self) {
        self.entries.clear();
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
        assert_eq!(r.right.len(), TILE_SIZE as usize * 2 * 3);
        assert_eq!(r.bottom.len(), 2 * TILE_SIZE as usize * 3);
        assert_eq!(r.corner.len(), CORNER_PATCH * CORNER_PATCH * 3);
        assert!(r.right.iter().all(|&v| v == 0.0));
        assert!(r.bottom.iter().all(|&v| v == 0.0));
        assert!(r.corner.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn store_and_get_diag() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);
        let mut residuals = ErrorResiduals::new();
        residuals.corner[0] = 0.25;

        store.store(layer, tc(1, 1), residuals);

        let got = store.get_diag(layer, tc(2, 2));
        assert!(got.is_some());
        assert_eq!(got.unwrap().corner[0], 0.25);
    }

    #[test]
    fn get_diag_returns_none_at_edge() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);
        assert!(store.get_diag(layer, tc(0, 1)).is_none());
        assert!(store.get_diag(layer, tc(1, 0)).is_none());
    }

    #[test]
    fn store_and_get_left() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);
        let mut residuals = ErrorResiduals::new();
        residuals.right[0] = 0.5;

        // Store residuals for tile (2, 3)
        store.store(layer, tc(2, 3), residuals);

        // Get from the right neighbor's perspective (tile 3, 3 looking left)
        let got = store.get_left(layer, tc(3, 3));
        assert!(got.is_some());
        assert_eq!(got.unwrap().right[0], 0.5);
    }

    #[test]
    fn get_left_returns_none_at_edge() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // x == 0 means no left neighbor
        let got = store.get_left(layer, tc(0, 5));
        assert!(got.is_none());
    }

    #[test]
    fn get_left_returns_none_when_not_stored() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // Left neighbor (4, 3) not stored
        let got = store.get_left(layer, tc(5, 3));
        assert!(got.is_none());
    }

    #[test]
    fn store_and_get_top() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(2);
        let mut residuals = ErrorResiduals::new();
        residuals.bottom[0] = 0.75;

        // Store residuals for tile (4, 1)
        store.store(layer, tc(4, 1), residuals);

        // Get from the bottom neighbor's perspective (tile 4, 2 looking up)
        let got = store.get_top(layer, tc(4, 2));
        assert!(got.is_some());
        assert_eq!(got.unwrap().bottom[0], 0.75);
    }

    #[test]
    fn get_top_returns_none_at_edge() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // y == 0 means no top neighbor
        let got = store.get_top(layer, tc(5, 0));
        assert!(got.is_none());
    }

    #[test]
    fn get_top_returns_none_when_not_stored() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        // Top neighbor (3, 4) not stored
        let got = store.get_top(layer, tc(3, 5));
        assert!(got.is_none());
    }

    #[test]
    fn clear_removes_all_entries() {
        let store = ErrorResidualsStore::new();
        let layer = LayerId::new(1);

        store.store(layer, tc(0, 0), ErrorResiduals::new());
        store.store(layer, tc(1, 0), ErrorResiduals::new());
        store.store(layer, tc(0, 1), ErrorResiduals::new());

        store.clear();

        assert!(store.get_left(layer, tc(1, 0)).is_none());
        assert!(store.get_top(layer, tc(0, 1)).is_none());
    }

    #[test]
    fn evict_layer_removes_target_keeps_other() {
        let store = ErrorResidualsStore::new();
        let layer_a = LayerId::new(1);
        let layer_b = LayerId::new(2);

        let mut a = ErrorResiduals::new();
        a.right[0] = 1.0;
        store.store(layer_a, tc(3, 3), a);
        store.store(layer_b, tc(3, 3), ErrorResiduals::new());

        store.evict_layer(layer_a);

        assert!(store.get_left(layer_a, tc(4, 3)).is_none());
        assert!(store.get_left(layer_b, tc(4, 3)).is_some());
    }

    #[test]
    fn different_layers_are_independent() {
        let store = ErrorResidualsStore::new();
        let layer_a = LayerId::new(1);
        let layer_b = LayerId::new(2);

        let mut residuals = ErrorResiduals::new();
        residuals.right[0] = 1.0;
        store.store(layer_a, tc(3, 3), residuals);

        // Layer B has no data at that coord
        assert!(store.get_left(layer_b, tc(4, 3)).is_none());

        // Layer A does
        let got = store.get_left(layer_a, tc(4, 3));
        assert!(got.is_some());
        assert_eq!(got.unwrap().right[0], 1.0);
    }
}
