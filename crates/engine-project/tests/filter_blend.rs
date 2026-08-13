//! Track I — per-filter opacity / blend wrapper.
//!
//! Fast path is bit-identical to today's apply. ED at 50% opacity on a 2×2
//! grid stays seamless (residuals from the full result) and is a 50% mix
//! with the pre-filter tile.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::compositor::blend_tile;
use engine_project::document::Document;
use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams};
use engine_project::filters::apply_filter_to_tile_with_caches;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::layer::Layer;
use engine_project::types::{BlendMode, DocumentId, LayerId, LayerKind};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;
const IMG_W: u32 = 512;
const IMG_H: u32 = 512;

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

fn fs_filter(opacity: f32) -> FilterInstance {
    let mut filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }),
    );
    filter.opacity = opacity;
    filter
}

fn apply_layer(
    tile: &PixelTile,
    layer: &Layer,
    coord: TileCoord,
    residuals: &ErrorResidualsStore,
) -> PixelTile {
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let blocks = BlockRepresentativeCache::new();
    apply_filter_to_tile_with_caches(
        tile,
        layer,
        coord,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
        residuals,
        &blocks,
        None,
    )
    .unwrap()
}

fn vertical_seam_jump(left: &PixelTile, right: &PixelTile) -> f32 {
    let mut max_jump = 0.0f32;
    for y in HALO..(HALO + TILE_SIZE) {
        let jump = (left.at(HALO + TILE_SIZE - 1, y, 0) - right.at(HALO, y, 0)).abs();
        if jump > max_jump {
            max_jump = jump;
        }
    }
    max_jump
}

#[test]
fn fast_path_opacity_one_normal_matches_full_apply() {
    let rgba = gradient_rgba();
    let coord = TileCoord { level: 0, x: 0, y: 0 };
    let pre = gradient_tile(coord, &rgba);

    let mut layer_default = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    layer_default.filters.push(fs_filter(1.0));

    let mut layer_explicit = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    let mut explicit = fs_filter(1.0);
    explicit.blend_mode = BlendMode::Normal;
    layer_explicit.filters.push(explicit);

    let a = apply_layer(&pre, &layer_default, coord, &ErrorResidualsStore::new());
    let b = apply_layer(&pre, &layer_explicit, coord, &ErrorResidualsStore::new());
    assert_eq!(a.data.as_ref(), b.data.as_ref());
}

#[test]
fn ed_opacity_50_is_mix_with_pre() {
    let rgba = gradient_rgba();
    let coord = TileCoord { level: 0, x: 0, y: 0 };
    let pre = gradient_tile(coord, &rgba);

    let mut layer_full = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    layer_full.filters.push(fs_filter(1.0));
    let full = apply_layer(&pre, &layer_full, coord, &ErrorResidualsStore::new());

    let mut layer_half = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    layer_half.filters.push(fs_filter(0.5));
    let half = apply_layer(&pre, &layer_half, coord, &ErrorResidualsStore::new());

    let mut expected = PixelTile::new();
    expected.data.copy_from_slice(&pre.data);
    blend_tile(&mut expected, &full, BlendMode::Normal, 0.5);

    for y in HALO..(HALO + TILE_SIZE) {
        for x in HALO..(HALO + TILE_SIZE) {
            for c in 0..4u32 {
                let got = half.at(x, y, c);
                let exp = expected.at(x, y, c);
                assert!(
                    (got - exp).abs() < 1e-5,
                    "50% mix mismatch at ({x},{y},c{c}): {got} vs {exp}"
                );
            }
        }
    }
}

#[test]
fn ed_opacity_50_2x2_no_worse_seam_than_full() {
    let rgba = gradient_rgba();
    let left_c = TileCoord { level: 0, x: 0, y: 0 };
    let right_c = TileCoord { level: 0, x: 1, y: 0 };
    let left_pre = gradient_tile(left_c, &rgba);
    let right_pre = gradient_tile(right_c, &rgba);

    let mut layer_full = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    layer_full.filters.push(fs_filter(1.0));
    let store_full = ErrorResidualsStore::new();
    let left_full = apply_layer(&left_pre, &layer_full, left_c, &store_full);
    let right_full = apply_layer(&right_pre, &layer_full, right_c, &store_full);
    let full_jump = vertical_seam_jump(&left_full, &right_full);

    let mut layer_half = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    layer_half.filters.push(fs_filter(0.5));
    let store_half = ErrorResidualsStore::new();
    let left_half = apply_layer(&left_pre, &layer_half, left_c, &store_half);
    let right_half = apply_layer(&right_pre, &layer_half, right_c, &store_half);
    let half_jump = vertical_seam_jump(&left_half, &right_half);

    assert!(
        half_jump <= full_jump + 1e-4,
        "50% opacity introduced a worse seam: half={half_jump} full={full_jump}"
    );

    let mut expected_left = PixelTile::new();
    expected_left.data.copy_from_slice(&left_pre.data);
    blend_tile(&mut expected_left, &left_full, BlendMode::Normal, 0.5);
    assert!(
        (left_half.at(HALO, HALO, 0) - expected_left.at(HALO, HALO, 0)).abs() < 1e-5,
        "left tile is not a 50% mix with pre"
    );
}
