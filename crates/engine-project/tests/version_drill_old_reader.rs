//! Version-drill matrix — old reader (build WITHOUT `version-drill`).
//!
//! Scenarios 1–3 and 7 from SPEC_dither_beta_readiness §2.5.
//! Fixtures under `tests/fixtures/dyproj/drill/` are produced by the
//! `version-drill` generate test and committed permanently.

use engine_project::serialize::{
    open_project_from_bytes, ProjectError, SUPPORTED_FORMAT,
};
use engine_project::types::DocumentId;
use engine_tiles::TileCache;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn read_fixture(rel: &str) -> Vec<u8> {
    let path = fixtures_root().join(rel);
    fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn open_bytes(bytes: &[u8]) -> Result<(), ProjectError> {
    let staging = TileCache::new(32 * 1024 * 1024);
    open_project_from_bytes(bytes, &staging, DocumentId::new(1)).map(|_| ())
}

/// §2.5 #1 — v1.0 golden opens on this (non-drill) engine.
#[test]
fn matrix_1_v1_minimal_opens_on_old_reader() {
    assert_eq!(
        SUPPORTED_FORMAT,
        engine_project::serialize::FormatVersion::V1_0
    );
    let bytes = read_fixture("dyproj/v1/minimal.dyproj");
    open_bytes(&bytes).expect("v1.0 must open");
}

/// §2.5 #2 — optional canary opens on old reader (unknown field ignored).
#[test]
fn matrix_2_optional_used_opens_on_old_reader() {
    let path = fixtures_root().join("dyproj/drill/optional-used.dyproj");
    if !path.exists() {
        eprintln!("skip: drill fixture missing — generate with version-drill");
        return;
    }
    let bytes = fs::read(&path).unwrap();
    open_bytes(&bytes).expect("optional canary must open on old reader");
}

/// §2.5 #3 — required canary is refused with a typed error (no panic).
#[test]
fn matrix_3_required_used_refused_by_old_reader() {
    let path = fixtures_root().join("dyproj/drill/required-used.dyproj");
    if !path.exists() {
        eprintln!("skip: drill fixture missing — generate with version-drill");
        return;
    }
    let bytes = fs::read(&path).unwrap();
    let err = open_bytes(&bytes).expect_err("required canary must refuse");
    assert!(
        matches!(
            err,
            ProjectError::NeedsNewerApp { .. } | ProjectError::UnsupportedFeatures(_)
        ),
        "expected NeedsNewerApp or UnsupportedFeatures, got {err:?}"
    );
}

/// §2.5 #7 — catch_unwind around open for every drill/v1 fixture.
#[test]
fn matrix_7_no_process_panic_on_any_fixture() {
    let rels = [
        "dyproj/v1/minimal.dyproj",
        "dyproj/drill/optional-used.dyproj",
        "dyproj/drill/required-used.dyproj",
        "dyproj/drill/neither-used.dyproj",
    ];
    for rel in rels {
        let path = fixtures_root().join(rel);
        if !path.exists() {
            continue;
        }
        let bytes = fs::read(&path).unwrap();
        let result = std::panic::catch_unwind(|| {
            let staging = TileCache::new(32 * 1024 * 1024);
            let _ = open_project_from_bytes(&bytes, &staging, DocumentId::new(1));
        });
        assert!(
            result.is_ok(),
            "open panicked for {rel} — catch_loader_panic contract broken"
        );
    }
}

/// Reference-reader CLI (`dyproj-cli` without version-drill) refuses required canary.
#[test]
fn reference_reader_cli_refuses_required_canary() {
    let path = fixtures_root().join("dyproj/drill/required-used.dyproj");
    if !path.exists() {
        eprintln!("skip: drill fixture missing");
        return;
    }
    let bin = env!("CARGO_BIN_EXE_dyproj-cli");
    let out = Command::new(bin)
        .args(["open", path.to_str().unwrap()])
        .output()
        .expect("spawn dyproj-cli");
    assert!(
        !out.status.success(),
        "reference reader must refuse required-used; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("needs newer app")
            || stderr.contains("unsupported required features")
            || stderr.contains("UnsupportedFeatures")
            || stderr.contains("canary-required"),
        "stderr should explain refusal: {stderr}"
    );
}

/// Release build must not embed the canary string marker.
#[test]
fn release_build_has_no_canary_marker_in_cli_help_path() {
    // Compile-time: canary ids are absent from the registry in this cfg.
    assert!(engine_project::serialize::feature_by_id("canary-optional").is_none());
    assert!(engine_project::serialize::feature_by_id("canary-required").is_none());
}
