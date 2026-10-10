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

pub mod auto_interpolate;
pub mod brightness_sorted;
pub mod display_rgb;
pub mod harmony;
pub mod icc_precheck;
pub mod kdtree;
pub mod oklab;
pub mod oklch;
pub mod palette;
pub mod palette_cache;
pub mod palette_guided;
pub mod palette_lut;
pub mod preview_encode;
pub mod print_export;
pub mod ramps;
pub mod soft_proof;
pub mod threshold_map;

pub use display_rgb::{DisplayRgbF32, LinearRgbF32};
pub use icc_precheck::precheck_icc_bytes;
pub use preview_encode::{
    delta_e2000, delta_e2000_srgb8, encode_preview_rgb, encode_preview_rgba, linear_to_srgb_f32,
    srgb_f32_to_linear, SrgbEncodeLut,
};
pub use print_export::{
    count_unique_rgb, estimate_export, gamut_report, rgba8_to_cmyk8, scale_cmyk8_nearest,
    scaled_dimensions, validate_config, ExportSummary, GamutReport, PrintExportConfig,
    PrintExportError, PrintExportEstimate, PrintExportFormat, PrintExportTransform,
    TiffCompression, GAMUT_DELTA_E2000_THRESHOLD, MAX_PALETTE_COLORS,
};
pub use soft_proof::{
    soft_proof_config_hash, SoftProofConfig, SoftProofError, SoftProofIntent, SoftProofLut3D,
    SoftProofTransform, BUILTIN_FOGRA51_ICC, BUILTIN_FOGRA51_ID, BUILTIN_FOGRA52_ID,
    DEFAULT_PROOF_LUT_SIZE, FALLBACK_PROOF_LUT_SIZE, LUT_MAX_DELTA_E2000, LUT_SIZE_CANDIDATES,
};

pub use auto_interpolate::{auto_interpolate, would_auto_interpolate, AutoInterpolateResult};
pub use brightness_sorted::BrightnessSortedPalette;
pub use palette_guided::{
    default_channel_levels, palette_channel_ranges, quantize_channel_guided, ChannelRange,
    PaletteChannelRangeCache,
};
pub use palette_lut::{PaletteLut3D, PaletteLutCache, DEFAULT_LUT_SIZE};

pub use harmony::{generate_harmony, generate_harmony_with_spread, HarmonyRule};
pub use oklab::{
    linear_to_oklab, oklab_dist_sq, oklab_to_linear, oklab_to_linear_unclamped, LinRgb, Oklab,
};
pub use oklch::{clip_to_srgb_gamut, is_out_of_srgb_gamut, OkLch};
pub use ramps::generate_ramp;
