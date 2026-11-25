//! F2: independent cells — preview == export; neighbor isolation.

use std::sync::atomic::AtomicBool;

use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::full_document::{
    compute_full_document_rgba, layer_needs_full_document,
};
use engine_project::gamedev::{
    evaluate_cells_chip, layer_needs_independent_cells, process_independent_cells_rgba8,
    CellSettings, ChipState,
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

fn gradient(w: u32, h: u32) -> Vec<f32> {
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            put(&mut rgba, w, x, y, (x as f32 / w as f32) * 0.8 + 0.1);
        }
    }
    rgba
}

fn ed_layer(w: u32, h: u32) -> Layer {
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            pixel_size: 1,
            ..DitherParamsV2::default()
        }),
    ));
    layer
}

#[test]
fn preview_export_cells_bitwise() {
    let w = 16u32;
    let h = 8u32;
    let rgba = gradient(w, h);
    let cache = TileCache::new(64 * 1024 * 1024);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let layer = ed_layer(w, h);
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.gamedev.cell = Some(CellSettings {
        w: 8,
        h: 8,
        padding: 0,
        independent: true,
    });
    doc.root.push(LayerNode::Leaf(layer.clone()));

    assert!(layer_needs_independent_cells(&layer, &doc.gamedev.cell));
    assert!(layer_needs_full_document(&layer, &doc, true));

    let cancel = AtomicBool::new(false);
    let preview = compute_full_document_rgba(&cache, &layer, &doc, &cancel).unwrap();
    // Same source as FD: assemble from tiles (not the pre-decompose float buffer).
    let assembled = engine_project::filters::full_document::assemble_layer_rgba_f32(
        &cache, &layer, w, h, doc.id.0,
    )
    .unwrap();
    let export = process_independent_cells_rgba8(
        &assembled,
        w,
        h,
        doc.gamedev.cell.as_ref().unwrap(),
        &layer.filters,
        &doc.palettes,
        &cancel,
        None,
    )
    .unwrap();

    assert_eq!(preview.width, w);
    assert_eq!(preview.height, h);
    let preview_u8: Vec<u8> = preview
        .rgba_f32
        .iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect();
    assert_eq!(
        preview_u8, export,
        "preview FD cells must match export cell pipeline bytes"
    );
    // Determinism: second FD pass identical.
    let preview2 = compute_full_document_rgba(&cache, &layer, &doc, &cancel).unwrap();
    assert_eq!(preview.rgba_f32.as_ref(), preview2.rgba_f32.as_ref());
}

#[test]
fn cells_chip_ok_when_aligned() {
    let mut ctx = engine_project::GamedevContext::default();
    ctx.cell = Some(CellSettings {
        w: 8,
        h: 8,
        padding: 0,
        independent: true,
    });
    let chip = evaluate_cells_chip(&ctx, 64, 32);
    assert_eq!(chip.state, ChipState::Ok);
    assert!(chip.label.contains("independent"));
}

#[test]
fn cells_chip_warn_when_misaligned() {
    let mut ctx = engine_project::GamedevContext::default();
    ctx.cell = Some(CellSettings {
        w: 8,
        h: 8,
        padding: 0,
        independent: true,
    });
    let chip = evaluate_cells_chip(&ctx, 33, 32);
    assert_eq!(chip.state, ChipState::Warn);
    assert_eq!(
        chip.action_id.as_deref(),
        Some(engine_project::chip_action::CROP_TO_CELL_GRID)
    );
}
