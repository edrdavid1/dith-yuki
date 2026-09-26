//! Golden v1 fixtures for `.dyproj` / `.dyuki` (SPEC Stage 0).
//!
//! Fixtures under `tests/fixtures/{dyproj,dyuki}/v1/` are immutable after
//! commit. Regenerate **new** names only with `GENERATE_GOLDEN_V1=1`; never
//! overwrite existing files when their SHA-256 is locked in `SHA256SUMS`.

use engine_color::palette::{LinearColor, Palette};
use engine_project::document::Document;
use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::{
    open_project_from_bytes, pack_pattern_to_bytes, save_project_to_bytes,
    unpack_pattern_from_bytes, ArchiveKind, Manifest, PatternExportMeta, ProjectError,
};
use engine_project::types::{DocumentId, LayerId, LayerKind, PaletteId};
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn dyproj_v1_dir() -> PathBuf {
    fixtures_root().join("dyproj/v1")
}

fn dyuki_v1_dir() -> PathBuf {
    fixtures_root().join("dyuki/v1")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_encode(&hasher.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn parse_sha256sums(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let hash = parts.next().expect("hash").to_string();
        let name = parts.next().expect("name").to_string();
        out.push((hash, name));
    }
    out
}

fn build_minimal_dyproj_bytes() -> Result<Vec<u8>, ProjectError> {
    let w = 32u32;
    let h = 24u32;
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        let t = (i as f32) / (w * h) as f32;
        rgba[i * 4] = t;
        rgba[i * 4 + 1] = 0.25;
        rgba[i * 4 + 2] = 1.0 - t;
        rgba[i * 4 + 3] = 1.0;
    }

    let cache = TileCache::new(50_000_000);
    decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache)
        .map_err(|e| ProjectError::Codec(format!("decompose: {e}")))?;

    let mut doc = Document::new(DocumentId::new(1), w, h);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.name = "Background".into();
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::Bayer4x4,
            levels: 4,
            ..DitherParamsV2::default()
        }),
    ));
    doc.root.push(LayerNode::Leaf(layer));

    let saved = save_project_to_bytes(&doc, &cache, "0.2.0", |_| {
        Err(ProjectError::Io("no custom png in golden v1".into()))
    })?;
    Ok(saved.zip_bytes)
}

fn build_minimal_dyuki_bytes() -> Result<Vec<u8>, ProjectError> {
    let palette = Palette {
        id: 1,
        name: "Ink".into(),
        colors: vec![
            LinearColor {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
            LinearColor {
                r: 1.0,
                g: 1.0,
                b: 1.0,
            },
        ],
        revision: 1,
    };
    let filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 2,
            palette_id: Some(PaletteId::new(1)),
            ..DitherParamsV2::default()
        }),
    );
    let meta = PatternExportMeta {
        name: "Golden V1 Pattern".into(),
        description: Some("Immutable Stage-0 fixture".into()),
        author: Some("fixture".into()),
    };
    pack_pattern_to_bytes(&[filter], &[palette], &meta, "0.2.0", |_| {
        Err(ProjectError::Io("no custom png".into()))
    })
}

/// One-shot generator. Writes fixtures only when missing, unless forced.
#[test]
#[ignore = "run manually: GENERATE_GOLDEN_V1=1 cargo test -p engine-project --test golden_v1 generate_golden_v1_fixtures -- --ignored --nocapture"]
fn generate_golden_v1_fixtures() {
    if std::env::var("GENERATE_GOLDEN_V1").ok().as_deref() != Some("1") {
        eprintln!("set GENERATE_GOLDEN_V1=1 to write fixtures");
        return;
    }

    let force = std::env::var("FORCE_GOLDEN_V1").ok().as_deref() == Some("1");
    fs::create_dir_all(dyproj_v1_dir()).unwrap();
    fs::create_dir_all(dyuki_v1_dir()).unwrap();

    let dyproj_path = dyproj_v1_dir().join("minimal.dyproj");
    let dyuki_path = dyuki_v1_dir().join("minimal.dyuki");

    if force || !dyproj_path.exists() {
        let bytes = build_minimal_dyproj_bytes().expect("build dyproj");
        fs::write(&dyproj_path, &bytes).unwrap();
        eprintln!("wrote {} ({} bytes)", dyproj_path.display(), bytes.len());
    } else {
        eprintln!("skip existing {}", dyproj_path.display());
    }

    if force || !dyuki_path.exists() {
        let bytes = build_minimal_dyuki_bytes().expect("build dyuki");
        fs::write(&dyuki_path, &bytes).unwrap();
        eprintln!("wrote {} ({} bytes)", dyuki_path.display(), bytes.len());
    } else {
        eprintln!("skip existing {}", dyuki_path.display());
    }

    // Refresh SHA256SUMS from whatever is on disk under v1/.
    write_sha256sums().unwrap();
}

fn write_sha256sums() -> Result<(), Box<dyn std::error::Error>> {
    let mut lines = vec![
        "# SHA-256 of golden v1 fixtures. Do not regenerate existing entries.".to_string(),
        "# Format: <hash>  <path-relative-to-tests/fixtures>".to_string(),
    ];
    let pairs = [
        (
            "dyproj/v1/minimal.dyproj",
            dyproj_v1_dir().join("minimal.dyproj"),
        ),
        (
            "dyuki/v1/minimal.dyuki",
            dyuki_v1_dir().join("minimal.dyuki"),
        ),
    ];
    for (rel, path) in pairs {
        let bytes = fs::read(&path)?;
        lines.push(format!("{}  {}", sha256_hex(&bytes), rel));
    }
    let out = fixtures_root().join("SHA256SUMS");
    fs::write(&out, lines.join("\n") + "\n")?;
    eprintln!("wrote {}", out.display());
    Ok(())
}

#[test]
fn golden_v1_sha256sums_locked() {
    let sums_path = fixtures_root().join("SHA256SUMS");
    let text = fs::read_to_string(&sums_path).unwrap_or_else(|e| {
        panic!(
            "missing {}: {e} (run generate_golden_v1_fixtures first)",
            sums_path.display()
        )
    });
    let entries = parse_sha256sums(&text);
    assert!(
        !entries.is_empty(),
        "SHA256SUMS must list at least one fixture"
    );
    for (expected, rel) in entries {
        let path = fixtures_root().join(&rel);
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let actual = sha256_hex(&bytes);
        assert_eq!(
            actual, expected,
            "fixture {rel} hash changed — golden files must never be rewritten"
        );
    }
}

#[test]
fn golden_v1_dyproj_opens() {
    let path = dyproj_v1_dir().join("minimal.dyproj");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let staging = TileCache::new(50_000_000);
    let opened =
        open_project_from_bytes(&bytes, &staging, DocumentId::new(1)).expect("v1 dyproj must open");

    assert_eq!(opened.document.width, 32);
    assert_eq!(opened.document.height, 24);
    assert_eq!(opened.document.root.len(), 1);
    match &opened.document.root[0] {
        LayerNode::Leaf(layer) => {
            assert_eq!(layer.name, "Background");
            assert_eq!(layer.kind, LayerKind::Raster);
            assert_eq!(layer.filters.len(), 1);
            assert_eq!(layer.filters[0].kind, FilterKind::Dither);
            match &layer.filters[0].params {
                FilterParams::DitherV2(p) => {
                    assert!(matches!(p.mode, DitherModeV2::Bayer4x4));
                    assert_eq!(p.levels, 4);
                }
                other => panic!("unexpected params: {other:?}"),
            }
        }
        other => panic!("expected leaf, got {other:?}"),
    }
    assert!(staging.entry_count() > 0, "Raw tiles must be staged");
}

#[test]
fn golden_v1_dyuki_opens() {
    let path = dyuki_v1_dir().join("minimal.dyuki");
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let unpacked = unpack_pattern_from_bytes(&bytes, "0.2.0").expect("v1 dyuki must open");
    assert_eq!(unpacked.manifest.format_version, 1);
    assert_eq!(unpacked.manifest.name, "Golden V1 Pattern");
    assert_eq!(
        unpacked.manifest.description.as_deref(),
        Some("Immutable Stage-0 fixture")
    );
    assert_eq!(unpacked.filters.len(), 1);
    assert_eq!(unpacked.filters[0].kind, FilterKind::Dither);
    assert_eq!(unpacked.palettes.len(), 1);
    assert!(unpacked.palettes.contains_key("p0"));
}

#[test]
fn golden_v1_manifest_unknown_fields_would_be_ignored() {
    // Documents Stage-0 finding: released Manifest has no deny_unknown_fields.
    // A v1 file with an extra key still deserializes.
    let json = r#"{
        "format_version": 1,
        "kind": "dyproj",
        "app_version": "0.2.0",
        "created_at": "2026-09-21T00:00:00Z",
        "modified_at": "2026-09-21T00:00:00Z",
        "width": 32,
        "height": 24,
        "format": { "major": 1, "minor": 0 },
        "future_unknown": true
    }"#;
    let m: Manifest = serde_json::from_str(json).expect("extra keys ignored");
    assert_eq!(m.format_version, 1);
    assert_eq!(m.kind, ArchiveKind::Dyproj);
    assert_eq!(m.width, Some(32));
}
