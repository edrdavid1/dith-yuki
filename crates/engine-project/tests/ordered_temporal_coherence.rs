//! Phase 2.1 — temporal coherence for ordered dither.
//!
//! Invariant: for an ordered algorithm, an output pixel depends only on the
//! input pixel (or its `pixel_size` block representative) and its global
//! coordinates. Changing a subset of input pixels must change the output in
//! exactly those places (or their blocks).

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::filters::dither_ordered::apply_ordered_with_cache_into;
use engine_project::types::{DocumentId, LayerId};
use engine_project::Document;
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use proptest::prelude::*;

const CORE: u32 = TILE_SIZE;
const FULL: u32 = TILE_SIZE + 2 * HALO;

fn apply_ordered(src: &PixelTile, params: &DitherParamsV2, coord: TileCoord) -> PixelTile {
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let block_cache = BlockRepresentativeCache::new();
    let mut out = PixelTile::new();
    apply_ordered_with_cache_into(
        src,
        coord,
        params,
        &threshold_cache,
        &palette_cache,
        &lut_cache,
        &doc,
        &block_cache,
        LayerId::new(1),
        &mut out,
    )
    .expect("ordered apply");
    out
}

fn default_params(mode: DitherModeV2, pixel_size: u8) -> DitherParamsV2 {
    DitherParamsV2 {
        mode,
        levels: 4,
        threshold_scale: 1.0,
        pixel_size,
        color_mode: DitherColorMode::Rgb,
        palette_id: None,
        dither_alpha: false,
        pattern_angle: 0.0,
        ..DitherParamsV2::default()
    }
}

fn tile_from_vals(vals: &[f32]) -> PixelTile {
    let mut tile = PixelTile::new();
    for y in 0..FULL {
        for x in 0..FULL {
            let i = ((y * FULL + x) * 4) as usize;
            tile.set(x, y, 0, vals[i]);
            tile.set(x, y, 1, vals[i + 1]);
            tile.set(x, y, 2, vals[i + 2]);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

/// Core (non-halo) pixels where RGB differs between two tiles.
fn differing_core_pixels(a: &PixelTile, b: &PixelTile) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for y in HALO..HALO + CORE {
        for x in HALO..HALO + CORE {
            let dr = (a.at(x, y, 0) - b.at(x, y, 0)).abs();
            let dg = (a.at(x, y, 1) - b.at(x, y, 1)).abs();
            let db = (a.at(x, y, 2) - b.at(x, y, 2)).abs();
            if dr > 1e-5 || dg > 1e-5 || db > 1e-5 {
                out.push((x, y));
            }
        }
    }
    out
}

fn block_origin(x: u32, y: u32, ps: u8, coord: TileCoord) -> (i32, i32) {
    use engine_tiles::coords::GlobalCoordSigned;
    let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
    let b = g.aligned(ps.max(1) as u32);
    (b.x, b.y)
}

fn arb_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::Bayer2x2),
        Just(DitherModeV2::Bayer4x4),
        Just(DitherModeV2::Bayer8x8),
        Just(DitherModeV2::ClusteredDotOrdered),
        Just(DitherModeV2::DispersedDotOrdered),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Changing a subset of input pixels changes ordered output only in those
    /// pixels (pixel_size = 1) or their blocks (pixel_size > 1).
    #[test]
    fn ordered_output_differs_only_where_input_changed(
        mode in arb_mode(),
        pixel_size in prop_oneof![Just(1u8), Just(2u8), Just(4u8)],
        vals in prop::collection::vec(0.0f32..1.0f32, (FULL * FULL * 4) as usize),
        flip_mask in prop::collection::vec(proptest::bool::weighted(0.15), (CORE * CORE) as usize),
    ) {
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let params = default_params(mode.clone(), pixel_size);

        let frame_a = tile_from_vals(&vals);
        let mut frame_b = PixelTile::new();
        frame_b.copy_from(&frame_a);

        let mut changed_blocks = std::collections::HashSet::new();
        for y in 0..CORE {
            for x in 0..CORE {
                let mi = (y * CORE + x) as usize;
                if !flip_mask[mi] {
                    continue;
                }
                let lx = x + HALO;
                let ly = y + HALO;
                frame_b.set(lx, ly, 0, (1.0 - frame_a.at(lx, ly, 0)).clamp(0.0, 1.0));
                frame_b.set(lx, ly, 1, frame_a.at(lx, ly, 1) * 0.37);
                frame_b.set(lx, ly, 2, (frame_a.at(lx, ly, 2) + 0.31).rem_euclid(1.0));
                changed_blocks.insert(block_origin(lx, ly, pixel_size, coord));
            }
        }

        if changed_blocks.is_empty() {
            let out_a = apply_ordered(&frame_a, &params, coord);
            let out_b = apply_ordered(&frame_b, &params, coord);
            prop_assert_eq!(out_a.data.as_ref(), out_b.data.as_ref());
            return Ok(());
        }

        let out_a = apply_ordered(&frame_a, &params, coord);
        let out_b = apply_ordered(&frame_b, &params, coord);

        for (x, y) in differing_core_pixels(&out_a, &out_b) {
            let block = block_origin(x, y, pixel_size, coord);
            prop_assert!(
                changed_blocks.contains(&block),
                "ordered output changed at local ({x},{y}) block {block:?} \
                 but no input change touched that block (mode={mode:?}, ps={pixel_size})"
            );
        }
    }

    /// Same input + same global coords ⇒ identical ordered output (determinism /
    /// fixed pattern phase across "frames" / files).
    #[test]
    fn ordered_same_input_same_coords_is_byte_identical(
        mode in arb_mode(),
        pixel_size in 1u8..=4u8,
        vals in prop::collection::vec(0.0f32..1.0f32, (FULL * FULL * 4) as usize),
        tile_x in 0u32..3u32,
        tile_y in 0u32..3u32,
    ) {
        let coord = TileCoord { level: 0, x: tile_x, y: tile_y };
        let params = default_params(mode, pixel_size);
        let frame = tile_from_vals(&vals);
        let a = apply_ordered(&frame, &params, coord);
        let b = apply_ordered(&frame, &params, coord);
        prop_assert_eq!(a.data.as_ref(), b.data.as_ref());
    }
}

#[test]
fn pattern_angle_is_stable_across_identical_frames() {
    let coord = TileCoord {
        level: 0,
        x: 1,
        y: 2,
    };
    let mut params = default_params(DitherModeV2::Bayer8x8, 1);
    params.pattern_angle = 37.0;

    let mut frame = PixelTile::new();
    for y in 0..FULL {
        for x in 0..FULL {
            let t = ((x + y) as f32) / (FULL as f32);
            frame.set(x, y, 0, t);
            frame.set(x, y, 1, 1.0 - t);
            frame.set(x, y, 2, 0.5);
            frame.set(x, y, 3, 1.0);
        }
    }

    let a = apply_ordered(&frame, &params, coord);
    let b = apply_ordered(&frame, &params, coord);
    assert_eq!(
        a.data.as_ref(),
        b.data.as_ref(),
        "pattern_angle must not introduce per-call phase drift"
    );
}
