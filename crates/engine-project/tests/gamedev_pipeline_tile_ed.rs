//! F1 step 3: ED Pipeline Tile mosaic + mandatory memory gate.

use std::sync::atomic::AtomicBool;

use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::full_document::{
    compute_full_document_rgba, layer_needs_full_document,
};
use engine_project::gamedev::{
    begin_pipeline_tile_pass, chip_state_for_seam_report, layer_needs_pipeline_tile_full_document,
    pipeline_tile_fits_limits, pipeline_tile_memory_gate_error, seamless_chip_from_tile,
    should_skip_mosaic, validate_seams_rgba8, PipelineTileError, SeamMode, PIPELINE_TILE_MAX_MOSAIC_PIXELS,
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

fn gradient_src(w: u32, h: u32) -> Vec<f32> {
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let v = (x as f32 / w.max(1) as f32) * 0.7 + (y as f32 / h.max(1) as f32) * 0.3;
            put(&mut rgba, w, x, y, v.clamp(0.0, 1.0));
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
            wrap_padding: false,
            ..DitherParamsV2::default()
        }),
    ));
    layer
}

fn to_rgba8(f32s: &[f32]) -> Vec<u8> {
    f32s.iter()
        .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
        .collect()
}

#[test]
fn ed_tile_always_mosaics_and_changes_output() {
    let w = 48u32;
    let h = 40u32;
    let rgba = gradient_src(w, h);
    let cache = TileCache::new(64 * 1024 * 1024);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();

    let layer = ed_layer(w, h);
    assert!(!should_skip_mosaic(&layer, w, h));
    assert!(layer_needs_pipeline_tile_full_document(&layer, true, w, h));

    let mut doc_off = Document::new(DocumentId::new(1), w, h);
    doc_off.gamedev.tile.enabled = false;
    doc_off.root.push(LayerNode::Leaf(layer.clone()));

    let mut doc_on = Document::new(DocumentId::new(1), w, h);
    doc_on.gamedev.tile.enabled = true;
    doc_on.root.push(LayerNode::Leaf(layer.clone()));

    assert!(layer_needs_full_document(&layer, &doc_on, true));
    // Without wrap_padding / Tile, plain ED stays on the tiled path.
    assert!(!layer_needs_full_document(&layer, &doc_off, true));

    let cancel = AtomicBool::new(false);
    // Tile off: force FD by temporarily enabling wrap... actually tiled path
    // for ED without FD triggers may not use compute_full_document_rgba.
    // Compare Tile-on FD result against a non-mosaic FD pass of the same stack
    // by running begin without tile.
    let out_on = compute_full_document_rgba(&cache, &layer, &doc_on, &cancel).unwrap();
    assert_eq!(out_on.width, w);
    assert_eq!(out_on.height, h);

    // Synthetic non-tiled FD: same layer/doc size, tile disabled — but
    // compute_full_document still works when we call it directly.
    let out_off = compute_full_document_rgba(&cache, &layer, &doc_off, &cancel).unwrap();
    assert_ne!(
        out_on.rgba_f32.as_ref(),
        out_off.rgba_f32.as_ref(),
        "ED Pipeline Tile mosaic must change output vs non-tiled dither"
    );

    // Relative seam metric on quantized result — soft: Warn if not ok, never Exact.
    let rgba8 = to_rgba8(&out_on.rgba_f32);
    let report = validate_seams_rgba8(&rgba8, w, h, SeamMode::Relative).unwrap();
    assert_eq!(report.mode, SeamMode::Relative);
    let chip = seamless_chip_from_tile(true, w, h, Some(&report));
    if report.ok {
        assert_eq!(chip.state, engine_project::ChipState::Ok);
    } else {
        assert_eq!(
            chip_state_for_seam_report(&report),
            engine_project::ChipState::Warn,
            "Relative seam fail must be Warn, not Error"
        );
        assert_eq!(chip.state, engine_project::ChipState::Warn);
    }
}

#[test]
fn ed_tile_preview_export_bitwise() {
    let w = 32u32;
    let h = 24u32;
    let rgba = gradient_src(w, h);
    let cache = TileCache::new(64 * 1024 * 1024);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let layer = ed_layer(w, h);
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.gamedev.tile.enabled = true;
    doc.root.push(LayerNode::Leaf(layer.clone()));
    let cancel = AtomicBool::new(false);
    let a = compute_full_document_rgba(&cache, &layer, &doc, &cancel).unwrap();
    let b = compute_full_document_rgba(&cache, &layer, &doc, &cancel).unwrap();
    assert_eq!(a.rgba_f32.as_ref(), b.rgba_f32.as_ref());
}

#[test]
fn interactive_limit_refuses_oversize_without_alloc() {
    let w = 3000u32;
    let h = 3000u32;
    assert!(!pipeline_tile_fits_limits(w, h));
    let mosaic = (w as u64) * 3 * (h as u64) * 3;
    assert!(mosaic as usize > PIPELINE_TILE_MAX_MOSAIC_PIXELS);

    let layer = ed_layer(w, h);
    let err = pipeline_tile_memory_gate_error(&layer, true, w, h).unwrap();
    assert!(matches!(err, PipelineTileError::InteractiveLimit { .. }));

    // begin must refuse before expanding (empty buffer — would OOM if allocated).
    let mut rgba = vec![0.0f32; 4]; // deliberately tiny; gate fires first
    let mut bw = w;
    let mut bh = h;
    let refused = begin_pipeline_tile_pass(&mut rgba, &mut bw, &mut bh, &layer, true);
    assert!(matches!(
        refused,
        Err(PipelineTileError::InteractiveLimit { .. })
    ));
    assert_eq!(rgba.len(), 4, "must not allocate mosaic on refuse");

    let chip = seamless_chip_from_tile(true, w, h, None);
    assert_eq!(chip.state, engine_project::ChipState::Error);
    assert_eq!(chip.label, "tile · too large");
}
