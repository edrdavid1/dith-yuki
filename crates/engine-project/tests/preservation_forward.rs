//! Forward-compat preservation (SPEC §9.1 / Stage 3).

use engine_project::document::Document;
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::archive::{create_zip, ZipArchiveReader};
use engine_project::serialize::{
    open_project_from_bytes, save_project_to_bytes, ProjectError,
};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;

fn minimal_saved() -> Vec<u8> {
    let w = 16u32;
    let h = 16u32;
    let rgba = vec![0.5f32; (w * h * 4) as usize];
    let cache = TileCache::new(20_000_000);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(Layer::new(
        LayerId::new(1),
        LayerKind::Raster,
        w,
        h,
    )));
    save_project_to_bytes(&doc, &cache, "0.3.0", |_| {
        Err(ProjectError::Io("none".into()))
    })
    .unwrap()
    .zip_bytes
}

#[test]
fn preserves_unknown_document_field_and_unknown_layer_node() {
    let saved = minimal_saved();
    let mut reader = ZipArchiveReader::open(&saved).unwrap();
    let mut doc_json: serde_json::Value =
        serde_json::from_slice(&reader.read_entry("document.json").unwrap()).unwrap();
    doc_json["vendor_blob"] = serde_json::json!({ "keep": true });
    doc_json["root"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "node": "voxel_layer",
            "id": 42,
            "mesh": "teapot"
        }));
    let mut manifest_val: serde_json::Value =
        serde_json::from_slice(&reader.read_entry("manifest.json").unwrap()).unwrap();
    if let Some(obj) = manifest_val.as_object_mut() {
        obj.remove("files");
    }
    let manifest = serde_json::to_vec_pretty(&manifest_val).unwrap();
    let layer = reader.read_entry("layers/1.png").unwrap();
    let composite = reader.read_entry("composite.png").unwrap();
    let thumbnail = reader.read_entry("thumbnail.png").unwrap();
    let doc_bytes = serde_json::to_vec_pretty(&doc_json).unwrap();
    let zip = create_zip(&[
        ("manifest.json", manifest.as_slice()),
        ("document.json", doc_bytes.as_slice()),
        ("layers/1.png", layer.as_slice()),
        ("composite.png", composite.as_slice()),
        ("thumbnail.png", thumbnail.as_slice()),
        ("ext/com.example.meta/note.txt", b"hello-ext"),
    ])
    .unwrap();

    let staging = TileCache::new(20_000_000);
    let opened = open_project_from_bytes(&zip, &staging, DocumentId::new(1)).unwrap();
    assert_eq!(opened.document.extra["vendor_blob"]["keep"], true);
    assert_eq!(opened.document.ext_blobs.len(), 1);
    assert_eq!(opened.document.ext_blobs[0].0, "ext/com.example.meta/note.txt");
    assert!(opened.document.root.iter().any(|n| match n {
        LayerNode::Leaf(l) => l.extra.contains_key("__forward_compat_node"),
        _ => false,
    }));

    // Save again and ensure opaque data survives.
    let again = save_project_to_bytes(&opened.document, &staging, "0.3.0", |_| {
        Err(ProjectError::Io("none".into()))
    })
    .unwrap();
    let mut r2 = ZipArchiveReader::open(&again.zip_bytes).unwrap();
    let doc2: serde_json::Value =
        serde_json::from_slice(&r2.read_entry("document.json").unwrap()).unwrap();
    assert_eq!(doc2["vendor_blob"]["keep"], true);
    let root = doc2["root"].as_array().unwrap();
    assert!(root.iter().any(|n| n.get("node") == Some(&serde_json::json!("voxel_layer"))));
    let ext = r2.read_entry("ext/com.example.meta/note.txt").unwrap();
    assert_eq!(ext, b"hello-ext");
    assert_eq!(
        r2.read_entry("mimetype").unwrap(),
        b"application/vnd.dither.project+zip"
    );
    assert!(r2.contains("composite.png"));
    assert!(r2.contains("thumbnail.png"));
}

#[test]
fn golden_v1_still_opens() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dyproj/v1/minimal.dyproj");
    let bytes = std::fs::read(path).unwrap();
    let staging = TileCache::new(50_000_000);
    open_project_from_bytes(&bytes, &staging, DocumentId::new(1)).unwrap();
}
