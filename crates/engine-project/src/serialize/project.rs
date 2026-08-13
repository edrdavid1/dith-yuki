//! `.dyproj` save / open orchestration (zip + migrate + remap + pixels + assets).

use crate::document::Document;
use crate::filter::{DitherModeV2, FilterParams};
use crate::layer::LayerNode;
use crate::serialize::archive::{create_zip, ZipArchiveReader};
use crate::serialize::assets::{
    content_hash, materialize_threshold_map, parse_threshold_basename, threshold_map_basename,
    threshold_map_zip_entry, threshold_maps_cache_dir,
};
use crate::serialize::document_dto::DocumentFile;
use crate::serialize::id_remap::remap_document_file;
use crate::serialize::migrate::{
    migrate_dyproj, ArchiveKind, Manifest, ProjectError, SUPPORTED_DYPROJ_VERSION,
};
use crate::serialize::pixels::{
    assemble_layer_png, collect_raster_layers, count_raster_layers, decode_png_to_f32,
    soft_size_warning,
};
use crate::types::{DocumentId, LayerId};
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::TileCache;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Result of saving a project to zip bytes.
#[derive(Debug)]
pub struct SaveProjectResult {
    pub zip_bytes: Vec<u8>,
    /// True when estimated uncompressed raster payload ≥ soft warn threshold.
    pub size_warning: bool,
}

/// Result of opening a project (staging complete — caller swaps into AppState).
#[derive(Debug)]
pub struct OpenProjectResult {
    pub document: Document,
    /// File-local layer id → remapped runtime LayerId (for debugging).
    pub layer_remap: HashMap<LayerId, LayerId>,
}

/// Collect CustomPng user/synthetic paths from a live document and return
/// basename → PNG bytes to embed (deduped by content hash).
fn collect_custom_png_embeds(
    doc: &Document,
    read_png: &mut dyn FnMut(&str) -> Result<Vec<u8>, ProjectError>,
) -> Result<(HashMap<String, Vec<u8>>, HashMap<String, String>), ProjectError> {
    // path_in_doc → basename
    let mut path_to_basename: HashMap<String, String> = HashMap::new();
    let mut embeds: HashMap<String, Vec<u8>> = HashMap::new();

    fn walk(
        nodes: &[LayerNode],
        read_png: &mut dyn FnMut(&str) -> Result<Vec<u8>, ProjectError>,
        path_to_basename: &mut HashMap<String, String>,
        embeds: &mut HashMap<String, Vec<u8>>,
    ) -> Result<(), ProjectError> {
        for node in nodes {
            match node {
                LayerNode::Leaf(layer) => {
                    for filter in &layer.filters {
                        if let FilterParams::DitherV2(p) = &filter.params {
                            if let DitherModeV2::CustomPng { path } = &p.mode {
                                if path_to_basename.contains_key(path) {
                                    continue;
                                }
                                let bytes = read_threshold_png_for_save(path, read_png)?;
                                let basename = threshold_map_basename(&bytes);
                                embeds.insert(basename.clone(), bytes);
                                path_to_basename.insert(path.clone(), basename);
                            }
                        }
                    }
                }
                LayerNode::Group(g) => {
                    walk(&g.children, read_png, path_to_basename, embeds)?;
                }
            }
        }
        Ok(())
    }

    walk(
        &doc.root,
        read_png,
        &mut path_to_basename,
        &mut embeds,
    )?;
    Ok((embeds, path_to_basename))
}

/// Read threshold PNG for save: filesystem path, or content-addressed cache if
/// the document still holds a `{hash}.png` basename.
pub(crate) fn read_threshold_png_for_save(
    path: &str,
    read_png: &mut dyn FnMut(&str) -> Result<Vec<u8>, ProjectError>,
) -> Result<Vec<u8>, ProjectError> {
    match read_png(path) {
        Ok(bytes) => Ok(bytes),
        Err(first_err) => {
            // Basename-only (or missing user file): try shared asset cache.
            if let Ok(stem) = parse_threshold_basename(
                Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(path),
            ) {
                if let Ok(cache_dir) = threshold_maps_cache_dir() {
                    let cached = cache_dir.join(format!("{stem}.png"));
                    if cached.exists() {
                        return fs::read(&cached).map_err(|e| ProjectError::Io(e.to_string()));
                    }
                }
            }
            Err(first_err)
        }
    }
}

fn rewrite_custom_png_paths(nodes: &mut [LayerNode], path_to_basename: &HashMap<String, String>) {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                for filter in &mut layer.filters {
                    if let FilterParams::DitherV2(p) = &mut filter.params {
                        if let DitherModeV2::CustomPng { path } = &mut p.mode {
                            if let Some(b) = path_to_basename.get(path) {
                                *path = b.clone();
                            }
                        }
                    }
                }
            }
            LayerNode::Group(g) => rewrite_custom_png_paths(&mut g.children, path_to_basename),
        }
    }
}

fn rewrite_custom_png_to_synthetic(
    nodes: &mut [LayerNode],
    basename_to_path: &HashMap<String, PathBuf>,
) -> Result<(), ProjectError> {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                for filter in &mut layer.filters {
                    if let FilterParams::DitherV2(p) = &mut filter.params {
                        if let DitherModeV2::CustomPng { path } = &mut p.mode {
                            let key = Path::new(path.as_str())
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| path.clone());
                            let Some(synth) = basename_to_path.get(&key) else {
                                return Err(ProjectError::UnresolvedCustomPng(path.clone()));
                            };
                            *path = synth.to_string_lossy().into_owned();
                        }
                    }
                }
            }
            LayerNode::Group(g) => {
                rewrite_custom_png_to_synthetic(&mut g.children, basename_to_path)?;
            }
        }
    }
    Ok(())
}

/// Serialize a live document + TileCache Raw tiles into `.dyproj` zip bytes.
pub fn save_project_to_bytes(
    doc: &Document,
    cache: &TileCache,
    app_version: &str,
    mut read_threshold_png: impl FnMut(&str) -> Result<Vec<u8>, ProjectError>,
) -> Result<SaveProjectResult, ProjectError> {
    let size_warning = soft_size_warning(
        doc.width,
        doc.height,
        count_raster_layers(&doc.root),
    );

    let (embeds, path_to_basename) = collect_custom_png_embeds(doc, &mut read_threshold_png)?;

    // Clone document to rewrite CustomPng paths to basenames for JSON.
    let mut doc_for_json = doc.clone();
    rewrite_custom_png_paths(&mut doc_for_json.root, &path_to_basename);

    let rasters = collect_raster_layers(&doc.root);
    let mut layer_pngs: HashMap<u32, Vec<u8>> = HashMap::new();
    for layer in rasters {
        let png = assemble_layer_png(cache, layer, doc.width, doc.height)?;
        layer_pngs.insert(layer.id.0, png);
    }

    let file = DocumentFile::from_document(&doc_for_json, |id| {
        if layer_pngs.contains_key(&id.0) {
            Some(format!("{}.png", id.0))
        } else {
            None
        }
    });

    let now = chrono_like_now();
    let manifest = Manifest {
        format_version: SUPPORTED_DYPROJ_VERSION,
        kind: ArchiveKind::Dyproj,
        app_version: app_version.to_string(),
        created_at: now.clone(),
        modified_at: now,
        width: Some(doc.width),
        height: Some(doc.height),
    };

    let manifest_json =
        serde_json::to_vec_pretty(&manifest).map_err(|e| ProjectError::Codec(e.to_string()))?;
    let document_json =
        serde_json::to_vec_pretty(&file).map_err(|e| ProjectError::Codec(e.to_string()))?;

    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    entries.push(("manifest.json".into(), manifest_json));
    entries.push(("document.json".into(), document_json));
    for (id, png) in &layer_pngs {
        entries.push((format!("layers/{id}.png"), png.clone()));
    }
    for (basename, bytes) in &embeds {
        entries.push((threshold_map_zip_entry(basename), bytes.clone()));
    }

    let refs: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let zip_bytes = create_zip(&refs).map_err(|e| ProjectError::Io(e.to_string()))?;

    Ok(SaveProjectResult {
        zip_bytes,
        size_warning,
    })
}

/// Write zip bytes to a filesystem path.
pub fn save_project_to_path(
    path: &Path,
    doc: &Document,
    cache: &TileCache,
    app_version: &str,
    read_threshold_png: impl FnMut(&str) -> Result<Vec<u8>, ProjectError>,
) -> Result<SaveProjectResult, ProjectError> {
    let result = save_project_to_bytes(doc, cache, app_version, read_threshold_png)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ProjectError::Io(e.to_string()))?;
    }
    fs::write(path, &result.zip_bytes).map_err(|e| ProjectError::Io(e.to_string()))?;
    Ok(result)
}

/// Open a `.dyproj` from bytes into a staging document + Raw tiles in `staging_cache`.
///
/// On success, `staging_cache` holds the new Raw tiles. Caller should swap
/// `document_handle` and adopt the cache entries (or use a dedicated staging cache
/// then migrate keys). On failure, staging cache may contain partial tiles — caller
/// should discard them.
pub fn open_project_from_bytes(
    zip_bytes: &[u8],
    staging_cache: &TileCache,
    runtime_doc_id: DocumentId,
) -> Result<OpenProjectResult, ProjectError> {
    let mut reader =
        ZipArchiveReader::open(zip_bytes).map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;

    let manifest_bytes = reader
        .read_entry("manifest.json")
        .map_err(|_| ProjectError::MissingEntry("manifest.json".into()))?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;

    if manifest.kind != ArchiveKind::Dyproj {
        return Err(ProjectError::KindMismatch {
            expected: "dyproj".into(),
            found: manifest.kind.as_str().to_string(),
        });
    }

    let doc_bytes = reader
        .read_entry("document.json")
        .map_err(|_| ProjectError::MissingEntry("document.json".into()))?;
    let doc_value: serde_json::Value = serde_json::from_slice(&doc_bytes)
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;
    let doc_value = migrate_dyproj(manifest.format_version, doc_value)?;
    let file: DocumentFile = serde_json::from_value(doc_value)
        .map_err(|e| ProjectError::InvalidArchive(e.to_string()))?;

    // Materialize threshold maps into the shared content-addressed asset cache.
    let mut basename_to_synth: HashMap<String, PathBuf> = HashMap::new();
    collect_custom_png_basenames_from_file(&file, &mut |basename| {
        let entry = threshold_map_zip_entry(basename);
        let bytes = reader
            .read_entry(&entry)
            .map_err(|_| ProjectError::MissingEntry(entry.clone()))?;
        let stem = parse_threshold_basename(basename).map_err(|e| {
            ProjectError::InvalidArchive(format!("invalid CustomPng basename '{basename}': {e}"))
        })?;
        let actual = content_hash(&bytes);
        if actual != stem {
            return Err(ProjectError::HashMismatch {
                entry: basename.to_string(),
                actual,
            });
        }
        let path = materialize_threshold_map(&bytes)
            .map_err(|e| ProjectError::Io(e.to_string()))?;
        basename_to_synth.insert(basename.to_string(), path);
        Ok(())
    })?;

    let mut remapped = remap_document_file(&file, runtime_doc_id);
    rewrite_custom_png_to_synthetic(&mut remapped.document.root, &basename_to_synth)?;

    // Decode each raw_asset PNG and decompose under remapped LayerId.
    for (old_layer_id, asset_name) in &remapped.raw_assets {
        let entry = if asset_name.starts_with("layers/") {
            asset_name.clone()
        } else {
            format!("layers/{asset_name}")
        };
        let png = reader
            .read_entry(&entry)
            .map_err(|_| ProjectError::MissingEntry(entry))?;
        let (w, h, rgba) = decode_png_to_f32(&png)?;
        let new_id = remapped
            .tables
            .layers
            .get(old_layer_id)
            .copied()
            .ok_or_else(|| ProjectError::InvalidArchive(format!("no remap for layer {old_layer_id}")))?;

        // Prefer document dims; PNG should match doc size per assemble contract.
        let width = remapped.document.width.max(w);
        let height = remapped.document.height.max(h);
        let _ = (width, height); // dims on document already set
        decompose_image_to_tiles(&rgba, w, h, new_id.0, staging_cache).map_err(|e| {
            ProjectError::Codec(format!("decompose failed for layer {}: {e}", new_id.0))
        })?;
    }

    // Ensure adjustment layers never required PNG (already skipped via raw_assets).

    Ok(OpenProjectResult {
        document: remapped.document,
        layer_remap: remapped.tables.layers,
    })
}

/// Open from filesystem path.
pub fn open_project_from_path(
    path: &Path,
    staging_cache: &TileCache,
    runtime_doc_id: DocumentId,
) -> Result<OpenProjectResult, ProjectError> {
    let bytes = fs::read(path).map_err(|e| ProjectError::Io(e.to_string()))?;
    open_project_from_bytes(&bytes, staging_cache, runtime_doc_id)
}

fn collect_custom_png_basenames_from_file(
    file: &DocumentFile,
    visit: &mut dyn FnMut(&str) -> Result<(), ProjectError>,
) -> Result<(), ProjectError> {
    fn walk(
        nodes: &[crate::serialize::document_dto::LayerNodeFile],
        visit: &mut dyn FnMut(&str) -> Result<(), ProjectError>,
    ) -> Result<(), ProjectError> {
        use crate::serialize::document_dto::LayerNodeFile;
        for node in nodes {
            match node {
                LayerNodeFile::Leaf(layer) => {
                    for filter in &layer.filters {
                        if let FilterParams::DitherV2(p) = &filter.params {
                            if let DitherModeV2::CustomPng { path } = &p.mode {
                                visit(path)?;
                            }
                        }
                    }
                }
                LayerNodeFile::Group(g) => walk(&g.children, visit)?,
            }
        }
        Ok(())
    }
    walk(&file.root, visit)
}

pub(crate) fn chrono_like_now() -> String {
    // Avoid chrono dependency: RFC3339-ish UTC via system time.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

/// Read a threshold PNG from disk (user or synthetic path) without sandbox —
/// caller is responsible for sandboxing user paths at the IPC boundary.
pub fn read_png_file(path: &str) -> Result<Vec<u8>, ProjectError> {
    fs::read(path).map_err(|e| ProjectError::Io(format!("{path}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherParamsV2, FilterInstance, FilterKind};
    use crate::layer::Layer;
    use crate::serialize::pixels::force_drop_raw_tile;
    use crate::types::LayerKind;
    use engine_tiles::decompose::decompose_image_to_tiles;

    #[test]
    fn save_open_round_trip_structure_and_pixels() {
        let w = 32u32;
        let h = 32u32;
        let mut rgba = vec![0.0f32; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            rgba[i * 4] = 0.2;
            rgba[i * 4 + 1] = 0.4;
            rgba[i * 4 + 2] = 0.6;
            rgba[i * 4 + 3] = 1.0;
        }

        let cache = TileCache::new(50_000_000);
        decompose_image_to_tiles(&rgba, w, h, 1, &cache).unwrap();

        let mut doc = Document::new(DocumentId::new(1), w, h);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                ..DitherParamsV2::default()
            }),
        ));
        doc.root.push(LayerNode::Leaf(layer));

        let saved = save_project_to_bytes(&doc, &cache, "0.1.0", |_| {
            Err(ProjectError::Io("no custom png".into()))
        })
        .unwrap();
        assert!(!saved.size_warning);

        let staging = TileCache::new(50_000_000);
        let opened = open_project_from_bytes(&saved.zip_bytes, &staging, DocumentId::new(1)).unwrap();
        assert_eq!(opened.document.width, w);
        assert_eq!(opened.document.height, h);
        assert_eq!(opened.document.root.len(), 1);
        match &opened.document.root[0] {
            LayerNode::Leaf(l) => {
                assert_eq!(l.kind, LayerKind::Raster);
                assert_eq!(l.filters.len(), 1);
                assert!(!l.filters[0].requires_full_row);
                // New layer id (still starts at 1 for single-layer, but filter id is fresh)
                assert_eq!(l.id.0, 1);
            }
            _ => panic!("expected leaf"),
        }

        // Raw tile present under remapped id
        let new_id = opened.layer_remap[&LayerId::new(1)];
        let key = engine_tiles::TileKey {
            layer: new_id.0,
            coord: engine_tiles::TileCoord { level: 0, x: 0, y: 0 },
            stage: engine_tiles::CacheStage::Raw,
        };
        assert!(staging.get_entry(key).is_some());
    }

    #[test]
    fn custom_png_survives_without_original_user_path() {
        use std::fs;

        let w = 16u32;
        let h = 16u32;
        let rgba = vec![0.3f32; (w * h * 4) as usize];
        let cache = TileCache::new(20_000_000);
        decompose_image_to_tiles(&rgba, w, h, 1, &cache).unwrap();

        // Write a grayscale threshold PNG, then embed via save.
        let mut png_buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_buf, 2, 2);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[0, 85, 170, 255]).unwrap();
        }
        let home = dirs::home_dir().expect("home");
        let user_path = home.join(".dither_yuki_test_dyproj").join("thresh.png");
        fs::create_dir_all(user_path.parent().unwrap()).unwrap();
        fs::write(&user_path, &png_buf).unwrap();

        let mut doc = Document::new(DocumentId::new(1), w, h);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::CustomPng {
                    path: user_path.to_string_lossy().into_owned(),
                },
                levels: 2,
                ..DitherParamsV2::default()
            }),
        ));
        doc.root.push(LayerNode::Leaf(layer));

        let saved = save_project_to_bytes(&doc, &cache, "0.1.0", |p| read_png_file(p)).unwrap();

        // Remove original user file — open must not need it.
        let _ = fs::remove_file(&user_path);

        let staging = TileCache::new(20_000_000);
        let opened =
            open_project_from_bytes(&saved.zip_bytes, &staging, DocumentId::new(1)).unwrap();
        match &opened.document.root[0] {
            LayerNode::Leaf(l) => match &l.filters[0].params {
                FilterParams::DitherV2(p) => match &p.mode {
                    DitherModeV2::CustomPng { path } => {
                        assert!(path.contains("asset-cache"));
                        assert!(path.contains("threshold-maps"));
                        assert!(std::path::Path::new(path).exists());
                        assert!(path.ends_with(".png"));
                    }
                    other => panic!("expected CustomPng, got {other:?}"),
                },
                other => panic!("unexpected {other:?}"),
            },
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn hash_mismatch_on_open_errors() {
        let w = 16u32;
        let h = 16u32;
        let rgba = vec![1.0f32; (w * h * 4) as usize];
        let cache = TileCache::new(20_000_000);
        decompose_image_to_tiles(&rgba, w, h, 1, &cache).unwrap();
        let mut doc = Document::new(DocumentId::new(1), w, h);
        doc.root
            .push(LayerNode::Leaf(Layer::new(LayerId::new(1), LayerKind::Raster, w, h)));
        let saved = save_project_to_bytes(&doc, &cache, "0.1.0", |_| unreachable!()).unwrap();

        // Inject a fake CustomPng pointing at a wrong-named empty asset.
        let mut reader = ZipArchiveReader::open(&saved.zip_bytes).unwrap();
        let mut file: DocumentFile =
            serde_json::from_slice(&reader.read_entry("document.json").unwrap()).unwrap();
        let fake_name = "00000000000000000000000000000000.png";
        match &mut file.root[0] {
            crate::serialize::document_dto::LayerNodeFile::Leaf(l) => {
                l.filters.push(crate::serialize::document_dto::FilterInstanceFile {
                    id: crate::types::FilterInstanceId::new(),
                    kind: FilterKind::Dither,
                    params: FilterParams::DitherV2(DitherParamsV2 {
                        mode: DitherModeV2::CustomPng {
                            path: fake_name.into(),
                        },
                        levels: 2,
                        ..DitherParamsV2::default()
                    }),
                    enabled: true,
                    opacity: 1.0,
                    blend_mode: crate::types::BlendMode::Normal,
                });
            }
            _ => panic!(),
        }
        let manifest = reader.read_entry("manifest.json").unwrap();
        let layer = reader.read_entry("layers/1.png").unwrap();
        let doc_bytes = serde_json::to_vec_pretty(&file).unwrap();
        // Real PNG bytes that won't match the fake zero hash name
        let mut png_buf = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut png_buf, 2, 2);
            encoder.set_color(png::ColorType::Grayscale);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 2, 3, 4]).unwrap();
        }
        let zip = create_zip(&[
            ("manifest.json", manifest.as_slice()),
            ("document.json", doc_bytes.as_slice()),
            ("layers/1.png", layer.as_slice()),
            (
                &format!("assets/threshold_maps/{fake_name}"),
                png_buf.as_slice(),
            ),
        ])
        .unwrap();

        let staging = TileCache::new(20_000_000);
        let err = open_project_from_bytes(&zip, &staging, DocumentId::new(1)).unwrap_err();
        assert!(matches!(err, ProjectError::HashMismatch { .. }), "got {err:?}");
    }

    #[test]
    fn save_fails_incomplete_raw() {
        let w = 300u32;
        let h = 300u32;
        let rgba = vec![0.5f32; (w * h * 4) as usize];
        let cache = TileCache::new(50_000_000);
        decompose_image_to_tiles(&rgba, w, h, 1, &cache).unwrap();
        force_drop_raw_tile(&cache, LayerId::new(1), 0, 0);

        let mut doc = Document::new(DocumentId::new(1), w, h);
        doc.root
            .push(LayerNode::Leaf(Layer::new(LayerId::new(1), LayerKind::Raster, w, h)));

        let err = save_project_to_bytes(&doc, &cache, "0.1.0", |_| unreachable!()).unwrap_err();
        assert!(matches!(err, ProjectError::IncompleteRaw { layer_id: 1 }));
    }

    #[test]
    fn future_format_version_errors_without_partial_doc() {
        // Build a minimal valid zip then bump format_version
        let w = 16u32;
        let h = 16u32;
        let rgba = vec![1.0f32; (w * h * 4) as usize];
        let cache = TileCache::new(20_000_000);
        decompose_image_to_tiles(&rgba, w, h, 1, &cache).unwrap();
        let mut doc = Document::new(DocumentId::new(1), w, h);
        doc.root
            .push(LayerNode::Leaf(Layer::new(LayerId::new(1), LayerKind::Raster, w, h)));
        let saved = save_project_to_bytes(&doc, &cache, "0.1.0", |_| unreachable!()).unwrap();

        let mut reader = ZipArchiveReader::open(&saved.zip_bytes).unwrap();
        let mut manifest: Manifest =
            serde_json::from_slice(&reader.read_entry("manifest.json").unwrap()).unwrap();
        let document = reader.read_entry("document.json").unwrap();
        let layer = reader.read_entry("layers/1.png").unwrap();
        manifest.format_version = 99;
        let manifest_bytes = serde_json::to_vec_pretty(&manifest).unwrap();
        let zip = create_zip(&[
            ("manifest.json", manifest_bytes.as_slice()),
            ("document.json", document.as_slice()),
            ("layers/1.png", layer.as_slice()),
        ])
        .unwrap();

        let staging = TileCache::new(20_000_000);
        let err = open_project_from_bytes(&zip, &staging, DocumentId::new(1)).unwrap_err();
        assert!(matches!(
            err,
            ProjectError::UnsupportedVersion { found: 99, .. }
        ));
        assert_eq!(staging.entry_count(), 0);
    }
}
