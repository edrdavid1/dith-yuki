//! Unit tests for the algorithm registry.
//!
//! # CI gates
//! - `algorithm_id_registry_txt_is_sorted`  — the file on disk is sorted
//!   alphabetically, one ID per line, no duplicates.
//! - `algorithm_id_registry_matches_txt`    — every ID in the file is present
//!   in the runtime registry and vice versa.
//! - `register_all_ids_unique` — `register_all` does not insert duplicate IDs.

use engine_project::algorithms::register_all;
use engine_project::serialize::{filter_from_file, FilterInstanceFile};
use engine_registry::{AlgorithmId, AlgorithmRegistry};

/// Path to the registry file, relative to the workspace root.
///
/// The test locates the file by walking up from the cargo manifest directory
/// (`CARGO_MANIFEST_DIR`) to the repository root.
fn registry_txt_path() -> std::path::PathBuf {
    // CARGO_MANIFEST_DIR is .../crates/engine-project
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // repo root is two levels up
    manifest_dir
        .parent() // crates/
        .expect("crates/ dir")
        .parent() // repo root
        .expect("repo root")
        .join("ALGORITHM_ID_REGISTRY.txt")
}

/// Parse ALGORITHM_ID_REGISTRY.txt into a sorted Vec of non-empty lines.
fn read_registry_txt() -> Vec<String> {
    let path = registry_txt_path();
    let content = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

// ---------------------------------------------------------------------------
// Test: file format invariants
// ---------------------------------------------------------------------------

/// ALGORITHM_ID_REGISTRY.txt must be sorted alphabetically and contain no
/// duplicate entries.  This catches accidental manual edits.
#[test]
fn algorithm_id_registry_txt_is_sorted_and_unique() {
    let ids = read_registry_txt();
    assert!(
        !ids.is_empty(),
        "ALGORITHM_ID_REGISTRY.txt is empty — expected 17 entries"
    );

    // Check sorted order.
    for window in ids.windows(2) {
        assert!(
            window[0] < window[1],
            "ALGORITHM_ID_REGISTRY.txt is not sorted: {:?} appears before {:?}",
            window[0],
            window[1]
        );
    }

    // Check uniqueness (implied by sorted + strict less-than, but be explicit).
    let mut seen = std::collections::HashSet::new();
    for id in &ids {
        assert!(
            seen.insert(id.as_str()),
            "duplicate entry in ALGORITHM_ID_REGISTRY.txt: {id}"
        );
    }
}

/// Snapshot test: the file currently contains exactly the IDs defined in
/// the design doc.  Update this list when new algorithms are added.
#[test]
fn algorithm_id_registry_txt_contains_expected_ids() {
    let ids = read_registry_txt();
    let expected = [
        "adjust",
        "atkinson",
        "bayer_16x16",
        "bayer_2x2",
        "bayer_4x4",
        "bayer_8x8",
        "burkes",
        "clustered_dot_ordered",
        "cmyk_halftone",
        "crt",
        "curves",
        "dispersed_dot_ordered",
        "floyd_steinberg",
        "glitch",
        "glow",
        "halftone_screen_angled",
        "jarvis_judice_ninke",
        "palette_quantize",
        "sierra",
        "sierra_lite",
        "sierra_two_row",
        "stevenson_arce",
        "stucki",
        "wave",
    ];
    assert_eq!(
        ids, expected,
        "ALGORITHM_ID_REGISTRY.txt contents differ from the design-doc baseline"
    );
}

// ---------------------------------------------------------------------------
// Test: registry ↔ file parity
// ---------------------------------------------------------------------------

/// Verify that every ID in `ALGORITHM_ID_REGISTRY.txt` is present in the
/// runtime registry, and that the runtime registry contains no ID absent from
/// the file.
///
/// _Requirements: 2.2, 2.3_
#[test]
fn algorithm_id_registry_matches_txt() {
    let file_ids: std::collections::HashSet<String> = read_registry_txt().into_iter().collect();

    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);

    // Collect registry IDs.
    let registry_ids: std::collections::HashSet<String> = registry
        .all_ids()
        .map(|id| id.as_str().to_owned())
        .collect();

    // Every file entry must be in the registry.
    let missing_from_registry: Vec<&String> = file_ids
        .iter()
        .filter(|id| !registry_ids.contains(id.as_str()))
        .collect();
    assert!(
        missing_from_registry.is_empty(),
        "IDs in ALGORITHM_ID_REGISTRY.txt but absent from registry: {missing_from_registry:?}"
    );

    // Every registry entry must be in the file.
    let missing_from_file: Vec<&String> = registry_ids
        .iter()
        .filter(|id| !file_ids.contains(id.as_str()))
        .collect();
    assert!(
        missing_from_file.is_empty(),
        "IDs in registry but absent from ALGORITHM_ID_REGISTRY.txt: {missing_from_file:?}"
    );
}

// ---------------------------------------------------------------------------
// Test: no duplicate IDs in register_all
// ---------------------------------------------------------------------------

/// Calling `register_all` must not panic (debug_assert in `register`) and must
/// produce a registry with no duplicate IDs.
///
/// _Requirements: 1.2_
#[test]
fn register_all_ids_unique() {
    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);

    let all_ids: Vec<AlgorithmId> = registry.all_ids().collect();
    let unique: std::collections::HashSet<AlgorithmId> = all_ids.iter().copied().collect();
    assert_eq!(
        all_ids.len(),
        unique.len(),
        "register_all() registered duplicate AlgorithmIds"
    );
    assert!(
        registry.get_by_str("bayer_2x2").is_some(),
        "Phase 2.1 must register bayer_2x2"
    );
    assert!(
        registry.get_by_str("bayer_4x4").is_some(),
        "Phase 1 must register bayer_4x4"
    );
    assert!(
        registry.get_by_str("bayer_8x8").is_some(),
        "Phase 2.1 must register bayer_8x8"
    );
    assert!(
        registry.get_by_str("sierra_lite").is_some(),
        "Batch B must register sierra_lite"
    );
    assert!(
        registry.get_by_str("sierra_two_row").is_some(),
        "Batch B must register sierra_two_row"
    );
    assert!(
        registry.get_by_str("stevenson_arce").is_some(),
        "Batch B must register stevenson_arce"
    );
    assert!(
        registry.get_by_str("palette_quantize").is_some(),
        "Phase 2.2 must register palette_quantize"
    );
    assert_eq!(
        all_ids.len(),
        24,
        "must register all 24 built-in algorithms"
    );
}

/// `migrate_params(1, json)` twice equals calling it once (Req 7.3).
#[test]
fn migrate_params_is_idempotent() {
    let mut registry = AlgorithmRegistry::new();
    register_all(&mut registry);
    for id in registry.all_ids() {
        let algo = registry.get(id).expect("registered");
        let mut once = serde_json::json!({});
        algo.migrate_params(1, &mut once);
        let mut twice = serde_json::json!({});
        algo.migrate_params(1, &mut twice);
        algo.migrate_params(1, &mut twice);
        assert_eq!(
            once,
            twice,
            "migrate_params is not idempotent for {}",
            id.as_str()
        );
    }
}

/// Every fixture in `tests/fixtures/migration/` loads through `filter_from_file`.
#[test]
fn migration_corpus() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/migration");
    let mut count = 0usize;
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        count += 1;
        let bytes = std::fs::read(&path).unwrap();
        let file: FilterInstanceFile =
            serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let inst = filter_from_file(&file, None);
        assert!(
            inst.algorithm_id.is_some(),
            "{} missing algorithm_id after load",
            path.display()
        );
    }
    assert_eq!(
        count, 24,
        "expected one migration fixture per built-in algorithm"
    );
}
