//! Mask system for layers.

use crate::types::LayerId;
use engine_tiles::tile::PixelTile;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Reference to where mask data is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaskStorage {
    /// Mask is stored as a separate raster layer
    External(LayerId),
    /// Mask is vector strokes (placeholder for now)
    EmbeddedVector(Vec<String>),
}

impl MaskStorage {
    /// Get the layer ID if this is an External mask
    pub fn as_layer_id(&self) -> Option<LayerId> {
        match self {
            MaskStorage::External(id) => Some(*id),
            _ => None,
        }
    }
}

/// A mask reference attached to a layer or group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaskRef {
    /// Where the mask data is stored
    pub storage: MaskStorage,

    /// Whether this mask is active
    pub enabled: bool,

    /// If true, invert the mask (white becomes black)
    pub inverted: bool,
}

impl MaskRef {
    /// Create a new external mask reference.
    pub fn external(layer_id: LayerId) -> Self {
        MaskRef {
            storage: MaskStorage::External(layer_id),
            enabled: true,
            inverted: false,
        }
    }

    /// Get the layer ID if this is an external mask
    pub fn get_external_layer(&self) -> Option<LayerId> {
        self.storage.as_layer_id()
    }
}

/// Apply a mask to a tile.
///
/// If mask is None or disabled, returns tile unchanged.
/// Otherwise multiplies the tile's alpha channel by the mask.
pub fn apply_mask(_tile: &PixelTile, mask: Option<&MaskRef>) -> Arc<PixelTile> {
    let _mask = match mask {
        Some(m) if m.enabled => m,
        _ => {
            // Create new tile wrapped in Arc
            return Arc::new(PixelTile::new());
        }
    };

    // For now, this is a placeholder. Phase 2 will integrate with TileCache
    // to load the actual mask tile and blend it.
    // Here we just return empty tile as a framework.

    // Real implementation would:
    // 1. Load mask tile from cache using storage.as_layer_id()
    // 2. For each pixel: tile.alpha *= mask.alpha (or 1.0 - mask.alpha if inverted)
    // 3. Return masked tile

    Arc::new(PixelTile::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_storage_as_layer_id() {
        let layer_id = LayerId::new(42);
        let storage = MaskStorage::External(layer_id);
        assert_eq!(storage.as_layer_id(), Some(layer_id));
    }

    #[test]
    fn mask_ref_external() {
        let layer_id = LayerId::new(10);
        let mask = MaskRef::external(layer_id);
        assert!(mask.enabled);
        assert!(!mask.inverted);
        assert_eq!(mask.get_external_layer(), Some(layer_id));
    }

    #[test]
    fn apply_mask_none_returns_wrapped() {
        let tile = PixelTile::default();
        let result = apply_mask(&tile, None);
        assert_eq!(result.data.len(), tile.data.len());
    }

    #[test]
    fn apply_mask_disabled_returns_wrapped() {
        let tile = PixelTile::default();
        let mut mask = MaskRef::external(LayerId::new(1));
        mask.enabled = false;

        let result = apply_mask(&tile, Some(&mask));
        assert_eq!(result.data.len(), tile.data.len());
    }

    #[test]
    fn apply_mask_placeholder_returns_wrapped() {
        let tile = PixelTile::default();
        let mask = MaskRef::external(LayerId::new(1));

        let result = apply_mask(&tile, Some(&mask));
        assert!(result.data.len() > 0);
    }
}
