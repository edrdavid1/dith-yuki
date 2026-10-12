//! Graph-level e2e for GPU dither families: pixel_size changes output,
//! GPU==CPU through the full-document path, cache invalidates on ps change.

use std::sync::atomic::AtomicBool;

use engine_project::algorithms::gpu_dither::last_gpu_dither_status;
use engine_project::document::Document;
use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::full_document::{
    compute_full_document_rgba, ensure_full_document_ex, FullDocumentCache,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::decompose::decompose_image_to_tiles_at_generation;
use engine_tiles::TileCache;
use sha2::{Digest, Sha256};

fn gray_ramp_f32(w: u32, h: u32) -> Vec<f32> {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    let denom = w.max(2) - 1;
    for _y in 0..h {
        for x in 0..w {
            let g = (x * 255) / denom;
            let lin = engine_color::palette::srgb_to_linear(g as u8);
            v.extend_from_slice(&[lin, lin, lin, 1.0]);
        }
    }
    v
}

fn load_photo_f32() -> (u32, u32, Vec<f32>) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/gpu_parity/photos/photo_grad.png"
    );
    let img = image::open(path).expect("photo_grad").to_rgba8();
    let (w, h) = img.dimensions();
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for p in img.pixels() {
        out.push(p[0] as f32 / 255.0);
        out.push(p[1] as f32 / 255.0);
        out.push(p[2] as f32 / 255.0);
        out.push(p[3] as f32 / 255.0);
    }
    (w, h, out)
}

fn sha256_f32(rgba: &[f32]) -> String {
    let bytes: Vec<u8> = rgba
        .chunks_exact(4)
        .flat_map(|c| {
            [
                engine_color::palette::linear_to_srgb(c[0]),
                engine_color::palette::linear_to_srgb(c[1]),
                engine_color::palette::linear_to_srgb(c[2]),
                (c[3].clamp(0.0, 1.0) * 255.0).round() as u8,
            ]
        })
        .collect();
    format!("{:x}", Sha256::digest(&bytes))
}

fn make_doc(
    w: u32,
    h: u32,
    rgba: &[f32],
    mode: DitherModeV2,
    pixel_size: u8,
) -> (Document, Layer, TileCache) {
    let doc_id = 1u32;
    let layer_id = 1u32;
    let gen = 1u64;
    let cache = TileCache::new(64_000_000);
    decompose_image_to_tiles_at_generation(rgba, w, h, doc_id, layer_id, &cache, gen)
        .expect("decompose");
    let mut doc = Document::new(DocumentId::new(doc_id), w, h);
    doc.generations.set_document_gen(gen);
    let mut layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, w, h);
    let params = DitherParamsV2 {
        mode: mode.clone(),
        pixel_size,
        ..DitherParamsV2::default()
    };
    let algo = match mode {
        DitherModeV2::BlockedFs => "blocked_fs",
        DitherModeV2::BlueNoise => "blue_noise",
        _ => "blocked_fs",
    };
    let mut filter = FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(params));
    filter.algorithm_id = Some(algo.into());
    layer.filters = vec![filter];
    doc.root.push(LayerNode::Leaf(layer.clone()));
    (doc, layer, cache)
}

fn render_path(path: &str, mode: DitherModeV2, w: u32, h: u32, rgba: &[f32], ps: u8) -> Vec<f32> {
    std::env::set_var("DITHER_GPU_PATH", path);
    let (doc, layer, cache) = make_doc(w, h, rgba, mode, ps);
    let cancel = AtomicBool::new(false);
    let result = compute_full_document_rgba(&cache, &layer, &doc, &cancel).expect("compute");
    assert_eq!(result.rgba_f32.len(), (w * h * 4) as usize);
    result.rgba_f32.to_vec()
}

#[test]
fn pixel_size_changes_output_through_graph() {
    if engine_gpu::cached_blocked_fs_device().is_none() {
        eprintln!("skip: no GPU");
        return;
    }
    let w = 128u32;
    let h = 96u32;
    let rgba = gray_ramp_f32(w, h);
    for mode in [DitherModeV2::BlockedFs, DitherModeV2::BlueNoise] {
        let h1 = sha256_f32(&render_path("cpu", mode.clone(), w, h, &rgba, 1));
        let h2 = sha256_f32(&render_path("cpu", mode.clone(), w, h, &rgba, 2));
        let h4 = sha256_f32(&render_path("cpu", mode.clone(), w, h, &rgba, 4));
        assert_ne!(h1, h2, "{mode:?} ps=1 vs 2 identical");
        assert_ne!(h2, h4, "{mode:?} ps=2 vs 4 identical");
        assert_ne!(h1, h4, "{mode:?} ps=1 vs 4 identical");
    }
}

#[test]
fn graph_gpu_matches_cpu_for_pixel_sizes() {
    if engine_gpu::cached_blocked_fs_device().is_none() {
        eprintln!("skip: no GPU");
        return;
    }
    let (pw, ph, photo_full) = load_photo_f32();
    let crop_w = pw.min(160);
    let crop_h = ph.min(120);
    let mut photo = Vec::with_capacity((crop_w * crop_h * 4) as usize);
    for y in 0..crop_h {
        for x in 0..crop_w {
            let i = ((y * pw + x) * 4) as usize;
            photo.extend_from_slice(&photo_full[i..i + 4]);
        }
    }
    let w = 128u32;
    let h = 96u32;
    let grad = gray_ramp_f32(w, h);
    let cases: &[(u32, u32, &[f32])] = &[(w, h, &grad), (crop_w, crop_h, &photo)];
    for mode in [DitherModeV2::BlockedFs, DitherModeV2::BlueNoise] {
        for &(rw, rh, buf) in cases {
            for ps in [1u8, 2, 4] {
                let cpu = sha256_f32(&render_path("cpu", mode.clone(), rw, rh, buf, ps));
                let gpu = sha256_f32(&render_path("gpu", mode.clone(), rw, rh, buf, ps));
                assert_eq!(
                    cpu, gpu,
                    "{mode:?} ps={ps} {rw}x{rh} GPU≠CPU through graph"
                );
                let st = last_gpu_dither_status().expect("status");
                assert_eq!(st.pixel_size, ps as u32);
            }
        }
    }
}

#[test]
fn pixel_size_change_invalidates_full_document_cache() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    let w = 64u32;
    let h = 64u32;
    let rgba = gray_ramp_f32(w, h);
    let (mut doc, mut layer, cache) = make_doc(w, h, &rgba, DitherModeV2::BlockedFs, 1);
    let fd = FullDocumentCache::new();
    let first = ensure_full_document_ex(&fd, &cache, &layer, &doc, 1, false).expect("first");
    let h1 = sha256_f32(&first.rgba_f32);
    // Mutate pixel_size on the layer filter.
    if let FilterParams::DitherV2(ref mut p) = layer.filters[0].params {
        p.pixel_size = 4;
    }
    doc.root = vec![LayerNode::Leaf(layer.clone())];
    let second = ensure_full_document_ex(&fd, &cache, &layer, &doc, 1, false).expect("second");
    let h2 = sha256_f32(&second.rgba_f32);
    assert_ne!(h1, h2, "cache returned stale result after pixel_size change");
}

#[test]
fn identical_requests_hit_full_document_cache() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    let w = 48u32;
    let h = 48u32;
    let rgba = gray_ramp_f32(w, h);
    let (doc, layer, cache) = make_doc(w, h, &rgba, DitherModeV2::BlueNoise, 2);
    let fd = FullDocumentCache::new();
    let a = ensure_full_document_ex(&fd, &cache, &layer, &doc, 1, false).expect("a");
    let b = ensure_full_document_ex(&fd, &cache, &layer, &doc, 1, false).expect("b");
    assert!(
        std::sync::Arc::ptr_eq(&a.rgba_f32, &b.rgba_f32),
        "second identical request must return the cached Arc"
    );
}

#[test]
fn rapid_param_changes_last_request_wins() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    let w = 64u32;
    let h = 64u32;
    let rgba = gray_ramp_f32(w, h);
    let (mut doc, mut layer, cache) = make_doc(w, h, &rgba, DitherModeV2::BlockedFs, 1);
    let fd = std::sync::Arc::new(FullDocumentCache::new());
    let cache = std::sync::Arc::new(cache);

    // Start a slow-ish first job (ps=1), then supersede with ps=8 before it settles.
    let fd1 = std::sync::Arc::clone(&fd);
    let cache1 = std::sync::Arc::clone(&cache);
    let layer1 = layer.clone();
    let doc1 = doc.clone();
    let t1 = std::thread::spawn(move || {
        ensure_full_document_ex(&fd1, &cache1, &layer1, &doc1, 1, false)
    });
    std::thread::sleep(std::time::Duration::from_millis(5));
    if let FilterParams::DitherV2(ref mut p) = layer.filters[0].params {
        p.pixel_size = 8;
    }
    doc.root = vec![LayerNode::Leaf(layer.clone())];
    let last = ensure_full_document_ex(&fd, &cache, &layer, &doc, 2, false).expect("last");
    let first = t1.join().expect("join");
    // First job must be cancelled/superseded OR both succeed with last cached.
    match first {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("superseded") || msg.contains("cancel"),
                "expected supersede/cancel, got {msg}"
            );
        }
        Ok(_) => {}
    }
    let again = ensure_full_document_ex(&fd, &cache, &layer, &doc, 2, false).expect("cached last");
    assert!(
        std::sync::Arc::ptr_eq(&last.rgba_f32, &again.rgba_f32),
        "last request must be the cached result"
    );
}
