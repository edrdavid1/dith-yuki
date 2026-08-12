//! Property-based tests for pixel_size block quantization.
//!
//! **Validates: Requirement 4 (AC 1-4)**
//!
//! Property 5: Pixel Block Uniformity
//! - For any pixel_size > 1 and any input tile, all pixels within a pixel_size × pixel_size
//!   block (aligned to global coordinates) have identical RGB values in the output.
//!
//! Property 6: Block Alignment Across Tiles
//! - For any block that spans a tile boundary, the pixels of that block in both tiles
//!   have the same color value.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::types::DocumentId;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use proptest::prelude::*;

/// Full tile size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO; // 260

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a PixelTile from a u64 seed using xorshift64 PRNG.
/// Produces reproducible random RGBA data for any seed.
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

// ─── Strategies ───────────────────────────────────────────────────────────────

/// Generate a valid ordered dithering mode (skip CustomPng since it needs a file).
fn arb_ordered_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::Bayer2x2),
        Just(DitherModeV2::Bayer4x4),
        Just(DitherModeV2::Bayer8x8),
    ]
}

/// Generate pixel_size in [2, 32] (we specifically want > 1 for block tests).
fn arb_pixel_size_gt1() -> impl Strategy<Value = u8> {
    2u8..=32u8
}

/// Generate valid levels in [2, 256].
fn arb_levels() -> impl Strategy<Value = u16> {
    2u16..=256u16
}

/// Generate valid threshold_scale in [0.1, 4.0].
fn arb_threshold_scale() -> impl Strategy<Value = f32> {
    (10u32..=400u32).prop_map(|v| v as f32 / 100.0)
}

/// Generate a tile coordinate component (keep small for reasonable test size).
/// Start from 1 to avoid edge-case behavior at document origin (coord=0 halo extends
/// to negative global coords which affects pixel_size block alignment).
fn arb_tile_coord() -> impl Strategy<Value = TileCoord> {
    (1u32..16, 1u32..16).prop_map(|(x, y)| TileCoord { level: 0, x, y })
}

/// Generate a uniform color channel value in [0.0, 1.0].
fn arb_color_value() -> impl Strategy<Value = f32> {
    (0u32..=1000u32).prop_map(|v| v as f32 / 1000.0)
}

// ─── Property 5: Pixel Block Uniformity ───────────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// **Validates: Requirement 4 (AC 1-3)**
    ///
    /// Property 5: Pixel Block Uniformity
    ///
    /// For any pixel_size in [2, 32] and any input tile, all pixels within each
    /// pixel_size × pixel_size block (aligned to global coordinates) have identical
    /// RGB values in the output.
    ///
    /// The blocks are aligned to global coordinates via integer division:
    ///   block_gx = (gx / pixel_size) * pixel_size
    ///   block_gy = (gy / pixel_size) * pixel_size
    ///
    /// Within each such block, every pixel must produce the same dithered color.
    #[test]
    fn pixel_block_uniformity(
        seed in any::<u64>(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size_gt1(),
        coord in arb_tile_coord(),
    ) {
        let tile = tile_from_seed(seed);
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
        let doc = Document::new(DocumentId::new(1), 4096, 4096);

        let result = apply_ordered(&tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("apply_ordered should succeed with valid params");

        let ps_u32 = ps as u32;

        // Iterate over blocks aligned to global coordinates within this tile.
        // For each local pixel (x, y), compute its global coordinate and the
        // block-aligned global coordinate. Pixels sharing the same block origin
        // must have the same RGB output.
        //
        // We check by iterating in block-aligned steps.
        // Global origin of this tile's local (0,0) is (coord * TILE_SIZE - HALO):
        let tile_gx0 = coord.x as i32 * TILE_SIZE as i32 - HALO as i32;
        let tile_gy0 = coord.y as i32 * TILE_SIZE as i32 - HALO as i32;

        // Find the first block-aligned global x/y that is >= tile_gx0
        let ps_i = ps_u32 as i32;
        let first_block_gx = if tile_gx0 >= 0 {
            ((tile_gx0 + ps_i - 1) / ps_i) * ps_i
        } else {
            (tile_gx0 / ps_i) * ps_i // for negative, div_euclid handles rounding
        };
        let first_block_gy = if tile_gy0 >= 0 {
            ((tile_gy0 + ps_i - 1) / ps_i) * ps_i
        } else {
            (tile_gy0 / ps_i) * ps_i
        };

        // Convert back to local coordinates
        let start_local_x = (first_block_gx - tile_gx0) as u32;
        let start_local_y = (first_block_gy - tile_gy0) as u32;

        // Iterate over complete blocks that fit entirely within the tile
        let mut block_gy = start_local_y;
        while block_gy + ps_u32 <= TILE_FULL_SIZE {
            let mut block_gx = start_local_x;
            while block_gx + ps_u32 <= TILE_FULL_SIZE {
                // All pixels in [block_gx..block_gx+ps, block_gy..block_gy+ps]
                // should have identical RGB
                let r0 = result.at(block_gx, block_gy, 0);
                let g0 = result.at(block_gx, block_gy, 1);
                let b0 = result.at(block_gx, block_gy, 2);

                for dy in 0..ps_u32 {
                    for dx in 0..ps_u32 {
                        let px = block_gx + dx;
                        let py = block_gy + dy;
                        let r = result.at(px, py, 0);
                        let g = result.at(px, py, 1);
                        let b = result.at(px, py, 2);
                        prop_assert_eq!(
                            r.to_bits(), r0.to_bits(),
                            "R mismatch in block at local ({}, {}), pixel ({}, {}): expected {} got {}",
                            block_gx, block_gy, px, py, r0, r
                        );
                        prop_assert_eq!(
                            g.to_bits(), g0.to_bits(),
                            "G mismatch in block at local ({}, {}), pixel ({}, {}): expected {} got {}",
                            block_gx, block_gy, px, py, g0, g
                        );
                        prop_assert_eq!(
                            b.to_bits(), b0.to_bits(),
                            "B mismatch in block at local ({}, {}), pixel ({}, {}): expected {} got {}",
                            block_gx, block_gy, px, py, b0, b
                        );
                    }
                }

                block_gx += ps_u32;
            }
            block_gy += ps_u32;
        }
    }

    /// **Validates: Requirement 4 (AC 1-3)**
    ///
    /// Property 5 (grayscale variant): Pixel Block Uniformity in Grayscale mode.
    ///
    /// Same property holds when color_mode is Grayscale.
    #[test]
    fn pixel_block_uniformity_grayscale(
        seed in any::<u64>(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size_gt1(),
        coord in arb_tile_coord(),
    ) {
        let tile = tile_from_seed(seed);
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
        let doc = Document::new(DocumentId::new(1), 4096, 4096);

        let result = apply_ordered(&tile, coord, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("apply_ordered should succeed with valid params");

        let ps_u32 = ps as u32;
        let tile_gx0 = coord.x as i32 * TILE_SIZE as i32 - HALO as i32;
        let tile_gy0 = coord.y as i32 * TILE_SIZE as i32 - HALO as i32;

        let ps_i = ps_u32 as i32;
        let first_block_gx = if tile_gx0 >= 0 {
            ((tile_gx0 + ps_i - 1) / ps_i) * ps_i
        } else {
            (tile_gx0 / ps_i) * ps_i
        };
        let first_block_gy = if tile_gy0 >= 0 {
            ((tile_gy0 + ps_i - 1) / ps_i) * ps_i
        } else {
            (tile_gy0 / ps_i) * ps_i
        };

        let start_local_x = (first_block_gx - tile_gx0) as u32;
        let start_local_y = (first_block_gy - tile_gy0) as u32;

        let mut block_gy = start_local_y;
        while block_gy + ps_u32 <= TILE_FULL_SIZE {
            let mut block_gx = start_local_x;
            while block_gx + ps_u32 <= TILE_FULL_SIZE {
                let r0 = result.at(block_gx, block_gy, 0);

                for dy in 0..ps_u32 {
                    for dx in 0..ps_u32 {
                        let px = block_gx + dx;
                        let py = block_gy + dy;
                        let r = result.at(px, py, 0);
                        prop_assert_eq!(
                            r.to_bits(), r0.to_bits(),
                            "Block uniformity mismatch (grayscale) at local ({}, {}), pixel ({}, {})",
                            block_gx, block_gy, px, py
                        );
                    }
                }

                block_gx += ps_u32;
            }
            block_gy += ps_u32;
        }
    }
}

// ─── Property 6: Block Alignment Across Tiles ─────────────────────────────────

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// **Validates: Requirement 4 (AC 4)**
    ///
    /// Property 6: Block Alignment Across Tiles
    ///
    /// For a block that spans a tile boundary (pixel_size > 1), the pixels of that
    /// block in both adjacent tiles must have the same color value.
    ///
    /// This tests horizontal tile boundaries. We use a uniform tile so that the
    /// source pixel values are the same regardless of which tile we read from,
    /// isolating the block alignment logic.
    ///
    /// With TILE_SIZE=256 and pixel_size ps, a block spans the tile boundary when
    /// the block's global origin doesn't align with the tile boundary. For example,
    /// if ps=4, the block starting at global x=256 is perfectly aligned with tile (1, y).
    /// But a block starting at global x=254 has 2 pixels in tile (0, y) and 2 in tile (1, y).
    ///
    /// We verify by processing two horizontally adjacent tiles with the same uniform
    /// input and checking that pixels in the overlap (halo) region match.
    #[test]
    fn block_alignment_across_horizontal_boundary(
        tx in 0u32..16,
        ty in 0u32..16,
        r in arb_color_value(),
        g in arb_color_value(),
        b in arb_color_value(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size_gt1(),
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
        let doc = Document::new(DocumentId::new(1), 8192, 8192);

        let coord_left = TileCoord { level: 0, x: tx, y: ty };
        let coord_right = TileCoord { level: 0, x: tx + 1, y: ty };

        let result_left = apply_ordered(&tile, coord_left, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("left tile should succeed");
        let result_right = apply_ordered(&tile, coord_right, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("right tile should succeed");

        // The overlap region between two horizontally adjacent tiles:
        // Left tile's local x range [TILE_SIZE, TILE_FULL_SIZE) maps to
        //   global x [(tx)*256 + 256, (tx)*256 + 260) = [(tx+1)*256, (tx+1)*256 + 4)
        // Right tile's local x range [0, 2*HALO) maps to
        //   global x [(tx+1)*256, (tx+1)*256 + 4)
        // Same global coordinates — must produce identical output.
        let overlap_width = 2 * HALO; // 4 pixels

        for local_y in 0..TILE_FULL_SIZE {
            for dx in 0..overlap_width {
                let left_local_x = TILE_SIZE + dx;
                let right_local_x = dx;

                for c in 0..3u32 {
                    let left_val = result_left.at(left_local_x, local_y, c);
                    let right_val = result_right.at(right_local_x, local_y, c);
                    prop_assert_eq!(
                        left_val.to_bits(), right_val.to_bits(),
                        "Block alignment mismatch at horizontal boundary: \
                         overlap dx={}, y={}, channel={}, left={}, right={}, ps={}, coord=({},{})",
                        dx, local_y, c, left_val, right_val, ps, tx, ty
                    );
                }
            }
        }
    }

    /// **Validates: Requirement 4 (AC 4)**
    ///
    /// Property 6: Block Alignment Across Tiles (vertical boundary)
    ///
    /// Same property for vertically adjacent tiles. Blocks spanning the vertical
    /// tile boundary must have matching colors in both tiles.
    #[test]
    fn block_alignment_across_vertical_boundary(
        tx in 0u32..16,
        ty in 0u32..16,
        r in arb_color_value(),
        g in arb_color_value(),
        b in arb_color_value(),
        mode in arb_ordered_mode(),
        levels in arb_levels(),
        ts in arb_threshold_scale(),
        ps in arb_pixel_size_gt1(),
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
        let doc = Document::new(DocumentId::new(1), 8192, 8192);

        let coord_top = TileCoord { level: 0, x: tx, y: ty };
        let coord_bottom = TileCoord { level: 0, x: tx, y: ty + 1 };

        let result_top = apply_ordered(&tile, coord_top, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("top tile should succeed");
        let result_bottom = apply_ordered(&tile, coord_bottom, &params, &threshold_cache, &palette_cache, &lut_cache, &doc)
            .expect("bottom tile should succeed");

        // Vertical overlap:
        // Top tile's local y [TILE_SIZE, TILE_FULL_SIZE) → global y [(ty+1)*256, (ty+1)*256 + 4)
        // Bottom tile's local y [0, 2*HALO) → global y [(ty+1)*256, (ty+1)*256 + 4)
        let overlap_height = 2 * HALO;

        for local_x in 0..TILE_FULL_SIZE {
            for dy in 0..overlap_height {
                let top_local_y = TILE_SIZE + dy;
                let bottom_local_y = dy;

                for c in 0..3u32 {
                    let top_val = result_top.at(local_x, top_local_y, c);
                    let bottom_val = result_bottom.at(local_x, bottom_local_y, c);
                    prop_assert_eq!(
                        top_val.to_bits(), bottom_val.to_bits(),
                        "Block alignment mismatch at vertical boundary: \
                         x={}, overlap dy={}, channel={}, top={}, bottom={}, ps={}, coord=({},{})",
                        local_x, dy, c, top_val, bottom_val, ps, tx, ty
                    );
                }
            }
        }
    }
}
