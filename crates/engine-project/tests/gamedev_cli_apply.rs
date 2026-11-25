//! F7: CLI apply output matches headless/GUI path for the same context.

use std::sync::atomic::AtomicBool;

use engine_project::filter::{
    DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::save_project_to_bytes;
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{
    parse_cell_wh, run_cli_apply, run_headless_job, CliApplyOptions, Document, HeadlessJob,
    HeadlessOutputFormat,
};
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;
use image::RgbaImage;
use tempfile::tempdir;

fn bayer() -> FilterInstance {
    let mut f = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::Bayer8x8,
            levels: 4,
            color_mode: DitherColorMode::Grayscale,
            ..DitherParamsV2::default()
        }),
    );
    f.enabled = true;
    f.algorithm_id = Some("bayer_8x8".into());
    f
}

#[test]
fn parse_cell_wh_ok() {
    assert_eq!(parse_cell_wh("32x48").unwrap(), (32, 48));
    assert_eq!(parse_cell_wh("16×16").unwrap(), (16, 16));
}

#[test]
fn cli_apply_matches_headless_bytes() {
    let dir = tempdir().unwrap();
    let input = dir.path().join("in.png");
    let img = RgbaImage::from_fn(24, 24, |x, y| {
        let v = (((x + y) * 7) % 255) as u8;
        image::Rgba([v, v, v, 255])
    });
    img.save(&input).unwrap();

    let mut doc = Document::new(DocumentId::new(1), 24, 24);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 24, 24);
    layer.filters = vec![bayer()];
    doc.root.push(LayerNode::Leaf(layer));
    doc.gamedev.export.png8 = false;

    let cache = TileCache::new(8 * 1024 * 1024);
    let mut rgba = vec![0.5f32; 24 * 24 * 4];
    for i in 0..(24 * 24) {
        rgba[i * 4 + 3] = 1.0;
    }
    decompose_image_to_tiles(&rgba, 24, 24, 1, 1, &cache).unwrap();
    let saved = save_project_to_bytes(&doc, &cache, "0.1.0", |_| unreachable!()).unwrap();
    let project = dir.path().join("preset.dyproj");
    std::fs::write(&project, &saved.zip_bytes).unwrap();

    let headless_out = dir.path().join("headless.png");
    run_headless_job(&HeadlessJob {
        input: input.clone(),
        output: headless_out.clone(),
        format: HeadlessOutputFormat::Png,
        filters: vec![bayer()],
        palettes: vec![],
        lock_pattern_phase: true,
        png8_palette_id: None,
    })
    .unwrap();

    let cli_out = dir.path().join("cli.png");
    let opts = CliApplyOptions {
        project: Some(project),
        lock_pattern_phase: true,
        ..CliApplyOptions::default()
    };
    let results = run_cli_apply(&opts, &input, &cli_out).unwrap();
    assert_eq!(results.len(), 1);
    assert!(results[0].error.is_none());

    let a = std::fs::read(&headless_out).unwrap();
    let b = std::fs::read(&cli_out).unwrap();
    assert_eq!(a, b, "CLI apply must match headless for the same filters");
    let _ = AtomicBool::new(false);
}
