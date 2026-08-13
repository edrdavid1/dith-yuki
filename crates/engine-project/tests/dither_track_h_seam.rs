//! Track H — dedicated seam tests for pattern_angle and threshold_bias.
//!
//! These are not a reuse of the A2 `dither_seam_matrix` matrix: they cover the
//! new degrees of freedom (angle ≠ 0, bias ≠ 0, and the combination with
//! pixel_size > 1).

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::types::DocumentId;
use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

fn uniform_tile(v: f32) -> PixelTile {
    let mut t = PixelTile::new();
    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            t.set(x, y, 0, v);
            t.set(x, y, 1, v);
            t.set(x, y, 2, v);
            t.set(x, y, 3, 1.0);
        }
    }
    t
}

fn bayer_params(ps: u8, angle: f32, bias: f32) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::Bayer4x4,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: ps,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        threshold_bias: bias,
        pattern_angle: angle,
        ..Default::default()
    }
}

/// Halo of the left tile overlaps the first core column of the right tile
/// (global X = TILE_SIZE). A local-coord reset would mismatch here.
fn assert_vertical_seam_continuous(params: &DitherParamsV2) {
    let tile = uniform_tile(0.45);
    let cache = ThresholdMapCache::new();
    let pk = PaletteKdCache::new();
    let lut = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), 512, 512);

    let left = apply_ordered(
        &tile,
        TileCoord { level: 0, x: 0, y: 0 },
        params,
        &cache,
        &pk,
        &lut,
        &doc,
    )
    .unwrap();
    let right = apply_ordered(
        &tile,
        TileCoord { level: 0, x: 1, y: 0 },
        params,
        &cache,
        &pk,
        &lut,
        &doc,
    )
    .unwrap();

    let g_l = GlobalCoordSigned::from_local_with_halo(
        TileCoord { level: 0, x: 0, y: 0 },
        HALO + TILE_SIZE - 1,
        HALO,
        HALO,
    );
    let g_r = GlobalCoordSigned::from_local_with_halo(
        TileCoord { level: 0, x: 1, y: 0 },
        HALO,
        HALO,
        HALO,
    );
    assert_eq!(g_l.x + 1, g_r.x);

    let left_halo_x = HALO + TILE_SIZE;
    for y in 0..16u32 {
        let rv = right.at(HALO, HALO + y, 0);
        let from_left_halo = left.at(left_halo_x, HALO + y, 0);
        assert!(
            (from_left_halo - rv).abs() < 1e-5,
            "seam mismatch at y={y}: left halo {from_left_halo} vs right core {rv}"
        );
    }
}

fn assert_axis_aligned_blocks(params: &DitherParamsV2) {
    let ps = params.pixel_size as u32;
    assert!(ps > 1);
    let tile = uniform_tile(0.45);
    let cache = ThresholdMapCache::new();
    let pk = PaletteKdCache::new();
    let lut = PaletteLutCache::new();
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let coord = TileCoord { level: 0, x: 0, y: 0 };
    let result = apply_ordered(&tile, coord, params, &cache, &pk, &lut, &doc).unwrap();

    for y in HALO..(HALO + TILE_SIZE) {
        for x in HALO..(HALO + TILE_SIZE) {
            let gcoord = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
            let block = gcoord.aligned(ps);
            // Neighbor in +x that still belongs to this block must match.
            if x + 1 < HALO + TILE_SIZE {
                let n = GlobalCoordSigned::from_local_with_halo(coord, x + 1, y, HALO);
                if n.aligned(ps) == block {
                    assert_eq!(
                        result.at(x, y, 0),
                        result.at(x + 1, y, 0),
                        "horizontal run broken at ({x},{y}) under angle={}",
                        params.pattern_angle
                    );
                }
            }
            if y + 1 < HALO + TILE_SIZE {
                let n = GlobalCoordSigned::from_local_with_halo(coord, x, y + 1, HALO);
                if n.aligned(ps) == block {
                    assert_eq!(
                        result.at(x, y, 0),
                        result.at(x, y + 1, 0),
                        "vertical run broken at ({x},{y}) under angle={}",
                        params.pattern_angle
                    );
                }
            }
        }
    }
}

#[test]
fn bayer_angle_ps1_vertical_seam() {
    assert_vertical_seam_continuous(&bayer_params(1, 15.0, 0.0));
}

#[test]
fn bayer_ps4_angle30_vertical_seam_and_rect_blocks() {
    let params = bayer_params(4, 30.0, 0.0);
    assert_vertical_seam_continuous(&params);
    assert_axis_aligned_blocks(&params);
}

#[test]
fn bayer_bias_only_vertical_seam() {
    assert_vertical_seam_continuous(&bayer_params(1, 0.0, 0.2));
}
