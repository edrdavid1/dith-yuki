//! Foundation acceptance: GamedevContext persist / migrate / invalidation.

use engine_project::gamedev::{
    gamedev_cache_invalidation, GamedevCacheLayer, GamedevContext, GAMEDEV_EXTRA_KEY,
};
use engine_project::serialize::document_dto::DocumentFile;
use engine_project::serialize::features::{
    collect_used_format_features, feature_by_id, FormatVersion, SUPPORTED_FORMAT,
};
use engine_project::serialize::manifest::{check_open_gate, normalize_manifest_value};
use engine_project::serialize::ProjectError;
use engine_project::types::DocumentId;
use engine_project::Document;
use engine_tiles::TileCache;
use serde_json::json;

#[test]
fn supported_format_includes_gamedev_context_1_3() {
    assert_eq!(SUPPORTED_FORMAT, FormatVersion::new(1, 3));
    let f = feature_by_id("gamedev-context").expect("registry row");
    assert_eq!(f.since, FormatVersion::new(1, 3));
}

#[test]
fn old_document_json_without_gamedev_gets_default() {
    let v = json!({
        "id": 1,
        "width": 16,
        "height": 16,
        "color_profile": "SRgb",
        "root": [],
        "palettes": []
    });
    let mut file: DocumentFile = serde_json::from_value(v).unwrap();
    file.finalize_gamedev_migration();
    assert!(file.gamedev.is_default());
}

#[test]
fn bootstrap_extra_gamedev_lifts_to_typed_field() {
    let mut ctx = GamedevContext::default();
    ctx.export.png8 = true;
    let v = json!({
        "id": 1,
        "width": 16,
        "height": 16,
        "color_profile": "SRgb",
        "root": [],
        "palettes": [],
        "gamedev": serde_json::to_value(&ctx).unwrap()
    });
    // When typed field is present in JSON it deserializes directly.
    let file: DocumentFile = serde_json::from_value(v).unwrap();
    assert!(file.gamedev.export.png8);

    // Bootstrap via flatten extra only (simulate pre-typed writer that stuffed extra).
    // After typed field exists, unknown sibling keys go to flatten — but "gamedev" is typed.
    // Exercise lift helper path used when default + extra key:
    let v2 = json!({
        "id": 1,
        "width": 16,
        "height": 16,
        "color_profile": "SRgb",
        "root": [],
        "palettes": [],
        "vendor_only": true
    });
    let mut file2: DocumentFile = serde_json::from_value(v2).unwrap();
    file2
        .extra
        .insert(GAMEDEV_EXTRA_KEY.into(), serde_json::to_value(&ctx).unwrap());
    file2.finalize_gamedev_migration();
    assert!(file2.gamedev.export.png8);
    assert!(!file2.extra.contains_key(GAMEDEV_EXTRA_KEY));
}

#[test]
fn document_file_round_trip_preserves_context_bitwise() {
    let mut doc = Document::new(DocumentId::new(1), 32, 32);
    doc.gamedev.export.png8 = true;
    doc.gamedev.tile.enabled = true;
    doc.gamedev.job = engine_project::GamedevJob::Tile;
    doc.gamedev.cell = Some(engine_project::CellSettings {
        w: 16,
        h: 16,
        padding: 0,
        independent: true,
    });

    let file = DocumentFile::from_document(&doc, |_| None);
    let bytes = serde_json::to_vec(&file).unwrap();
    let mut back: DocumentFile = serde_json::from_slice(&bytes).unwrap();
    back.finalize_gamedev_migration();
    assert_eq!(
        serde_json::to_vec(&doc.gamedev).unwrap(),
        serde_json::to_vec(&back.gamedev).unwrap(),
        "gamedev context must round-trip bitwise (JSON)"
    );
}

#[test]
fn non_default_context_declares_format_feature() {
    let mut doc = Document::new(DocumentId::new(1), 8, 8);
    assert!(!collect_used_format_features(&doc)
        .0
        .iter()
        .any(|s| s == "gamedev-context"));
    doc.gamedev.export.png8 = true;
    let (req, _) = collect_used_format_features(&doc);
    assert!(req.iter().any(|s| s == "gamedev-context"));
}

#[test]
fn set_export_png8_does_not_invalidate_render() {
    let old = GamedevContext::default();
    let mut new = old.clone();
    new.export.png8 = true;
    let layers = gamedev_cache_invalidation(&old, &new);
    assert!(
        !layers.contains(&GamedevCacheLayer::Render),
        "non-pixel field must not invalidate render: {layers:?}"
    );
}

#[test]
fn older_app_rejects_gamedev_context_with_clear_error() {
    // Current build accepts min_reader 1.3 + features_required gamedev-context.
    let ok = json!({
        "format_version": 1,
        "kind": "dyproj",
        "app_version": "2.0.6-beta",
        "created_at": "t",
        "modified_at": "t",
        "format": { "major": 1, "minor": 3 },
        "min_reader": { "major": 1, "minor": 3 },
        "features_required": ["gamedev-context"]
    });
    let n = normalize_manifest_value(ok).unwrap();
    assert!(check_open_gate(&n).is_ok());

    // Pre-1.3 reader path: min_reader above supported → NeedsNewerApp with hint
    // (simulates opening a 1.3 file in a 1.2-only build by asking for 1.4 here).
    let too_new = json!({
        "format_version": 1,
        "kind": "dyproj",
        "app_version": "2.0.6-beta",
        "created_at": "t",
        "modified_at": "t",
        "format": { "major": 1, "minor": 4 },
        "min_reader": { "major": 1, "minor": 4 },
        "features_required": ["gamedev-context"]
    });
    let n = normalize_manifest_value(too_new).unwrap();
    let err = check_open_gate(&n).unwrap_err();
    match &err {
        ProjectError::NeedsNewerApp {
            required_format,
            hint,
        } => {
            assert!(required_format.contains("1.4"), "{required_format}");
            assert!(
                hint.contains("needs format reader") || hint.contains("update"),
                "hint must be actionable, got: {hint}"
            );
        }
        other => panic!("expected NeedsNewerApp, got {other:?}"),
    }

    // Unknown required feature (older build without registry row) → clear list.
    let unknown = json!({
        "format_version": 1,
        "kind": "dyproj",
        "app_version": "2.0.6-beta",
        "created_at": "t",
        "modified_at": "t",
        "format": { "major": 1, "minor": 0 },
        "min_reader": { "major": 1, "minor": 0 },
        "features_required": ["gamedev-context-typo"]
    });
    let n = normalize_manifest_value(unknown).unwrap();
    let err = check_open_gate(&n).unwrap_err();
    match err {
        ProjectError::UnsupportedFeatures(ids) => {
            assert_eq!(ids, vec!["gamedev-context-typo"]);
            let msg = ProjectError::UnsupportedFeatures(ids.clone()).to_string();
            assert!(
                msg.contains("unsupported required features"),
                "Display must not silently drop feature ids: {msg}"
            );
        }
        other => panic!("expected UnsupportedFeatures, got {other:?}"),
    }
}

#[test]
fn save_open_dyproj_preserves_gamedev_context() {
    let mut doc = Document::new(DocumentId::new(1), 64, 64);
    doc.gamedev.export.png8 = true;
    doc.gamedev.pixel_perfect.nearest_preview = true;

    let cache = TileCache::new(64 * 1024 * 1024);
    let saved = engine_project::serialize::save_project_to_bytes(
        &doc,
        &cache,
        "0.0.0-test",
        |_| Err(engine_project::serialize::ProjectError::MissingEntry("n/a".into())),
    )
    .expect("save");

    let staging = TileCache::new(64 * 1024 * 1024);
    let opened = engine_project::serialize::open_project_from_bytes(
        &saved.zip_bytes,
        &staging,
        DocumentId::new(1),
    )
    .expect("open old-or-new project");

    assert_eq!(
        serde_json::to_vec(&doc.gamedev).unwrap(),
        serde_json::to_vec(&opened.document.gamedev).unwrap()
    );
}
