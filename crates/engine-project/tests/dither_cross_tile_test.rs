//! Integration test: Cross-tile error diffusion propagation.
//!
//! Verifies that the `ErrorResidualsStore` correctly propagates quantization
//! error across tile boundaries when processing tiles in row-major order.
//!
//! **Requirements:** 3.5, 3.6

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::apply_error_diffusion;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

/// Create a uniform tile filled with a single RGBA color (including halo region).
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

fn tc(x: u32, y: u32) -> TileCoord {
    TileCoord { level: 0, x, y }
}

fn make_fs_params(levels: u16) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        ..Default::default()
    }
}

/// Test that processing tiles in row-major order with cross-tile propagation
/// produces different output than processing each tile in isolation.
///
/// Strategy:
/// - Process a 2×2 grid of tiles in row-major order: (0,0), (1,0), (0,1), (1,1)
/// - Tiles are filled with 0.4 gray (not at an exact quantization boundary for 4 levels)
/// - When using 4 levels, boundaries are at 0, 1/3, 2/3, 1 — so 0.4 generates quantization error
/// - Tile (1,0) should receive left residuals from (0,0)
/// - Tile (0,1) should receive top residuals from (0,0) — if bottom overflow is non-zero
/// - Tile (1,1) should receive residuals from neighbors
/// - Verify that at least one neighbor tile's output is affected by cross-tile propagation
///
/// Note: For uniform inputs, the error diffusion pattern converges within the tile.
/// Bottom-edge overflow may be negligible for some input values. The test verifies
/// that the mechanism works by checking left-propagation (which is always significant)
/// and conditionally checking top-propagation based on whether bottom residuals are non-zero.
#[test]
fn cross_tile_propagation_affects_neighbor_output() {
    let params = make_fs_params(4); // 4 levels: boundaries at 0, 1/3, 2/3, 1
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);

    // Use 0.4 gray — not at a quantization boundary, so error will be generated
    let tile = make_uniform_tile(0.4, 0.4, 0.4, 1.0);

    // ─── Process 2×2 grid in row-major order WITH cross-tile propagation ────
    let store = ErrorResidualsStore::new();

    // Process tile (0,0) — no neighbors, stores residuals
    let _result_00 = apply_error_diffusion(
        &tile,
        tc(0, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Process tile (1,0) — has left neighbor (0,0)
    let result_10_with = apply_error_diffusion(
        &tile,
        tc(1, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Process tile (0,1) — has top neighbor (0,0)
    let result_01_with = apply_error_diffusion(
        &tile,
        tc(0, 1),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Process tile (1,1) — has left neighbor (0,1) and top neighbor (1,0)
    let result_11_with = apply_error_diffusion(
        &tile,
        tc(1, 1),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // ─── Process tiles WITHOUT cross-tile propagation (isolated) ────
    let isolated_store = ErrorResidualsStore::new();

    // Process tile (1,0) in isolation — no left neighbor available
    let result_10_without = apply_error_diffusion(
        &tile,
        tc(1, 0),
        &params,
        &isolated_store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // ─── Verify cross-tile propagation affected output ────

    // Tile (1,0): left boundary pixels should differ due to residuals from (0,0)
    let mut tile_10_differs = false;
    for y in HALO..(HALO + 8) {
        let x = HALO; // first core column
        for c in 0..3u32 {
            if result_10_with.at(x, y, c) != result_10_without.at(x, y, c) {
                tile_10_differs = true;
                break;
            }
        }
        if tile_10_differs {
            break;
        }
    }
    assert!(
        tile_10_differs,
        "Tile (1,0) should differ from isolated processing due to left residuals from (0,0)"
    );

    // Verify top residuals from (0,0) exist — if they contain non-zero values,
    // then tile (0,1) must differ from isolated processing
    let top_residuals = store.get_top(1, layer_id, 0, tc(0, 1));
    assert!(
        top_residuals.is_some(),
        "Top residuals from (0,0) should be stored"
    );
    let top_res = top_residuals.unwrap();
    let has_nonzero_bottom = top_res.bottom.iter().any(|&v| v.abs() > 1e-10);

    if has_nonzero_bottom {
        // If there are non-zero bottom residuals, tile (0,1) should differ
        let isolated_store_2 = ErrorResidualsStore::new();
        let result_01_without = apply_error_diffusion(
            &tile,
            tc(0, 1),
            &params,
            &isolated_store_2,
            layer_id,
            &palette_cache,
            &lut_cache,
            &doc,
        )
        .unwrap();

        let mut tile_01_differs = false;
        for x in HALO..(HALO + TILE_SIZE) {
            for y in HALO..(HALO + 4) {
                for c in 0..3u32 {
                    if result_01_with.at(x, y, c) != result_01_without.at(x, y, c) {
                        tile_01_differs = true;
                        break;
                    }
                }
                if tile_01_differs {
                    break;
                }
            }
            if tile_01_differs {
                break;
            }
        }
        assert!(
            tile_01_differs,
            "Tile (0,1) should differ when non-zero top residuals are applied"
        );
    }

    // Tile (1,1): should differ from isolated due to at least left residuals from (0,1)
    let isolated_store_3 = ErrorResidualsStore::new();
    let result_11_without = apply_error_diffusion(
        &tile,
        tc(1, 1),
        &params,
        &isolated_store_3,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    let mut tile_11_differs = false;
    for y in HALO..(HALO + 8) {
        for x in HALO..(HALO + 8) {
            for c in 0..3u32 {
                if result_11_with.at(x, y, c) != result_11_without.at(x, y, c) {
                    tile_11_differs = true;
                    break;
                }
            }
            if tile_11_differs {
                break;
            }
        }
        if tile_11_differs {
            break;
        }
    }
    assert!(
        tile_11_differs,
        "Tile (1,1) should differ from isolated processing due to left and/or top residuals"
    );
}

/// Test that residuals are stored correctly after processing each tile
/// and can be retrieved by the appropriate neighbor.
#[test]
fn residuals_stored_and_retrievable_for_2x2_grid() {
    let params = make_fs_params(2); // Binary quantization — maximum error
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);

    let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
    let store = ErrorResidualsStore::new();

    // Process all 4 tiles in row-major order
    apply_error_diffusion(
        &tile,
        tc(0, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();
    apply_error_diffusion(
        &tile,
        tc(1, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();
    apply_error_diffusion(
        &tile,
        tc(0, 1),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();
    apply_error_diffusion(
        &tile,
        tc(1, 1),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // After processing, residuals should exist for all 4 tiles:
    // - (0,0)'s right residuals readable by (1,0) via get_left
    assert!(
        store.get_left(1, layer_id, 0, tc(1, 0)).is_some(),
        "Tile (1,0) should find left residuals from (0,0)"
    );
    // - (0,0)'s bottom residuals readable by (0,1) via get_top
    assert!(
        store.get_top(1, layer_id, 0, tc(0, 1)).is_some(),
        "Tile (0,1) should find top residuals from (0,0)"
    );
    // - (1,0)'s right residuals (would be (2,0) neighbor, not tested here)
    // - (1,0)'s bottom residuals readable by (1,1) via get_top
    assert!(
        store.get_top(1, layer_id, 0, tc(1, 1)).is_some(),
        "Tile (1,1) should find top residuals from (1,0)"
    );
    // - (0,1)'s right residuals readable by (1,1) via get_left
    assert!(
        store.get_left(1, layer_id, 0, tc(1, 1)).is_some(),
        "Tile (1,1) should find left residuals from (0,1)"
    );
}

/// Test that processing tile (0,0) first and then (1,0) with its left residuals
/// produces output that differs in the first few pixels compared to processing
/// tile (1,0) without any residuals.
///
/// This is the minimal cross-tile propagation test described in the task:
/// - Process tile (0,0) → stores residuals
/// - Process tile (1,0) → reads left residuals from (0,0)
/// - Verify first few pixels of tile (1,0) differ from processing without neighbors
#[test]
fn minimal_left_propagation_test() {
    let params = make_fs_params(4);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);

    // Use a value that generates significant error: 0.4 with 4 levels
    // Levels: 0, 1/3 ≈ 0.333, 2/3 ≈ 0.667, 1.0
    // 0.4 rounds to 1/3, error = 0.4 - 0.333 = 0.067 per pixel
    let tile = make_uniform_tile(0.4, 0.4, 0.4, 1.0);

    // Step 1: Process tile (0,0) — stores residuals
    let store = ErrorResidualsStore::new();
    apply_error_diffusion(
        &tile,
        tc(0, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Verify residuals were stored
    let left_residuals = store.get_left(1, layer_id, 0, tc(1, 0));
    assert!(
        left_residuals.is_some(),
        "Residuals from (0,0) should be available"
    );

    // Step 2: Check that stored residuals have non-zero values
    let residuals = left_residuals.unwrap();
    let has_nonzero = residuals.right.iter().any(|&v| v.abs() > 1e-10);
    assert!(
        has_nonzero,
        "Right residuals from (0,0) should contain non-zero error values"
    );

    // Step 3: Process tile (1,0) WITH residuals from (0,0)
    let result_with = apply_error_diffusion(
        &tile,
        tc(1, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Step 4: Process tile (1,0) WITHOUT any residuals (isolated)
    let empty_store = ErrorResidualsStore::new();
    let result_without = apply_error_diffusion(
        &tile,
        tc(1, 0),
        &params,
        &empty_store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Step 5: Verify first few pixels differ
    let mut found_difference = false;
    for y in HALO..(HALO + 4) {
        let x = HALO; // first core column — directly affected by left residuals
        for c in 0..3u32 {
            if result_with.at(x, y, c) != result_without.at(x, y, c) {
                found_difference = true;
                break;
            }
        }
        if found_difference {
            break;
        }
    }
    assert!(
        found_difference,
        "First pixels of tile (1,0) should differ when left residuals are applied vs. isolated"
    );
}

/// Test that Atkinson kernel also propagates cross-tile errors correctly.
#[test]
fn atkinson_cross_tile_propagation() {
    let params = DitherParamsV2 {
        mode: DitherModeV2::Atkinson,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        ..Default::default()
    };
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);

    let tile = make_uniform_tile(0.4, 0.4, 0.4, 1.0);

    // Process (0,0) then (1,0) with propagation
    let store = ErrorResidualsStore::new();
    apply_error_diffusion(
        &tile,
        tc(0, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    let result_with = apply_error_diffusion(
        &tile,
        tc(1, 0),
        &params,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Process (1,0) in isolation
    let empty_store = ErrorResidualsStore::new();
    let result_without = apply_error_diffusion(
        &tile,
        tc(1, 0),
        &params,
        &empty_store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
    )
    .unwrap();

    // Verify difference in first column due to left residuals
    let mut found_difference = false;
    for y in HALO..(HALO + 8) {
        let x = HALO;
        for c in 0..3u32 {
            if result_with.at(x, y, c) != result_without.at(x, y, c) {
                found_difference = true;
                break;
            }
        }
        if found_difference {
            break;
        }
    }
    assert!(
        found_difference,
        "Atkinson: first pixels of tile (1,0) should differ with cross-tile propagation"
    );
}

/// Gold standard: strip ED must match a monolithic FS pass on the same buffer.
/// Tile-at-a-time wavefront drops FS below-left taps into the previous tile.
#[test]
fn tiled_fs_palette_matches_monolithic_at_seam() {
    use engine_color::palette::LinearColor;
    use engine_color::palette_cache::PaletteKdCache;
    use engine_color::palette_lut::{PaletteLutCache, DEFAULT_LUT_SIZE};
    use engine_project::filter::PaletteDitherMode;
    use engine_project::filters::dither_diffusion::apply_error_diffusion_tile_row_strip;
    use engine_tiles::block_cache::BlockRepresentativeCache;
    use engine_tiles::{HALO, TILE_SIZE};

    const W: u32 = 512;
    const H: u32 = 256;

    let mut rgba = vec![0.0f32; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let t = x as f32 / (W - 1) as f32;
            let v = 0.25 + 0.5 * t;
            let i = ((y * W + x) * 4) as usize;
            rgba[i] = v;
            rgba[i + 1] = v;
            rgba[i + 2] = v;
            rgba[i + 3] = 1.0;
        }
    }

    let mut doc = Document::new(DocumentId::new(1), W, H);
    let palette_id = doc.add_palette(
        "BW".into(),
        vec![
            LinearColor { r: 0.0, g: 0.0, b: 0.0 },
            LinearColor { r: 1.0, g: 1.0, b: 1.0 },
        ],
    );
    let palette = doc.get_palette(palette_id).unwrap().clone();
    let params = DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 2,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: Some(palette_id),
        palette_dither_mode: PaletteDitherMode::Strict,
        ..Default::default()
    };

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let lut = lut_cache
        .get_or_build(doc.id.0, &palette, &palette_cache, DEFAULT_LUT_SIZE)
        .unwrap();

    let mut mono = vec![0.0f32; (W * H * 3) as usize];
    let mut err = vec![0.0f32; (W * H * 3) as usize];
    let offsets = [
        (1i32, 0i32, 7.0 / 16.0),
        (-1, 1, 3.0 / 16.0),
        (0, 1, 5.0 / 16.0),
        (1, 1, 1.0 / 16.0),
    ];
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let i = ((y as u32 * W + x as u32) * 4) as usize;
            let ei = ((y as u32 * W + x as u32) * 3) as usize;
            let adj_r = (rgba[i] + err[ei]).clamp(0.0, 1.0);
            let adj_g = (rgba[i + 1] + err[ei + 1]).clamp(0.0, 1.0);
            let adj_b = (rgba[i + 2] + err[ei + 2]).clamp(0.0, 1.0);
            let oklab = engine_color::oklab::linear_to_oklab(engine_color::oklab::LinRgb {
                r: adj_r,
                g: adj_g,
                b: adj_b,
            });
            let idx = lut.nearest_index(oklab) as usize;
            let c = &palette.colors[idx];
            mono[ei] = c.r;
            mono[ei + 1] = c.g;
            mono[ei + 2] = c.b;
            let qe = [adj_r - c.r, adj_g - c.g, adj_b - c.b];
            for &(dx, dy, wgt) in &offsets {
                let nx = x + dx;
                let ny = y + dy;
                if nx >= 0 && ny >= 0 && (nx as u32) < W && (ny as u32) < H {
                    let ni = ((ny as u32 * W + nx as u32) * 3) as usize;
                    err[ni] += qe[0] * wgt;
                    err[ni + 1] += qe[1] * wgt;
                    err[ni + 2] += qe[2] * wgt;
                }
            }
        }
    }

    fn tile_from(rgba: &[f32], w: u32, h: u32, coord: TileCoord) -> PixelTile {
        let full = TILE_SIZE + 2 * HALO;
        let mut tile = PixelTile::new();
        for y in 0..full {
            for x in 0..full {
                let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
                let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
                if gx >= 0 && gy >= 0 && (gx as u32) < w && (gy as u32) < h {
                    let i = ((gy as u32 * w + gx as u32) * 4) as usize;
                    tile.set(x, y, 0, rgba[i]);
                    tile.set(x, y, 1, rgba[i + 1]);
                    tile.set(x, y, 2, rgba[i + 2]);
                    tile.set(x, y, 3, rgba[i + 3]);
                }
            }
        }
        tile
    }

    let store = ErrorResidualsStore::new();
    let blocks = BlockRepresentativeCache::new();
    let layer_id = LayerId::new(1);
    let left_c = tc(0, 0);
    let right_c = tc(1, 0);
    let left_src = tile_from(&rgba, W, H, left_c);
    let right_src = tile_from(&rgba, W, H, right_c);
    let strip = [(left_c, &left_src), (right_c, &right_src)];
    let mut outs = [PixelTile::new(), PixelTile::new()];
    apply_error_diffusion_tile_row_strip(
        &strip,
        &params,
        &store,
        layer_id,
        0,
        &palette_cache,
        &lut_cache,
        &doc,
        &blocks,
        &mut outs,
    )
    .unwrap();
    let left = &outs[0];
    let right = &outs[1];

    let mut mismatches = 0usize;
    let mut checked = 0usize;
    for gy in 0..H {
        for gx in 248u32..264 {
            let mv = mono[((gy * W + gx) * 3) as usize];
            let tv = if gx < TILE_SIZE {
                left.at(HALO + gx, HALO + gy, 0)
            } else {
                right.at(HALO + (gx - TILE_SIZE), HALO + gy, 0)
            };
            checked += 1;
            if (mv - tv).abs() > 1e-4 {
                mismatches += 1;
            }
        }
    }
    assert_eq!(
        mismatches, 0,
        "strip FS+palette must match monolithic at seam ({mismatches}/{checked})"
    );
}

/// Mega-pixel blocks that straddle a vertical strip boundary (`y = TILE_SIZE`)
/// must stay uniform. Without BRC lookup in the strip path, the next strip
/// re-quantizes non-reps and draws a hard cut / tile line when `pixel_size > 1`.
#[test]
fn tiled_fs_strip_megapixel_uniform_across_vertical_seam() {
    use engine_project::filters::dither_diffusion::apply_error_diffusion_tile_row_strip;
    use engine_tiles::block_cache::BlockRepresentativeCache;

    const W: u32 = 512;
    const H: u32 = 512;
    const PS: u32 = 3;

    let mut rgba = vec![0.0f32; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let t = (x as f32 + y as f32) / ((W + H - 2) as f32);
            let v = 0.2 + 0.6 * t;
            let i = ((y * W + x) * 4) as usize;
            rgba[i] = v;
            rgba[i + 1] = v * 0.95;
            rgba[i + 2] = v * 0.9;
            rgba[i + 3] = 1.0;
        }
    }

    let doc = Document::new(DocumentId::new(2), W, H);
    let params = DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 2,
        threshold_scale: 1.0,
        pixel_size: PS as u8,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        ..Default::default()
    };

    fn tile_from(rgba: &[f32], w: u32, h: u32, coord: TileCoord) -> PixelTile {
        let full = TILE_SIZE + 2 * HALO;
        let mut tile = PixelTile::new();
        for y in 0..full {
            for x in 0..full {
                let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
                let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
                if gx >= 0 && gy >= 0 && (gx as u32) < w && (gy as u32) < h {
                    let i = ((gy as u32 * w + gx as u32) * 4) as usize;
                    tile.set(x, y, 0, rgba[i]);
                    tile.set(x, y, 1, rgba[i + 1]);
                    tile.set(x, y, 2, rgba[i + 2]);
                    tile.set(x, y, 3, rgba[i + 3]);
                }
            }
        }
        tile
    }

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let store = ErrorResidualsStore::new();
    let blocks = BlockRepresentativeCache::new();
    blocks.populate_from_buffer(&rgba, W, H, doc.id.0, 1, PS);
    let layer_id = LayerId::new(1);

    // Process ty=0 then ty=1 strips (2 tiles wide), sharing residuals + BRC.
    let mut processed: Vec<(TileCoord, PixelTile)> = Vec::new();
    for ty in 0..2u32 {
        let c0 = tc(0, ty);
        let c1 = tc(1, ty);
        let t0 = tile_from(&rgba, W, H, c0);
        let t1 = tile_from(&rgba, W, H, c1);
        let strip = [(c0, &t0), (c1, &t1)];
        let mut outs = [PixelTile::new(), PixelTile::new()];
        apply_error_diffusion_tile_row_strip(
            &strip,
            &params,
            &store,
            layer_id,
            0,
            &palette_cache,
            &lut_cache,
            &doc,
            &blocks,
            &mut outs,
        )
        .unwrap();
        let mut keep0 = PixelTile::new();
        let mut keep1 = PixelTile::new();
        keep0.copy_from(&outs[0]);
        keep1.copy_from(&outs[1]);
        processed.push((c0, keep0));
        processed.push((c1, keep1));
    }

    let sample = |gx: u32, gy: u32| -> [f32; 3] {
        let tx = gx / TILE_SIZE;
        let ty = gy / TILE_SIZE;
        let tile = processed
            .iter()
            .find(|(c, _)| c.x == tx && c.y == ty)
            .map(|(_, t)| t)
            .expect("tile");
        let lx = HALO + (gx % TILE_SIZE);
        let ly = HALO + (gy % TILE_SIZE);
        [tile.at(lx, ly, 0), tile.at(lx, ly, 1), tile.at(lx, ly, 2)]
    };

    // Block origin at y=255 with ps=3 covers global rows 255,256,257 — straddles strips.
    let block_gy = (TILE_SIZE as i32 - 1).div_euclid(PS as i32) * PS as i32;
    assert_eq!(block_gy, 255, "expected straddling block origin for ps=3");
    let gx = 100u32;
    let block_gx = (gx as i32).div_euclid(PS as i32) * PS as i32;
    let rep = sample(block_gx as u32, block_gy as u32);
    for dy in 0..PS {
        for dx in 0..PS {
            let px = sample((block_gx as u32) + dx, (block_gy as u32) + dy);
            assert!(
                (px[0] - rep[0]).abs() < 1e-4
                    && (px[1] - rep[1]).abs() < 1e-4
                    && (px[2] - rep[2]).abs() < 1e-4,
                "mega-pixel block must be uniform across vertical strip seam; \
                 rep={rep:?} at +({dx},{dy})={px:?} (global {},{})",
                block_gx as u32 + dx,
                block_gy as u32 + dy
            );
        }
    }

    // Same check for a block straddling the horizontal tile seam (x=256).
    let block_gx_h = (TILE_SIZE as i32 - 1).div_euclid(PS as i32) * PS as i32;
    assert_eq!(block_gx_h, 255);
    let gy = 100u32;
    let block_gy_h = (gy as i32).div_euclid(PS as i32) * PS as i32;
    let rep_h = sample(block_gx_h as u32, block_gy_h as u32);
    for dy in 0..PS {
        for dx in 0..PS {
            let px = sample((block_gx_h as u32) + dx, (block_gy_h as u32) + dy);
            assert!(
                (px[0] - rep_h[0]).abs() < 1e-4
                    && (px[1] - rep_h[1]).abs() < 1e-4
                    && (px[2] - rep_h[2]).abs() < 1e-4,
                "mega-pixel block must be uniform across horizontal tile seam; \
                 rep={rep_h:?} at +({dx},{dy})={px:?}"
            );
        }
    }
}
