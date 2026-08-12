//! Property-based tests for grayscale output uniformity.
//!
//! **Validates: Requirement 5 (AC 2)**
//!
//! Property 8: Grayscale Output Uniformity
//! - For any input tile processed with `color_mode = Grayscale`, every output pixel
//!   SHALL have R = G = B (ignoring alpha).
//!
//! Tests both ordered dithering and error diffusion in grayscale mode.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::filters::dither_diffusion::apply_error_diffusion;
use engine_project::types::{DocumentId, LayerId};
use engine_project::Document;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use proptest::prelude::*;

/// Full tile size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a PixelTile from a u64 seed using xorshift64 PRNG.
/// This gives us reproducible random RGBA data for any seed.
fn tile_from_seed(seed: u64) -> PixelTile {
    let mut tile = PixelTile::new();
    let size = TILE_FULL_SIZE as usize;
    let mut state = seed.wrapping_add(1); // avoid zero state
    for y in 0..size {
        for x in 0..size {
            for c in 0..4u32 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                // Map to [0.0, 1.0]
                let val = (state as f32) / (u64::MAX as f32);
                tile.set(x as u32, y as u32, c, val);
            }
        }
    }
    tile
}

// ─── Strategies ───────────────────────────────────────────────────────────────

/// Generate a valid ordered dithering mode (skip CustomPng since it needs a file).
fn arb_ordered_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::Bayer2x2),
        Just(DitherModeV2::Bayer4x4),
        Just(DitherModeV2::Bayer8x8),
    ]
}

/// Generate a valid error diffusion dithering mode.
fn arb_diffusion_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::FloydSteinberg),
        Just(DitherModeV2::Atkinson),
    ]
}

/// Generate valid DitherParamsV2 with grayscale color mode and an ordered dithering mode.
fn arb_grayscale_ordered_params() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_ordered_mode(),
        2u16..=256u16,        // valid levels
        (10u32..=400u32),     // maps to threshold_scale 0.1..=4.0
        1u8..=32u8,           // valid pixel_size
    )
        .prop_map(|(mode, levels, ts_raw, pixel_size)| {
            let threshold_scale = ts_raw as f32 / 100.0;
            DitherParamsV2 {
                mode,
                levels,
                threshold_scale,
                pixel_size,
                color_mode: DitherColorMode::Grayscale,
                palette_id: None,
            ..Default::default()
            }
        })
}

/// Generate valid DitherParamsV2 with grayscale color mode and an error diffusion mode.
fn arb_grayscale_diffusion_params() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_diffusion_mode(),
        2u16..=256u16,        // valid levels
        1u8..=32u8,           // valid pixel_size
    )
        .prop_map(|(mode, levels, pixel_size)| {
            DitherParamsV2 {
                mode,
                levels,
                threshold_scale: 1.0, // not used by diffusion engine
                pixel_size,
                color_mode: DitherColorMode::Grayscale,
                palette_id: None,
            ..Default::default()
            }
        })
}

/// Generate a valid TileCoord (level 0, reasonable x/y).
fn arb_tile_coord() -> impl Strategy<Value = TileCoord> {
    (0u32..16, 0u32..16).prop_map(|(x, y)| TileCoord { level: 0, x, y })
}

// ─── Property Tests ───────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// **Validates: Requirement 5 (AC 2)**
    ///
    /// Property 8: Grayscale Output Uniformity (Ordered Dithering)
    /// For any input tile processed with color_mode = Grayscale using ordered
    /// dithering, every output pixel shall have R == G == B.
    #[test]
    fn grayscale_rgb_equal_ordered_dithering(
        seed in any::<u64>(),
        params in arb_grayscale_ordered_params(),
        coord in arb_tile_coord(),
    ) {
        let tile = tile_from_seed(seed);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 1024, 1024);

        let result = apply_ordered(&tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("apply_ordered should not fail with valid grayscale params and no palette");

        // Verify R == G == B for every pixel in the full tile (including halo)
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let r = result.at(x, y, 0);
                let g = result.at(x, y, 1);
                let b = result.at(x, y, 2);
                prop_assert_eq!(
                    r.to_bits(), g.to_bits(),
                    "R != G at ({}, {}): R={}, G={}, B={}, params={:?}, coord={:?}",
                    x, y, r, g, b, params, coord
                );
                prop_assert_eq!(
                    g.to_bits(), b.to_bits(),
                    "G != B at ({}, {}): R={}, G={}, B={}, params={:?}, coord={:?}",
                    x, y, r, g, b, params, coord
                );
            }
        }
    }

    /// **Validates: Requirement 5 (AC 2)**
    ///
    /// Property 8: Grayscale Output Uniformity (Error Diffusion)
    /// For any input tile processed with color_mode = Grayscale using error
    /// diffusion, every output pixel in the core area shall have R == G == B.
    #[test]
    fn grayscale_rgb_equal_error_diffusion(
        seed in any::<u64>(),
        params in arb_grayscale_diffusion_params(),
        coord in arb_tile_coord(),
    ) {
        let tile = tile_from_seed(seed);
        let residuals_store = ErrorResidualsStore::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 1024, 1024);
        let layer_id = LayerId::new(1);

        let result = apply_error_diffusion(
            &tile, coord, &params, &residuals_store, layer_id,
            &palette_cache, &lut_cache, &doc,
        ).expect("apply_error_diffusion should not fail with valid grayscale params and no palette");

        // Verify R == G == B for every pixel in the core area
        // (halo is copied from input which is random RGB, so skip it)
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                let r = result.at(x, y, 0);
                let g = result.at(x, y, 1);
                let b = result.at(x, y, 2);
                prop_assert_eq!(
                    r.to_bits(), g.to_bits(),
                    "R != G at ({}, {}): R={}, G={}, B={}, params={:?}, coord={:?}",
                    x, y, r, g, b, params, coord
                );
                prop_assert_eq!(
                    g.to_bits(), b.to_bits(),
                    "G != B at ({}, {}): R={}, G={}, B={}, params={:?}, coord={:?}",
                    x, y, r, g, b, params, coord
                );
            }
        }
    }
}
