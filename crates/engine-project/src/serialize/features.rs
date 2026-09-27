//! Format version + feature registry (SPEC §5.3).
//!
//! Writers record the **minimum** format version required by used features,
//! not the app's maximum. The registry is the source of truth for `FORMAT.md`.

use crate::document::Document;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// Semantic format version `{ major, minor }` (independent of app semver).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FormatVersion {
    pub major: u32,
    pub minor: u32,
}

impl FormatVersion {
    pub const V1_0: Self = Self { major: 1, minor: 0 };

    pub const fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }
}

impl PartialOrd for FormatVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FormatVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
    }
}

impl std::fmt::Display for FormatVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// One row in the feature registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FeatureDef {
    pub id: &'static str,
    pub since: FormatVersion,
    /// If true, unknown id → refuse open; if false, safe to ignore.
    pub required: bool,
    pub description: &'static str,
}

/// Highest format version this build can open (major+minor).
///
/// With `version-drill`, the canary features raise support to `1.1`. Release /
/// alpha builds keep `1.0` so files that require `min_reader 1.1` are refused.
#[cfg(feature = "version-drill")]
pub const SUPPORTED_FORMAT: FormatVersion = FormatVersion::new(1, 1);
#[cfg(not(feature = "version-drill"))]
pub const SUPPORTED_FORMAT: FormatVersion = FormatVersion::V1_0;

/// Highest format major this build can open (per kind still uses migrate ladders).
pub const SUPPORTED_FORMAT_MAJOR: u32 = SUPPORTED_FORMAT.major;

/// Registry of format features. Bump `since` only when on-disk schema needs it.
///
/// `tiled-layers` is reserved (SPEC §2) — not emitted by writers until implemented.
/// Canary rows exist only under `--features version-drill` (beta readiness Goal B).
pub fn feature_registry() -> &'static [FeatureDef] {
    #[cfg(feature = "version-drill")]
    {
        // DRILL-ONLY, remove or keep gated forever
        const REG: &[FeatureDef] = &[
            FeatureDef {
                id: "tiled-layers",
                since: FormatVersion::new(2, 0),
                required: true,
                description: "Reserved: per-layer tile stores instead of full-frame PNG",
            },
            FeatureDef {
                id: "canary-optional",
                since: FormatVersion::new(1, 1),
                required: false,
                description: "DRILL-ONLY-CANARY-MARKER optional field canary_note",
            },
            FeatureDef {
                id: "canary-required",
                since: FormatVersion::new(1, 1),
                required: true,
                description: "DRILL-ONLY-CANARY-MARKER required canary-drill layer node",
            },
        ];
        REG
    }
    #[cfg(not(feature = "version-drill"))]
    {
        const REG: &[FeatureDef] = &[FeatureDef {
            id: "tiled-layers",
            since: FormatVersion::new(2, 0),
            required: true,
            description: "Reserved: per-layer tile stores instead of full-frame PNG",
        }];
        REG
    }
}

/// Legacy alias — prefer [`feature_registry`].
pub static FEATURE_REGISTRY: &[FeatureDef] = &[FeatureDef {
    id: "tiled-layers",
    since: FormatVersion::new(2, 0),
    required: true,
    description: "Reserved: per-layer tile stores instead of full-frame PNG",
}];

/// Document.json key for the optional canary field (version-drill only).
#[cfg(feature = "version-drill")]
pub const CANARY_NOTE_KEY: &str = "canary_note";

/// Marker stored on a runtime leaf that round-trips as `node: "canary-drill"`.
#[cfg(feature = "version-drill")]
pub const CANARY_DRILL_EXTRA_KEY: &str = "__canary_drill";

/// Zip/JSON node tag for the required canary layer type.
#[cfg(feature = "version-drill")]
pub const CANARY_DRILL_NODE: &str = "canary-drill";

/// Look up a feature by id.
pub fn feature_by_id(id: &str) -> Option<&'static FeatureDef> {
    feature_registry().iter().find(|f| f.id == id)
}

/// Features actually used by a document that affect the written `format` version.
pub fn required_version_for_features<'a>(used: impl IntoIterator<Item = &'a str>) -> FormatVersion {
    let mut best = FormatVersion::V1_0;
    for id in used {
        if let Some(f) = feature_by_id(id) {
            if f.since > best {
                best = f.since;
            }
        }
    }
    best
}

/// Computed `format` (any used feature) and `min_reader` (required used only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsedFormatVersions {
    pub format: FormatVersion,
    pub min_reader: FormatVersion,
}

/// Derive write-time versions from feature id lists already collected from a doc.
pub fn format_versions_for_used_features(
    features_required: &[String],
    features_optional: &[String],
) -> UsedFormatVersions {
    let format = required_version_for_features(
        features_required
            .iter()
            .chain(features_optional.iter())
            .map(|s| s.as_str()),
    );
    let min_reader = required_version_for_features(features_required.iter().map(|s| s.as_str()));
    UsedFormatVersions { format, min_reader }
}

/// Scan a live document for format features that must be declared on Save.
///
/// Default build: always empty (write `1.0`). With `version-drill`, detects
/// canary markers. Writers emit the **minimum** version needed (§2.6).
pub fn collect_used_format_features(doc: &Document) -> (Vec<String>, Vec<String>) {
    #[cfg(feature = "version-drill")]
    {
        let mut required = Vec::new();
        let mut optional = Vec::new();
        if doc
            .extra
            .get(CANARY_NOTE_KEY)
            .and_then(|v| v.as_str())
            .is_some()
        {
            optional.push("canary-optional".into());
        }
        if root_has_canary_drill(&doc.root) {
            required.push("canary-required".into());
        }
        (required, optional)
    }
    #[cfg(not(feature = "version-drill"))]
    {
        let _ = doc;
        (Vec::new(), Vec::new())
    }
}

#[cfg(feature = "version-drill")]
fn root_has_canary_drill(nodes: &[crate::layer::LayerNode]) -> bool {
    use crate::layer::LayerNode;
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if layer
                    .extra
                    .get(CANARY_DRILL_EXTRA_KEY)
                    .and_then(|v| v.as_bool())
                    == Some(true)
                {
                    return true;
                }
            }
            LayerNode::Group(g) => {
                if root_has_canary_drill(&g.children) {
                    return true;
                }
            }
        }
    }
    false
}

/// Validate `features_required` from a manifest against the registry.
///
/// Returns unknown ids (caller maps to [`super::ProjectError::UnsupportedFeatures`]).
pub fn unknown_required_features(features_required: &[String]) -> Vec<String> {
    features_required
        .iter()
        .filter(|id| feature_by_id(id).is_none())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_features_yield_v1() {
        assert_eq!(
            required_version_for_features(std::iter::empty::<&str>()),
            FormatVersion::V1_0
        );
    }

    #[test]
    fn tiled_layers_bumps_to_v2() {
        assert_eq!(
            required_version_for_features(["tiled-layers"]),
            FormatVersion::new(2, 0)
        );
    }

    #[test]
    fn unknown_required_detected() {
        let unk = unknown_required_features(&["nope".into(), "tiled-layers".into()]);
        assert_eq!(unk, vec!["nope".to_string()]);
    }

    #[test]
    fn optional_only_keeps_min_reader_at_v1() {
        let v = format_versions_for_used_features(&[], &["tiled-layers".into()]);
        // tiled-layers is required:true in registry, but when listed only as
        // "optional" bucket for this helper, min_reader stays V1_0.
        assert_eq!(v.min_reader, FormatVersion::V1_0);
        assert_eq!(v.format, FormatVersion::new(2, 0));
    }

    #[cfg(feature = "version-drill")]
    #[test]
    fn canary_optional_bumps_format_only() {
        let v = format_versions_for_used_features(&[], &["canary-optional".into()]);
        assert_eq!(v.format, FormatVersion::new(1, 1));
        assert_eq!(v.min_reader, FormatVersion::V1_0);
    }

    #[cfg(feature = "version-drill")]
    #[test]
    fn canary_required_bumps_min_reader() {
        let v = format_versions_for_used_features(&["canary-required".into()], &[]);
        assert_eq!(v.format, FormatVersion::new(1, 1));
        assert_eq!(v.min_reader, FormatVersion::new(1, 1));
    }

    #[cfg(not(feature = "version-drill"))]
    #[test]
    fn release_build_has_no_canary_ids() {
        assert!(feature_by_id("canary-optional").is_none());
        assert!(feature_by_id("canary-required").is_none());
        assert_eq!(SUPPORTED_FORMAT, FormatVersion::V1_0);
    }
}
