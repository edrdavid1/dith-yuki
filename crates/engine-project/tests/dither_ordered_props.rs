//! Property-based tests for ordered dithering seamless tiling.
//!
//! **Validates: Requirements 2 (AC 1-3)**
//!
//! Property 2: Ordered Dithering Seamless Tiling
//! - For any uniform-color tile processed with ordered dithering, the output pixel
//!   at global coordinate (gx, gy) is identical regardless of which tile boundary
//!   placement contains that coordinate.
//!
//! The key insight: ordered dithering uses global coordinates to index into the
//! threshold matrix, so processing the same global pixel from different tiles must
//! produce the same result.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::types::DocumentId;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use proptest::prelude::*;

/// Tile full size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO; // 260

// ─── Strategies ───────────────────────────────────────────────────────────────

/// Generate an ordered dither mode (no error diffusion modes).
fn arb_ordered_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::Bayer2x2),
        Just(DitherModeV2::Bayer4x4),
        Just(DitherModeV2::Bayer8x8),
    ]
}

/// Generate valid levels in [2, 256].
fn arb_levels() -> impl Strategy<Value = u16> {
    2u16..=256u16
}

/// Generate valid threshold_scale in [0.1, 4.0] using integer mapping.
fn arb_threshold_scale() -> impl Strategy<Value = f32> {
    (10u32..=400u32).prop_map(|v| v as f32 / 100.0)
}

/// Generate valid pixel_size in [1, 32].
fn arb_pixel_size() -> impl Strategy<Value = u8> {
    1u8..=32u8
}

/// Generate a uniform color channel value in [0.0, 1.0].
fn arb_color_value() -> impl Strategy<Value = f32> {
    (0u32..=1000u32).prop_map(|v| v as f32 / 1000.0)
}

/// Generate a tile coordinate component (keep small to avoid overflow concerns).
fn arb_tile_coord_x() -> impl Strategy<Value = u32> {
    0u32..100u32
}

fn arb_tile_coord_y() -> impl Strategy<Value = u32> {
    0u32..100u32
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Create a uniform-color tile (all pixels set to the same RGBA).
fn make_uniform_tile(r: f32, g: f32, b: f32, a: f32) -> PixelTile {
    let mut tile = PixelTile::new();
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            tile.set(x, y, 0, r);
            tile.set(x, y, 1, g);
            tile.set(x, y, 2, b);
            tile.set(x, y, 3, a);
        }
    }
    tile
}

// ─── Property Tests ───────────────────────────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// **Validates: Requirements 2 (AC 1-3)**
    ///
    /// Property 2: Ordered Dithering Seamless Tiling
    ///
    /// For a uniform-color tile, process at tile coord (tx, ty) and at (tx+1, ty).
    /// The overlapping global coordinates (at the seam) must produce identical output.
    ///
    /// With TILE_SIZE=256 and HALO=2, tile at (tx, ty) covers local x in [0, 260),
    /// mapping to global x in [tx*256, tx*256 + 260).
    /// Tile at (tx+1, ty) covers global x in [(tx+1)*256, (tx+1)*256 + 260).
    /// The overlap region in global x is [(tx+1)*256, tx*256 + 260) = [(tx+1)*256, tx*256 + 260).
    /// That's a 4-pixel overlap (HALO * 2 = 4... actually from tx*256+260 we get (tx+1)*256 = tx*256+256,
    /// so overlap is from (tx+1)*256 to tx*256+260 = 4 pixels wide).
    ///
    /// We verify that for every pixel in this overlap region, both tiles produce the same output.
    #[test]
    fn seamless_tiling_horizontal(
        tx in arb_tile_coord_x(),
        ty in arb_tile_coord_y(),
        r in arb_color_value(),
        g in arb_color_value(),
        b in arb_color_value(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size(),
    ) {
        let tile = make_uniform_tile(r, g, b, 1.0);
        let params = DitherParamsV2 {
            mode,
            levels,
            threshold_scale: ts,
            pixel_size: ps,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        };

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 1024, 1024);

        let coord_left = TileCoord { level: 0, x: tx, y: ty };
        let coord_right = TileCoord { level: 0, x: tx + 1, y: ty };

        let result_left = apply_ordered(&tile, coord_left, &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();
        let result_right = apply_ordered(&tile, coord_right, &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // The overlap region:
        // Left tile's local x range [TILE_SIZE, TILE_FULL_SIZE) maps to global x [tx*256 + 256, tx*256 + 260)
        // Right tile's local x range [0, HALO*2) maps to global x [(tx+1)*256, (tx+1)*256 + 4)
        // These are the same global coordinates: [(tx+1)*256, (tx+1)*256 + 4)
        //
        // Left tile local x for overlap: starts at TILE_SIZE (256), width = HALO*2 = 4
        // Right tile local x for overlap: starts at 0, width = HALO*2 = 4

        let overlap_width = 2 * HALO; // 4 pixels

        for local_y in 0..TILE_FULL_SIZE {
            for dx in 0..overlap_width {
                let left_local_x = TILE_SIZE + dx;
                let right_local_x = dx;

                for c in 0..3 {
                    let left_val = result_left.at(left_local_x, local_y, c);
                    let right_val = result_right.at(right_local_x, local_y, c);
                    prop_assert_eq!(
                        left_val, right_val,
                        "Seam mismatch at overlap dx={}, y={}, channel={}: left={}, right={}",
                        dx, local_y, c, left_val, right_val
                    );
                }
            }
        }
    }

    /// **Validates: Requirements 2 (AC 1-3)**
    ///
    /// Property 2: Ordered Dithering Seamless Tiling (vertical seam)
    ///
    /// Same property but for vertically adjacent tiles.
    /// Tile at (tx, ty) and (tx, ty+1) overlap in a 4-pixel tall band.
    #[test]
    fn seamless_tiling_vertical(
        tx in arb_tile_coord_x(),
        ty in arb_tile_coord_y(),
        r in arb_color_value(),
        g in arb_color_value(),
        b in arb_color_value(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size(),
    ) {
        let tile = make_uniform_tile(r, g, b, 1.0);
        let params = DitherParamsV2 {
            mode,
            levels,
            threshold_scale: ts,
            pixel_size: ps,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        };

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 1024, 1024);

        let coord_top = TileCoord { level: 0, x: tx, y: ty };
        let coord_bottom = TileCoord { level: 0, x: tx, y: ty + 1 };

        let result_top = apply_ordered(&tile, coord_top, &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();
        let result_bottom = apply_ordered(&tile, coord_bottom, &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        // Vertical overlap:
        // Top tile's local y range [TILE_SIZE, TILE_FULL_SIZE) maps to global y [ty*256 + 256, ty*256 + 260)
        // Bottom tile's local y range [0, HALO*2) maps to global y [(ty+1)*256, (ty+1)*256 + 4)
        // Same global y coords.

        let overlap_height = 2 * HALO; // 4 pixels

        for local_x in 0..TILE_FULL_SIZE {
            for dy in 0..overlap_height {
                let top_local_y = TILE_SIZE + dy;
                let bottom_local_y = dy;

                for c in 0..3 {
                    let top_val = result_top.at(local_x, top_local_y, c);
                    let bottom_val = result_bottom.at(local_x, bottom_local_y, c);
                    prop_assert_eq!(
                        top_val, bottom_val,
                        "Vertical seam mismatch at x={}, overlap dy={}, channel={}: top={}, bottom={}",
                        local_x, dy, c, top_val, bottom_val
                    );
                }
            }
        }
    }

    /// **Validates: Requirements 2 (AC 1-3)**
    ///
    /// Property 2: Ordered Dithering Seamless Tiling (grayscale mode)
    ///
    /// Same seamless tiling property holds in grayscale color mode.
    #[test]
    fn seamless_tiling_grayscale(
        tx in arb_tile_coord_x(),
        ty in arb_tile_coord_y(),
        r in arb_color_value(),
        g in arb_color_value(),
        b in arb_color_value(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size(),
    ) {
        let tile = make_uniform_tile(r, g, b, 1.0);
        let params = DitherParamsV2 {
            mode,
            levels,
            threshold_scale: ts,
            pixel_size: ps,
            color_mode: DitherColorMode::Grayscale,
            palette_id: None,
            ..Default::default()
        };

        let threshold_cache = ThresholdMapCache::new();
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let doc = Document::new(DocumentId::new(1), 1024, 1024);

        let coord_left = TileCoord { level: 0, x: tx, y: ty };
        let coord_right = TileCoord { level: 0, x: tx + 1, y: ty };

        let result_left = apply_ordered(&tile, coord_left, &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();
        let result_right = apply_ordered(&tile, coord_right, &params, &threshold_cache, &palette_cache, &lut_cache, &doc).unwrap();

        let overlap_width = 2 * HALO;

        for local_y in 0..TILE_FULL_SIZE {
            for dx in 0..overlap_width {
                let left_local_x = TILE_SIZE + dx;
                let right_local_x = dx;

                for c in 0..3 {
                    let left_val = result_left.at(left_local_x, local_y, c);
                    let right_val = result_right.at(right_local_x, local_y, c);
                    prop_assert_eq!(
                        left_val, right_val,
                        "Grayscale seam mismatch at overlap dx={}, y={}, channel={}: left={}, right={}",
                        dx, local_y, c, left_val, right_val
                    );
                }
            }
        }
    }
}
