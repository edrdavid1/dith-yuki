//! Test-only helpers for loading CMYK ICC fixtures (not shipped in the app).

use std::path::PathBuf;

/// Path to the FOGRA51 / PSO Coated v3 fixture used by engine-color tests.
pub fn fogra51_fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/PSOcoated_v3.icc")
}

/// Load FOGRA51 fixture bytes. Panics if the fixture is missing from the tree.
pub fn load_fogra51_fixture() -> Vec<u8> {
    std::fs::read(fogra51_fixture_path()).unwrap_or_else(|e| {
        panic!(
            "missing test fixture {}: {e}",
            fogra51_fixture_path().display()
        )
    })
}
