//! Track C Phase 1 — pattern seam continuity (Halftone, Wave) on 2×2 tiles.
//!
//! CRT seam coverage lives in `filters/crt.rs` unit tests.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered;
use engine_project::types::DocumentId;
use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

fn uniform_tile(v: f32) -> PixelTile {
    let mut t = PixelTile::new();
    let full = TILE_SIZE + 2 * HALO;
    for y in 0..full {
        for x in 0..full {
            t.set(x, y, 0, v);
            t.set(x, y, 1, v);
            t.set(x, y, 2, v);
            t.set(x, y, 3, 1.0);
        }
    }
    t
}

fn core_at(tile: &PixelTile, local_x: u32, local_y: u32, c: u32) -> f32 {
    tile.at(HALO + local_x, HALO + local_y, c)
}

/// Shared vertical edge: last column of tile (0,0) vs first column of tile (1,0)
/// must match the continuous global pattern (identical when both tiles process
/// the same global source tone).
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

    // Verify global coords are consecutive across the seam.
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

    // For a uniform field, each tile's output at a given global (X,Y) must equal
    // the other tile's output at the same global — here we check that adjacent
    // columns are produced from GlobalCoord (no local-reset seam): recompute by
    // applying both tiles and comparing a synthetic full-canvas stitch is heavy;
    // instead compare that left's last column equals what right would produce if
    // we sampled the same global via applying a third reference... Simpler check:
    // apply once more on tile (0,0) at the *same* global column as right's first
    // by ensuring pattern helpers are continuous — max abs diff between consecutive
    // global samples from the two tiles' shared edge neighborhood is bounded by
    // the pattern's own step (not a hard discontinuity from local coords).
    //
    // Practical assertion: values at global X=255 and X=256 from left/right tiles
    // must equal apply on a single reference path. We reconstruct expected by
    // running apply on both and reading; for Halftone/Wave continuity, the key
    // bug (local coords) would make column 255 of left match column -1 equivalent
    // of local wrap — i.e. identical to left's column 0. So assert last≠first of left
    // for Wave (varies), and that right's first column differs from left's first
    // the same way consecutive globals should.
    let left_first = core_at(&left, 0, 0, 0);
    let left_last = core_at(&left, TILE_SIZE - 1, 0, 0);
    let right_first = core_at(&right, 0, 0, 0);

    // If tiles used local coords, left_last would equal left's pattern at local TILE_SIZE-1
    // while right_first would restart at local 0 — often left_first == right_first always
    // (same local). Continuity: right_first should equal the value that belongs at global 256,
    // which for a seamless filter is generally NOT equal to left_first when the pattern
    // period doesn't divide 256 oddly for Wave wavelength 8 (256%8==0 → left_first==right_first
    // is actually correct!). Use wavelength 7 so period doesn't align with TILE_SIZE.
    let _ = (left_first, left_last, right_first);

    for y in 0..16u32 {
        let lv = core_at(&left, TILE_SIZE - 1, y, 0);
        let rv = core_at(&right, 0, y, 0);
        // Both must be finite valid outputs; for uniform input they should be a
        // continuous pair — max jump limited. For binary halftone, jump can be 1.0;
        // we only assert both tiles produced something and that a full-row re-apply
        // of left at the right's global isn't needed — check equality of the *same*
        // global pixel computed from both tiles via halo overlap.
        // Halo of left includes global 256; local x for global 256 on tile (0,0):
        // local_with_halo = 256 - 0 + HALO = 258. That pixel should match right core x=0.
        let left_halo_x = HALO + TILE_SIZE; // first pixel past core = global TILE_SIZE
        if left_halo_x < TILE_SIZE + 2 * HALO {
            let from_left_halo = left.at(left_halo_x, HALO + y, 0);
            assert!(
                (from_left_halo - rv).abs() < 1e-5,
                "seam mismatch at y={y}: left halo {from_left_halo} vs right core {rv} (left_edge={lv})"
            );
        }
    }
}

#[test]
fn cmyk_halftone_2x2_vertical_seam() {
    let params = DitherParamsV2 {
        mode: DitherModeV2::CmykHalftone,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        halftone_cell_size: 8,
        ..Default::default()
    };
    assert_vertical_seam_continuous(&params);
}

#[test]
fn wave_2x2_vertical_seam() {
    let params = DitherParamsV2 {
        mode: DitherModeV2::Wave,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size: 1,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        wave_wavelength: 7.0,
        wave_amplitude: 1.0,
        wave_phase: 0.0,
        wave_angle: 0.0,
        ..Default::default()
    };
    assert_vertical_seam_continuous(&params);
}
