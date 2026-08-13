//! Property-based tests for uniform quantization level validity.
//!
//! **Validates: Requirement 7 (AC 1-4)**
//!
//! Property 10: Uniform Quantization Level Validity
//! - For any input tile processed with `palette_id = null` and `levels = L`, every output
//!   pixel channel value SHALL be a member of the set `{k / (L-1) : k ∈ {0, 1, ..., L-1}}`.
//!
//! This property is tested for both ordered dithering and error diffusion modes.
//! For every output pixel, we verify each channel value v satisfies:
//!   `(v * (levels - 1)).round() - v * (levels - 1)).abs() < epsilon`

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::apply_error_diffusion;
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use proptest::prelude::*;

/// Tile full size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO; // 260

/// Epsilon for floating-point comparison of quantized levels.
const EPSILON: f32 = 1e-4;

// ─── Strategies ───────────────────────────────────────────────────────────────

/// Generate an ordered dither mode.
fn arb_ordered_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::Bayer2x2),
        Just(DitherModeV2::Bayer4x4),
        Just(DitherModeV2::Bayer8x8),
    ]
}

/// Generate an error diffusion mode.
fn arb_diffusion_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::FloydSteinberg),
        Just(DitherModeV2::Atkinson),
        Just(DitherModeV2::JarvisJudiceNinke),
        Just(DitherModeV2::Stucki),
        Just(DitherModeV2::Burkes),
        Just(DitherModeV2::Sierra),
    ]
}

/// Generate valid levels in [2, 256].
fn arb_levels() -> impl Strategy<Value = u16> {
    2u16..=256u16
}

/// Generate valid threshold_scale in [0.1, 4.0].
fn arb_threshold_scale() -> impl Strategy<Value = f32> {
    (10u32..=400u32).prop_map(|v| v as f32 / 100.0)
}

/// Generate a seed for deterministic random tile generation.
fn arb_seed() -> impl Strategy<Value = u64> {
    any::<u64>()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Create a seeded random tile with pixel values in [0.0, 1.0].
/// Uses a simple xorshift-based PRNG for reproducibility.
fn make_random_tile(seed: u64) -> PixelTile {
    let mut tile = PixelTile::new();
    let mut state = seed.wrapping_add(1); // avoid zero seed

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            for c in 0..4u32 {
                // xorshift64
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let value = (state & 0xFFFF) as f32 / 65535.0;
                tile.set(x, y, c, value);
            }
        }
    }
    tile
}

/// Check if a value is a valid quantization level for the given number of levels.
/// Valid values are exactly `k / (L-1)` for k in {0, 1, ..., L-1}.
#[inline]
fn is_valid_level(v: f32, levels: f32) -> bool {
    let scaled = v * (levels - 1.0);
    (scaled - scaled.round()).abs() < EPSILON
}

// ─── Property Tests ───────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// **Validates: Requirement 7 (AC 1-4)**
    ///
    /// Property 10: Uniform Quantization Level Validity (Ordered Dithering)
    ///
    /// For random tiles processed with ordered dithering and palette_id=null,
    /// every output channel value must be a valid quantization level.
    #[test]
    fn ordered_dithering_produces_valid_levels(
        seed in arb_seed(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
    ) {
        let tile = make_random_tile(seed);
        let params = DitherParamsV2 {
            mode,
            levels,
            threshold_scale: ts,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        };

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 512, 512);
        let coord = TileCoord { level: 0, x: 1, y: 2 };

        let result = apply_ordered(
            &tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        ).unwrap();

        let lvl = levels as f32;
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                for c in 0..3u32 {
                    let v = result.at(x, y, c);
                    prop_assert!(
                        is_valid_level(v, lvl),
                        "Invalid level at ({}, {}, ch={}): v={}, levels={}, scaled={}",
                        x, y, c, v, levels, v * (lvl - 1.0)
                    );
                }
            }
        }
    }

    /// **Validates: Requirement 7 (AC 1-4)**
    ///
    /// Property 10: Uniform Quantization Level Validity (Error Diffusion)
    ///
    /// For random tiles processed with error diffusion and palette_id=null,
    /// every output channel value in the core area must be a valid quantization level.
    #[test]
    fn error_diffusion_produces_valid_levels(
        seed in arb_seed(),
        mode in arb_diffusion_mode(),
        levels in arb_levels(),
    ) {
        let tile = make_random_tile(seed);
        let params = DitherParamsV2 {
            mode,
            levels,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        };

        let store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 512, 512);
        let layer_id = LayerId::new(1);
        let coord = TileCoord { level: 0, x: 0, y: 0 };

        let result = apply_error_diffusion(
            &tile, coord, &params, &store, layer_id, &palette_cache, &lut_cache, &doc,
        ).unwrap();

        let lvl = levels as f32;
        // Check core area only (error diffusion processes the core TILE_SIZE×TILE_SIZE area)
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                for c in 0..3u32 {
                    let v = result.at(x, y, c);
                    prop_assert!(
                        is_valid_level(v, lvl),
                        "Invalid level at ({}, {}, ch={}): v={}, levels={}, scaled={}",
                        x, y, c, v, levels, v * (lvl - 1.0)
                    );
                }
            }
        }
    }
}
