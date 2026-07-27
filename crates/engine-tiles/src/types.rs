//! Core addressing types for the tile engine.
//!
//! This module defines the fundamental types used throughout the tile caching and pyramid system.
//! For architecture details, see `tile-engine-architecture.md` §1 (Addressing Types).
//!
//! # Overview
//!
//! - **LayerId**: Stable unique identifier for a layer within a document
//! - **MipLevel**: Pyramid level within the image (0 = full resolution, 1 = 1:2 downsampled, etc.)
//! - **TileCoord**: 3D coordinate in tile space (level, x, y)
//! - **TileKey**: Complete address of a tile (layer, coord, stage)
//! - **CacheStage**: Lifecycle stage of tile data (Raw → Processed → Composite)
//! - **TILE_SIZE**: Fixed tile dimension (256 pixels per side)
//! - **HALO**: Overlap region for error diffusion filters (2 pixels)

/// Unique identifier for a layer, stable across document lifetime.
pub type LayerId = u32;

/// Pyramid level: 0 = full resolution, 1 = 1:2 downsampled, 2 = 1:4, etc.
pub type MipLevel = u8;

/// Fixed tile dimension in pixels per side.
pub const TILE_SIZE: u32 = 256;

/// Overlap region for error diffusion filters (e.g., Floyd-Steinberg dithering).
/// Tiles are processed with this many extra pixels on each side to maintain correctness at boundaries.
pub const HALO: u32 = 2;

/// 3D coordinate identifying a tile within one pyramid level of a layer.
///
/// - `level`: Pyramid level (0 = full resolution)
/// - `x`: Horizontal tile index at this level
/// - `y`: Vertical tile index at this level
///
/// For example, at level 0, a 512×512 image has a 2×2 grid of tiles (x,y ∈ {0,1}).
/// At level 1 (downsampled 1:2), the same image has a 1×1 grid of tiles.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileCoord {
    pub level: MipLevel,
    pub x: u32,
    pub y: u32,
}

/// Lifecycle stage of tile data in the cache.
///
/// Represents three stages of processing:
/// - **Raw**: Original pixels of the layer, before any filters
/// - **Processed**: After layer-specific filters and masks
/// - **Composite**: After blending with layers below (final visible result for this layer)
///
/// This three-stage model enables selective invalidation and fine-grained cache coherence.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CacheStage {
    /// Original pixels from the layer source, no filters applied.
    Raw,
    /// After applying layer filters and masks, before blending with layers below.
    Processed,
    /// After blending with all layers below and applying opacity/blend mode.
    Composite,
}

/// Complete stable address of a tile in the cache.
///
/// Uniquely identifies a single tile by specifying:
/// - `layer`: Which layer the tile belongs to
/// - `coord`: The spatial and pyramid-level coordinate (level, x, y)
/// - `stage`: Which processing stage (Raw, Processed, or Composite)
///
/// This type is used as the key in the TileCache DashMap and is hashable and copyable
/// for efficient lookups and task scheduling.
///
/// # Examples
///
/// ```ignore
/// // Raw pixel data for layer 5, tile (256, 256) at pyramid level 0
/// let raw_key = TileKey {
///     layer: 5,
///     coord: TileCoord { level: 0, x: 1, y: 1 },
///     stage: CacheStage::Raw,
/// };
///
/// // Processed data (after filters) for the same tile
/// let processed_key = TileKey {
///     layer: 5,
///     coord: TileCoord { level: 0, x: 1, y: 1 },
///     stage: CacheStage::Processed,
/// };
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileKey {
    pub layer: LayerId,
    pub coord: TileCoord,
    pub stage: CacheStage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_coord_is_hashable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(TileCoord {
            level: 0,
            x: 0,
            y: 0,
        });
        set.insert(TileCoord {
            level: 1,
            x: 0,
            y: 0,
        });
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn tile_key_is_hashable_and_copyable() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        let key1 = TileKey {
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let key2 = TileKey {
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Processed,
        };
        set.insert(key1);
        set.insert(key2);
        assert_eq!(set.len(), 2);

        // Verify copyability
        let key_copy = key1;
        assert_eq!(key_copy, key1);
    }

    #[test]
    fn cache_stage_equality() {
        assert_eq!(CacheStage::Raw, CacheStage::Raw);
        assert_ne!(CacheStage::Raw, CacheStage::Processed);
        assert_ne!(CacheStage::Processed, CacheStage::Composite);
    }

    #[test]
    fn constants_are_defined() {
        assert_eq!(TILE_SIZE, 256);
        assert_eq!(HALO, 2);
    }
}
