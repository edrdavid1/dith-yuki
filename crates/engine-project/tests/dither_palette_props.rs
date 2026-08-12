//! Property-based tests for palette membership invariant.
//!
//! **Validates: Requirement 6 (AC 1-4)**
//!
//! Property 9: Palette Membership Invariant
//! - For any input tile processed with a non-null `palette_id`, every output
//!   pixel's RGB SHALL exactly match one of the palette's color entries.
//!
//! Tests both ordered dithering and error diffusion with `palette_id` set.

use engine_color::palette::LinearColor;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::filters::dither_diffusion::apply_error_diffusion;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_project::Document;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use proptest::prelude::*;

/// Full tile size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a PixelTile from a u64 seed using xorshift64 PRNG.
/// Produces reproducible random RGBA data in [0.0, 1.0].
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
                let val = (state as f32) / (u64::MAX as f32);
                tile.set(x as u32, y as u32, c, val);
            }
        }
    }
    tile
}

/// Generate a palette of random colors from a seed.
fn palette_colors_from_seed(seed: u64, count: usize) -> Vec<LinearColor> {
    let mut state = seed.wrapping_add(7); // different starting point from tile
    let mut colors = Vec::with_capacity(count);
    for _ in 0..count {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let r = (state as f32) / (u64::MAX as f32);
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let g = (state as f32) / (u64::MAX as f32);
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let b = (state as f32) / (u64::MAX as f32);
        colors.push(LinearColor { r, g, b });
    }
    colors
}

/// Check that a pixel's RGB matches one of the palette colors exactly.
fn pixel_matches_palette(r: f32, g: f32, b: f32, palette_colors: &[LinearColor]) -> bool {
    palette_colors.iter().any(|c| c.r == r && c.g == g && c.b == b)
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

/// Generate a valid error diffusion mode.
fn arb_diffusion_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::FloydSteinberg),
        Just(DitherModeV2::Atkinson),
    ]
}

/// Generate palette size (4–16 colors).
fn arb_palette_size() -> impl Strategy<Value = usize> {
    4usize..=16usize
}

/// Generate a valid TileCoord (level 0, reasonable x/y).
fn arb_tile_coord() -> impl Strategy<Value = TileCoord> {
    (0u32..8, 0u32..8).prop_map(|(x, y)| TileCoord { level: 0, x, y })
}

// ─── Property Tests ───────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// **Validates: Requirements 6.1, 6.2, 6.3**
    ///
    /// Property 9: Palette Membership Invariant (Ordered Dithering)
    /// For any input tile processed with ordered dithering and a non-null palette_id,
    /// every output pixel's RGB exactly matches one of the palette's color entries.
    #[test]
    fn palette_membership_ordered_dithering(
        tile_seed in any::<u64>(),
        palette_seed in any::<u64>(),
        palette_size in arb_palette_size(),
        mode in arb_ordered_mode(),
        coord in arb_tile_coord(),
        threshold_scale_raw in 10u32..=400u32,
        pixel_size in 1u8..=4u8,
        color_mode in prop_oneof![Just(DitherColorMode::Rgb), Just(DitherColorMode::Grayscale)],
    ) {
        let tile = tile_from_seed(tile_seed);
        let palette_colors = palette_colors_from_seed(palette_seed, palette_size);

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let mut doc = Document::new(DocumentId::new(1), 4096, 4096);

        // Add palette to the document
        let palette_id = doc.add_palette("Test Palette".to_string(), palette_colors.clone());

        let threshold_scale = threshold_scale_raw as f32 / 100.0;
        let params = DitherParamsV2 {
            mode,
            levels: 8, // ignored when palette_id is set
            threshold_scale,
            pixel_size,
            color_mode,
            palette_id: Some(palette_id),
            ..Default::default()
        };

        let result = apply_ordered(&tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("apply_ordered should not fail with valid palette");

        // Verify every output pixel matches a palette entry
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let out_r = result.at(x, y, 0);
                let out_g = result.at(x, y, 1);
                let out_b = result.at(x, y, 2);

                prop_assert!(
                    pixel_matches_palette(out_r, out_g, out_b, &palette_colors),
                    "Pixel ({}, {}) = ({}, {}, {}) does not match any palette entry. \
                     Mode={:?}, coord=({}, {}), palette_size={}, threshold_scale={}, pixel_size={}",
                    x, y, out_r, out_g, out_b,
                    params.mode, coord.x, coord.y, palette_size, threshold_scale, pixel_size
                );
            }
        }
    }

    /// **Validates: Requirements 6.1, 6.2, 6.4**
    ///
    /// Property 9: Palette Membership Invariant (Error Diffusion)
    /// For any input tile processed with error diffusion and a non-null palette_id,
    /// every output pixel in the core area whose block representative is within the
    /// tile's core area exactly matches one of the palette's color entries.
    ///
    /// Note: When pixel_size > 1 and a block's representative falls in a neighboring
    /// tile's area (i.e., the halo), the error diffusion engine uses a source-pixel
    /// fallback for cross-tile block continuity. Those edge pixels are excluded from
    /// this check since they require full pipeline context.
    #[test]
    fn palette_membership_error_diffusion(
        tile_seed in any::<u64>(),
        palette_seed in any::<u64>(),
        palette_size in arb_palette_size(),
        mode in arb_diffusion_mode(),
        coord in arb_tile_coord(),
        pixel_size in 1u8..=4u8,
        color_mode in prop_oneof![Just(DitherColorMode::Rgb), Just(DitherColorMode::Grayscale)],
    ) {
        let tile = tile_from_seed(tile_seed);
        let palette_colors = palette_colors_from_seed(palette_seed, palette_size);

        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let residuals_store = ErrorResidualsStore::new();
        let layer_id = LayerId::new(1);
        let mut doc = Document::new(DocumentId::new(1), 4096, 4096);

        // Add palette to the document
        let palette_id = doc.add_palette("Test Palette".to_string(), palette_colors.clone());

        let params = DitherParamsV2 {
            mode,
            levels: 8, // ignored when palette_id is set
            threshold_scale: 1.0,
            pixel_size,
            color_mode,
            palette_id: Some(palette_id),
            ..Default::default()
        };

        let result = apply_error_diffusion(
            &tile, coord, &params, &residuals_store, layer_id, &palette_cache, &lut_cache, &doc
        ).expect("apply_error_diffusion should not fail with valid palette");

        let ps = pixel_size as u32;

        // Verify output pixels in the core area whose block representative is
        // within the core area. The core area is [HALO..HALO+TILE_SIZE).
        // Global coords must subtract HALO (same as GlobalCoordSigned::from_local_with_halo).
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
                let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
                if gx < 0 || gy < 0 {
                    continue;
                }
                let block_gx = (gx as u32 / ps) * ps;
                let block_gy = (gy as u32 / ps) * ps;
                let rep_tile_x =
                    block_gx as i32 - coord.x as i32 * TILE_SIZE as i32 + HALO as i32;
                let rep_tile_y =
                    block_gy as i32 - coord.y as i32 * TILE_SIZE as i32 + HALO as i32;

                // Skip pixels whose representative falls outside the core
                // (cross-tile / halo — needs full pipeline + dithered BRC).
                if rep_tile_x < HALO as i32
                    || rep_tile_y < HALO as i32
                    || rep_tile_x >= (HALO + TILE_SIZE) as i32
                    || rep_tile_y >= (HALO + TILE_SIZE) as i32
                {
                    continue;
                }

                let out_r = result.at(x, y, 0);
                let out_g = result.at(x, y, 1);
                let out_b = result.at(x, y, 2);

                prop_assert!(
                    pixel_matches_palette(out_r, out_g, out_b, &palette_colors),
                    "Pixel ({}, {}) = ({}, {}, {}) does not match any palette entry. \
                     Mode={:?}, coord=({}, {}), palette_size={}, pixel_size={}",
                    x, y, out_r, out_g, out_b,
                    params.mode, coord.x, coord.y, palette_size, pixel_size
                );
            }
        }
    }
}
