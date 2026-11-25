//! F1 step 2: ordered / blue-noise Pipeline Tile route.

use std::sync::atomic::AtomicBool;

use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::full_document::{
    compute_full_document_rgba, layer_needs_full_document,
};
use engine_project::gamedev::{
    clear_wrap_padding_on_layer, dither_period_aligned, layer_needs_pipeline_tile_full_document,
    should_skip_mosaic,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::Document;
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;

fn put(buf: &mut [f32], w: u32, x: u32, y: u32, v: f32) {
    let i = ((y * w + x) * 4) as usize;
    buf[i] = v;
    buf[i + 1] = v;
    buf[i + 2] = v;
    buf[i + 3] = 1.0;
}

fn get(buf: &[f32], w: u32, x: u32, y: u32) -> f32 {
    buf[((y * w + x) * 4) as usize]
}

/// Content continuous on a torus **and** with equal opposite edge pixels so
/// period-aligned ordered dither can be Exact-seamless.
fn seamless_src(w: u32, h: u32) -> Vec<f32> {
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    let tw = std::f32::consts::TAU / w as f32;
    let th = std::f32::consts::TAU / h as f32;
    for y in 0..h {
        for x in 0..w {
            let v = 0.5 + 0.25 * (x as f32 * tw).sin() + 0.25 * (y as f32 * th).cos();
            put(&mut rgba, w, x, y, v.clamp(0.0, 1.0));
        }
    }
    // Force bit-equal wrap edges (source must tile for Exact ring on output).
    for y in 0..h {
        let v = get(&rgba, w, 0, y);
        put(&mut rgba, w, w - 1, y, v);
    }
    for x in 0..w {
        let v = get(&rgba, w, x, 0);
        put(&mut rgba, w, x, h - 1, v);
    }
    rgba
}

fn bayer_layer(w: u32, h: u32) -> Layer {
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::Bayer8x8,
            levels: 4,
            pixel_size: 1,
            wrap_padding: true,
            ..DitherParamsV2::default()
        }),
    ));
    layer
}

fn setup(w: u32, h: u32, tile: bool) -> (Document, Layer, TileCache) {
    let rgba = seamless_src(w, h);
    let cache = TileCache::new(64 * 1024 * 1024);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.gamedev.tile.enabled = tile;
    let layer = bayer_layer(w, h);
    doc.root.push(LayerNode::Leaf(layer.clone()));
    (doc, layer, cache)
}

#[test]
fn period_aligned_skips_mosaic_and_tile_continuity() {
    let w = 32u32;
    let h = 32u32;
    let params = DitherParamsV2 {
        mode: DitherModeV2::Bayer8x8,
        pixel_size: 1,
        ..DitherParamsV2::default()
    };
    assert!(dither_period_aligned(&params, w, h));

    let (doc_on, layer, cache) = setup(w, h, true);
    assert!(should_skip_mosaic(&layer, w, h));
    assert!(!layer_needs_pipeline_tile_full_document(&layer, true, w, h));
    assert!(!layer_needs_full_document(&layer, &doc_on, true));

    let cancel = AtomicBool::new(false);
    let tile = compute_full_document_rgba(&cache, &layer, &doc_on, &cancel).unwrap();
    assert_eq!(tile.width, w);
    assert_eq!(tile.height, h);

    // Continuity: dither(tile) placed twice == continuous dither of [src|src].
    // (Opposite-edge *equality* is the wrong check for Bayer — cells 0 and N-1 differ.)
    let src = seamless_src(w, h);
    let mut double = vec![0.0f32; w as usize * 2 * h as usize * 4];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let si = (y * w as usize + x) * 4;
            let d0 = (y * (w as usize * 2) + x) * 4;
            let d1 = (y * (w as usize * 2) + w as usize + x) * 4;
            double[d0..d0 + 4].copy_from_slice(&src[si..si + 4]);
            double[d1..d1 + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    let cache2 = TileCache::new(64 * 1024 * 1024);
    decompose_image_to_tiles(&double, w * 2, h, 1, 1, &cache2).unwrap();
    let mut doc2 = Document::new(DocumentId::new(1), w * 2, h);
    let layer2 = bayer_layer(w * 2, h);
    doc2.root.push(LayerNode::Leaf(layer2.clone()));
    let cont = compute_full_document_rgba(&cache2, &layer2, &doc2, &cancel).unwrap();

    // Left half of continuous dither must match the single-tile dither.
    for y in 0..h as usize {
        for x in 0..w as usize {
            for c in 0..4 {
                let a = tile.rgba_f32[(y * w as usize + x) * 4 + c];
                let b = cont.rgba_f32[(y * (w as usize * 2) + x) * 4 + c];
                assert!(
                    (a - b).abs() < 1e-5,
                    "tile continuity broken at ({x},{y},c{c}): {a} vs {b}"
                );
            }
        }
    }
    // Right half likewise (second copy).
    for y in 0..h as usize {
        for x in 0..w as usize {
            for c in 0..4 {
                let a = tile.rgba_f32[(y * w as usize + x) * 4 + c];
                let b = cont.rgba_f32[(y * (w as usize * 2) + w as usize + x) * 4 + c];
                assert!(
                    (a - b).abs() < 1e-5,
                    "second copy continuity broken at ({x},{y},c{c}): {a} vs {b}"
                );
            }
        }
    }
}

#[test]
fn tile_on_non_multiple_changes_output() {
    let w = 33u32;
    let h = 32u32;
    let params = DitherParamsV2 {
        mode: DitherModeV2::Bayer8x8,
        pixel_size: 1,
        ..DitherParamsV2::default()
    };
    assert!(!dither_period_aligned(&params, w, h));

    let rgba = seamless_src(w, h);
    let cache = TileCache::new(64 * 1024 * 1024);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();

    let mut doc_off = Document::new(DocumentId::new(1), w, h);
    doc_off.gamedev.tile.enabled = false;
    let layer_off = bayer_layer(w, h);
    doc_off.root.push(LayerNode::Leaf(layer_off.clone()));

    let mut doc_on = Document::new(DocumentId::new(1), w, h);
    doc_on.gamedev.tile.enabled = true;
    let layer_on = bayer_layer(w, h);
    doc_on.root.push(LayerNode::Leaf(layer_on.clone()));

    assert!(layer_needs_pipeline_tile_full_document(&layer_on, true, w, h));
    assert!(layer_needs_full_document(&layer_on, &doc_on, true));
    assert!(!layer_needs_full_document(&layer_off, &doc_off, true));

    let cancel = AtomicBool::new(false);
    let out_off = compute_full_document_rgba(&cache, &layer_off, &doc_off, &cancel).unwrap();
    let out_on = compute_full_document_rgba(&cache, &layer_on, &doc_on, &cancel).unwrap();
    assert_eq!(out_off.width, w);
    assert_eq!(out_on.width, w);
    assert_ne!(
        out_off.rgba_f32.as_ref(),
        out_on.rgba_f32.as_ref(),
        "enabling Tile on non-period-multiple ordered must change output (mosaic phase)"
    );
}

#[test]
fn preview_export_same_context_bitwise() {
    let w = 33u32;
    let h = 24u32;
    let (doc, layer, cache) = setup(w, h, true);
    let cancel = AtomicBool::new(false);
    let a = compute_full_document_rgba(&cache, &layer, &doc, &cancel).unwrap();
    let b = compute_full_document_rgba(&cache, &layer, &doc, &cancel).unwrap();
    assert_eq!(a.rgba_f32.as_ref(), b.rgba_f32.as_ref());
}

#[test]
fn tile_clears_wrap_padding() {
    let mut layer = bayer_layer(32, 32);
    assert!(matches!(
        &layer.filters[0].params,
        FilterParams::DitherV2(p) if p.wrap_padding
    ));
    clear_wrap_padding_on_layer(&mut layer);
    assert!(matches!(
        &layer.filters[0].params,
        FilterParams::DitherV2(p) if !p.wrap_padding
    ));
}
