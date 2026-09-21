//! Manifest normalize + open gate (SPEC §5.2–§5.4).

use crate::serialize::assets::sha256_hex;
use crate::serialize::features::{
    unknown_required_features, FormatVersion, SUPPORTED_FORMAT_MAJOR,
};
use crate::serialize::migrate::{ArchiveKind, Manifest, ProjectError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Per-entry integrity record in `manifest.files` (SPEC §6.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileIntegrity {
    pub size: u64,
    pub sha256: String,
}

/// Sorted map of archive paths → integrity (excluding `mimetype` / `manifest.json`).
pub type ManifestFiles = BTreeMap<String, FileIntegrity>;

/// Build `manifest.files` from payload entries (name → bytes).
pub fn build_manifest_files(entries: &[(String, Vec<u8>)]) -> ManifestFiles {
    let mut files = ManifestFiles::new();
    for (name, bytes) in entries {
        if name == "mimetype" || name == "manifest.json" {
            continue;
        }
        files.insert(
            name.clone(),
            FileIntegrity {
                size: bytes.len() as u64,
                sha256: sha256_hex(bytes),
            },
        );
    }
    files
}

/// Verify `manifest.files` against bytes already read from the archive.
///
/// Legacy manifests without `files` are accepted (no-op). Present map: every
/// listed path must match size + sha256; missing entry → Corrupt.
pub fn verify_manifest_files(
    files: &ManifestFiles,
    read_entry: &mut dyn FnMut(&str) -> Result<Vec<u8>, ProjectError>,
) -> Result<(), ProjectError> {
    if files.is_empty() {
        return Ok(());
    }
    for (path, expected) in files {
        let bytes = read_entry(path)?;
        if bytes.len() as u64 != expected.size {
            return Err(ProjectError::Corrupt(format!(
                "manifest.files[{path}]: size {} != {}",
                bytes.len(),
                expected.size
            )));
        }
        let actual = sha256_hex(&bytes);
        if actual != expected.sha256 {
            return Err(ProjectError::Corrupt(format!(
                "manifest.files[{path}]: sha256 mismatch"
            )));
        }
    }
    Ok(())
}

/// `generator` block written into new manifests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratorInfo {
    pub app: String,
    pub version: String,
}

impl Default for GeneratorInfo {
    fn default() -> Self {
        Self {
            app: "Dither".into(),
            version: String::new(),
        }
    }
}

/// Color / size summary (optional on write until Stage 4 fills it fully).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ManifestDocumentInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// Normalized view used by the open algorithm (legacy + new fields unified).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedManifest {
    pub kind: ArchiveKind,
    pub format: FormatVersion,
    pub min_reader: FormatVersion,
    pub features_required: Vec<String>,
    pub features_optional: Vec<String>,
    pub generator: GeneratorInfo,
    pub created_at: String,
    pub modified_at: String,
    pub document: ManifestDocumentInfo,
    /// Legacy alias always present after normalize (`format.major`).
    pub format_version: u32,
    /// Pattern-only fields (empty for dyproj).
    pub name: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    /// Ignored for compatibility gates (SPEC §5.1); retained for old files.
    pub app_version_min: Option<String>,
    /// Content integrity map; empty for legacy v1 without `files`.
    pub files: ManifestFiles,
}

/// Parse `manifest.json` Value into [`NormalizedManifest`] (SPEC §5.2 legacy rules).
pub fn normalize_manifest_value(value: Value) -> Result<NormalizedManifest, ProjectError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ProjectError::InvalidArchive("manifest.json must be an object".into()))?;

    let kind: ArchiveKind = serde_json::from_value(
        obj.get("kind")
            .cloned()
            .ok_or_else(|| ProjectError::InvalidArchive("manifest missing kind".into()))?,
    )
    .map_err(|e| ProjectError::InvalidArchive(format!("manifest.kind: {e}")))?;

    let format_version = obj
        .get("format_version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| ProjectError::InvalidArchive("manifest missing format_version".into()))?
        as u32;

    let format = match obj.get("format") {
        Some(v) => serde_json::from_value::<FormatVersion>(v.clone())
            .map_err(|e| ProjectError::InvalidArchive(format!("manifest.format: {e}")))?,
        None => FormatVersion {
            major: format_version,
            minor: 0,
        },
    };

    // Legacy writers may disagree; prefer explicit `format` but keep alias coherent.
    if format.major != format_version && obj.contains_key("format") {
        // Soft: trust `format`, keep reported format_version from field for migrate().
    }

    let min_reader = match obj.get("min_reader") {
        Some(v) => serde_json::from_value::<FormatVersion>(v.clone())
            .map_err(|e| ProjectError::InvalidArchive(format!("manifest.min_reader: {e}")))?,
        None => FormatVersion {
            major: format_version,
            minor: 0,
        },
    };

    let features_required: Vec<String> = obj
        .get("features_required")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?
        .unwrap_or_default();

    let features_optional: Vec<String> = obj
        .get("features_optional")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?
        .unwrap_or_default();

    let generator = if let Some(v) = obj.get("generator") {
        serde_json::from_value(v.clone())
            .map_err(|e| ProjectError::InvalidArchive(format!("manifest.generator: {e}")))?
    } else {
        let app_version = obj
            .get("app_version")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        GeneratorInfo {
            app: "Dither".into(),
            version: app_version,
        }
    };

    let created_at = obj
        .get("created_at")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let modified_at = obj
        .get("modified_at")
        .and_then(|v| v.as_str())
        .unwrap_or(&created_at)
        .to_string();

    let width = obj
        .get("width")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .or_else(|| {
            obj.get("document")
                .and_then(|d| d.get("width"))
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
        });
    let height = obj
        .get("height")
        .and_then(|v| v.as_u64())
        .map(|n| n as u32)
        .or_else(|| {
            obj.get("document")
                .and_then(|d| d.get("height"))
                .and_then(|v| v.as_u64())
                .map(|n| n as u32)
        });

    let files = match obj.get("files") {
        Some(v) => serde_json::from_value::<ManifestFiles>(v.clone()).map_err(|e| {
            ProjectError::InvalidArchive(format!("manifest.files: {e}"))
        })?,
        None => ManifestFiles::new(),
    };

    Ok(NormalizedManifest {
        kind,
        format,
        min_reader,
        features_required,
        features_optional,
        generator,
        created_at,
        modified_at,
        document: ManifestDocumentInfo { width, height },
        format_version,
        name: obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        description: obj
            .get("description")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        author: obj
            .get("author")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        app_version_min: obj
            .get("app_version_min")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        files,
    })
}

/// SPEC §5.4 steps 3–5 (kind already checked by caller when expected is known).
pub fn check_open_gate(manifest: &NormalizedManifest) -> Result<(), ProjectError> {
    if manifest.min_reader.major > SUPPORTED_FORMAT_MAJOR {
        return Err(ProjectError::NeedsNewerApp {
            required_format: manifest.min_reader.to_string(),
            hint: format!(
                "This file needs format reader {}.{}+; this app supports format major ≤ {}. See docs/FORMAT.md.",
                manifest.min_reader.major, manifest.min_reader.minor, SUPPORTED_FORMAT_MAJOR
            ),
        });
    }

    let unknown = unknown_required_features(&manifest.features_required);
    if !unknown.is_empty() {
        return Err(ProjectError::UnsupportedFeatures(unknown));
    }

    // Still gate absurd future format_version for migrate ladder.
    if manifest.format_version > SUPPORTED_FORMAT_MAJOR {
        return Err(ProjectError::UnsupportedVersion {
            kind: manifest.kind.as_str().to_string(),
            found: manifest.format_version,
            supported: SUPPORTED_FORMAT_MAJOR,
        });
    }

    Ok(())
}

/// Build a write-time dyproj manifest JSON (legacy fields + new optional block).
///
/// Always writes `format_version` (legacy alias) so released apps keep opening
/// files that only use v1 features.
pub fn build_dyproj_manifest_json(
    format: FormatVersion,
    min_reader: FormatVersion,
    features_required: &[String],
    features_optional: &[String],
    app_version: &str,
    created_at: &str,
    modified_at: &str,
    width: u32,
    height: u32,
    files: &ManifestFiles,
) -> Result<Vec<u8>, ProjectError> {
    let m = serde_json::json!({
        "format_version": format.major,
        "kind": "dyproj",
        "app_version": app_version,
        "created_at": created_at,
        "modified_at": modified_at,
        "width": width,
        "height": height,
        "format": { "major": format.major, "minor": format.minor },
        "min_reader": { "major": min_reader.major, "minor": min_reader.minor },
        "features_required": features_required,
        "features_optional": features_optional,
        "generator": { "app": "Dither", "version": app_version },
        "document": { "width": width, "height": height },
        "files": files,
    });
    serde_json::to_vec_pretty(&m).map_err(|e| ProjectError::Codec(e.to_string()))
}

/// Build write-time dyuki manifest. Still includes `app_version_min` for byte-compat
/// with older readers; new open path **ignores** it for gates.
pub fn build_dyuki_manifest_json(
    format: FormatVersion,
    min_reader: FormatVersion,
    features_required: &[String],
    features_optional: &[String],
    name: &str,
    description: Option<&str>,
    author: Option<&str>,
    created_at: &str,
    app_version_min_legacy: &str,
    files: &ManifestFiles,
) -> Result<Vec<u8>, ProjectError> {
    let mut m = serde_json::json!({
        "format_version": format.major,
        "kind": "dyuki",
        "app_version_min": app_version_min_legacy,
        "name": name,
        "created_at": created_at,
        "format": { "major": format.major, "minor": format.minor },
        "min_reader": { "major": min_reader.major, "minor": min_reader.minor },
        "features_required": features_required,
        "features_optional": features_optional,
        "generator": { "app": "Dither", "version": app_version_min_legacy },
        "files": files,
    });
    if let Some(d) = description {
        m["description"] = Value::String(d.to_string());
    }
    if let Some(a) = author {
        m["author"] = Value::String(a.to_string());
    }
    serde_json::to_vec_pretty(&m).map_err(|e| ProjectError::Codec(e.to_string()))
}

/// Convert legacy [`Manifest`] (unit tests / old helpers) into normalized form.
pub fn normalize_legacy_manifest(m: &Manifest) -> NormalizedManifest {
    let format = FormatVersion {
        major: m.format_version,
        minor: 0,
    };
    NormalizedManifest {
        kind: m.kind,
        format,
        min_reader: format,
        features_required: vec![],
        features_optional: vec![],
        generator: GeneratorInfo {
            app: "Dither".into(),
            version: m.app_version.clone(),
        },
        created_at: m.created_at.clone(),
        modified_at: m.modified_at.clone(),
        document: ManifestDocumentInfo {
            width: m.width,
            height: m.height,
        },
        format_version: m.format_version,
        name: None,
        description: None,
        author: None,
        app_version_min: None,
        files: ManifestFiles::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_manifest_without_format_block() {
        let v = json!({
            "format_version": 1,
            "kind": "dyproj",
            "app_version": "0.2.0",
            "created_at": "t",
            "modified_at": "t",
            "width": 10,
            "height": 20
        });
        let n = normalize_manifest_value(v).unwrap();
        assert_eq!(n.format, FormatVersion::V1_0);
        assert_eq!(n.min_reader, FormatVersion::V1_0);
        assert!(n.features_required.is_empty());
        assert_eq!(n.document.width, Some(10));
        check_open_gate(&n).unwrap();
    }

    #[test]
    fn min_reader_too_new_errors() {
        let v = json!({
            "format_version": 1,
            "kind": "dyproj",
            "app_version": "0.2.0",
            "created_at": "t",
            "modified_at": "t",
            "format": { "major": 1, "minor": 0 },
            "min_reader": { "major": 9, "minor": 0 }
        });
        let n = normalize_manifest_value(v).unwrap();
        let err = check_open_gate(&n).unwrap_err();
        assert!(matches!(err, ProjectError::NeedsNewerApp { .. }), "{err:?}");
    }

    #[test]
    fn unknown_required_feature_errors() {
        let v = json!({
            "format_version": 1,
            "kind": "dyuki",
            "app_version_min": "99.0.0",
            "name": "x",
            "created_at": "t",
            "features_required": ["totally-unknown"]
        });
        let n = normalize_manifest_value(v).unwrap();
        // app_version_min is ignored by the gate
        let err = check_open_gate(&n).unwrap_err();
        match err {
            ProjectError::UnsupportedFeatures(ids) => assert_eq!(ids, vec!["totally-unknown"]),
            other => panic!("{other:?}"),
        }
    }
}
