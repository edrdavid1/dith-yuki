//! `.dyuki` sharable pattern pack / unpack (Track F).
//!
//! Reuses E0 zip + content-addressed threshold-map embedding. File DTOs replace
//! live `palette_id` with Placeholder_Key (`p0`, `p1`, …) and CustomPng paths
//! with `{content_hash}.png` basenames. Import always creates new ids and appends.

use crate::document::Document;
use crate::filter::{DitherModeV2, FilterInstance, FilterKind, FilterParams};
use crate::layer::{Layer, LayerNode};
use crate::serialize::archive::{create_zip, ZipArchiveReader};
use crate::serialize::assets::{
    content_hash, materialize_threshold_map, parse_threshold_basename, threshold_map_basename,
    threshold_map_zip_entry,
};
use crate::serialize::migrate::{
    migrate_dyuki, ArchiveKind, ProjectError, SUPPORTED_DYUKI_VERSION,
};
use crate::serialize::project::{chrono_like_now, read_threshold_png_for_save};
use crate::types::{BlendMode, FilterInstanceId, LayerId, PaletteId};

fn default_filter_opacity() -> f32 {
    1.0
}
use engine_color::palette::{LinearColor, Palette};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

/// `manifest.json` for `.dyuki` (distinct from `.dyproj` [`super::migrate::Manifest`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PatternManifest {
    pub format_version: u32,
    pub kind: ArchiveKind,
    pub app_version_min: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
}

/// File-only filter instance: no runtime id, no `requires_full_row`.
/// `params` is JSON with `palette_ref` placeholders (not live `palette_id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternFilterFile {
    pub kind: FilterKind,
    pub params: serde_json::Value,
    pub enabled: bool,
    #[serde(default = "default_filter_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub blend_mode: BlendMode,
}

/// Palette payload in `palettes.json` (same fields as `add_palette`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PalettePayload {
    pub name: String,
    pub colors: Vec<LinearColor>,
}

/// Optional metadata written into the pattern manifest on export.
#[derive(Debug, Clone, Default)]
pub struct PatternExportMeta {
    pub name: String,
    pub description: Option<String>,
    pub author: Option<String>,
}

/// Result of appending an imported pattern onto a layer.
#[derive(Debug, Clone)]
pub struct ImportPatternResult {
    pub filter_ids: Vec<FilterInstanceId>,
    pub palette_ids: Vec<PaletteId>,
}

/// Unpacked archive after version checks and threshold-map materialize (no doc mutation).
#[derive(Debug, Clone)]
pub struct UnpackedPattern {
    pub manifest: PatternManifest,
    pub filters: Vec<PatternFilterFile>,
    pub palettes: BTreeMap<String, PalettePayload>,
    pub basename_to_synth: HashMap<String, PathBuf>,
}

// ─── app_version_min policy ──────────────────────────────────────────────────

/// Semver triple `major.minor.patch` (pre-release suffix ignored).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct VersionTriple(u32, u32, u32);

fn parse_version(s: &str) -> Result<VersionTriple, ProjectError> {
    let core = s.split('-').next().unwrap_or(s).trim();
    let mut parts = core.split('.');
    let major: u32 = parts
        .next()
        .unwrap_or("0")
        .parse()
        .map_err(|_| ProjectError::InvalidArchive(format!("invalid semver: {s}")))?;
    let minor: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    let patch: u32 = parts.next().unwrap_or("0").parse().unwrap_or(0);
    Ok(VersionTriple(major, minor, patch))
}

fn format_version_triple(v: VersionTriple) -> String {
    format!("{}.{}.{}", v.0, v.1, v.2)
}

/// Kind → first-shipped app version. Exhaustive so new variants fail to compile
/// until the table is updated. Fallback for incomplete entries is `current_app`.
fn min_version_for_kind(kind: FilterKind) -> Option<&'static str> {
    match kind {
        FilterKind::Curves
        | FilterKind::Levels
        | FilterKind::Dither
        | FilterKind::PaletteQuantize
        | FilterKind::Glitch
        | FilterKind::Placeholder
        | FilterKind::Glow
        | FilterKind::Crt => Some("0.1.0"),
        FilterKind::Adjust => Some("0.2.0"),
    }
}

fn min_version_for_dither_mode(mode: &DitherModeV2) -> Option<&'static str> {
    match mode {
        DitherModeV2::Bayer2x2
        | DitherModeV2::Bayer4x4
        | DitherModeV2::Bayer8x8
        | DitherModeV2::CustomPng { .. }
        | DitherModeV2::FloydSteinberg
        | DitherModeV2::Atkinson
        | DitherModeV2::JarvisJudiceNinke
        | DitherModeV2::Stucki
        | DitherModeV2::Burkes
        | DitherModeV2::Sierra
        | DitherModeV2::CmykHalftone
        | DitherModeV2::Wave => Some("0.1.0"),
    }
}

fn version_for_filter(filter: &FilterInstance, current_app: &str) -> VersionTriple {
    let mut best = parse_version("0.0.0").unwrap_or(VersionTriple(0, 0, 0));
    let kind_v = min_version_for_kind(filter.kind).unwrap_or(current_app);
    if let Ok(t) = parse_version(kind_v) {
        best = best.max(t);
    }
    if let FilterParams::DitherV2(p) = &filter.params {
        let mode_v = min_version_for_dither_mode(&p.mode).unwrap_or(current_app);
        if let Ok(t) = parse_version(mode_v) {
            best = best.max(t);
        }
    }
    best
}

/// Highest `app_version_min` required by the included filter kinds/modes.
///
/// If the per-kind table has no entry, uses `current_app` (safe/strict).
pub fn min_app_version_for_filters(filters: &[FilterInstance], current_app: &str) -> String {
    if filters.is_empty() {
        return current_app.to_string();
    }
    let mut best = VersionTriple(0, 0, 0);
    for f in filters {
        best = best.max(version_for_filter(f, current_app));
    }
    format_version_triple(best)
}

/// Fail if the running app is older than `app_version_min`.
pub fn check_app_version_min(required: &str, running: &str) -> Result<(), ProjectError> {
    let req = parse_version(required)?;
    let run = parse_version(running)?;
    if run < req {
        return Err(ProjectError::AppVersionTooOld {
            required: required.to_string(),
            running: running.to_string(),
        });
    }
    Ok(())
}

// ─── palette / CustomPng collection ──────────────────────────────────────────

fn collect_palette_ids(filters: &[FilterInstance]) -> Vec<PaletteId> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    for f in filters {
        match &f.params {
            FilterParams::PaletteQuantize { palette_id, .. } => {
                if seen.insert(*palette_id) {
                    order.push(*palette_id);
                }
            }
            FilterParams::DitherV2(p) => {
                if let Some(id) = p.palette_id {
                    if seen.insert(id) {
                        order.push(id);
                    }
                }
            }
            _ => {}
        }
    }
    order
}

fn collect_custom_png_paths(filters: &[FilterInstance]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut order = Vec::new();
    for f in filters {
        match &f.params {
            FilterParams::DitherV2(p) => {
                if let DitherModeV2::CustomPng { path } = &p.mode {
                    if seen.insert(path.clone()) {
                        order.push(path.clone());
                    }
                }
            }
            FilterParams::Dither {
                mode: crate::filter::DitherMode::ThresholdMap { path },
                ..
            } => {
                if seen.insert(path.clone()) {
                    order.push(path.clone());
                }
            }
            _ => {}
        }
    }
    order
}

fn rewrite_params_for_file(
    params: &FilterParams,
    palette_to_key: &HashMap<u32, String>,
    path_to_basename: &HashMap<String, String>,
) -> Result<serde_json::Value, ProjectError> {
    let mut v = serde_json::to_value(params).map_err(|e| ProjectError::Codec(e.to_string()))?;
    rewrite_value_export(&mut v, palette_to_key, path_to_basename);
    Ok(v)
}

fn rewrite_value_export(
    v: &mut serde_json::Value,
    palette_to_key: &HashMap<u32, String>,
    path_to_basename: &HashMap<String, String>,
) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(pid) = map.remove("palette_id") {
                if let Some(n) = pid.as_u64() {
                    if let Some(key) = palette_to_key.get(&(n as u32)) {
                        map.insert(
                            "palette_ref".into(),
                            serde_json::Value::String(key.clone()),
                        );
                    }
                }
            }
            if let Some(custom) = map.get_mut("custom_png") {
                if let Some(obj) = custom.as_object_mut() {
                    if let Some(path) = obj.get("path").and_then(|p| p.as_str()) {
                        if let Some(b) = path_to_basename.get(path) {
                            obj.insert("path".into(), serde_json::Value::String(b.clone()));
                        } else {
                            // Already a basename, keep filename only.
                            let name = Path::new(path)
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.to_string());
                            obj.insert("path".into(), serde_json::Value::String(name));
                        }
                    }
                }
            }
            for val in map.values_mut() {
                rewrite_value_export(val, palette_to_key, path_to_basename);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                rewrite_value_export(item, palette_to_key, path_to_basename);
            }
        }
        _ => {}
    }
}

fn collect_palette_refs(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(r) = map.get("palette_ref").and_then(|x| x.as_str()) {
                out.push(r.to_string());
            }
            for val in map.values() {
                collect_palette_refs(val, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_palette_refs(item, out);
            }
        }
        _ => {}
    }
}

fn collect_embed_basenames(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(custom) = map.get("custom_png") {
                if let Some(path) = custom.get("path").and_then(|p| p.as_str()) {
                    let name = Path::new(path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| path.to_string());
                    out.push(name);
                }
            }
            for val in map.values() {
                collect_embed_basenames(val, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_embed_basenames(item, out);
            }
        }
        _ => {}
    }
}

/// Stub palette_ref / CustomPng so serde can reject unknown FilterParams / modes
/// before any document mutation.
fn validate_params_shape(params: &serde_json::Value) -> Result<(), ProjectError> {
    let mut stub = params.clone();
    stub_refs_for_validate(&mut stub);
    serde_json::from_value::<FilterParams>(stub).map_err(|e| {
        ProjectError::InvalidArchive(format!(
            "unknown or invalid filter kind/mode (update the app): {e}"
        ))
    })?;
    Ok(())
}

fn stub_refs_for_validate(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(map) => {
            if map.contains_key("palette_ref") {
                map.remove("palette_ref");
                map.insert("palette_id".into(), serde_json::json!(1));
            }
            if let Some(custom) = map.get_mut("custom_png") {
                if let Some(obj) = custom.as_object_mut() {
                    obj.insert("path".into(), serde_json::json!("stub.png"));
                }
            }
            for val in map.values_mut() {
                stub_refs_for_validate(val);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                stub_refs_for_validate(item);
            }
        }
        _ => {}
    }
}

fn rewrite_params_from_file(
    mut v: serde_json::Value,
    key_to_palette: &HashMap<String, PaletteId>,
    basename_to_synth: &HashMap<String, PathBuf>,
) -> Result<FilterParams, ProjectError> {
    rewrite_value_import(&mut v, key_to_palette, basename_to_synth)?;
    serde_json::from_value(v).map_err(|e| {
        ProjectError::InvalidArchive(format!(
            "unknown or invalid filter kind/mode (update the app): {e}"
        ))
    })
}

fn rewrite_value_import(
    v: &mut serde_json::Value,
    key_to_palette: &HashMap<String, PaletteId>,
    basename_to_synth: &HashMap<String, PathBuf>,
) -> Result<(), ProjectError> {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(r) = map.remove("palette_ref") {
                let key = r
                    .as_str()
                    .ok_or_else(|| {
                        ProjectError::InvalidArchive("palette_ref must be a string".into())
                    })?
                    .to_string();
                let id = key_to_palette
                    .get(&key)
                    .ok_or_else(|| ProjectError::MissingPalettePlaceholder(key))?;
                map.insert("palette_id".into(), serde_json::json!(id.0));
            }
            if let Some(custom) = map.get_mut("custom_png") {
                if let Some(obj) = custom.as_object_mut() {
                    if let Some(path) = obj.get("path").and_then(|p| p.as_str()) {
                        let key = Path::new(path)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.to_string());
                        let synth = basename_to_synth.get(&key).ok_or_else(|| {
                            ProjectError::UnresolvedCustomPng(key.clone())
                        })?;
                        obj.insert(
                            "path".into(),
                            serde_json::Value::String(synth.to_string_lossy().into_owned()),
                        );
                    }
                }
            }
            let children: Vec<String> = map.keys().cloned().collect();
            for k in children {
                if let Some(val) = map.get_mut(&k) {
                    rewrite_value_import(val, key_to_palette, basename_to_synth)?;
                }
            }
            Ok(())
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                rewrite_value_import(item, key_to_palette, basename_to_synth)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// ─── layer lookup ────────────────────────────────────────────────────────────

enum LayerLookup {
    Leaf,
    Group,
    Missing,
}

fn lookup_layer(nodes: &[LayerNode], id: LayerId) -> LayerLookup {
    for node in nodes {
        match node {
            LayerNode::Leaf(l) if l.id == id => return LayerLookup::Leaf,
            LayerNode::Group(g) if g.id == id => return LayerLookup::Group,
            LayerNode::Group(g) => match lookup_layer(&g.children, id) {
                LayerLookup::Missing => {}
                other => return other,
            },
            _ => {}
        }
    }
    LayerLookup::Missing
}

fn find_leaf<'a>(nodes: &'a [LayerNode], id: LayerId) -> Result<&'a Layer, ProjectError> {
    match lookup_layer(nodes, id) {
        LayerLookup::Leaf => {}
        LayerLookup::Group => return Err(ProjectError::TargetIsGroup),
        LayerLookup::Missing => return Err(ProjectError::LayerNotFound(id.0)),
    }
    fn walk<'a>(nodes: &'a [LayerNode], id: LayerId) -> Option<&'a Layer> {
        for node in nodes {
            match node {
                LayerNode::Leaf(l) if l.id == id => return Some(l),
                LayerNode::Group(g) => {
                    if let Some(found) = walk(&g.children, id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(nodes, id).ok_or(ProjectError::LayerNotFound(id.0))
}

fn find_leaf_mut<'a>(
    nodes: &'a mut [LayerNode],
    id: LayerId,
) -> Result<&'a mut Layer, ProjectError> {
    match lookup_layer(nodes, id) {
        LayerLookup::Leaf => {}
        LayerLookup::Group => return Err(ProjectError::TargetIsGroup),
        LayerLookup::Missing => return Err(ProjectError::LayerNotFound(id.0)),
    }
    fn walk<'a>(nodes: &'a mut [LayerNode], id: LayerId) -> Option<&'a mut Layer> {
        for node in nodes {
            match node {
                LayerNode::Leaf(l) if l.id == id => return Some(l),
                LayerNode::Group(g) => {
                    if let Some(found) = walk(&mut g.children, id) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(nodes, id).ok_or(ProjectError::LayerNotFound(id.0))
}

fn select_filters(
    layer: &Layer,
    filter_instance_ids: Option<&[FilterInstanceId]>,
) -> Result<Vec<FilterInstance>, ProjectError> {
    match filter_instance_ids {
        None => Ok(layer.filters.clone()),
        Some(ids) if ids.is_empty() => Ok(layer.filters.clone()),
        Some(ids) => {
            let wanted: HashSet<FilterInstanceId> = ids.iter().copied().collect();
            for id in ids {
                if !layer.filters.iter().any(|f| f.id == *id) {
                    return Err(ProjectError::FilterNotFound(id.to_string()));
                }
            }
            Ok(layer
                .filters
                .iter()
                .filter(|f| wanted.contains(&f.id))
                .cloned()
                .collect())
        }
    }
}

// ─── pack / unpack ───────────────────────────────────────────────────────────

/// Serialize a filter list + referenced palettes/maps into `.dyuki` zip bytes.
pub fn pack_pattern_to_bytes(
    filters: &[FilterInstance],
    palettes: &[Palette],
    meta: &PatternExportMeta,
    running_app_version: &str,
    mut read_png: impl FnMut(&str) -> Result<Vec<u8>, ProjectError>,
) -> Result<Vec<u8>, ProjectError> {
    if filters.is_empty() {
        return Err(ProjectError::EmptyExport);
    }

    let palette_ids = collect_palette_ids(filters);
    let mut palette_to_key: HashMap<u32, String> = HashMap::new();
    let mut palettes_file: BTreeMap<String, PalettePayload> = BTreeMap::new();
    for (i, pid) in palette_ids.iter().enumerate() {
        let key = format!("p{i}");
        let pal = palettes
            .iter()
            .find(|p| p.id == pid.0)
            .ok_or(ProjectError::MissingPalette(pid.0))?;
        palettes_file.insert(
            key.clone(),
            PalettePayload {
                name: pal.name.clone(),
                colors: pal.colors.clone(),
            },
        );
        palette_to_key.insert(pid.0, key);
    }

    let mut path_to_basename: HashMap<String, String> = HashMap::new();
    let mut embeds: HashMap<String, Vec<u8>> = HashMap::new();
    for path in collect_custom_png_paths(filters) {
        if path_to_basename.contains_key(&path) {
            continue;
        }
        let bytes = read_threshold_png_for_save(&path, &mut read_png)?;
        let basename = threshold_map_basename(&bytes);
        embeds.insert(basename.clone(), bytes);
        path_to_basename.insert(path, basename);
    }

    let file_filters: Vec<PatternFilterFile> = filters
        .iter()
        .map(|f| {
            Ok(PatternFilterFile {
                kind: f.kind,
                params: rewrite_params_for_file(&f.params, &palette_to_key, &path_to_basename)?,
                enabled: f.enabled,
                opacity: f.opacity,
                blend_mode: f.blend_mode,
            })
        })
        .collect::<Result<_, ProjectError>>()?;

    let manifest = PatternManifest {
        format_version: SUPPORTED_DYUKI_VERSION,
        kind: ArchiveKind::Dyuki,
        app_version_min: min_app_version_for_filters(filters, running_app_version),
        name: if meta.name.is_empty() {
            "Pattern".into()
        } else {
            meta.name.clone()
        },
        description: meta.description.clone(),
        author: meta.author.clone(),
        created_at: chrono_like_now(),
    };

    let manifest_json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| ProjectError::Codec(e.to_string()))?;
    let filters_json =
        serde_json::to_vec_pretty(&file_filters).map_err(|e| ProjectError::Codec(e.to_string()))?;
    let palettes_json =
        serde_json::to_vec_pretty(&palettes_file).map_err(|e| ProjectError::Codec(e.to_string()))?;

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    entries.push(("manifest.json".into(), manifest_json));
    entries.push(("filters.json".into(), filters_json));
    entries.push(("palettes.json".into(), palettes_json));
    for (basename, bytes) in &embeds {
        entries.push((threshold_map_zip_entry(basename), bytes.clone()));
    }

    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    create_zip(&refs).map_err(|e| ProjectError::Io(e.to_string()))
}

/// Open a `.dyuki`, migrate, enforce `app_version_min`, materialize maps.
/// Does not mutate a document.
pub fn unpack_pattern_from_bytes(
    zip_bytes: &[u8],
    running_app_version: &str,
) -> Result<UnpackedPattern, ProjectError> {
    let mut reader =
        ZipArchiveReader::open(zip_bytes).map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;

    let manifest_bytes = reader
        .read_entry("manifest.json")
        .map_err(|_| ProjectError::MissingEntry("manifest.json".into()))?;
    let manifest: PatternManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;

    if manifest.kind != ArchiveKind::Dyuki {
        return Err(ProjectError::KindMismatch {
            expected: "dyuki".into(),
            found: manifest.kind.as_str().to_string(),
        });
    }

    let filters_bytes = reader
        .read_entry("filters.json")
        .map_err(|_| ProjectError::MissingEntry("filters.json".into()))?;
    let palettes_bytes = reader
        .read_entry("palettes.json")
        .map_err(|_| ProjectError::MissingEntry("palettes.json".into()))?;

    let filters_value: serde_json::Value = serde_json::from_slice(&filters_bytes)
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;
    let palettes_value: serde_json::Value = serde_json::from_slice(&palettes_bytes)
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;

    let combined = serde_json::json!({
        "filters": filters_value,
        "palettes": palettes_value,
    });
    let combined = migrate_dyuki(manifest.format_version, combined)?;
    let filters_value = combined
        .get("filters")
        .cloned()
        .ok_or_else(|| ProjectError::InvalidArchive("migrated payload missing filters".into()))?;
    let palettes_value = combined
        .get("palettes")
        .cloned()
        .ok_or_else(|| ProjectError::InvalidArchive("migrated payload missing palettes".into()))?;

    check_app_version_min(&manifest.app_version_min, running_app_version)?;

    let filters: Vec<PatternFilterFile> = serde_json::from_value(filters_value).map_err(|e| {
        ProjectError::InvalidArchive(format!(
            "unknown or invalid filter kind/mode (update the app): {e}"
        ))
    })?;
    let palettes: BTreeMap<String, PalettePayload> = serde_json::from_value(palettes_value)
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;

    for f in &filters {
        validate_params_shape(&f.params)?;
        let mut refs = Vec::new();
        collect_palette_refs(&f.params, &mut refs);
        for r in refs {
            if !palettes.contains_key(&r) {
                return Err(ProjectError::MissingPalettePlaceholder(r));
            }
        }
    }

    let mut needed = Vec::new();
    for f in &filters {
        collect_embed_basenames(&f.params, &mut needed);
    }
    needed.sort();
    needed.dedup();

    let mut basename_to_synth: HashMap<String, PathBuf> = HashMap::new();
    for basename in needed {
        let stem = parse_threshold_basename(&basename).map_err(|e| {
            ProjectError::InvalidArchive(format!("invalid CustomPng basename '{basename}': {e}"))
        })?;
        let entry = threshold_map_zip_entry(&basename);
        let bytes = reader
            .read_entry(&entry)
            .map_err(|_| ProjectError::MissingEntry(entry.clone()))?;
        let actual = content_hash(&bytes);
        if actual != stem {
            return Err(ProjectError::HashMismatch {
                entry: basename.clone(),
                actual,
            });
        }
        let path = materialize_threshold_map(&bytes).map_err(|e| ProjectError::Io(e.to_string()))?;
        basename_to_synth.insert(basename, path);
    }

    Ok(UnpackedPattern {
        manifest,
        filters,
        palettes,
        basename_to_synth,
    })
}

/// Export selected (or all) filters on a leaf layer.
pub fn export_pattern_from_document(
    doc: &Document,
    layer_id: LayerId,
    filter_instance_ids: Option<&[FilterInstanceId]>,
    meta: &PatternExportMeta,
    running_app_version: &str,
    read_png: impl FnMut(&str) -> Result<Vec<u8>, ProjectError>,
) -> Result<Vec<u8>, ProjectError> {
    let layer = find_leaf(&doc.root, layer_id)?;
    let filters = select_filters(layer, filter_instance_ids)?;
    pack_pattern_to_bytes(
        &filters,
        &doc.palettes,
        meta,
        running_app_version,
        read_png,
    )
}

/// Import: new palettes + new filters, append to a leaf layer. No-op on groups.
pub fn import_pattern_into_document(
    zip_bytes: &[u8],
    doc: &mut Document,
    target_layer_id: LayerId,
    running_app_version: &str,
) -> Result<ImportPatternResult, ProjectError> {
    let unpacked = unpack_pattern_from_bytes(zip_bytes, running_app_version)?;

    match lookup_layer(&doc.root, target_layer_id) {
        LayerLookup::Leaf => {}
        LayerLookup::Group => return Err(ProjectError::TargetIsGroup),
        LayerLookup::Missing => return Err(ProjectError::LayerNotFound(target_layer_id.0)),
    }

    let mut key_to_palette: HashMap<String, PaletteId> = HashMap::new();
    let mut palette_ids = Vec::new();
    for (key, payload) in &unpacked.palettes {
        let id = doc.add_palette(payload.name.clone(), payload.colors.clone());
        key_to_palette.insert(key.clone(), id);
        palette_ids.push(id);
    }

    let mut new_filters = Vec::new();
    for file in &unpacked.filters {
        let params = rewrite_params_from_file(
            file.params.clone(),
            &key_to_palette,
            &unpacked.basename_to_synth,
        )?;
        let mut inst = FilterInstance::new(file.kind, params);
        inst.enabled = file.enabled;
        inst.opacity = file.opacity;
        inst.blend_mode = file.blend_mode;
        inst.validate()
            .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;
        new_filters.push(inst);
    }

    let layer = find_leaf_mut(&mut doc.root, target_layer_id)?;
    let mut filter_ids = Vec::new();
    for inst in new_filters {
        filter_ids.push(inst.id);
        layer.filters.push(inst);
    }

    Ok(ImportPatternResult {
        filter_ids,
        palette_ids,
    })
}

/// Write packed bytes to a filesystem path.
pub fn write_pattern_to_path(path: &Path, zip_bytes: &[u8]) -> Result<(), ProjectError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ProjectError::Io(e.to_string()))?;
    }
    std::fs::write(path, zip_bytes).map_err(|e| ProjectError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherColorMode, DitherParamsV2};
    use crate::layer::LayerGroup;
    use crate::serialize::archive::read_zip_entry;
    use crate::serialize::project::read_png_file;
    use crate::types::{DocumentId, LayerKind};
    use crate::filters::apply::apply_filter_to_tile;
    use engine_color::palette_cache::PaletteKdCache;
    use engine_color::palette_lut::PaletteLutCache;
    use engine_color::threshold_map::ThresholdMapCache;
    use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

    fn grayscale_png() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut buf, 2, 2);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 85, 170, 255]).unwrap();
        }
        buf
    }

    fn make_gradient_tile() -> PixelTile {
        let mut tile = PixelTile::new();
        let full_size = TILE_SIZE + 2 * HALO;
        for y in 0..full_size {
            for x in 0..full_size {
                let val = x as f32 / full_size as f32;
                tile.set(x, y, 0, val);
                tile.set(x, y, 1, val * 0.7);
                tile.set(x, y, 2, 1.0 - val);
                tile.set(x, y, 3, 1.0);
            }
        }
        tile
    }

    fn bayer_with_palette(palette_id: PaletteId) -> FilterInstance {
        FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                palette_id: Some(palette_id),
                color_mode: DitherColorMode::Rgb,
                ..DitherParamsV2::default()
            }),
        )
    }

    fn custom_png_filter(path: String, palette_id: Option<PaletteId>) -> FilterInstance {
        FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::CustomPng { path },
                levels: 2,
                palette_id,
                ..DitherParamsV2::default()
            }),
        )
    }

    fn atkinson_filter() -> FilterInstance {
        FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Atkinson,
                levels: 4,
                ..DitherParamsV2::default()
            }),
        )
    }

    #[test]
    fn pack_unpack_preserves_placeholders_and_enabled() {
        let mut doc = Document::new(DocumentId::new(1), 32, 32);
        let pal = doc.add_palette(
            "Look".into(),
            vec![
                LinearColor {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                },
                LinearColor {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                },
            ],
        );
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 32, 32);
        let mut filt = bayer_with_palette(pal);
        filt.enabled = false;
        layer.filters.push(filt);
        doc.root.push(LayerNode::Leaf(layer));

        let zip = export_pattern_from_document(
            &doc,
            LayerId::new(1),
            None,
            &PatternExportMeta {
                name: "My Look".into(),
                ..Default::default()
            },
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap();

        let filters_json = String::from_utf8(read_zip_entry(&zip, "filters.json").unwrap()).unwrap();
        assert!(filters_json.contains("palette_ref"));
        assert!(filters_json.contains("p0"));
        assert!(!filters_json.contains("palette_id"));
        assert!(!filters_json.contains("requires_full_row"));
        assert!(
            filters_json.contains("\"enabled\": false") || filters_json.contains("\"enabled\":false")
        );
        let palettes_json =
            String::from_utf8(read_zip_entry(&zip, "palettes.json").unwrap()).unwrap();
        assert!(palettes_json.contains("\"p0\""));
        assert!(palettes_json.contains("Look"));

        let mut dest = Document::new(DocumentId::new(1), 32, 32);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(7),
            LayerKind::Raster,
            32,
            32,
        )));
        let preexisting = dest.add_palette(
            "Look".into(),
            vec![LinearColor {
                r: 0.5,
                g: 0.5,
                b: 0.5,
            }],
        );
        let imported =
            import_pattern_into_document(&zip, &mut dest, LayerId::new(7), "0.1.0").unwrap();
        assert_eq!(imported.palette_ids.len(), 1);
        assert_ne!(imported.palette_ids[0], preexisting);
        let dest_pal = dest.get_palette(imported.palette_ids[0]).unwrap();
        assert_eq!(dest_pal.colors.len(), 2);
        match &dest.root[0] {
            LayerNode::Leaf(l) => {
                assert!(!l.filters[0].enabled);
                match &l.filters[0].params {
                    FilterParams::DitherV2(p) => {
                        assert_eq!(p.palette_id, Some(imported.palette_ids[0]));
                    }
                    other => panic!("{other:?}"),
                }
            }
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn no_user_paths_in_zip_json() {
        let png = grayscale_png();
        let dir = tempfile::tempdir().unwrap();
        let user_path = dir.path().join("secret_machine_map.png");
        std::fs::write(&user_path, &png).unwrap();

        let mut doc = Document::new(DocumentId::new(1), 16, 16);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 16, 16);
        layer.filters.push(custom_png_filter(
            user_path.to_string_lossy().into_owned(),
            None,
        ));
        doc.root.push(LayerNode::Leaf(layer));

        let zip = export_pattern_from_document(
            &doc,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |p| read_png_file(p),
        )
        .unwrap();

        let filters_json = String::from_utf8(read_zip_entry(&zip, "filters.json").unwrap()).unwrap();
        assert!(
            !filters_json.contains("secret_machine_map"),
            "{filters_json}"
        );
        assert!(!filters_json.contains(user_path.to_string_lossy().as_ref()));
        let hash_name = threshold_map_basename(&png);
        assert!(filters_json.contains(&hash_name));
    }

    #[test]
    fn missing_filter_id_fails_export() {
        let mut doc = Document::new(DocumentId::new(1), 8, 8);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        layer.filters.push(atkinson_filter());
        doc.root.push(LayerNode::Leaf(layer));
        let missing = FilterInstanceId::new();
        let err = export_pattern_from_document(
            &doc,
            LayerId::new(1),
            Some(&[missing]),
            &PatternExportMeta::default(),
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap_err();
        assert!(matches!(err, ProjectError::FilterNotFound(_)));
    }

    #[test]
    fn app_version_min_too_old_errors_without_mutating() {
        let mut src = Document::new(DocumentId::new(1), 8, 8);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        layer.filters.push(atkinson_filter());
        src.root.push(LayerNode::Leaf(layer));
        let zip = export_pattern_from_document(
            &src,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap();

        // Rewrite manifest with a future min version.
        let mut reader = ZipArchiveReader::open(&zip).unwrap();
        let mut manifest: PatternManifest =
            serde_json::from_slice(&reader.read_entry("manifest.json").unwrap()).unwrap();
        manifest.app_version_min = "99.0.0".into();
        let filters = reader.read_entry("filters.json").unwrap();
        let palettes = reader.read_entry("palettes.json").unwrap();
        let man = serde_json::to_vec(&manifest).unwrap();
        let zip = create_zip(&[
            ("manifest.json", man.as_slice()),
            ("filters.json", filters.as_slice()),
            ("palettes.json", palettes.as_slice()),
        ])
        .unwrap();

        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(1),
            LayerKind::Raster,
            8,
            8,
        )));
        let err = import_pattern_into_document(&zip, &mut dest, LayerId::new(1), "0.1.0").unwrap_err();
        assert!(
            matches!(err, ProjectError::AppVersionTooOld { .. }),
            "{err:?}"
        );
        match &dest.root[0] {
            LayerNode::Leaf(l) => assert!(l.filters.is_empty()),
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn future_format_version_errors() {
        let mut src = Document::new(DocumentId::new(1), 8, 8);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        layer.filters.push(atkinson_filter());
        src.root.push(LayerNode::Leaf(layer));
        let zip = export_pattern_from_document(
            &src,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap();
        let mut reader = ZipArchiveReader::open(&zip).unwrap();
        let mut manifest: PatternManifest =
            serde_json::from_slice(&reader.read_entry("manifest.json").unwrap()).unwrap();
        manifest.format_version = 99;
        let filters = reader.read_entry("filters.json").unwrap();
        let palettes = reader.read_entry("palettes.json").unwrap();
        let man = serde_json::to_vec(&manifest).unwrap();
        let zip = create_zip(&[
            ("manifest.json", man.as_slice()),
            ("filters.json", filters.as_slice()),
            ("palettes.json", palettes.as_slice()),
        ])
        .unwrap();

        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(1),
            LayerKind::Raster,
            8,
            8,
        )));
        let err = import_pattern_into_document(&zip, &mut dest, LayerId::new(1), "0.1.0").unwrap_err();
        assert!(
            matches!(err, ProjectError::UnsupportedVersion { ref kind, .. } if kind == "dyuki"),
            "{err:?}"
        );
        match &dest.root[0] {
            LayerNode::Leaf(l) => assert!(l.filters.is_empty()),
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn unknown_enum_deserialize_errors_with_low_app_version_min() {
        let manifest = PatternManifest {
            format_version: 1,
            kind: ArchiveKind::Dyuki,
            app_version_min: "0.0.1".into(),
            name: "x".into(),
            description: None,
            author: None,
            created_at: "0".into(),
        };
        let man = serde_json::to_vec(&manifest).unwrap();
        let filters = br#"[{"kind":"HyperDither","params":{"Placeholder":"x"},"enabled":true}]"#;
        let palettes = b"{}";
        let zip = create_zip(&[
            ("manifest.json", man.as_slice()),
            ("filters.json", filters.as_slice()),
            ("palettes.json", palettes.as_slice()),
        ])
        .unwrap();

        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(1),
            LayerKind::Raster,
            8,
            8,
        )));
        let err = import_pattern_into_document(&zip, &mut dest, LayerId::new(1), "0.1.0").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown") || msg.contains("update the app") || msg.contains("invalid"),
            "{msg}"
        );
        match &dest.root[0] {
            LayerNode::Leaf(l) => assert!(l.filters.is_empty()),
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn unknown_dither_mode_errors() {
        let manifest = PatternManifest {
            format_version: 1,
            kind: ArchiveKind::Dyuki,
            app_version_min: "0.0.1".into(),
            name: "x".into(),
            description: None,
            author: None,
            created_at: "0".into(),
        };
        let man = serde_json::to_vec(&manifest).unwrap();
        let filters = serde_json::json!([{
            "kind": "Dither",
            "params": {"DitherV2": {"mode": "quantum_foam", "levels": 4}},
            "enabled": true
        }]);
        let filters = serde_json::to_vec(&filters).unwrap();
        let zip = create_zip(&[
            ("manifest.json", man.as_slice()),
            ("filters.json", filters.as_slice()),
            ("palettes.json", b"{}".as_slice()),
        ])
        .unwrap();

        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(1),
            LayerKind::Raster,
            8,
            8,
        )));
        let err = import_pattern_into_document(&zip, &mut dest, LayerId::new(1), "0.1.0").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("unknown") || msg.contains("update the app") || msg.contains("invalid"),
            "{msg}"
        );
        match &dest.root[0] {
            LayerNode::Leaf(l) => assert!(l.filters.is_empty()),
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn import_onto_group_rejected() {
        let mut src = Document::new(DocumentId::new(1), 8, 8);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        layer.filters.push(atkinson_filter());
        src.root.push(LayerNode::Leaf(layer));
        let zip = export_pattern_from_document(
            &src,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap();

        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        let mut group = LayerGroup::new(LayerId::new(9));
        group.children.push(LayerNode::Leaf(Layer::new(
            LayerId::new(2),
            LayerKind::Raster,
            8,
            8,
        )));
        dest.root.push(LayerNode::Group(group));
        let err = import_pattern_into_document(&zip, &mut dest, LayerId::new(9), "0.1.0").unwrap_err();
        assert!(matches!(err, ProjectError::TargetIsGroup), "{err:?}");
        match &dest.root[0] {
            LayerNode::Group(g) => match &g.children[0] {
                LayerNode::Leaf(l) => assert!(l.filters.is_empty()),
                _ => panic!("leaf child"),
            },
            _ => panic!("group"),
        }
    }

    #[test]
    fn double_import_two_independent_stacks() {
        let png = grayscale_png();
        let dir = tempfile::tempdir().unwrap();
        let user_path = dir.path().join("map.png");
        std::fs::write(&user_path, &png).unwrap();

        let mut src = Document::new(DocumentId::new(1), 16, 16);
        let pal = src.add_palette(
            "P".into(),
            vec![LinearColor {
                r: 0.2,
                g: 0.2,
                b: 0.2,
            }],
        );
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 16, 16);
        layer.filters.push(custom_png_filter(
            user_path.to_string_lossy().into_owned(),
            Some(pal),
        ));
        src.root.push(LayerNode::Leaf(layer));
        let zip = export_pattern_from_document(
            &src,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |p| read_png_file(p),
        )
        .unwrap();

        let mut dest = Document::new(DocumentId::new(1), 16, 16);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(3),
            LayerKind::Raster,
            16,
            16,
        )));
        let a = import_pattern_into_document(&zip, &mut dest, LayerId::new(3), "0.1.0").unwrap();
        let b = import_pattern_into_document(&zip, &mut dest, LayerId::new(3), "0.1.0").unwrap();
        assert_ne!(a.filter_ids[0], b.filter_ids[0]);
        assert_ne!(a.palette_ids[0], b.palette_ids[0]);
        match &dest.root[0] {
            LayerNode::Leaf(l) => {
                assert_eq!(l.filters.len(), 2);
                assert_ne!(l.filters[0].id, l.filters[1].id);
            }
            _ => panic!("leaf"),
        }
        // Same hash → one cache file.
        let cache_path = crate::serialize::assets::threshold_maps_cache_dir()
            .unwrap()
            .join(threshold_map_basename(&png));
        assert!(cache_path.exists());
    }

    #[test]
    fn requires_full_row_matches_add_filter() {
        let mut src = Document::new(DocumentId::new(1), 8, 8);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        let fresh = atkinson_filter();
        assert!(fresh.requires_full_row);
        layer.filters.push(fresh);
        src.root.push(LayerNode::Leaf(layer));
        let zip = export_pattern_from_document(
            &src,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap();
        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(1),
            LayerKind::Raster,
            8,
            8,
        )));
        import_pattern_into_document(&zip, &mut dest, LayerId::new(1), "0.1.0").unwrap();
        let expected = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Atkinson,
                levels: 4,
                ..DitherParamsV2::default()
            }),
        );
        match &dest.root[0] {
            LayerNode::Leaf(l) => {
                assert_eq!(l.filters[0].requires_full_row, expected.requires_full_row);
            }
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn check_app_version_min_unit() {
        assert!(check_app_version_min("0.1.0", "0.1.0").is_ok());
        assert!(check_app_version_min("0.1.0", "0.2.0").is_ok());
        let err = check_app_version_min("0.2.0", "0.1.0").unwrap_err();
        assert!(matches!(err, ProjectError::AppVersionTooOld { .. }));
    }

    #[test]
    fn cross_machine_custom_png_and_palette_match() {
        let png = grayscale_png();
        let home = dirs::home_dir().expect("home");
        let user_path = home.join(".dither_yuki_test_dyuki").join("author_only.png");
        std::fs::create_dir_all(user_path.parent().unwrap()).unwrap();
        std::fs::write(&user_path, &png).unwrap();

        let mut src = Document::new(DocumentId::new(1), 64, 64);
        let pal = src.add_palette(
            "Author".into(),
            vec![
                LinearColor {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                },
                LinearColor {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                },
            ],
        );
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        layer.filters.push(custom_png_filter(
            user_path.to_string_lossy().into_owned(),
            Some(pal),
        ));
        src.root.push(LayerNode::Leaf(layer));

        let kd = PaletteKdCache::new();
        let lut = PaletteLutCache::new();
        let thresh = ThresholdMapCache::new();
        let tile = make_gradient_tile();
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let src_layer = match &src.root[0] {
            LayerNode::Leaf(l) => l,
            _ => panic!(),
        };
        let src_out = apply_filter_to_tile(&tile, src_layer, coord, &kd, &lut, &thresh, &src)
            .expect("src apply");

        let zip = export_pattern_from_document(
            &src,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |p| read_png_file(p),
        )
        .unwrap();

        std::fs::remove_file(&user_path).unwrap();
        assert!(!user_path.exists());

        let mut dest = Document::new(DocumentId::new(1), 64, 64);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(4),
            LayerKind::Raster,
            64,
            64,
        )));
        import_pattern_into_document(&zip, &mut dest, LayerId::new(4), "0.1.0").unwrap();

        let dest_layer = match &dest.root[0] {
            LayerNode::Leaf(l) => l,
            _ => panic!(),
        };
        let dest_out =
            apply_filter_to_tile(&tile, dest_layer, coord, &kd, &lut, &thresh, &dest).expect("dest apply");
        assert_eq!(src_out.data.as_ref(), dest_out.data.as_ref());
    }

    #[test]
    fn subset_export_stack_order() {
        let mut doc = Document::new(DocumentId::new(1), 8, 8);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        let a = atkinson_filter();
        let b = FilterInstance::new(
            FilterKind::Crt,
            FilterParams::Crt {
                period: 2,
                strength: 0.5,
                mask_strength: 0.0,
            },
        );
        let c = FilterInstance::new(
            FilterKind::Glow,
            FilterParams::Glow {
                radius: 1.0,
                intensity: 1.0,
                threshold: 0.0,
            },
        );
        let id_a = a.id;
        let id_c = c.id;
        layer.filters.push(a);
        layer.filters.push(b);
        layer.filters.push(c);
        doc.root.push(LayerNode::Leaf(layer));

        // Request C then A (selection order); export must be stack order A then C.
        let zip = export_pattern_from_document(
            &doc,
            LayerId::new(1),
            Some(&[id_c, id_a]),
            &PatternExportMeta::default(),
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap();
        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        dest.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(1),
            LayerKind::Raster,
            8,
            8,
        )));
        import_pattern_into_document(&zip, &mut dest, LayerId::new(1), "0.1.0").unwrap();
        match &dest.root[0] {
            LayerNode::Leaf(l) => {
                assert_eq!(l.filters.len(), 2);
                assert_eq!(l.filters[0].kind, FilterKind::Dither);
                assert_eq!(l.filters[1].kind, FilterKind::Glow);
            }
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn append_does_not_wipe_existing() {
        let mut src = Document::new(DocumentId::new(1), 8, 8);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        layer.filters.push(atkinson_filter());
        src.root.push(LayerNode::Leaf(layer));
        let zip = export_pattern_from_document(
            &src,
            LayerId::new(1),
            None,
            &PatternExportMeta::default(),
            "0.1.0",
            |_| unreachable!(),
        )
        .unwrap();

        let mut dest = Document::new(DocumentId::new(1), 8, 8);
        let mut dest_layer = Layer::new(LayerId::new(1), LayerKind::Raster, 8, 8);
        dest_layer.filters.push(FilterInstance::new(
            FilterKind::Crt,
            FilterParams::Crt {
                period: 2,
                strength: 0.4,
                mask_strength: 0.0,
            },
        ));
        dest.root.push(LayerNode::Leaf(dest_layer));
        import_pattern_into_document(&zip, &mut dest, LayerId::new(1), "0.1.0").unwrap();
        match &dest.root[0] {
            LayerNode::Leaf(l) => {
                assert_eq!(l.filters.len(), 2);
                assert_eq!(l.filters[0].kind, FilterKind::Crt);
                assert_eq!(l.filters[1].kind, FilterKind::Dither);
            }
            _ => panic!("leaf"),
        }
    }
}
