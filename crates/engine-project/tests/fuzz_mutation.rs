//! CI substitute for cargo-fuzz (SPEC §14.9 / review close-out).
//!
//! Mutates golden archive bytes and asserts open/unpack never panics —
//! only typed errors or success.

use engine_project::serialize::{
    decode_png_to_f32_with_limits, migrate_dyproj, migrate_dyuki, normalize_manifest_value,
    open_project_from_bytes, parse_json_value, threshold_map_png_limits, unpack_pattern_from_bytes,
    MAX_JSON_DEPTH,
};
use engine_project::types::DocumentId;
use engine_tiles::TileCache;
use std::path::PathBuf;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn load_fixture(rel: &str) -> Vec<u8> {
    std::fs::read(fixtures_root().join(rel)).unwrap_or_else(|e| panic!("load {rel}: {e}"))
}

/// Deterministic byte mutations of a seed (flip / insert / truncate / xor).
fn mutations(seed: &[u8]) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    if seed.is_empty() {
        return out;
    }
    // Truncations
    for keep in [0usize, 1, 4, seed.len() / 2, seed.len().saturating_sub(1)] {
        if keep < seed.len() {
            out.push(seed[..keep].to_vec());
        }
    }
    // Single-byte flips at several offsets
    for &idx in &[
        0usize,
        1,
        10,
        100,
        seed.len() / 3,
        seed.len().saturating_sub(1),
    ] {
        if idx < seed.len() {
            let mut m = seed.to_vec();
            m[idx] ^= 0xff;
            out.push(m);
            let mut m2 = seed.to_vec();
            m2[idx] = m2[idx].wrapping_add(1);
            out.push(m2);
        }
    }
    // Insert junk mid-stream
    if seed.len() > 8 {
        let mut m = seed.to_vec();
        m.splice(8..8, [0u8, 0xff, b'{', b'}']);
        out.push(m);
    }
    // Prefix with random-looking header
    let mut prefixed = vec![0x50, 0x4b, 0x03, 0x04];
    prefixed.extend_from_slice(seed);
    out.push(prefixed);
    // Empty / tiny
    out.push(vec![]);
    out.push(vec![0]);
    out.push(b"PK\x05\x06".to_vec());
    out
}

#[test]
fn mutated_dyproj_never_panics() {
    let seed = load_fixture("dyproj/v1/minimal.dyproj");
    for (i, bytes) in mutations(&seed).into_iter().enumerate() {
        let cache = TileCache::new(20_000_000);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            open_project_from_bytes(&bytes, &cache, DocumentId::new(1))
        }));
        assert!(
            result.is_ok(),
            "mutation {i} panicked (len={})",
            bytes.len()
        );
    }
}

#[test]
fn mutated_dyuki_never_panics() {
    let seed = load_fixture("dyuki/v1/minimal.dyuki");
    for (i, bytes) in mutations(&seed).into_iter().enumerate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            unpack_pattern_from_bytes(&bytes, "0.3.0")
        }));
        assert!(
            result.is_ok(),
            "mutation {i} panicked (len={})",
            bytes.len()
        );
    }
}

#[test]
fn mutated_manifest_json_never_panics() {
    let seeds: &[&[u8]] = &[
        br#"{"format_version":1,"kind":"dyproj"}"#,
        br#"{"format_version":1,"kind":"dyuki","app_version_min":"0.1.0","name":"x","created_at":"t"}"#,
        br#"{}"#,
        br#"[]"#,
        br#"null"#,
    ];
    for seed in seeds {
        for (i, bytes) in mutations(seed).into_iter().enumerate() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Ok(v) = parse_json_value(&bytes, MAX_JSON_DEPTH) {
                    let _ = normalize_manifest_value(v);
                }
            }));
            assert!(result.is_ok(), "manifest mutation {i} panicked");
        }
    }
}

#[test]
fn mutated_png_never_panics() {
    // Minimal valid-ish PNG header + garbage
    let mut seed = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
    seed.extend_from_slice(&[0u8; 64]);
    for (i, bytes) in mutations(&seed).into_iter().enumerate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = decode_png_to_f32_with_limits(&bytes, threshold_map_png_limits());
        }));
        assert!(result.is_ok(), "png mutation {i} panicked");
    }
}

#[test]
fn mutated_migrate_never_panics() {
    let seed = br#"{"root":[],"width":1,"height":1}"#;
    for (i, bytes) in mutations(seed).into_iter().enumerate() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if let Ok(v) = parse_json_value(&bytes, MAX_JSON_DEPTH) {
                let _ = migrate_dyproj(1, v.clone());
                let _ = migrate_dyuki(1, v);
                let _ = migrate_dyproj(99, serde_json::json!({}));
            }
        }));
        assert!(result.is_ok(), "migrate mutation {i} panicked");
    }
}

#[test]
fn proptest_random_bytes_open_dyproj_no_panic() {
    use proptest::prelude::*;
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = proptest::collection::vec(any::<u8>(), 0..4_096);
    for _ in 0..64 {
        let bytes = strategy.new_tree(&mut runner).unwrap().current();
        let cache = TileCache::new(8 * 1024 * 1024);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = open_project_from_bytes(&bytes, &cache, DocumentId::new(1));
        }));
        assert!(result.is_ok(), "proptest case panicked");
    }
}
