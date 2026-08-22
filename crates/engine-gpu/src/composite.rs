//! Multi-layer composite types (Path B T7.5) — independent of filter graph.

use engine_tiles::{TileCoord, TileKey};

/// Porter-Duff over + blend mode (matches `engine_project::BlendMode` discriminant).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompositePassParams {
    pub blend_mode: u32,
    pub opacity: f32,
}

/// One leaf layer in a bottom-up composite chain.
#[derive(Clone)]
pub struct GpuCompositeLayerOp {
    /// Must already be resident (or will be promoted from `pixels` if provided on work).
    pub processed_key: TileKey,
    pub blend_mode: u32,
    pub opacity: f32,
    /// Optional upload when Processed is not yet in GPU cache.
    pub pixels: Option<std::sync::Arc<engine_tiles::PixelTile>>,
}

impl std::fmt::Debug for GpuCompositeLayerOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuCompositeLayerOp")
            .field("processed_key", &self.processed_key)
            .field("blend_mode", &self.blend_mode)
            .field("opacity", &self.opacity)
            .field("pixels", &self.pixels.as_ref().map(|_| "PixelTile(..)"))
            .finish()
    }
}

/// One document tile: blend layers bottom→top into `composite_key`.
#[derive(Clone, Debug)]
pub struct GpuCompositeTileWork {
    pub coord: TileCoord,
    pub composite_key: TileKey,
    pub generation: u64,
    /// Bottom-to-top order (same as CPU `composite_nodes`).
    pub layers: Vec<GpuCompositeLayerOp>,
}

#[derive(Clone, Debug)]
pub struct GpuCompositeFrameJob {
    pub doc_gen: u64,
    pub tiles: Vec<GpuCompositeTileWork>,
}
