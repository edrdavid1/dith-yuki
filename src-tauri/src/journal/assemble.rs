//! Assemble an in-memory `.dyproj` zip from H9 v2 state + blobs (recovery).

use std::path::Path;

use engine_project::serialize::archive::create_zip;
use engine_project::serialize::document_dto::{DocumentFile, LayerNodeFile};
use engine_project::serialize::features::SUPPORTED_FORMAT;
use engine_project::serialize::{
    blob_sha256_hex, build_dyproj_manifest_json, build_manifest_files, decode_journal_state,
    threshold_map_zip_entry, JournalBlobRef, MIME_DYPROJ,
};

/// Build zip bytes suitable for [`engine_project::serialize::open_project_from_bytes`].
///
/// Omits `composite.png` / `thumbnail.png` (not required on open — verified in Step 2).
pub fn assemble_dyproj_from_state_bytes(
    state_bytes: &[u8],
    blobs_dir: &Path,
    app_version: &str,
) -> Result<Vec<u8>, String> {
    let state = decode_journal_state(state_bytes).map_err(|e| e.to_string())?;

    let mut doc = state.document;
    rewrite_raw_assets_to_layer_png(&mut doc);
    let document_json = serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?;

    let mut payload: Vec<(String, Vec<u8>)> = Vec::new();
    payload.push(("document.json".into(), document_json));

    for blob in &state.blobs {
        let path = blobs_dir.join(blob.filename());
        let data = std::fs::read(&path).map_err(|_| {
            format!(
                "missing pixel blob «{}» (file may have been deleted or never finished writing)",
                blob.filename()
            )
        })?;
        let actual = blob_sha256_hex(&data);
        if actual != blob.sha256() {
            return Err(format!(
                "pixel blob «{}» failed integrity check (hash mismatch)",
                blob.filename()
            ));
        }
        match blob {
            JournalBlobRef::Layer { layer_id, .. } => {
                payload.push((format!("layers/{layer_id}.png"), data));
            }
            JournalBlobRef::Threshold { content_hash, .. } => {
                let name = if content_hash.ends_with(".png") {
                    content_hash.clone()
                } else {
                    format!("{content_hash}.png")
                };
                payload.push((threshold_map_zip_entry(&name), data));
            }
        }
    }

    let files = build_manifest_files(&payload);
    let manifest = build_dyproj_manifest_json(
        SUPPORTED_FORMAT,
        SUPPORTED_FORMAT,
        &[],
        &[],
        app_version,
        "0",
        "0",
        doc.width,
        doc.height,
        &files,
    )
    .map_err(|e| e.to_string())?;

    let mut owned: Vec<(String, Vec<u8>)> = Vec::new();
    owned.push(("mimetype".into(), MIME_DYPROJ.as_bytes().to_vec()));
    owned.push(("manifest.json".into(), manifest));
    owned.extend(payload);

    let refs: Vec<(&str, &[u8])> = owned
        .iter()
        .map(|(n, b)| (n.as_str(), b.as_slice()))
        .collect();
    create_zip(&refs).map_err(|e| e.to_string())
}

fn rewrite_raw_assets_to_layer_png(doc: &mut DocumentFile) {
    fn walk(nodes: &mut [LayerNodeFile]) {
        for node in nodes {
            match node {
                LayerNodeFile::Leaf(layer) => {
                    if let Some(ref asset) = layer.raw_asset.clone() {
                        if let Some(id) = asset
                            .strip_prefix("layer-")
                            .and_then(|s| s.split('-').next())
                        {
                            layer.raw_asset = Some(format!("{id}.png"));
                        }
                    }
                }
                LayerNodeFile::Group(g) => walk(&mut g.children),
                _ => {}
            }
        }
    }
    walk(&mut doc.root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::AppState;
    use crate::journal::meta::{journal_blobs_dir, journal_state_path};
    use crate::journal::{set_recovery_dir, write_journal_for_doc};
    use engine_project::document::Document;
    use engine_project::layer::{Layer, LayerNode};
    use engine_project::types::{DocumentId, LayerId, LayerKind};
    use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey, HALO, TILE_SIZE};
    use std::sync::Arc;

    #[test]
    fn assemble_round_trip_opens() {
        use engine_project::serialize::open_project_from_bytes;
        use engine_tiles::TileCache;

        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
        set_recovery_dir(&state, recovery.clone());

        let mut doc = Document::new(DocumentId::new(1), 64, 64);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        layer.bump_raw_rev();
        doc.root.push(LayerNode::Leaf(layer));
        let session = state.spawn_session(doc);
        let full = TILE_SIZE + 2 * HALO;
        let mut tile = PixelTile::new();
        for y in 0..full {
            for x in 0..full {
                tile.set(x, y, 0, 0.4);
                tile.set(x, y, 1, 0.5);
                tile.set(x, y, 2, 0.6);
                tile.set(x, y, 3, 1.0);
            }
        }
        state.tiles.tile_cache.insert_fresh(
            TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord {
                    level: 0,
                    x: 0,
                    y: 0,
                },
                stage: CacheStage::Raw,
            },
            Arc::new(tile),
        );

        write_journal_for_doc(&state, 1).unwrap();
        let state_bytes =
            std::fs::read(journal_state_path(&recovery, session.recovery_id)).unwrap();
        let blobs = journal_blobs_dir(&recovery, session.recovery_id);
        let zip = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, "0.0.0-test").unwrap();
        let staging = TileCache::new(64 * 1024 * 1024);
        let opened = open_project_from_bytes(&zip, &staging, DocumentId::new(9)).unwrap();
        assert_eq!(opened.document.width, 64);
        assert_eq!(opened.document.layer_raw_rev(LayerId::new(1)), Some(1));
    }

    #[test]
    fn missing_blob_errors_clearly_without_panic() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = dir.path().join("recovery");
        let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
        set_recovery_dir(&state, recovery.clone());

        let mut doc = Document::new(DocumentId::new(1), 64, 64);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        layer.bump_raw_rev();
        doc.root.push(LayerNode::Leaf(layer));
        let session = state.spawn_session(doc);
        let full = TILE_SIZE + 2 * HALO;
        let mut tile = PixelTile::new();
        for y in 0..full {
            for x in 0..full {
                tile.set(x, y, 0, 0.4);
                tile.set(x, y, 1, 0.5);
                tile.set(x, y, 2, 0.6);
                tile.set(x, y, 3, 1.0);
            }
        }
        state.tiles.tile_cache.insert_fresh(
            TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord {
                    level: 0,
                    x: 0,
                    y: 0,
                },
                stage: CacheStage::Raw,
            },
            Arc::new(tile),
        );
        write_journal_for_doc(&state, 1).unwrap();
        let blobs = journal_blobs_dir(&recovery, session.recovery_id);
        std::fs::remove_file(blobs.join("layer-1-1.png")).unwrap();
        let state_bytes =
            std::fs::read(journal_state_path(&recovery, session.recovery_id)).unwrap();
        let err = assemble_dyproj_from_state_bytes(&state_bytes, &blobs, "0.0.0-test").unwrap_err();
        assert!(err.contains("missing pixel blob"), "{err}");
    }
}
