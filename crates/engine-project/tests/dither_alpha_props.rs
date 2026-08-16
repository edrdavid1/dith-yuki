//! Property-based tests for alpha channel preservation in ordered dithering.
//!
//! **Validates: Requirement 5 (AC 3)**
//!
//! Property 7: Alpha Preservation Invariant
//! - For any input tile and any valid DitherParamsV2 configuration with ordered
//!   dithering modes, the alpha channel of every pixel in the output is bitwise
//!   identical to the alpha channel in the input.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::types::DocumentId;
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

/// Generate valid DitherParamsV2 with an ordered dithering mode.
fn arb_ordered_params() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_ordered_mode(),
        2u16..=256u16,        // valid levels
        (10u32..=400u32),     // maps to threshold_scale 0.1..=4.0
        1u8..=32u8,           // valid pixel_size
        prop_oneof![Just(DitherColorMode::Rgb), Just(DitherColorMode::Grayscale)],
    )
        .prop_map(|(mode, levels, ts_raw, pixel_size, color_mode)| {
            let threshold_scale = ts_raw as f32 / 100.0;
            DitherParamsV2 {
                mode,
                levels,
                threshold_scale,
                pixel_size,
                color_mode,
                palette_id: None, // skip palette to avoid needing doc setup,
                dither_alpha: false,
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

    /// **Validates: Requirement 5 (AC 3)**
    ///
    /// Property 7: Alpha Preservation Invariant
    /// For any input tile and any valid ordered dithering params, the alpha
    /// channel of every pixel in the output is bitwise identical to the input.
    #[test]
    fn alpha_preserved_under_ordered_dithering(
        seed in any::<u64>(),
        params in arb_ordered_params(),
        coord in arb_tile_coord(),
    ) {
        let tile = tile_from_seed(seed);
        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 1024, 1024);

        let result = apply_ordered(&tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("apply_ordered should not fail with valid params and no palette");

        // Verify alpha channel is bitwise identical for every pixel
        let size = TILE_FULL_SIZE;
        for y in 0..size {
            for x in 0..size {
                let input_alpha = tile.at(x, y, 3);
                let output_alpha = result.at(x, y, 3);
                prop_assert_eq!(
                    input_alpha.to_bits(),
                    output_alpha.to_bits(),
                    "Alpha mismatch at ({}, {}): input={} output={}, params={:?}, coord={:?}",
                    x, y, input_alpha, output_alpha, params, coord
                );
            }
        }
    }
}
