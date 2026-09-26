//! Journal sidecar metadata (JSON next to the `.dyproj.journal` blob).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Soft cap for a full journal zip (policy C). Above this we skip the blob.
pub const JOURNAL_MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JournalContentKind {
    FullDyproj,
    SkippedTooLarge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalMeta {
    pub recovery_id: Uuid,
    pub runtime_doc_id: u32,
    pub display_name: String,
    pub project_path: Option<String>,
    pub source_path: Option<String>,
    pub original_mtime_ms: Option<u64>,
    pub written_at_ms: u64,
    pub app_version: String,
    pub content_kind: JournalContentKind,
    /// Soft-discard: keep journal until clean exit (Phase 3).
    #[serde(default)]
    pub discarded: bool,
}

pub fn recovery_subdir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("recovery")
}

pub fn journal_blob_path(recovery_dir: &Path, recovery_id: Uuid) -> PathBuf {
    recovery_dir.join(format!("{recovery_id}.dyproj.journal"))
}

pub fn journal_meta_path(recovery_dir: &Path, recovery_id: Uuid) -> PathBuf {
    recovery_dir.join(format!("{recovery_id}.meta.json"))
}

pub fn write_meta(recovery_dir: &Path, meta: &JournalMeta) -> Result<(), String> {
    fs::create_dir_all(recovery_dir).map_err(|e| e.to_string())?;
    let path = journal_meta_path(recovery_dir, meta.recovery_id);
    let json = serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?;
    engine_io::atomic_write(&path, &json).map_err(|e| e.to_string())
}

pub fn read_meta(path: &Path) -> Result<JournalMeta, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

pub fn list_metas(recovery_dir: &Path) -> Vec<JournalMeta> {
    let Ok(entries) = fs::read_dir(recovery_dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.ends_with(".meta.json") {
            continue;
        }
        if let Ok(meta) = read_meta(&path) {
            out.push(meta);
        }
    }
    out
}

pub fn file_mtime_ms(path: &Path) -> Option<u64> {
    let meta = fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    let dur = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(dur.as_millis() as u64)
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let recovery = recovery_subdir(dir.path());
        let id = Uuid::new_v4();
        let meta = JournalMeta {
            recovery_id: id,
            runtime_doc_id: 2,
            display_name: "a.dyproj".into(),
            project_path: Some("/tmp/a.dyproj".into()),
            source_path: None,
            original_mtime_ms: Some(100),
            written_at_ms: 200,
            app_version: "0.0.0".into(),
            content_kind: JournalContentKind::FullDyproj,
            discarded: false,
        };
        write_meta(&recovery, &meta).unwrap();
        let loaded = read_meta(&journal_meta_path(&recovery, id)).unwrap();
        assert_eq!(loaded.recovery_id, id);
        assert_eq!(loaded.display_name, "a.dyproj");
        let listed = list_metas(&recovery);
        assert_eq!(listed.len(), 1);
    }
}
