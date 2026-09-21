//! Share Copy, downgrade, and privacy helpers (SPEC §10–§11 / Stage 5).

use crate::document::Document;
use crate::serialize::archive::{create_dither_archive, ZipArchiveReader, MIME_DYPROJ};
use crate::serialize::assets::{threshold_map_basename, threshold_map_zip_entry};
use crate::serialize::document_dto::DocumentFile;
use crate::serialize::features::{required_version_for_features, FormatVersion};
use crate::serialize::manifest::{build_dyproj_manifest_json, build_manifest_files};
use crate::serialize::migrate::ProjectError;
use crate::serialize::pixels::{
    assemble_layer_png, build_composite_rgba8, collect_raster_layers, count_raster_layers,
    encode_rgba8_png, reencode_png_clean, soft_size_warning,
};
use crate::serialize::thumbnail::{build_thumbnail_png_cached, neutral_thumbnail_png};
use crate::serialize::project::{
    chrono_like_now, collect_custom_png_embeds, rewrite_custom_png_paths, SaveProjectResult,
};
use engine_tiles::TileCache;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

/// Options for the explicit «Share Copy» export (SPEC §11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareCopyOptions {
    /// Re-encode embedded PNGs without ancillary chunks (default: true).
    pub strip_metadata: bool,
    /// Reserved: originals are not stored separately today (default: false).
    pub include_original_images: bool,
    /// Write author-like fields when present (default: false).
    pub include_author: bool,
    /// Minify JSON payloads (default: false).
    pub compact: bool,
    /// Include real `thumbnail.png` (default: true). When false, write a neutral
    /// placeholder instead (preview SPEC §12 / Share Copy privacy).
    pub include_preview: bool,
}

impl Default for ShareCopyOptions {
    fn default() -> Self {
        Self {
            strip_metadata: true,
            include_original_images: false,
            include_author: false,
            compact: false,
            include_preview: true,
        }
    }
}

/// One feature/data loss when exporting for an older format (SPEC §10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LossReport {
    pub feature_id: String,
    pub detail: String,
}

/// Write options shared by normal save / share / downgrade.
#[derive(Debug, Clone)]
pub struct ProjectWriteOptions {
    pub compact_json: bool,
    pub strip_png_metadata: bool,
    /// When false, never emit author-like metadata (Share Copy default).
    pub include_author: bool,
    pub target_format: FormatVersion,
    /// When set, used for `created_at` / `modified_at` (byte-stable archives).
    pub timestamp: Option<String>,
    /// When false, write a neutral `thumbnail.png` placeholder.
    pub include_preview: bool,
}

impl ProjectWriteOptions {
    pub fn normal() -> Self {
        Self {
            compact_json: false,
            strip_png_metadata: false,
            include_author: true,
            target_format: FormatVersion::V1_0,
            timestamp: None,
            include_preview: true,
        }
    }

    pub fn share(opts: &ShareCopyOptions) -> Self {
        Self {
            compact_json: opts.compact,
            strip_png_metadata: opts.strip_metadata,
            include_author: opts.include_author,
            target_format: FormatVersion::V1_0,
            // Stable stamp so Share Copy of the same doc is byte-identical.
            timestamp: Some("0".into()),
            include_preview: opts.include_preview,
        }
    }
}

/// Explicit Share Copy export (not ordinary Save).
pub fn share_project_to_bytes(
    doc: &Document,
    cache: &TileCache,
    app_version: &str,
    read_threshold_png: impl FnMut(&str) -> Result<Vec<u8>, ProjectError>,
    opts: &ShareCopyOptions,
) -> Result<SaveProjectResult, ProjectError> {
    let _ = opts.include_original_images; // reserved; no original store yet
    write_project_to_bytes(
        doc,
        cache,
        app_version,
        read_threshold_png,
        &ProjectWriteOptions::share(opts),
    )
}

/// Plan losses for exporting at `target` without writing bytes.
pub fn plan_downgrade(doc: &Document, target: FormatVersion) -> Vec<LossReport> {
    let _ = doc;
    let mut losses = Vec::new();
    if target < FormatVersion::V1_0 {
        losses.push(LossReport {
            feature_id: "format".into(),
            detail: format!("target {target} is below the minimum writable format 1.0"),
        });
    }
    losses
}

/// Explicit «Export for older version» (SPEC §10). Never runs implicitly on Save.
pub fn downgrade_project_to_bytes(
    doc: &Document,
    cache: &TileCache,
    app_version: &str,
    read_threshold_png: impl FnMut(&str) -> Result<Vec<u8>, ProjectError>,
    target: FormatVersion,
) -> Result<(SaveProjectResult, Vec<LossReport>), ProjectError> {
    let losses = plan_downgrade(doc, target);
    if target.major != 1 {
        return Err(ProjectError::InvalidArchive(format!(
            "cannot export to format {target}; this build writes format 1.x only"
        )));
    }
    if losses.iter().any(|l| l.feature_id == "format") {
        return Err(ProjectError::InvalidArchive(format!(
            "cannot downgrade to {target}"
        )));
    }
    let mut opts = ProjectWriteOptions::normal();
    opts.target_format = target;
    let saved = write_project_to_bytes(doc, cache, app_version, read_threshold_png, &opts)?;
    Ok((saved, losses))
}

/// Core writer used by Save / Share / Downgrade.
pub fn write_project_to_bytes(
    doc: &Document,
    cache: &TileCache,
    app_version: &str,
    mut read_threshold_png: impl FnMut(&str) -> Result<Vec<u8>, ProjectError>,
    opts: &ProjectWriteOptions,
) -> Result<SaveProjectResult, ProjectError> {
    let size_warning = soft_size_warning(doc.width, doc.height, count_raster_layers(&doc.root));

    let (mut embeds, mut path_to_basename) =
        collect_custom_png_embeds(doc, &mut read_threshold_png)?;

    if opts.strip_png_metadata {
        let mut cleaned_embeds = HashMap::new();
        let mut old_to_new: HashMap<String, String> = HashMap::new();
        for (old_base, bytes) in embeds {
            let clean = reencode_png_clean(&bytes)?;
            let new_base = threshold_map_basename(&clean);
            old_to_new.insert(old_base, new_base.clone());
            cleaned_embeds.insert(new_base, clean);
        }
        for basename in path_to_basename.values_mut() {
            if let Some(new_b) = old_to_new.get(basename) {
                *basename = new_b.clone();
            }
        }
        embeds = cleaned_embeds;
    }

    let mut doc_for_json = doc.clone();
    rewrite_custom_png_paths(&mut doc_for_json.root, &path_to_basename);

    let rasters = collect_raster_layers(&doc.root);
    let mut layer_pngs: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    for layer in rasters {
        let mut png = assemble_layer_png(cache, layer, doc.width, doc.height, doc.id.0)?;
        if opts.strip_png_metadata {
            png = reencode_png_clean(&png)?;
        }
        layer_pngs.insert(layer.id.0, png);
    }

    let file = DocumentFile::from_document(&doc_for_json, |id| {
        if layer_pngs.contains_key(&id.0) {
            Some(format!("{}.png", id.0))
        } else {
            None
        }
    });

    let composite_rgba =
        build_composite_rgba8(cache, &doc.root, doc.width, doc.height, doc.id.0)?;
    let mut composite_png = encode_rgba8_png(&composite_rgba, doc.width, doc.height)?;
    let thumbnail_png = if opts.include_preview {
        build_thumbnail_png_cached(&composite_rgba, doc.width, doc.height)
    } else {
        neutral_thumbnail_png()
    };
    if opts.strip_png_metadata {
        composite_png = reencode_png_clean(&composite_png)?;
    }

    let document_json = encode_json(&file, opts.compact_json)?;

    // BTreeMap keeps asset order stable across HashMap reshuffles.
    let embeds: BTreeMap<String, Vec<u8>> = embeds.into_iter().collect();

    let mut payload: Vec<(String, Vec<u8>)> = Vec::new();
    payload.push(("document.json".into(), document_json));
    payload.push(("composite.png".into(), composite_png));
    payload.push(("thumbnail.png".into(), thumbnail_png));
    for (id, png) in &layer_pngs {
        payload.push((format!("layers/{id}.png"), png.clone()));
    }
    for (basename, bytes) in &embeds {
        payload.push((threshold_map_zip_entry(basename), bytes.clone()));
    }
    let mut ext_sorted: Vec<_> = doc
        .ext_blobs
        .iter()
        .filter(|(n, _)| n.starts_with("ext/"))
        .collect();
    ext_sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, bytes) in ext_sorted {
        payload.push((name.clone(), bytes.clone()));
    }

    let files = build_manifest_files(&payload);
    let now = opts
        .timestamp
        .clone()
        .unwrap_or_else(chrono_like_now);
    let format = required_version_for_features(std::iter::empty::<&str>());
    let format = if opts.target_format < format {
        opts.target_format
    } else {
        format
    };
    let min_reader = format;
    let _ = opts.include_author; // dyproj has no author field today
    let mut manifest_json = build_dyproj_manifest_json(
        format,
        min_reader,
        &[],
        &[],
        app_version,
        &now,
        &now,
        doc.width,
        doc.height,
        &files,
    )?;
    if opts.compact_json {
        let v: Value = serde_json::from_slice(&manifest_json)
            .map_err(|e| ProjectError::Codec(e.to_string()))?;
        manifest_json = serde_json::to_vec(&v).map_err(|e| ProjectError::Codec(e.to_string()))?;
    }
    payload.push(("manifest.json".into(), manifest_json));

    let refs: Vec<(&str, &[u8])> = payload
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    let zip_bytes =
        create_dither_archive(MIME_DYPROJ, &refs).map_err(|e| ProjectError::Io(e.to_string()))?;

    Ok(SaveProjectResult {
        zip_bytes,
        size_warning,
    })
}

fn encode_json<T: serde::Serialize>(value: &T, compact: bool) -> Result<Vec<u8>, ProjectError> {
    if compact {
        serde_json::to_vec(value).map_err(|e| ProjectError::Codec(e.to_string()))
    } else {
        serde_json::to_vec_pretty(value).map_err(|e| ProjectError::Codec(e.to_string()))
    }
}

/// Scan archive JSON/text for absolute paths / home-dir leaks (SPEC §11 always-rules).
pub fn scan_archive_for_privacy_leaks(zip_bytes: &[u8]) -> Result<Vec<String>, ProjectError> {
    let mut reader =
        ZipArchiveReader::open(zip_bytes).map_err(|e| ProjectError::Io(e.to_string()))?;
    let mut findings = Vec::new();
    let names = reader.entry_names();
    for name in names {
        if !(name.ends_with(".json") || name.ends_with(".txt")) {
            continue;
        }
        let bytes = reader
            .read_entry(&name)
            .map_err(|e| ProjectError::Io(e.to_string()))?;
        let text = String::from_utf8_lossy(&bytes);
        for (label, needle) in [
            ("unix home", "/Users/"),
            ("unix root home", "/home/"),
            ("windows drive", ":\\"),
            ("windows drive alt", ":/"),
            ("file url", "file://"),
        ] {
            if text.contains(needle) {
                findings.push(format!("{name}: possible {label} leak ({needle})"));
            }
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{Layer, LayerNode};
    use crate::serialize::archive::ZipArchiveReader;
    use crate::types::{DocumentId, LayerId, LayerKind};
    use engine_tiles::decompose::decompose_image_to_tiles;

    fn tiny_doc() -> (Document, TileCache) {
        let w = 16u32;
        let h = 16u32;
        let rgba = vec![0.7f32; (w * h * 4) as usize];
        let cache = TileCache::new(20_000_000);
        decompose_image_to_tiles(&rgba, w, h, 1, 1, &cache).unwrap();
        let mut doc = Document::new(DocumentId::new(1), w, h);
        doc.root.push(LayerNode::Leaf(Layer::new(
            LayerId::new(1),
            LayerKind::Raster,
            w,
            h,
        )));
        (doc, cache)
    }

    #[test]
    fn share_copy_defaults_have_no_privacy_leaks() {
        let (doc, cache) = tiny_doc();
        let saved = share_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            &ShareCopyOptions::default(),
        )
        .unwrap();
        let leaks = scan_archive_for_privacy_leaks(&saved.zip_bytes).unwrap();
        assert!(leaks.is_empty(), "{leaks:?}");

        let mut reader = ZipArchiveReader::open(&saved.zip_bytes).unwrap();
        assert_eq!(
            reader.read_entry("mimetype").unwrap(),
            b"application/vnd.dither.project+zip"
        );
        let manifest: Value =
            serde_json::from_slice(&reader.read_entry("manifest.json").unwrap()).unwrap();
        assert!(manifest.get("author").is_none());
        assert_eq!(manifest["generator"]["app"], "Dither");
        assert!(manifest["generator"].get("hostname").is_none());
        assert!(manifest["generator"].get("user").is_none());
        // No absolute-path-looking strings in document.json
        let doc_json = String::from_utf8(reader.read_entry("document.json").unwrap()).unwrap();
        assert!(!doc_json.contains("/Users/"));
        assert!(!doc_json.contains("C:\\"));
        // Defaults: originals not a separate entry (none stored)
        let names = reader.entry_names();
        assert!(!names.iter().any(|n| n.contains("original")));
        // PNG payload present and clean enough to decode
        let composite = reader.read_entry("composite.png").unwrap();
        assert!(composite.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(
            !composite.windows(4).any(|w| w == b"tEXt" || w == b"iTXt" || w == b"eXIf"),
            "share copy PNGs must not carry ancillary text/EXIF chunks"
        );
    }

    #[test]
    fn downgrade_to_v1_is_ok_with_empty_losses() {
        let (doc, cache) = tiny_doc();
        let (_saved, losses) = downgrade_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            FormatVersion::V1_0,
        )
        .unwrap();
        assert!(losses.is_empty());
        assert!(plan_downgrade(&doc, FormatVersion::V1_0).is_empty());
    }

    #[test]
    fn compact_share_minifies_json() {
        let (doc, cache) = tiny_doc();
        let opts = ShareCopyOptions {
            compact: true,
            ..ShareCopyOptions::default()
        };
        let saved = share_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            &opts,
        )
        .unwrap();
        let mut reader = ZipArchiveReader::open(&saved.zip_bytes).unwrap();
        let doc_json = reader.read_entry("document.json").unwrap();
        assert!(!doc_json.contains(&b'\n'), "compact JSON should be one line");
    }

    #[test]
    fn share_copy_can_omit_preview() {
        let (doc, cache) = tiny_doc();
        let opts = ShareCopyOptions {
            include_preview: false,
            ..ShareCopyOptions::default()
        };
        let saved = share_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            &opts,
        )
        .unwrap();
        let mut reader = ZipArchiveReader::open(&saved.zip_bytes).unwrap();
        let thumb = reader.read_entry("thumbnail.png").unwrap();
        assert_eq!(thumb, crate::serialize::thumbnail::neutral_thumbnail_png());
    }

    #[test]
    fn share_copy_is_byte_identical_for_same_document() {
        let (doc, cache) = tiny_doc();
        let opts = ShareCopyOptions::default();
        let a = share_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            &opts,
        )
        .unwrap()
        .zip_bytes;
        let b = share_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            &opts,
        )
        .unwrap()
        .zip_bytes;
        assert_eq!(a, b, "Share Copy must be deterministic");
    }

    #[test]
    fn write_with_fixed_timestamp_is_deterministic() {
        let (doc, cache) = tiny_doc();
        let mut opts = ProjectWriteOptions::normal();
        opts.timestamp = Some("fixed-ts".into());
        let a = write_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            &opts,
        )
        .unwrap()
        .zip_bytes;
        let b = write_project_to_bytes(
            &doc,
            &cache,
            "0.3.0",
            |_| Err(ProjectError::Io("none".into())),
            &opts,
        )
        .unwrap()
        .zip_bytes;
        assert_eq!(a, b);
    }
}
