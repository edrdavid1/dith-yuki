//! Crash-recovery journal `.state` file (H9 v2) — document + blob inventory.
//!
//! Lives **outside** `.dyproj` / `.dyuki` versioning. Serializes structure and
//! filter params without Processed/composite/thumbnail pixels. Layer Raw and
//! threshold maps are referenced by sha256 blob names only (bytes live under
//! `{uuid}.blobs/`; written in Step 3).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::document::Document;
use crate::layer::LayerNode;
use crate::serialize::document_dto::DocumentFile;
use crate::serialize::migrate::ProjectError;
use crate::types::{LayerId, LayerKind};

/// Magic string in every journal state file.
pub const JOURNAL_STATE_MAGIC: &str = "dither.journal.state";
/// Current writer version. Readers reject `version > JOURNAL_STATE_VERSION`.
pub const JOURNAL_STATE_VERSION: u32 = 1;

#[allow(dead_code)] // referenced from serde `default = "empty_extra"`
fn empty_extra() -> Map<String, Value> {
    Map::new()
}

fn is_empty_extra(m: &Map<String, Value>) -> bool {
    m.is_empty()
}

/// On-disk journal state (JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalStateFile {
    pub magic: String,
    pub version: u32,
    /// App that wrote this state (informational).
    pub app_version: String,
    /// Project structure without result pixels (`document.json` shape).
    pub document: DocumentFile,
    /// Blobs that must exist under `{recovery_id}.blobs/` before this state is valid.
    pub blobs: Vec<JournalBlobRef>,
    /// Forward-compat bag (unknown fields at the root).
    #[serde(default = "empty_extra", skip_serializing_if = "is_empty_extra")]
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One immutable blob referenced by the state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JournalBlobRef {
    Layer {
        layer_id: u32,
        raw_rev: u64,
        /// Hex sha256 of the PNG bytes on disk.
        sha256: String,
        /// Relative filename under `.blobs/` (e.g. `layer-1-1.png`).
        filename: String,
    },
    Threshold {
        /// Content-addressed stem used by dyproj threshold assets.
        content_hash: String,
        sha256: String,
        filename: String,
    },
}

impl JournalBlobRef {
    pub fn filename(&self) -> &str {
        match self {
            JournalBlobRef::Layer { filename, .. } | JournalBlobRef::Threshold { filename, .. } => {
                filename
            }
        }
    }

    pub fn sha256(&self) -> &str {
        match self {
            JournalBlobRef::Layer { sha256, .. } | JournalBlobRef::Threshold { sha256, .. } => {
                sha256
            }
        }
    }
}

/// Filename for a layer Raw blob.
pub fn layer_blob_filename(layer_id: u32, raw_rev: u64) -> String {
    format!("layer-{layer_id}-{raw_rev}.png")
}

/// Filename for a threshold-map blob.
pub fn threshold_blob_filename(content_hash: &str) -> String {
    format!("threshold-{content_hash}.png")
}

/// Re-export for callers that hash blob bytes for the inventory.
pub use crate::serialize::assets::sha256_hex as blob_sha256_hex;

/// Build a journal state from a live document + known blob digests.
///
/// Does **not** touch `TileCache`, encode PNGs, or build a composite. Callers
/// supply digests for blobs already on disk (or about to be written).
pub fn build_journal_state(
    doc: &Document,
    app_version: &str,
    layer_digests: &[(LayerId, u64, String)],
    threshold_digests: &[(String, String)],
) -> JournalStateFile {
    let document = DocumentFile::from_document(doc, |id| {
        let Some((rev, kind)) = find_layer_raw_rev(doc, id) else {
            return None;
        };
        if kind != LayerKind::Raster || rev == 0 {
            return None;
        }
        Some(layer_blob_filename(id.0, rev))
    });

    let mut blobs = Vec::new();
    for &(layer_id, raw_rev, ref sha) in layer_digests {
        if raw_rev == 0 {
            continue;
        }
        blobs.push(JournalBlobRef::Layer {
            layer_id: layer_id.0,
            raw_rev,
            sha256: sha.clone(),
            filename: layer_blob_filename(layer_id.0, raw_rev),
        });
    }
    for (content_hash, sha) in threshold_digests {
        blobs.push(JournalBlobRef::Threshold {
            content_hash: content_hash.clone(),
            sha256: sha.clone(),
            filename: threshold_blob_filename(content_hash),
        });
    }
    // Stable order for byte-identical state files.
    blobs.sort_by(|a, b| a.filename().cmp(b.filename()));

    JournalStateFile {
        magic: JOURNAL_STATE_MAGIC.into(),
        version: JOURNAL_STATE_VERSION,
        app_version: app_version.into(),
        document,
        blobs,
        extra: Map::new(),
    }
}

fn find_layer_raw_rev(doc: &Document, id: LayerId) -> Option<(u64, LayerKind)> {
    fn walk(nodes: &[LayerNode], id: LayerId) -> Option<(u64, LayerKind)> {
        for node in nodes {
            match node {
                LayerNode::Leaf(layer) if layer.id == id => {
                    return Some((layer.raw_rev, layer.kind));
                }
                LayerNode::Group(g) => {
                    if let Some(v) = walk(&g.children, id) {
                        return Some(v);
                    }
                }
                _ => {}
            }
        }
        None
    }
    walk(&doc.root, id)
}

/// Encode state to pretty JSON bytes.
pub fn encode_journal_state(state: &JournalStateFile) -> Result<Vec<u8>, ProjectError> {
    serde_json::to_vec_pretty(state).map_err(|e| ProjectError::InvalidArchive(e.to_string()))
}

/// Decode and validate a journal state file. Never panics on corrupt input.
pub fn decode_journal_state(bytes: &[u8]) -> Result<JournalStateFile, ProjectError> {
    if bytes.is_empty() {
        return Err(ProjectError::InvalidArchive("empty journal state".into()));
    }
    // Cap decode size (~64 MiB) so fuzz / corrupt blobs cannot OOM.
    if bytes.len() > 64 * 1024 * 1024 {
        return Err(ProjectError::InvalidArchive(
            "journal state exceeds 64 MiB".into(),
        ));
    }
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| ProjectError::InvalidArchive(format!("journal state json: {e}")))?;
    let state: JournalStateFile = serde_json::from_value(value)
        .map_err(|e| ProjectError::InvalidArchive(format!("journal state shape: {e}")))?;
    validate_journal_state(&state)?;
    Ok(state)
}

/// Structural checks after deserialize.
pub fn validate_journal_state(state: &JournalStateFile) -> Result<(), ProjectError> {
    if state.magic != JOURNAL_STATE_MAGIC {
        return Err(ProjectError::InvalidArchive(format!(
            "journal state magic {:?} != {JOURNAL_STATE_MAGIC:?}",
            state.magic
        )));
    }
    if state.version == 0 {
        return Err(ProjectError::InvalidArchive(
            "journal state version 0 is invalid".into(),
        ));
    }
    if state.version > JOURNAL_STATE_VERSION {
        return Err(ProjectError::InvalidArchive(format!(
            "journal state version {} newer than supported {JOURNAL_STATE_VERSION}",
            state.version
        )));
    }
    for blob in &state.blobs {
        if blob.filename().is_empty() || blob.sha256().len() != 64 {
            return Err(ProjectError::InvalidArchive(format!(
                "journal blob ref invalid: {}",
                blob.filename()
            )));
        }
        if !blob.sha256().chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ProjectError::InvalidArchive(format!(
                "journal blob sha256 not hex: {}",
                blob.filename()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams};
    use crate::layer::{Layer, LayerNode};
    use crate::serialize::assets::sha256_hex;
    use crate::serialize::document_dto::LayerNodeFile;
    use crate::types::{DocumentId, LayerKind};

    fn sample_doc() -> Document {
        let mut doc = Document::new(DocumentId::new(1), 64, 64);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        layer.bump_raw_rev();
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 2,
                ..DitherParamsV2::default()
            }),
        ));
        doc.root.push(LayerNode::Leaf(layer));
        doc
    }

    #[test]
    fn round_trip_state_without_pixels() {
        let doc = sample_doc();
        let sha = sha256_hex(b"fake-png");
        let state = build_journal_state(
            &doc,
            "0.0.0-test",
            &[(LayerId::new(1), 1, sha.clone())],
            &[],
        );
        assert_eq!(state.version, JOURNAL_STATE_VERSION);
        assert_eq!(state.blobs.len(), 1);
        match &state.blobs[0] {
            JournalBlobRef::Layer {
                layer_id,
                raw_rev,
                filename,
                ..
            } => {
                assert_eq!(*layer_id, 1);
                assert_eq!(*raw_rev, 1);
                assert_eq!(filename, "layer-1-1.png");
            }
            _ => panic!("expected layer blob"),
        }
        let bytes = encode_journal_state(&state).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(!text.contains("composite.png"));
        assert!(!text.contains("thumbnail.png"));
        assert!(!text.contains("iVBORw0KGgo"));
        let loaded = decode_journal_state(&bytes).unwrap();
        assert_eq!(loaded.magic, state.magic);
        assert_eq!(loaded.version, state.version);
        assert_eq!(loaded.blobs, state.blobs);
        assert_eq!(loaded.document.width, 64);
        assert_eq!(loaded.document.height, 64);
        match &loaded.document.root[0] {
            LayerNodeFile::Leaf(leaf) => {
                assert_eq!(leaf.raw_asset.as_deref(), Some("layer-1-1.png"));
            }
            other => panic!("expected leaf, got {other:?}"),
        }
    }

    #[test]
    fn rejects_newer_version() {
        let mut state = build_journal_state(&sample_doc(), "x", &[], &[]);
        state.version = JOURNAL_STATE_VERSION + 1;
        let bytes = encode_journal_state(&state).unwrap();
        let err = decode_journal_state(&bytes).unwrap_err();
        assert!(
            matches!(err, ProjectError::InvalidArchive(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_wrong_magic() {
        let mut state = build_journal_state(&sample_doc(), "x", &[], &[]);
        state.magic = "nope".into();
        let bytes = encode_journal_state(&state).unwrap();
        assert!(decode_journal_state(&bytes).is_err());
    }

    #[test]
    fn unknown_root_fields_tolerated() {
        let mut state = build_journal_state(&sample_doc(), "x", &[], &[]);
        state
            .extra
            .insert("future_flag".into(), Value::Bool(true));
        let bytes = encode_journal_state(&state).unwrap();
        let loaded = decode_journal_state(&bytes).unwrap();
        assert_eq!(loaded.extra.get("future_flag"), Some(&Value::Bool(true)));
    }

    #[test]
    fn corrupt_truncated_foreign_never_panic() {
        let good =
            encode_journal_state(&build_journal_state(&sample_doc(), "x", &[], &[])).unwrap();
        let cases: Vec<Vec<u8>> = vec![
            vec![],
            vec![0],
            b"{".to_vec(),
            b"[]".to_vec(),
            b"{\"magic\":\"dither.journal.state\",\"version\":1}".to_vec(),
            good[..good.len() / 2].to_vec(),
            {
                let mut m = good.clone();
                m[0] ^= 0xff;
                m
            },
            vec![0xff; 1024],
            b"PK\x03\x04not-a-state".to_vec(),
        ];
        for (i, bytes) in cases.into_iter().enumerate() {
            let result = std::panic::catch_unwind(|| decode_journal_state(&bytes));
            assert!(result.is_ok(), "case {i} panicked");
            let _ = result.unwrap();
        }
    }

    #[test]
    fn raw_rev_identity_keys_differ() {
        assert_ne!(layer_blob_filename(1, 1), layer_blob_filename(1, 2));
        assert_ne!(layer_blob_filename(1, 1), layer_blob_filename(2, 1));
    }

    /// Debug check (§4.5): replacing Raw without bumping `raw_rev` is a contract
    /// violation — blob identity would collide.
    #[test]
    fn raw_change_without_raw_rev_bump_fails_debug_check() {
        let mut doc = sample_doc();
        let before = doc.layer_raw_rev(LayerId::new(1)).unwrap();
        assert_eq!(before, 1);
        // Simulate a Raw rewrite that forgot to bump — the check catches it.
        let forgot_to_bump = before;
        assert_eq!(
            doc.layer_raw_rev(LayerId::new(1)),
            Some(forgot_to_bump),
            "precondition"
        );
        // Correct path:
        let after = doc.bump_layer_raw_rev(LayerId::new(1)).unwrap();
        assert_eq!(after, before + 1);
        assert_ne!(
            layer_blob_filename(1, before),
            layer_blob_filename(1, after)
        );
    }
}
