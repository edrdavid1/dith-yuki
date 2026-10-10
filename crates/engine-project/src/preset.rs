//! Filter-stack presets (user-scoped, app-data JSON).
//!
//! A preset snapshots the filter stack (algorithm_id + params + order) and
//! palette **references by id** — never embedded palette bytes. Export format,
//! PNG8, paths, and DPI are per-export and are not stored here.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::algorithms::builtin_registry;
use crate::filter::{FilterInstance, FilterParams};
use crate::serialize::document_dto::{filter_from_file, filter_to_file, FilterInstanceFile};
use crate::types::PaletteId;
use engine_registry::AlgorithmRegistry;

/// Wire format version for preset JSON files.
pub const PRESET_VERSION: u32 = 1;

/// Errors from preset parse / validate / apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetError {
    /// JSON / schema problems.
    Invalid(String),
    /// `algorithm_id` not in the registry (reject on save / strict validate).
    UnknownAlgorithm(String),
    /// Unsupported `preset_version`.
    UnsupportedVersion(u32),
}

impl std::fmt::Display for PresetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(m) => write!(f, "{m}"),
            Self::UnknownAlgorithm(id) => write!(f, "unknown algorithm_id: {id}"),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported preset_version: {v} (expected {PRESET_VERSION})")
            }
        }
    }
}

impl std::error::Error for PresetError {}

/// On-disk / IPC preset document (`presets/*.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterPreset {
    pub preset_version: u32,
    pub id: String,
    pub name: String,
    /// ISO-8601 / RFC3339 timestamp string.
    pub created_at: String,
    /// Filter instances in stack order (same shape as `document.json`).
    pub filters: Vec<FilterInstanceFile>,
    /// Palette ids referenced by filter params (not embedded colors).
    #[serde(default)]
    pub palette_ids: Vec<u32>,
}

/// Result of loading/applying a preset (tolerant of retired algorithms).
#[derive(Debug, Clone)]
pub struct PresetApplyResult {
    pub filters: Vec<FilterInstance>,
    /// Human-readable warnings (skipped retired filters, missing palettes, …).
    pub warnings: Vec<String>,
    /// Palette ids from the preset that are absent in the target document.
    pub missing_palette_ids: Vec<u32>,
}

impl FilterPreset {
    /// Build a preset from a live filter stack.
    ///
    /// Unknown `algorithm_id` values → [`PresetError::UnknownAlgorithm`] (save must not
    /// persist recipes the current app cannot name).
    pub fn from_stack(
        id: impl Into<String>,
        name: impl Into<String>,
        created_at: impl Into<String>,
        filters: &[FilterInstance],
        registry: Option<&AlgorithmRegistry>,
    ) -> Result<Self, PresetError> {
        let registry = registry.unwrap_or_else(|| builtin_registry());
        let mut files = Vec::with_capacity(filters.len());
        let mut palette_ids = HashSet::new();

        for f in filters {
            if let Some(aid) = f.algorithm_id.as_deref() {
                if registry.get_by_str(aid).is_none() {
                    return Err(PresetError::UnknownAlgorithm(aid.to_string()));
                }
            }
            let file = filter_to_file(f);
            collect_palette_ids_from_params(&file.params, &mut palette_ids);
            files.push(file);
        }

        let mut ids: Vec<u32> = palette_ids.into_iter().collect();
        ids.sort_unstable();

        Ok(Self {
            preset_version: PRESET_VERSION,
            id: id.into(),
            name: name.into(),
            created_at: created_at.into(),
            filters: files,
            palette_ids: ids,
        })
    }

    /// Parse + validate version. Does **not** reject retired algorithm ids
    /// (migration path uses [`Self::apply_tolerant`]).
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self, PresetError> {
        let preset: Self = serde_json::from_slice(bytes)
            .map_err(|e| PresetError::Invalid(format!("preset JSON: {e}")))?;
        if preset.preset_version != PRESET_VERSION {
            return Err(PresetError::UnsupportedVersion(preset.preset_version));
        }
        if preset.id.trim().is_empty() {
            return Err(PresetError::Invalid("preset id must not be empty".into()));
        }
        if preset.name.trim().is_empty() {
            return Err(PresetError::Invalid("preset name must not be empty".into()));
        }
        Ok(preset)
    }

    pub fn to_json_pretty(&self) -> Result<String, PresetError> {
        serde_json::to_string_pretty(self)
            .map_err(|e| PresetError::Invalid(format!("serialize preset: {e}")))
    }

    /// Strict validate: every `algorithm_id` must be known (used before save).
    pub fn validate_known_algorithms(
        &self,
        registry: Option<&AlgorithmRegistry>,
    ) -> Result<(), PresetError> {
        let registry = registry.unwrap_or_else(|| builtin_registry());
        for f in &self.filters {
            if let Some(id) = f.algorithm_id.as_deref() {
                if registry.get_by_str(id).is_none() {
                    return Err(PresetError::UnknownAlgorithm(id.to_string()));
                }
            }
        }
        Ok(())
    }

    /// Load filters for apply: skip retired/unknown algorithms with a warning
    /// (never panic). Missing palettes are kept in params and reported.
    pub fn apply_tolerant(
        &self,
        available_palette_ids: &HashSet<u32>,
        registry: Option<&AlgorithmRegistry>,
    ) -> PresetApplyResult {
        let registry = registry.unwrap_or_else(|| builtin_registry());
        let mut warnings = Vec::new();
        let mut out = Vec::with_capacity(self.filters.len());
        let mut missing = HashSet::new();

        for f in &self.filters {
            if let Some(id) = f.algorithm_id.as_deref() {
                if registry.get_by_str(id).is_none() {
                    warnings.push(format!(
                        "Skipped filter with retired or unknown algorithm_id '{id}'"
                    ));
                    continue;
                }
            }
            let inst = filter_from_file(f, Some(registry));
            // Placeholder from unknown id should not appear after the skip above,
            // but keep the guard for legacy files without algorithm_id that fail decode.
            if matches!(inst.params, FilterParams::Placeholder(_)) && inst.algorithm_id.is_some() {
                if let Some(id) = inst.algorithm_id.as_deref() {
                    warnings.push(format!(
                        "Skipped filter with retired or unknown algorithm_id '{id}'"
                    ));
                    continue;
                }
            }
            collect_missing_palettes(&inst.params, available_palette_ids, &mut missing);
            out.push(inst);
        }

        let mut missing_palette_ids: Vec<u32> = missing.into_iter().collect();
        missing_palette_ids.sort_unstable();
        for pid in &missing_palette_ids {
            warnings.push(format!(
                "Palette {pid} not found — pick another"
            ));
        }

        PresetApplyResult {
            filters: out,
            warnings,
            missing_palette_ids,
        }
    }
}

fn collect_palette_ids_from_params(params: &serde_json::Value, out: &mut HashSet<u32>) {
    if let Ok(FilterParams::DitherV2(p)) = serde_json::from_value::<FilterParams>(params.clone()) {
        if let Some(PaletteId(id)) = p.palette_id {
            out.insert(id);
        }
    }
    if let Ok(FilterParams::PaletteQuantize { palette_id, .. }) =
        serde_json::from_value::<FilterParams>(params.clone())
    {
        out.insert(palette_id.0);
    }
}

fn collect_missing_palettes(
    params: &FilterParams,
    available: &HashSet<u32>,
    missing: &mut HashSet<u32>,
) {
    match params {
        FilterParams::DitherV2(p) => {
            if let Some(PaletteId(id)) = p.palette_id {
                if !available.contains(&id) {
                    missing.insert(id);
                }
            }
        }
        FilterParams::PaletteQuantize { palette_id, .. }
            if !available.contains(&palette_id.0) =>
        {
            missing.insert(palette_id.0);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherModeV2, DitherParamsV2, FilterKind};
    use crate::types::FilterInstanceId;

    fn fs_filter(palette: Option<u32>) -> FilterInstance {
        let mut p = DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            ..DitherParamsV2::default()
        };
        p.palette_id = palette.map(PaletteId);
        let mut inst = FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(p));
        inst.id = FilterInstanceId::new();
        inst.algorithm_id = Some("floyd_steinberg".into());
        inst.schema_version = Some(1);
        inst
    }

    #[test]
    fn save_load_round_trip() {
        let stack = vec![fs_filter(Some(7))];
        let preset = FilterPreset::from_stack(
            "test_abcd",
            "My FS",
            "2026-01-01T00:00:00Z",
            &stack,
            None,
        )
        .unwrap();
        assert_eq!(preset.preset_version, 1);
        assert_eq!(preset.palette_ids, vec![7]);
        let json = preset.to_json_pretty().unwrap();
        let loaded = FilterPreset::from_json_slice(json.as_bytes()).unwrap();
        assert_eq!(loaded.id, preset.id);
        assert_eq!(loaded.name, preset.name);
        assert_eq!(loaded.filters.len(), preset.filters.len());
        assert_eq!(loaded.palette_ids, preset.palette_ids);
        assert_eq!(loaded.filters[0].algorithm_id, preset.filters[0].algorithm_id);
    }

    #[test]
    fn apply_produces_same_stack() {
        let stack = vec![fs_filter(None)];
        let preset =
            FilterPreset::from_stack("id1", "n", "t", &stack, None).unwrap();
        let available = HashSet::new();
        let result = preset.apply_tolerant(&available, None);
        assert!(result.warnings.is_empty());
        assert_eq!(result.filters.len(), 1);
        assert_eq!(
            result.filters[0].algorithm_id.as_deref(),
            Some("floyd_steinberg")
        );
        match &result.filters[0].params {
            FilterParams::DitherV2(p) => {
                assert!(matches!(p.mode, DitherModeV2::FloydSteinberg));
                assert_eq!(p.levels, 4);
            }
            other => panic!("unexpected params: {other:?}"),
        }
    }

    #[test]
    fn unknown_algorithm_on_save_is_typed_error() {
        let mut inst = fs_filter(None);
        inst.algorithm_id = Some("retired_algo_xyz".into());
        let err = FilterPreset::from_stack("id", "n", "t", &[inst], None).unwrap_err();
        assert!(matches!(err, PresetError::UnknownAlgorithm(id) if id == "retired_algo_xyz"));
    }

    #[test]
    fn missing_palette_keeps_binding_and_warns() {
        let stack = vec![fs_filter(Some(99))];
        let preset = FilterPreset::from_stack("id", "n", "t", &stack, None).unwrap();
        let available = HashSet::new(); // doc has no palettes
        let result = preset.apply_tolerant(&available, None);
        assert_eq!(result.missing_palette_ids, vec![99]);
        assert!(result.warnings.iter().any(|w| w.contains("Palette 99 not found")));
        match &result.filters[0].params {
            FilterParams::DitherV2(p) => {
                assert_eq!(p.palette_id, Some(PaletteId(99)));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn migration_skips_retired_algorithm() {
        let good = filter_to_file(&fs_filter(None));
        let mut bad = good.clone();
        bad.algorithm_id = Some("gone_algo".into());
        let preset = FilterPreset {
            preset_version: 1,
            id: "mig".into(),
            name: "mig".into(),
            created_at: "t".into(),
            filters: vec![bad, good],
            palette_ids: vec![],
        };
        let result = preset.apply_tolerant(&HashSet::new(), None);
        assert_eq!(result.filters.len(), 1);
        assert_eq!(
            result.filters[0].algorithm_id.as_deref(),
            Some("floyd_steinberg")
        );
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("gone_algo")));
    }
}
