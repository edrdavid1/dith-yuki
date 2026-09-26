//! Format version + feature registry (SPEC §5.3).
//!
//! Writers record the **minimum** format version required by used features,
//! not the app's maximum. The registry is the source of truth for `FORMAT.md`.

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

/// Registry of format features. Bump `since` only when on-disk schema needs it.
///
/// `tiled-layers` is reserved (SPEC §2) — not emitted by writers until implemented.
pub static FEATURE_REGISTRY: &[FeatureDef] = &[FeatureDef {
    id: "tiled-layers",
    since: FormatVersion::new(2, 0),
    required: true,
    description: "Reserved: per-layer tile stores instead of full-frame PNG",
}];

/// Highest format major this build can open (per kind still uses migrate ladders).
pub const SUPPORTED_FORMAT_MAJOR: u32 = 1;

/// Look up a feature by id.
pub fn feature_by_id(id: &str) -> Option<&'static FeatureDef> {
    FEATURE_REGISTRY.iter().find(|f| f.id == id)
}

/// Features actually used by a document that affect the written `format` version.
///
/// Today no optional/required format features are emitted for live docs — always
/// [`FormatVersion::V1_0`]. Hook for Stage 3+ when real features land.
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
}
