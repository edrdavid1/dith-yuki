//! Color space conversions, palette management, KD-tree nearest-color search,
//! and threshold map loading for the Dither Yuki 2 engine.
//!
//! This crate provides:
//! - Oklab color space conversions (linear RGB ↔ Oklab)
//! - KD-tree for efficient nearest-neighbor palette lookups
//! - 3D Oklab LUT for O(1) nearest-color in hot paths
//! - Palette entity management (import/export/generation)
//! - Concurrent palette KD-tree / LUT caches (DashMap-based)
//! - Threshold map loading and sampling for ordered dithering

pub mod oklab;
pub mod oklch;
pub mod ramps;
pub mod harmony;
pub mod kdtree;
pub mod palette;
pub mod palette_cache;
pub mod palette_lut;
pub mod palette_guided;
pub mod threshold_map;

pub use palette_lut::{PaletteLut3D, PaletteLutCache, DEFAULT_LUT_SIZE};
pub use palette_guided::{
    default_channel_levels, palette_channel_ranges, quantize_channel_guided, ChannelRange,
    PaletteChannelRangeCache,
};

pub use oklab::{linear_to_oklab, oklab_dist_sq, oklab_to_linear, oklab_to_linear_unclamped, LinRgb, Oklab};
pub use oklch::{clip_to_srgb_gamut, is_out_of_srgb_gamut, OkLch};
pub use ramps::generate_ramp;
pub use harmony::{generate_harmony, generate_harmony_with_spread, HarmonyRule};
