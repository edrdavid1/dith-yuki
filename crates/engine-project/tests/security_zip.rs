//! Security corpus — ZIP container (SPEC §14.8 subset, Stage 1).
//!
//! Each case expects a typed refusal, no panic, and no filesystem extraction.

use engine_project::serialize::archive::create_zip;
use engine_project::serialize::{
    ArchiveLimits, ExpectedKind, SecureZipArchive, SecureZipError,
};

fn open_dyproj(bytes: &[u8]) -> Result<SecureZipArchive, SecureZipError> {
    SecureZipArchive::open(bytes, ExpectedKind::Dyproj, ArchiveLimits::dyproj())
}

#[test]
fn zip_slip_dotdot_rejected() {
    let zip = create_zip(&[("../x", b"nope")]).unwrap();
    assert!(matches!(
        open_dyproj(&zip),
        Err(SecureZipError::UnsafeEntryName(_))
    ));
}

#[test]
fn zip_slip_absolute_rejected() {
    let zip = create_zip(&[("/etc/passwd", b"x")]).unwrap();
    assert!(matches!(
        open_dyproj(&zip),
        Err(SecureZipError::UnsafeEntryName(_))
    ));
}

#[test]
fn zip_slip_backslash_rejected() {
    let zip = create_zip(&[("..\\x", b"x")]).unwrap();
    assert!(matches!(
        open_dyproj(&zip),
        Err(SecureZipError::UnsafeEntryName(_))
    ));
}

#[test]
fn nul_in_name_rejected() {
    let zip = create_zip(&[("layers/1\0.png", b"x")]).unwrap();
    assert!(matches!(
        open_dyproj(&zip),
        Err(SecureZipError::UnsafeEntryName(_))
    ));
}

#[test]
fn colon_drive_rejected() {
    let zip = create_zip(&[("C:/windows", b"x")]).unwrap();
    assert!(matches!(
        open_dyproj(&zip),
        Err(SecureZipError::UnsafeEntryName(_))
    ));
}

#[test]
fn case_collision_rejected() {
    let zip = create_zip(&[("manifest.json", b"{}"), ("MANIFEST.JSON", b"{}")]).unwrap();
    assert!(matches!(
        open_dyproj(&zip),
        Err(SecureZipError::DuplicateEntry(_))
    ));
}

#[test]
fn too_many_entries_rejected() {
    let limits = ArchiveLimits {
        max_entries: 2,
        ..ArchiveLimits::dyproj()
    };
    let zip = create_zip(&[
        ("manifest.json", b"{}"),
        ("document.json", b"{}"),
        ("layers/1.png", b"PNG"),
    ])
    .unwrap();
    let err = SecureZipArchive::open(&zip, ExpectedKind::Dyproj, limits).unwrap_err();
    assert!(matches!(err, SecureZipError::TooManyEntries { .. }), "{err:?}");
}

#[test]
fn entry_budget_rejects_oversized_read() {
    let limits = ArchiveLimits {
        max_manifest_bytes: 8,
        ..ArchiveLimits::dyproj()
    };
    let zip = create_zip(&[("manifest.json", b"0123456789abcdef")]).unwrap();
    let mut ar = SecureZipArchive::open(&zip, ExpectedKind::Dyproj, limits).unwrap();
    let err = ar.read_entry("manifest.json").unwrap_err();
    assert!(matches!(err, SecureZipError::EntryTooLarge { .. }), "{err:?}");
}

#[test]
fn golden_v1_dyproj_opens_under_secure_loader() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dyproj/v1/minimal.dyproj");
    let bytes = std::fs::read(path).unwrap();
    let ar = open_dyproj(&bytes).expect("golden must pass secure open");
    assert!(ar.contains("manifest.json"));
    assert!(ar.contains("document.json"));
    assert!(ar.warnings.iter().any(|w| w.contains("mimetype")));
}
