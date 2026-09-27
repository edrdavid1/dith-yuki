//! Version-drill matrix — new engine (build WITH `--features version-drill`).
//!
//! Scenarios 1, 4–6 from SPEC_dither_beta_readiness §2.5, plus fixture generator.

#![cfg(feature = "version-drill")]

use engine_project::document::Document;
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::archive::ZipArchiveReader;
use engine_project::serialize::features::{
    CANARY_DRILL_EXTRA_KEY, CANARY_NOTE_KEY, SUPPORTED_FORMAT,
};
use engine_project::serialize::{
    open_project_from_bytes, save_project_to_bytes, write_project_to_bytes, FormatVersion,
    ProjectError, ProjectWriteOptions,
};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn drill_dir() -> PathBuf {
    fixtures_root().join("dyproj/drill")
}

fn tiny_doc() -> (Document, TileCache) {
    let w = 16u32;
    let h = 16u32;
    let rgba = vec![0.55f32; (w * h * 4) as usize];
    let cache = TileCache::new(20_000_000);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(Layer::new(
        LayerId::new(1),
        LayerKind::Raster,
        w,
        h,
    )));
    (doc, cache)
}

fn save_fixed(doc: &Document, cache: &TileCache) -> Vec<u8> {
    let mut opts = ProjectWriteOptions::normal();
    opts.timestamp = Some("drill-ts".into());
    write_project_to_bytes(
        doc,
        cache,
        "0.3.0-drill",
        |_| Err(ProjectError::Io("none".into())),
        &opts,
    )
    .unwrap()
    .zip_bytes
}

fn manifest_format(zip: &[u8]) -> (FormatVersion, FormatVersion, Vec<String>, Vec<String>) {
    let mut reader = ZipArchiveReader::open(zip).unwrap();
    let m: serde_json::Value =
        serde_json::from_slice(&reader.read_entry("manifest.json").unwrap()).unwrap();
    let format = FormatVersion {
        major: m["format"]["major"].as_u64().unwrap() as u32,
        minor: m["format"]["minor"].as_u64().unwrap() as u32,
    };
    let min_reader = FormatVersion {
        major: m["min_reader"]["major"].as_u64().unwrap() as u32,
        minor: m["min_reader"]["minor"].as_u64().unwrap() as u32,
    };
    let req: Vec<String> = serde_json::from_value(m["features_required"].clone()).unwrap();
    let opt: Vec<String> = serde_json::from_value(m["features_optional"].clone()).unwrap();
    (format, min_reader, req, opt)
}

/// Generate / refresh drill goldens. Run with:
/// `GENERATE_DRILL_FIXTURES=1 cargo test -p engine-project --features version-drill generate_drill_fixtures -- --ignored`
#[test]
#[ignore]
fn generate_drill_fixtures() {
    if std::env::var("GENERATE_DRILL_FIXTURES").ok().as_deref() != Some("1") {
        eprintln!("set GENERATE_DRILL_FIXTURES=1 to write fixtures");
        return;
    }
    fs::create_dir_all(drill_dir()).unwrap();

    // neither-used
    {
        let (doc, cache) = tiny_doc();
        let bytes = save_fixed(&doc, &cache);
        fs::write(drill_dir().join("neither-used.dyproj"), &bytes).unwrap();
    }
    // optional-used
    {
        let (mut doc, cache) = tiny_doc();
        doc.extra.insert(CANARY_NOTE_KEY.into(), json!("drill"));
        let bytes = save_fixed(&doc, &cache);
        fs::write(drill_dir().join("optional-used.dyproj"), &bytes).unwrap();
    }
    // required-used
    {
        let (mut doc, cache) = tiny_doc();
        let mut canary = Layer::new(LayerId::new(2), LayerKind::Adjustment, 16, 16);
        canary.name = "Canary Drill".into();
        canary.visible = false;
        canary
            .extra
            .insert(CANARY_DRILL_EXTRA_KEY.into(), json!(true));
        doc.root.push(LayerNode::Leaf(canary));
        let bytes = save_fixed(&doc, &cache);
        fs::write(drill_dir().join("required-used.dyproj"), &bytes).unwrap();
    }
    eprintln!("wrote fixtures under {}", drill_dir().display());
}

#[test]
fn matrix_1_v1_minimal_opens_with_drill_engine() {
    assert_eq!(SUPPORTED_FORMAT, FormatVersion::new(1, 1));
    let bytes = fs::read(fixtures_root().join("dyproj/v1/minimal.dyproj")).unwrap();
    let staging = TileCache::new(32 * 1024 * 1024);
    open_project_from_bytes(&bytes, &staging, DocumentId::new(1)).unwrap();
}

#[test]
fn matrix_4_neither_used_resave_stays_v1_0() {
    let (doc, cache) = tiny_doc();
    let bytes = save_fixed(&doc, &cache);
    let (format, min_reader, req, opt) = manifest_format(&bytes);
    assert_eq!(format, FormatVersion::V1_0);
    assert_eq!(min_reader, FormatVersion::V1_0);
    assert!(req.is_empty());
    assert!(opt.is_empty());

    // Also: open committed neither-used if present and resave.
    let path = drill_dir().join("neither-used.dyproj");
    if path.exists() {
        let staging = TileCache::new(32 * 1024 * 1024);
        let opened =
            open_project_from_bytes(&fs::read(&path).unwrap(), &staging, DocumentId::new(1))
                .unwrap();
        let again = save_project_to_bytes(&opened.document, &staging, "0.3.0-drill", |_| {
            Err(ProjectError::Io("none".into()))
        })
        .unwrap();
        let (f2, m2, _, _) = manifest_format(&again.zip_bytes);
        assert_eq!(f2, FormatVersion::V1_0, "§2.6 minimum version on resave");
        assert_eq!(m2, FormatVersion::V1_0);
    }
}

#[test]
fn matrix_5_optional_used_preserves_note_and_format_1_1() {
    let (mut doc, cache) = tiny_doc();
    doc.extra.insert(CANARY_NOTE_KEY.into(), json!("drill"));
    let bytes = save_fixed(&doc, &cache);
    let (format, min_reader, req, opt) = manifest_format(&bytes);
    assert_eq!(format, FormatVersion::new(1, 1));
    assert_eq!(min_reader, FormatVersion::V1_0);
    assert!(req.is_empty());
    assert_eq!(opt, vec!["canary-optional".to_string()]);

    let staging = TileCache::new(32 * 1024 * 1024);
    let opened = open_project_from_bytes(&bytes, &staging, DocumentId::new(1)).unwrap();
    assert_eq!(
        opened
            .document
            .extra
            .get(CANARY_NOTE_KEY)
            .and_then(|v| v.as_str()),
        Some("drill")
    );

    let again = save_fixed(&opened.document, &staging);
    let (f2, m2, _, opt2) = manifest_format(&again);
    assert_eq!(f2, FormatVersion::new(1, 1));
    assert_eq!(m2, FormatVersion::V1_0);
    assert_eq!(opt2, vec!["canary-optional".to_string()]);
}

#[test]
fn matrix_6_required_used_recognized_as_real_type() {
    let (mut doc, cache) = tiny_doc();
    let mut canary = Layer::new(LayerId::new(2), LayerKind::Adjustment, 16, 16);
    canary
        .extra
        .insert(CANARY_DRILL_EXTRA_KEY.into(), json!(true));
    doc.root.push(LayerNode::Leaf(canary));
    let bytes = save_fixed(&doc, &cache);
    let (format, min_reader, req, _) = manifest_format(&bytes);
    assert_eq!(format, FormatVersion::new(1, 1));
    assert_eq!(min_reader, FormatVersion::new(1, 1));
    assert_eq!(req, vec!["canary-required".to_string()]);

    // document.json must serialize canary-drill node, not Unknown bag.
    let mut reader = ZipArchiveReader::open(&bytes).unwrap();
    let doc_json: serde_json::Value =
        serde_json::from_slice(&reader.read_entry("document.json").unwrap()).unwrap();
    let nodes = doc_json["root"].as_array().unwrap();
    assert!(
        nodes.iter().any(|n| n["node"] == "canary-drill"),
        "expected canary-drill node in document.json: {doc_json}"
    );

    let staging = TileCache::new(32 * 1024 * 1024);
    let opened = open_project_from_bytes(&bytes, &staging, DocumentId::new(1)).unwrap();
    let has_marker = opened.document.root.iter().any(|n| match n {
        LayerNode::Leaf(l) => {
            l.extra
                .get(CANARY_DRILL_EXTRA_KEY)
                .and_then(|v| v.as_bool())
                == Some(true)
        }
        _ => false,
    });
    assert!(
        has_marker,
        "canary must load as recognized type, not Unknown"
    );
}
