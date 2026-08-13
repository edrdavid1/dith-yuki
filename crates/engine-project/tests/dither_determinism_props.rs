//! Property-based tests for dither filter determinism.
//!
//! **Validates: Requirement 9 (AC 5)**
//!
//! Property 13: Determinism
//! - For any valid input tile, TileCoord, and DitherParamsV2, applying the dither filter
//!   twice with the same inputs SHALL produce byte-identical output tiles.
//!
//! This is tested for both ordered dithering and error diffusion.
//! Error diffusion uses separate fresh `ErrorResidualsStore` instances each time
//! to ensure the determinism property holds independently.

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

/// Generate valid pixel_size in [1, 32].
fn arb_pixel_size() -> impl Strategy<Value = u8> {
    1u8..=32u8
}

/// Generate a color mode.
fn arb_color_mode() -> impl Strategy<Value = DitherColorMode> {
    prop_oneof![
        Just(DitherColorMode::Rgb),
        Just(DitherColorMode::Grayscale),
    ]
}

/// Generate a tile coordinate component.
fn arb_tile_coord() -> impl Strategy<Value = u32> {
    0u32..50u32
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

// ─── Property Tests ───────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// **Validates: Requirement 9 (AC 5)**
    ///
    /// Property 13: Determinism (Ordered Dithering)
    ///
    /// Apply ordered dithering twice with the same inputs and verify byte-identical output.
    #[test]
    fn ordered_dithering_is_deterministic(
        seed in arb_seed(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size(),
        color_mode in arb_color_mode(),
        tx in arb_tile_coord(),
        ty in arb_tile_coord(),
    ) {
        let tile = make_random_tile(seed);
        let params = DitherParamsV2 {
            mode,
            levels,
            threshold_scale: ts,
            pixel_size: ps,
            color_mode,
            palette_id: None,
            ..Default::default()
        };

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 2048, 2048);
        let coord = TileCoord { level: 0, x: tx, y: ty };

        // First application
        let result1 = apply_ordered(
            &tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Second application with same inputs
        let result2 = apply_ordered(
            &tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Verify byte-identical output
        prop_assert_eq!(
            &result1.data[..], &result2.data[..],
            "Ordered dithering produced non-deterministic output for seed={}, mode={:?}, levels={}, ts={}, ps={}, color_mode={:?}, coord=({}, {})",
            seed, params.mode, levels, ts, ps, color_mode, tx, ty
        );
    }

    /// **Validates: Requirement 9 (AC 5)**
    ///
    /// Property 13: Determinism (Error Diffusion)
    ///
    /// Apply error diffusion twice with the same inputs (using separate fresh
    /// ErrorResidualsStore instances) and verify byte-identical output.
    #[test]
    fn error_diffusion_is_deterministic(
        seed in arb_seed(),
        mode in arb_diffusion_mode(),
        levels in arb_levels(),
        ps in arb_pixel_size(),
        color_mode in arb_color_mode(),
        tx in arb_tile_coord(),
        ty in arb_tile_coord(),
    ) {
        let tile = make_random_tile(seed);
        let params = DitherParamsV2 {
            mode,
            levels,
            threshold_scale: 1.0,
            pixel_size: ps,
            color_mode,
            palette_id: None,
            ..Default::default()
        };

        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 2048, 2048);
        let layer_id = LayerId::new(1);
        let coord = TileCoord { level: 0, x: tx, y: ty };

        // First application with fresh store
        let store1 = ErrorResidualsStore::new();
        let result1 = apply_error_diffusion(
            &tile, coord, &params, &store1, layer_id, &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Second application with fresh store
        let store2 = ErrorResidualsStore::new();
        let result2 = apply_error_diffusion(
            &tile, coord, &params, &store2, layer_id, &palette_cache, &lut_cache, &doc,
        ).unwrap();

        // Verify byte-identical output
        prop_assert_eq!(
            &result1.data[..], &result2.data[..],
            "Error diffusion produced non-deterministic output for seed={}, mode={:?}, levels={}, ps={}, color_mode={:?}, coord=({}, {})",
            seed, params.mode, levels, ps, color_mode, tx, ty
        );
    }
}
