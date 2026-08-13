//! Per-kind archive versioning (`dyproj` vs `dyuki`).
//!
//! Each `kind` owns its own `format_version` ladder. Bumping support for one
//! format must not force phantom migrations on the other.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current supported `format_version` for `.dyproj` archives.
pub const SUPPORTED_DYPROJ_VERSION: u32 = 1;

/// Current supported `format_version` for `.dyuki` archives.
pub const SUPPORTED_DYUKI_VERSION: u32 = 1;

/// Soft warn threshold for estimated uncompressed raster payload (bytes).
pub const SOFT_SIZE_WARN_BYTES: u64 = 256 * 1024 * 1024;

/// Archive kind discriminant in `manifest.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArchiveKind {
    Dyproj,
    Dyuki,
}

impl ArchiveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArchiveKind::Dyproj => "dyproj",
            ArchiveKind::Dyuki => "dyuki",
        }
    }

    pub fn supported_version(self) -> u32 {
        match self {
            ArchiveKind::Dyproj => SUPPORTED_DYPROJ_VERSION,
            ArchiveKind::Dyuki => SUPPORTED_DYUKI_VERSION,
        }
    }
}

/// `manifest.json` v1 fields (shared shape; dims optional for tiny `.dyuki`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub format_version: u32,
    pub kind: ArchiveKind,
    pub app_version: String,
    pub created_at: String,
    pub modified_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// Project / pattern archive errors (versioning + shared I/O surface).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ProjectError {
    #[error(
        "unsupported {kind} format_version {found} (this app supports up to {supported}); update the app"
    )]
    UnsupportedVersion {
        kind: String,
        found: u32,
        supported: u32,
    },

    #[error("archive kind mismatch: expected {expected}, found {found}")]
    KindMismatch { expected: String, found: String },

    #[error("incomplete Raw tiles for layer {layer_id}; cannot assemble project PNG")]
    IncompleteRaw { layer_id: u32 },

    #[error("missing archive entry: {0}")]
    MissingEntry(String),

    #[error("invalid archive: {0}")]
    InvalidArchive(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("encode/decode error: {0}")]
    Codec(String),

    #[error("threshold map hash mismatch: entry {entry} has content hash {actual}")]
    HashMismatch { entry: String, actual: String },

    #[error("CustomPng path was not rewritten to a materialized asset: {0}")]
    UnresolvedCustomPng(String),

    #[error(
        "this pattern requires app version {required} or newer (running {running}); update the app"
    )]
    AppVersionTooOld { required: String, running: String },

    #[error("cannot apply pattern to a group, select a layer")]
    TargetIsGroup,

    #[error("layer {0} not found")]
    LayerNotFound(u32),

    #[error("filter id {0} not found on layer")]
    FilterNotFound(String),

    #[error("palette placeholder {0} is not in palettes.json")]
    MissingPalettePlaceholder(String),

    #[error("palette {0} referenced by filter is missing from the document")]
    MissingPalette(u32),

    #[error("no filters to export")]
    EmptyExport,
}

/// Gate `format_version` for a known kind. Future versions error; older ones
/// are accepted here and handled by the kind-specific migrate chain.
pub fn check_format_version(kind: ArchiveKind, found: u32) -> Result<(), ProjectError> {
    let supported = kind.supported_version();
    if found > supported {
        return Err(ProjectError::UnsupportedVersion {
            kind: kind.as_str().to_string(),
            found,
            supported,
        });
    }
    Ok(())
}

/// Migrate a `.dyproj` document payload up to [`SUPPORTED_DYPROJ_VERSION`].
///
/// MVP: only v1 exists (identity). `document_json` is returned unchanged when
/// `format_version == 1`.
pub fn migrate_dyproj(format_version: u32, document_json: serde_json::Value) -> Result<serde_json::Value, ProjectError> {
    check_format_version(ArchiveKind::Dyproj, format_version)?;
    // v1 → identity. Future: ordered migrate_vN_to_vN+1 chain for dyproj only.
    Ok(document_json)
}

/// Migrate a `.dyuki` pattern payload up to [`SUPPORTED_DYUKI_VERSION`].
///
/// MVP: only v1 exists (identity). Independent of the dyproj version ladder.
pub fn migrate_dyuki(format_version: u32, pattern_json: serde_json::Value) -> Result<serde_json::Value, ProjectError> {
    check_format_version(ArchiveKind::Dyuki, format_version)?;
    Ok(pattern_json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dyproj_v1_identity() {
        let doc = json!({"root": {}});
        let out = migrate_dyproj(1, doc.clone()).unwrap();
        assert_eq!(out, doc);
    }

    #[test]
    fn dyuki_v1_identity() {
        let pat = json!({"filters": []});
        let out = migrate_dyuki(1, pat.clone()).unwrap();
        assert_eq!(out, pat);
    }

    #[test]
    fn future_dyproj_version_errors() {
        let err = migrate_dyproj(99, json!({})).unwrap_err();
        match err {
            ProjectError::UnsupportedVersion {
                kind,
                found,
                supported,
            } => {
                assert_eq!(kind, "dyproj");
                assert_eq!(found, 99);
                assert_eq!(supported, SUPPORTED_DYPROJ_VERSION);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn future_dyuki_version_errors() {
        let err = check_format_version(ArchiveKind::Dyuki, 42).unwrap_err();
        assert!(matches!(
            err,
            ProjectError::UnsupportedVersion {
                kind: _,
                found: 42,
                supported: SUPPORTED_DYUKI_VERSION
            }
        ));
    }

    #[test]
    fn kind_version_ladders_are_independent() {
        // Document the lock: constants are separate symbols; bumping one must
        // not be expressed as a shared counter.
        assert_eq!(ArchiveKind::Dyproj.supported_version(), SUPPORTED_DYPROJ_VERSION);
        assert_eq!(ArchiveKind::Dyuki.supported_version(), SUPPORTED_DYUKI_VERSION);
        // Both start at 1 for MVP, but they are not the same binding.
        let _ = SUPPORTED_DYPROJ_VERSION;
        let _ = SUPPORTED_DYUKI_VERSION;
    }

    #[test]
    fn manifest_round_trip_json() {
        let m = Manifest {
            format_version: 1,
            kind: ArchiveKind::Dyproj,
            app_version: "0.1.0".into(),
            created_at: "2026-08-12T00:00:00Z".into(),
            modified_at: "2026-08-12T00:00:00Z".into(),
            width: Some(1920),
            height: Some(1080),
        };
        let s = serde_json::to_string(&m).unwrap();
        let back: Manifest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, m);
        assert!(s.contains("\"kind\":\"dyproj\""));
    }
}
