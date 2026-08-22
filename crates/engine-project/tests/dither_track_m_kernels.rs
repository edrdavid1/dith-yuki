//! Track M1: JJN / Stucki / Burkes / Sierra on the V2 residual path.
//!
//! Unit offset coverage lives in `dither_diffusion` tests. This file checks
//! a 2×2 gradient sample per kernel (seam helper, not the full A1 matrix).

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_diffusion::apply_error_diffusion_with_cache;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::types::{DocumentId, LayerId};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;
const IMG_W: u32 = 512;
const IMG_H: u32 = 512;
const LAYER: u32 = 1;

const M1_KERNELS: [DitherModeV2; 4] = [
    DitherModeV2::JarvisJudiceNinke,
    DitherModeV2::Stucki,
    DitherModeV2::Burkes,
    DitherModeV2::Sierra,
];

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

fn params(mode: DitherModeV2) -> DitherParamsV2 {
    DitherParamsV2 {
        mode,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Grayscale,
        palette_id: None,
        ..Default::default()
    }
}

/// 2×2 residual path: neighbor tile differs from isolated, and corner is stored.
#[test]
fn m1_kernels_2x2_seam_sample() {
    let rgba = gradient_rgba();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let layer_id = LayerId::new(LAYER);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let blocks = BlockRepresentativeCache::new();
    let coords = [
        TileCoord { level: 0, x: 0, y: 0 },
        TileCoord { level: 0, x: 1, y: 0 },
        TileCoord { level: 0, x: 0, y: 1 },
        TileCoord { level: 0, x: 1, y: 1 },
    ];

    for mode in M1_KERNELS {
        let p = params(mode.clone());
        let store = ErrorResidualsStore::new();
        for c in coords {
            apply_error_diffusion_with_cache(
                &gradient_tile(c, &rgba),
                c,
                &p,
                &store,
                layer_id,
                &palette_cache,
                &lut_cache,
                &doc,
                &blocks,
            )
            .unwrap();
        }

        let isolated = ErrorResidualsStore::new();
        let right_iso = apply_error_diffusion_with_cache(
            &gradient_tile(coords[1], &rgba),
            coords[1],
            &p,
            &isolated,
            layer_id,
            &palette_cache,
            &lut_cache,
            &doc,
            &blocks,
        )
        .unwrap();
        let right_seeded = apply_error_diffusion_with_cache(
            &gradient_tile(coords[1], &rgba),
            coords[1],
            &p,
            &store,
            layer_id,
            &palette_cache,
            &lut_cache,
            &doc,
            &blocks,
        )
        .unwrap();

        let differs = (HALO..(HALO + 16)).any(|y| {
            right_seeded.at(HALO, y, 0) != right_iso.at(HALO, y, 0)
        });
        assert!(
            differs,
            "{mode:?}: left residuals must change tile (1,0) left edge"
        );
        assert!(
            store
                .get_diag(1, layer_id, TileCoord { level: 0, x: 1, y: 1 })
                .is_some(),
            "{mode:?}: corner residuals must be stored from (0,0)"
        );
    }
}

/// FS and Atkinson still produce valid levels after the shared `distribute_kernel` path.
#[test]
fn m1_fs_atkinson_still_quantize() {
    let rgba = gradient_rgba();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let layer_id = LayerId::new(LAYER);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let blocks = BlockRepresentativeCache::new();
    let coord = TileCoord { level: 0, x: 0, y: 0 };
    for mode in [DitherModeV2::FloydSteinberg, DitherModeV2::Atkinson] {
        let p = params(mode.clone());
        let store = ErrorResidualsStore::new();
        let result = apply_error_diffusion_with_cache(
            &gradient_tile(coord, &rgba),
            coord,
            &p,
            &store,
            layer_id,
            &palette_cache,
            &lut_cache,
            &doc,
            &blocks,
        )
        .unwrap();
        let levels = p.levels as f32;
        for y in HALO..(HALO + 8) {
            for x in HALO..(HALO + 8) {
                let v = result.at(x, y, 0);
                let k = v * (levels - 1.0);
                assert!(
                    (k - k.round()).abs() < 1e-4,
                    "{mode:?} invalid level {v} at ({x},{y})"
                );
            }
        }
    }
}

/// Track M2: serpentine ON, 2×2 tiles, no seam on even **and** odd global rows.
#[test]
fn m2_serpentine_even_and_odd_global_row_seam() {
    const SEAM: f64 = 0.4;
    let rgba = gradient_rgba();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let layer_id = LayerId::new(LAYER);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let blocks = BlockRepresentativeCache::new();
    let mut p = params(DitherModeV2::FloydSteinberg);
    p.serpentine = true;
    p.levels = 4;

    let left_c = TileCoord { level: 0, x: 0, y: 0 };
    let right_c = TileCoord { level: 0, x: 1, y: 0 };
    let store = ErrorResidualsStore::new();
    let left = apply_error_diffusion_with_cache(
        &gradient_tile(left_c, &rgba),
        left_c,
        &p,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
        &blocks,
    )
    .unwrap();
    let right = apply_error_diffusion_with_cache(
        &gradient_tile(right_c, &rgba),
        right_c,
        &p,
        &store,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
        &blocks,
    )
    .unwrap();

    let isolated = ErrorResidualsStore::new();
    let right_iso = apply_error_diffusion_with_cache(
        &gradient_tile(right_c, &rgba),
        right_c,
        &p,
        &isolated,
        layer_id,
        &palette_cache,
        &lut_cache,
        &doc,
        &blocks,
    )
    .unwrap();

    let incoming = store
        .get_left(1, layer_id, right_c)
        .expect("left tile must store right-edge residuals");
    let energy: f32 = incoming.right.iter().map(|v| v.abs()).sum();
    assert!(energy > 1e-6, "serpentine must produce horizontal residuals");

    let any_seed = (HALO..(HALO + TILE_SIZE)).any(|y| {
        (0..4u32).any(|dx| right.at(HALO + dx, y, 0) != right_iso.at(HALO + dx, y, 0))
    });
    assert!(any_seed, "left residuals must change tile (1,0) output");

    for odd in [false, true] {
        let mut joint = 0.0f64;
        let mut n = 0u32;
        for local_y in 0..TILE_SIZE {
            let gy = local_y as i32;
            if (gy.rem_euclid(2) == 1) != odd {
                continue;
            }
            let ly = HALO + local_y;
            let l = left.at(HALO + TILE_SIZE - 1, ly, 0) as f64;
            let r = right.at(HALO, ly, 0) as f64;
            joint += (l - r).abs();
            n += 1;
        }
        let mean = joint / n as f64;
        assert!(
            mean <= SEAM,
            "serpentine {} global rows: joint mean |Δ|={mean} (n={n})",
            if odd { "odd" } else { "even" }
        );
    }
}
