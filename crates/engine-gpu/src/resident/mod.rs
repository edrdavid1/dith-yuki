//! GPU-resident tile cache (Path B).
//!
//! Resident [`Texture2DArray`] + frame scratch ping-pong. See `.cursor-spec/gpu-path-b/SPEC.md`.

mod cache;
mod format;
mod gather;
mod pipelines;
mod readback;
mod slot;

pub use cache::GpuTileCache;
pub use format::{
    compute_vram_layout, create_tile_array_desc, default_vram_config, pack_tile_upload,
    tile_array_bytes, tile_row_bytes_aligned, unpack_tile_download, VramBudgetConfig, VramLayout,
    DEFAULT_FRAME_BATCH_CAP, DEFAULT_VRAM_BUDGET_BYTES, FIXED_HEADROOM_BYTES, TILE_EXTENT,
};
pub use gather::ResidentGatherPipelines;
pub use pipelines::{
    ResidentBayerPipelines, ResidentCompositePipelines, ResidentCrtPipelines,
    ResidentHalftonePipelines, ResidentPaletteGuidedPipelines, ResidentPalettePipelines,
};
pub use readback::{ReadbackRing, TILE_CORE_RGBA8_BYTES};
pub use slot::{GpuSlotMeta, SlotAllocator, SlotHandle};
