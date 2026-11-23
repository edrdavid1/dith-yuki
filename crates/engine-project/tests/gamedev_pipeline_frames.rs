//! F3 acceptance: identical frames → bitwise identical; unique colors ≤ shared palette.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use engine_color::palette::{srgb_to_linear, LinearColor, Palette};
use engine_project::filter::{
    DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
    PaletteDitherMode,
};
use engine_project::filters::full_document::compute_full_document_rgba;
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind, PaletteId};
use engine_project::Document;
use engine_project::{
    apply_frame_stable_context, collect_frame_paths, filters_are_frame_stable,
    identical_frames_bitwise_equal, process_frame_set_to_folder, should_lock_pattern_phase,
    switch_filters_to_frame_stable, FrameSetReport, GamedevContext, GamedevJob,
    HeadlessOutputFormat, MaskKind, MaskPolicy, FRAME_PIPELINE_METRICS,
};
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;
use image::RgbaImage;
use tempfile::tempdir;

fn bayer_filter() -> FilterInstance {
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

fn bayer_palette_filter(palette_id: u32) -> FilterInstance {
    let mut f = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::Bayer8x8,
            levels: 4,
            color_mode: DitherColorMode::Rgb,
            palette_id: Some(PaletteId::new(palette_id)),
            palette_dither_mode: PaletteDitherMode::Strict,
            ..DitherParamsV2::default()
        }),
    );
    f.enabled = true;
    f.algorithm_id = Some("bayer_8x8".into());
    f
}

fn solid_rgba_f32(w: u32, h: u32, v: f32) -> Vec<f32> {
    let mut rgba = vec![v; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        rgba[i * 4 + 3] = 1.0;
    }
    rgba
}

fn write_solid_png(path: &std::path::Path, w: u32, h: u32, gray: u8) {
    let img = RgbaImage::from_fn(w, h, |_, _| image::Rgba([gray, gray, gray, 255]));
    img.save(path).expect("write png");
}

fn linear_rgb(r: u8, g: u8, b: u8) -> LinearColor {
    LinearColor {
        r: srgb_to_linear(r),
        g: srgb_to_linear(g),
        b: srgb_to_linear(b),
    }
}

#[test]
fn identical_frames_bitwise_equal_acceptance() {
    let w = 24u32;
    let h = 24u32;
    let a = solid_rgba_f32(w, h, 0.42);
    let b = a.clone();
    assert!(identical_frames_bitwise_equal(&a, &b, w, h, &[bayer_filter()], &[]).unwrap());
}

#[test]
fn switch_and_lock_context_for_animation() {
    let mut f = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            ..DitherParamsV2::default()
        }),
    );
    f.enabled = true;
    switch_filters_to_frame_stable(std::slice::from_mut(&mut f));
    assert!(filters_are_frame_stable(&[f]));

    let mut ctx = GamedevContext::default();
    assert!(!should_lock_pattern_phase(&ctx));
    apply_frame_stable_context(&mut ctx);
    assert_eq!(ctx.job, GamedevJob::Animation);
    assert!(matches!(
        ctx.threshold_mask,
        MaskPolicy::Fixed {
            kind: MaskKind::Ordered,
            phase: 0
        }
    ));
    assert!(should_lock_pattern_phase(&ctx));
}

#[test]
fn frame_set_folder_identical_skip() {
    FRAME_PIPELINE_METRICS
        .frames_skipped_identical
        .store(0, Ordering::Relaxed);

    let dir = tempdir().unwrap();
    write_solid_png(&dir.path().join("a.png"), 16, 16, 128);
    write_solid_png(&dir.path().join("b.png"), 16, 16, 128);

    let paths = collect_frame_paths(dir.path()).unwrap();
    assert_eq!(paths.len(), 2);

    let out = tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let (_results, report): (Vec<_>, FrameSetReport) = process_frame_set_to_folder(
        &paths,
        out.path(),
        "{name}_out.png",
        HeadlessOutputFormat::Png,
        &[bayer_filter()],
        &[],
        true,
        None,
        &cancel,
    )
    .unwrap();

    assert_eq!(report.frame_count, 2);
    assert!(report.frames_skipped_identical >= 1);
    assert!(report.palette_ok());

    let out_a = std::fs::read(out.path().join("a_out.png")).unwrap();
    let out_b = std::fs::read(out.path().join("b_out.png")).unwrap();
    assert_eq!(out_a, out_b);
}

#[test]
fn frame_set_unique_colors_within_shared_palette() {
    let dir = tempdir().unwrap();
    write_solid_png(&dir.path().join("f0.png"), 12, 12, 64);
    write_solid_png(&dir.path().join("f1.png"), 12, 12, 192);

    let paths = collect_frame_paths(dir.path()).unwrap();
    let out = tempdir().unwrap();
    let cancel = AtomicBool::new(false);

    let palettes = vec![Palette {
        id: 1,
        name: "shared".into(),
        colors: vec![
            linear_rgb(0, 0, 0),
            linear_rgb(85, 85, 85),
            linear_rgb(170, 170, 170),
            linear_rgb(255, 255, 255),
        ],
        revision: 1,
    }];

    let (_results, report) = process_frame_set_to_folder(
        &paths,
        out.path(),
        "{name}_out.png",
        HeadlessOutputFormat::Png,
        &[bayer_palette_filter(1)],
        &palettes,
        true,
        None,
        &cancel,
    )
    .unwrap();

    assert!(
        report.palette_ok(),
        "unique={} cap={}",
        report.unique_colors,
        report.shared_palette_len
    );
    assert!(report.unique_colors <= palettes[0].colors.len());
}

#[test]
fn collect_frame_paths_sorted() {
    let dir = tempdir().unwrap();
    write_solid_png(&dir.path().join("z.png"), 4, 4, 10);
    write_solid_png(&dir.path().join("a.png"), 4, 4, 20);
    let paths = collect_frame_paths(dir.path()).unwrap();
    let names: Vec<_> = paths
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.png".to_string(), "z.png".to_string()]);
    let _unused: Vec<PathBuf> = paths;
}

/// Regression: ED→Bayer left stale `requires_full_row` and FD used to call
/// `apply_ed_strip` → "error diffusion strip called with ordered mode".
/// Live-mode guard must succeed even when topology flags are intentionally stale.
#[test]
fn fd_tolerates_stale_ed_strip_flags_after_mode_switch_to_bayer() {
    let w = 16u32;
    let h = 16u32;
    let rgba = vec![0.4f32; (w * h * 4) as usize];
    let cache = TileCache::new(8 * 1024 * 1024);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();

    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    let mut f = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            color_mode: DitherColorMode::Grayscale,
            ..DitherParamsV2::default()
        }),
    );
    f.enabled = true;
    assert!(f.requires_full_row);
    // Simulate a buggy switch that only rewrites mode (pre-fix path).
    if let FilterParams::DitherV2(p) = &mut f.params {
        p.mode = DitherModeV2::Bayer8x8;
        p.serpentine = false;
    }
    f.algorithm_id = Some("bayer_8x8".into());
    assert!(
        f.requires_full_row,
        "stale flag retained on purpose for this regression"
    );
    layer.filters.push(f);

    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(layer.clone()));
    let cancel = AtomicBool::new(false);
    compute_full_document_rgba(&cache, &layer, &doc, &cancel)
        .expect("FD must not route Bayer through ED strip when flags are stale");
}
