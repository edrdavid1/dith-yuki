//! Goal A: ordinary Save strips PNG ancillary metadata (beta readiness).

use engine_project::document::Document;
use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::archive::ZipArchiveReader;
use engine_project::serialize::{
    find_forbidden_png_chunk, inject_png_text_chunk, open_project_from_bytes,
    save_project_to_bytes, ProjectError,
};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;
use std::path::PathBuf;

fn assert_png_clean(label: &str, png: &[u8]) {
    assert!(
        png.starts_with(&[0x89, b'P', b'N', b'G']),
        "{label}: not a PNG"
    );
    if let Some(ty) = find_forbidden_png_chunk(png) {
        panic!(
            "{label}: forbidden ancillary chunk {:?} still present after Save",
            std::str::from_utf8(&ty).unwrap_or("????")
        );
    }
}

fn doc_with_dirty_custom_png(dirty_path: &str) -> (Document, TileCache) {
    let w = 16u32;
    let h = 16u32;
    let rgba = vec![0.4f32; (w * h * 4) as usize];
    let cache = TileCache::new(20_000_000);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let mut doc = Document::new(DocumentId::new(1), w, h);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::CustomPng {
                path: dirty_path.into(),
            },
            levels: 2,
            ..DitherParamsV2::default()
        }),
    ));
    doc.root.push(LayerNode::Leaf(layer));
    (doc, cache)
}

fn write_dirty_threshold_png() -> PathBuf {
    // Grayscale threshold map (CustomPng requires gray) + injected tEXt.
    let mut gray = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut gray, 2, 2);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[0, 64, 128, 255]).unwrap();
    }
    let dirty = inject_png_text_chunk(&gray, "Comment", "GPS:37.77,-122.42 EXIF leak")
        .expect("inject tEXt");
    assert!(
        find_forbidden_png_chunk(&dirty).is_some(),
        "fixture must contain ancillary chunk before Save"
    );
    let path = std::env::temp_dir().join(format!("dither_dirty_thresh_{}.png", std::process::id()));
    std::fs::write(&path, &dirty).unwrap();
    path
}

#[test]
fn ordinary_save_strips_ancillary_from_all_embedded_pngs() {
    let dirty_path = write_dirty_threshold_png();
    let path_str = dirty_path.to_string_lossy().into_owned();
    let (doc, cache) = doc_with_dirty_custom_png(&path_str);

    let saved = save_project_to_bytes(&doc, &cache, "0.3.0", |p| {
        std::fs::read(p).map_err(|e| ProjectError::Io(e.to_string()))
    })
    .expect("ordinary Save");

    let mut reader = ZipArchiveReader::open(&saved.zip_bytes).unwrap();
    for name in reader.entry_names() {
        if !name.ends_with(".png") {
            continue;
        }
        let png = reader.read_entry(&name).unwrap();
        assert_png_clean(&name, &png);
    }

    let _ = std::fs::remove_file(&dirty_path);
}

#[test]
fn ordinary_save_keeps_real_preview_not_neutral() {
    // Doc larger than thumbnail max so a real thumb is a downscale, not 1×1.
    let w = 64u32;
    let h = 48u32;
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        rgba[i * 4] = 0.9;
        rgba[i * 4 + 1] = 0.1;
        rgba[i * 4 + 2] = 0.2;
        rgba[i * 4 + 3] = 1.0;
    }
    let cache = TileCache::new(20_000_000);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(Layer::new(
        LayerId::new(1),
        LayerKind::Raster,
        w,
        h,
    )));

    let saved = save_project_to_bytes(&doc, &cache, "0.3.0", |_| {
        Err(ProjectError::Io("none".into()))
    })
    .unwrap();
    let mut reader = ZipArchiveReader::open(&saved.zip_bytes).unwrap();

    let composite = reader.read_entry("composite.png").unwrap();
    assert_png_clean("composite.png", &composite);
    let (cw, ch) = png_size(&composite);
    assert_eq!(
        (cw, ch),
        (w, h),
        "composite must stay full document resolution"
    );

    let thumb = reader.read_entry("thumbnail.png").unwrap();
    assert_png_clean("thumbnail.png", &thumb);
    assert_ne!(
        thumb,
        engine_project::serialize::neutral_thumbnail_png(),
        "normal Save must not write the Share-Copy neutral placeholder"
    );
    let (tw, th) = png_size(&thumb);
    assert!(tw > 1 && th > 1, "thumbnail must not collapse to 1×1");
    assert!(
        tw.max(th) <= engine_project::serialize::THUMBNAIL_MAX_SIDE,
        "thumbnail long side ≤ 1024"
    );
}

fn png_size(png: &[u8]) -> (u32, u32) {
    // IHDR: signature(8) + len(4) + type(4) + width(4) + height(4)
    assert!(png.len() >= 24);
    let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
    (w, h)
}

#[test]
fn ordinary_save_open_round_trip_preserves_pixels() {
    let w = 8u32;
    let h = 8u32;
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        rgba[i * 4] = (i as f32) / (w * h) as f32;
        rgba[i * 4 + 1] = 0.5;
        rgba[i * 4 + 2] = 1.0 - rgba[i * 4];
        rgba[i * 4 + 3] = 1.0;
    }
    let cache = TileCache::new(10_000_000);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(Layer::new(
        LayerId::new(1),
        LayerKind::Raster,
        w,
        h,
    )));

    let saved = save_project_to_bytes(&doc, &cache, "0.3.0", |_| {
        Err(ProjectError::Io("none".into()))
    })
    .unwrap();

    let staging = TileCache::new(10_000_000);
    let opened = open_project_from_bytes(&saved.zip_bytes, &staging, DocumentId::new(1)).unwrap();
    assert_eq!(opened.document.width, w);
    assert_eq!(opened.document.height, h);
    assert!(staging.entry_count() > 0);

    // Spot-check first Raw tile pixel vs source (f32↔u8↔f32 within ~1 LSB).
    let layer_id = opened
        .layer_remap
        .values()
        .next()
        .copied()
        .unwrap_or(LayerId::new(1));
    let key = engine_tiles::TileKey {
        doc: 1,
        layer: layer_id.0,
        coord: engine_tiles::TileCoord {
            level: 0,
            x: 0,
            y: 0,
        },
        stage: engine_tiles::CacheStage::Raw,
    };
    let tile = staging.get_entry(key).expect("raw tile after open");
    let first = tile.data[0];
    assert!(
        (first - rgba[0]).abs() < 2.0 / 255.0,
        "pixel drift too large: {first} vs {}",
        rgba[0]
    );
}

#[test]
fn reencode_png_clean_drops_text_chunk() {
    let clean =
        engine_project::serialize::encode_thumbnail_png_deterministic(&[1, 2, 3, 255], 1, 1)
            .unwrap();
    let dirty = inject_png_text_chunk(&clean, "Author", "spy").unwrap();
    assert!(find_forbidden_png_chunk(&dirty).is_some());
    let scrubbed = engine_project::serialize::reencode_png_clean(&dirty).unwrap();
    assert!(find_forbidden_png_chunk(&scrubbed).is_none());
}

#[test]
fn strip_png_ancillary_preserves_grayscale() {
    let mut gray = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut gray, 2, 2);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&[10, 20, 30, 40]).unwrap();
    }
    let dirty = inject_png_text_chunk(&gray, "Comment", "meta").unwrap();
    let clean = engine_project::serialize::strip_png_ancillary_chunks(&dirty).unwrap();
    assert!(find_forbidden_png_chunk(&clean).is_none());
    // IHDR color type byte at offset 8+8+9 = 25 (sig + len+type + width/height/bitdepth)
    // Color type is the 10th byte of IHDR data (after w,h,bit depth).
    let color_type = clean[8 + 8 + 9];
    assert_eq!(
        color_type, 0,
        "must stay grayscale (color type 0), got {color_type}"
    );
}
