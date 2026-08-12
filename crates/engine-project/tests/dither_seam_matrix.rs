//! Diagnostic / acceptance matrix: seam vs pixel_size for Bayer and FS.
//!
//! Spec: `.cursor-spec/titel-line-fix.md` Steps 1–3.
//! After FS coord fix + BlockRepresentativeCache, the full matrix must be clean.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::apply_error_diffusion_with_cache;
use engine_project::filters::dither_ordered::apply_ordered_with_cache;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;
const PIXEL_SIZES: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 12, 16, 24, 32];
const IMG_W: u32 = 512;
const IMG_H: u32 = 512;
const LAYER: u32 = 1;

fn gradient_rgba() -> Vec<f32> {
    let mut rgba = vec![0.0f32; (IMG_W * IMG_H * 4) as usize];
    for y in 0..IMG_H {
        for x in 0..IMG_W {
            let t = (x as f32 / (IMG_W - 1) as f32).clamp(0.0, 1.0);
            let i = ((y * IMG_W + x) * 4) as usize;
            rgba[i] = t;
            rgba[i + 1] = t;
            rgba[i + 2] = t;
            rgba[i + 3] = 1.0;
        }
    }
    rgba
}

fn gradient_tile(coord: TileCoord, rgba: &[f32]) -> PixelTile {
    let mut tile = PixelTile::new();
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
            let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
            if gx >= 0 && gy >= 0 && (gx as u32) < IMG_W && (gy as u32) < IMG_H {
                let i = ((gy as u32 * IMG_W + gx as u32) * 4) as usize;
                tile.set(x, y, 0, rgba[i]);
                tile.set(x, y, 1, rgba[i + 1]);
                tile.set(x, y, 2, rgba[i + 2]);
                tile.set(x, y, 3, rgba[i + 3]);
            }
        }
    }
    tile
}

fn bayer_params(ps: u8) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::Bayer8x8,
        levels: 256,
        threshold_scale: 0.1,
        pixel_size: ps,
        color_mode: DitherColorMode::Grayscale,
        palette_id: None,
            ..Default::default()
    }
}

fn fs_params(ps: u8) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 256,
        threshold_scale: 1.0,
        pixel_size: ps,
        color_mode: DitherColorMode::Grayscale,
        palette_id: None,
            ..Default::default()
    }
}

fn atkinson_params(ps: u8) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::Atkinson,
        levels: 256,
        threshold_scale: 1.0,
        pixel_size: ps,
        color_mode: DitherColorMode::Grayscale,
        palette_id: None,
            ..Default::default()
    }
}

fn global_to_local(coord: TileCoord, gx: i32, gy: i32) -> Option<(u32, u32)> {
    let lx = gx - (coord.x as i32 * TILE_SIZE as i32 - HALO as i32);
    let ly = gy - (coord.y as i32 * TILE_SIZE as i32 - HALO as i32);
    if lx >= 0 && ly >= 0 && lx < TILE_FULL_SIZE as i32 && ly < TILE_FULL_SIZE as i32 {
        Some((lx as u32, ly as u32))
    } else {
        None
    }
}

fn boundary_block_metrics(left: &PixelTile, right: &PixelTile, ps: u32) -> (f64, f64) {
    if ps <= 1 {
        return (0.0, 0.0);
    }
    let left_c = TileCoord { level: 0, x: 0, y: 0 };
    let right_c = TileCoord { level: 0, x: 1, y: 0 };
    let boundary = TILE_SIZE as i32;
    let block_gx = (boundary / ps as i32) * ps as i32;
    let straddles = block_gx < boundary && block_gx + ps as i32 > boundary;

    let mut cross = 0.0f64;
    if straddles {
        for gy in [0i32, 1, 8, 32, 64, 128] {
            let Some((llx, lly)) = global_to_local(left_c, boundary - 1, gy) else {
                continue;
            };
            let Some((rlx, rly)) = global_to_local(right_c, boundary, gy) else {
                continue;
            };
            cross = cross.max(
                (left.at(llx, lly, 0) as f64 - right.at(rlx, rly, 0) as f64).abs(),
            );
        }
    }

    let first_block = block_gx;
    let mut nonuniform = 0.0f64;
    for gy in [0i32, 32, 128] {
        let mut ref_v: Option<f64> = None;
        for dx in 0..ps as i32 {
            let gx = first_block + dx;
            if gx < boundary {
                continue;
            }
            let Some((lx, ly)) = global_to_local(right_c, gx, gy) else {
                continue;
            };
            if lx < HALO || lx >= HALO + TILE_SIZE {
                continue;
            }
            let v = right.at(lx, ly, 0) as f64;
            if let Some(r0) = ref_v {
                nonuniform = nonuniform.max((v - r0).abs());
            } else {
                ref_v = Some(v);
            }
        }
    }

    (cross, nonuniform)
}

#[test]
fn step1_fs_divisors_of_256_clean() {
    let rgba = gradient_rgba();
    let blocks = BlockRepresentativeCache::new();
    let _threshold_cache = ThresholdMapCache::new();
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let layer_id = LayerId::new(LAYER);
    let left_c = TileCoord { level: 0, x: 0, y: 0 };
    let right_c = TileCoord { level: 0, x: 1, y: 0 };
    let left_src = gradient_tile(left_c, &rgba);
    let right_src = gradient_tile(right_c, &rgba);

    const SEAM: f64 = 1e-4;
    for ps in [4u8, 8, 16, 32] {
        blocks.clear_dithered();
        blocks.populate_from_buffer(&rgba, IMG_W, IMG_H, LAYER, ps as u32);
        let fs = fs_params(ps);
        let store = ErrorResidualsStore::new();
        let left = apply_error_diffusion_with_cache(
            &left_src, left_c, &fs, &store, layer_id, &palette_cache, &lut_cache, &doc, &blocks,
        )
        .unwrap();
        let right = apply_error_diffusion_with_cache(
            &right_src, right_c, &fs, &store, layer_id, &palette_cache, &lut_cache, &doc, &blocks,
        )
        .unwrap();
        let (c, u) = boundary_block_metrics(&left, &right, ps as u32);
        assert!(
            c.max(u) <= SEAM,
            "FS seam at ps={ps}: cross={c} nonuniform={u}"
        );
    }
}

#[test]
fn step2_full_seam_matrix_clean() {
    let rgba = gradient_rgba();
    let blocks = BlockRepresentativeCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let layer_id = LayerId::new(LAYER);
    let left_c = TileCoord { level: 0, x: 0, y: 0 };
    let right_c = TileCoord { level: 0, x: 1, y: 0 };
    let left_src = gradient_tile(left_c, &rgba);
    let right_src = gradient_tile(right_c, &rgba);

    const SEAM: f64 = 1e-4;

    eprintln!();
    eprintln!("ps | Bayer c/u       | FS c/u          | Bayer | FS");
    eprintln!("---+-----------------+-----------------+-------+----");

    for &ps in &PIXEL_SIZES {
        if ps > 1 {
            blocks.populate_from_buffer(&rgba, IMG_W, IMG_H, LAYER, ps as u32);
        }
        blocks.clear_dithered();

        let bayer = bayer_params(ps);
        let left_b = apply_ordered_with_cache(
            &left_src,
            left_c,
            &bayer,
            &threshold_cache,
            &palette_cache, &lut_cache, &doc,
            &blocks,
            layer_id,
        )
        .unwrap();
        let right_b = apply_ordered_with_cache(
            &right_src,
            right_c,
            &bayer,
            &threshold_cache,
            &palette_cache, &lut_cache, &doc,
            &blocks,
            layer_id,
        )
        .unwrap();

        let fs = fs_params(ps);
        let store = ErrorResidualsStore::new();
        let left_f = apply_error_diffusion_with_cache(
            &left_src, left_c, &fs, &store, layer_id, &palette_cache, &lut_cache, &doc, &blocks,
        )
        .unwrap();
        let right_f = apply_error_diffusion_with_cache(
            &right_src, right_c, &fs, &store, layer_id, &palette_cache, &lut_cache, &doc, &blocks,
        )
        .unwrap();

        let (bc, bu) = boundary_block_metrics(&left_b, &right_b, ps as u32);
        let (fc, fu) = boundary_block_metrics(&left_f, &right_f, ps as u32);
        let b_delta = bc.max(bu);
        let f_delta = fc.max(fu);

        eprintln!(
            "{ps:>2} | c{bc:.3}/u{bu:.3} | c{fc:.3}/u{fu:.3} | {:>5} | {:>2}",
            if ps == 1 || b_delta <= SEAM {
                "ok"
            } else {
                "SEAM"
            },
            if ps == 1 || f_delta <= SEAM {
                "ok"
            } else {
                "SEAM"
            },
        );

        if ps > 1 {
            assert!(
                b_delta <= SEAM,
                "Bayer seam at ps={ps}: cross={bc} nonuniform={bu}"
            );
            assert!(
                f_delta <= SEAM,
                "FS seam at ps={ps}: cross={fc} nonuniform={fu}"
            );
        }
    }
}

#[test]
fn step3_invalidation_recomputes_representative() {
    let mut rgba = gradient_rgba();
    let cache = BlockRepresentativeCache::new();
    cache.populate_from_buffer(&rgba, IMG_W, IMG_H, LAYER, 8);
    let key = engine_tiles::block_cache::BlockCoord::from_global(LAYER, 256, 0, 8);
    let before = cache.get_raw(key).unwrap()[0];

    // Edit the representative pixel of the block at gx=256
    let idx = (0 * IMG_W + 256) as usize * 4;
    rgba[idx] = 0.123;
    cache.invalidate_all();
    cache.populate_from_buffer(&rgba, IMG_W, IMG_H, LAYER, 8);
    let after = cache.get_raw(key).unwrap()[0];
    assert_ne!(before, after);
    assert!((after - 0.123).abs() < 1e-6);
}

#[test]
fn step3_populate_perf_is_linear() {
    use std::time::Instant;
    let rgba = gradient_rgba();
    let cache = BlockRepresentativeCache::new();
    let t0 = Instant::now();
    cache.populate_from_buffer(&rgba, IMG_W, IMG_H, LAYER, 16);
    let elapsed = t0.elapsed();
    eprintln!("populate 512×512 ps=16: {elapsed:?}");
    // Should be well under 100ms even in debug; catch accidental O(n²).
    assert!(
        elapsed.as_millis() < 500,
        "block cache populate too slow: {elapsed:?}"
    );
}

/// Track A: Atkinson × selected pixel_sizes stays seamless on gradient.
#[test]
fn track_a_atkinson_seam_sample_clean() {
    const SEAM: f64 = 1e-4;
    let rgba = gradient_rgba();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let layer_id = LayerId::new(LAYER);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let blocks = BlockRepresentativeCache::new();
    blocks.populate_from_buffer(&rgba, IMG_W, IMG_H, LAYER, 1);
    for ps in [1u8, 3, 8, 16] {
        if ps > 1 {
            blocks.populate_from_buffer(&rgba, IMG_W, IMG_H, LAYER, ps as u32);
        }
        let left_c = TileCoord { level: 0, x: 0, y: 0 };
        let right_c = TileCoord { level: 0, x: 1, y: 0 };
        let store = ErrorResidualsStore::new();
        let atk = atkinson_params(ps);
        let left = apply_error_diffusion_with_cache(
            &gradient_tile(left_c, &rgba),
            left_c,
            &atk,
            &store,
            layer_id,
            &palette_cache, &lut_cache, &doc,
            &blocks,
        )
        .unwrap();
        let right = apply_error_diffusion_with_cache(
            &gradient_tile(right_c, &rgba),
            right_c,
            &atk,
            &store,
            layer_id,
            &palette_cache, &lut_cache, &doc,
            &blocks,
        )
        .unwrap();
        if ps > 1 {
            let (c, u) = boundary_block_metrics(&left, &right, ps as u32);
            assert!(
                c.max(u) <= SEAM,
                "Atkinson seam at ps={ps}: cross={c} nonuniform={u}"
            );
        }
    }
}

/// Track A: 2×2 FS with corner channel — no systematic BR darkening at (1,1) corner.
#[test]
fn track_a_fs_2x2_diagonal_seed_no_boundary_darkening() {
    let rgba = gradient_rgba();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let layer_id = LayerId::new(LAYER);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let blocks = BlockRepresentativeCache::new();
    let params = fs_params(1);
    let store = ErrorResidualsStore::new();

    let coords = [
        TileCoord { level: 0, x: 0, y: 0 },
        TileCoord { level: 0, x: 1, y: 0 },
        TileCoord { level: 0, x: 0, y: 1 },
        TileCoord { level: 0, x: 1, y: 1 },
    ];
    let mut tiles = Vec::new();
    for c in coords {
        tiles.push(
            apply_error_diffusion_with_cache(
                &gradient_tile(c, &rgba),
                c,
                &params,
                &store,
                layer_id,
                &palette_cache, &lut_cache, &doc,
                &blocks,
            )
            .unwrap(),
        );
    }

    // Mean luminance on first core pixel of (1,1) vs a few pixels inward —
    // diagonal drop historically darkened the corner relative to interior.
    let t11 = &tiles[3];
    let corner_lum = t11.at(HALO, HALO, 0);
    let mut interior = 0.0f32;
    let mut n = 0u32;
    for y in (HALO + 8)..(HALO + 16) {
        for x in (HALO + 8)..(HALO + 16) {
            interior += t11.at(x, y, 0);
            n += 1;
        }
    }
    let interior_mean = interior / n as f32;
    // Gradient at that region is near mid-gray; allow dither noise but reject
    // a large systematic darkening from lost diagonal error.
    assert!(
        (corner_lum - interior_mean).abs() < 0.35,
        "tile (1,1) corner lum={corner_lum} vs interior={interior_mean} — possible diagonal loss"
    );
    assert!(
        store.get_diag(layer_id, TileCoord { level: 0, x: 1, y: 1 }).is_some(),
        "corner channel must be stored from (0,0)"
    );
}
