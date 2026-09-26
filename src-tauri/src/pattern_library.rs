//! User pattern library — `.dyuki` files in `{app_data_dir}/patterns/`.
//!
//! Apply / save reuse existing export_pattern / import_pattern; this module
//! only manages the on-disk library listing (+ optional display-name index).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use engine_project::serialize::{peek_pattern_manifest, sanitize_filename};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternLibraryEntry {
    /// Stable library id (= filename stem).
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub path: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LibraryIndex {
    /// Optional display-name overrides (after Rename).
    #[serde(default)]
    names: HashMap<String, String>,
}

pub fn patterns_dir(app_data: &Path) -> PathBuf {
    app_data.join("patterns")
}

fn index_path(app_data: &Path) -> PathBuf {
    patterns_dir(app_data).join("index.json")
}

pub fn ensure_patterns_dir(app_data: &Path) -> Result<PathBuf, String> {
    let dir = patterns_dir(app_data);
    fs::create_dir_all(&dir).map_err(|e| format!("create patterns dir: {e}"))?;
    Ok(dir)
}

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))
}

pub fn library_path(app_data: &Path, id: &str) -> PathBuf {
    patterns_dir(app_data).join(format!("{id}.dyuki"))
}

fn load_index(app_data: &Path) -> LibraryIndex {
    let path = index_path(app_data);
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => LibraryIndex::default(),
    }
}

fn save_index(app_data: &Path, index: &LibraryIndex) -> Result<(), String> {
    ensure_patterns_dir(app_data)?;
    let json = serde_json::to_string_pretty(index).map_err(|e| e.to_string())?;
    fs::write(index_path(app_data), json).map_err(|e| format!("write index: {e}"))
}

fn unique_id(preferred: &str) -> String {
    let base = sanitize_filename(preferred, 48);
    let base = if base.is_empty() || base == "_" {
        "pattern".into()
    } else {
        base
    };
    let short = &Uuid::new_v4().to_string()[..8];
    format!("{base}_{short}")
}

pub fn list_library(app_data: &Path) -> Result<Vec<PatternLibraryEntry>, String> {
    let dir = patterns_dir(app_data);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let index = load_index(app_data);
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("list patterns: {e}"))? {
        let entry = entry.map_err(|e| format!("list patterns: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("dyuki") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("pattern")
            .to_string();
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("skip pattern {}: {e}", path.display());
                continue;
            }
        };
        match peek_pattern_manifest(&bytes) {
            Ok(manifest) => {
                let name = index
                    .names
                    .get(&id)
                    .cloned()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        if manifest.name.is_empty() {
                            id.clone()
                        } else {
                            manifest.name
                        }
                    });
                out.push(PatternLibraryEntry {
                    id,
                    name,
                    created_at: manifest.created_at,
                    path: path.to_string_lossy().into_owned(),
                });
            }
            Err(e) => {
                log::warn!("skip pattern {}: {e}", path.display());
            }
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// Allocate a library path for a new save (caller writes via export_pattern).
pub fn allocate_save_path(app_data: &Path, name: &str) -> Result<(String, PathBuf), String> {
    let dir = ensure_patterns_dir(app_data)?;
    let id = unique_id(name);
    let path = dir.join(format!("{id}.dyuki"));
    Ok((id, path))
}

pub fn delete_from_library(app_data: &Path, id: &str) -> Result<(), String> {
    let path = library_path(app_data, id);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("delete pattern: {e}"))?;
    }
    let mut index = load_index(app_data);
    if index.names.remove(id).is_some() {
        save_index(app_data, &index)?;
    }
    Ok(())
}

pub fn rename_in_library(
    app_data: &Path,
    id: &str,
    new_name: &str,
) -> Result<PatternLibraryEntry, String> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err("Name must be 1–255 characters".into());
    }
    let path = library_path(app_data, id);
    if !path.exists() {
        return Err(format!("Pattern '{id}' not found"));
    }
    let mut index = load_index(app_data);
    index.names.insert(id.to_string(), trimmed.to_string());
    save_index(app_data, &index)?;
    let bytes = fs::read(&path).map_err(|e| format!("read pattern: {e}"))?;
    let created_at = peek_pattern_manifest(&bytes)
        .map(|m| m.created_at)
        .unwrap_or_default();
    Ok(PatternLibraryEntry {
        id: id.to_string(),
        name: trimmed.to_string(),
        created_at,
        path: path.to_string_lossy().into_owned(),
    })
}

/// Copy an external `.dyuki` into the library.
pub fn import_file_to_library(app_data: &Path, source: &Path) -> Result<PatternLibraryEntry, String> {
    let bytes = fs::read(source).map_err(|e| format!("read pattern: {e}"))?;
    let manifest = peek_pattern_manifest(&bytes).map_err(|e| e.to_string())?;
    let (id, dest) = allocate_save_path(app_data, &manifest.name)?;
    fs::write(&dest, &bytes).map_err(|e| format!("copy pattern: {e}"))?;
    Ok(PatternLibraryEntry {
        id,
        name: if manifest.name.is_empty() {
            id_fallback(&dest)
        } else {
            manifest.name
        },
        created_at: manifest.created_at,
        path: dest.to_string_lossy().into_owned(),
    })
}

fn id_fallback(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Pattern")
        .to_string()
}

pub fn export_from_library(app_data: &Path, id: &str, dest: &Path) -> Result<(), String> {
    let src = library_path(app_data, id);
    if !src.exists() {
        return Err(format!("Pattern '{id}' not found"));
    }
    fs::copy(&src, dest).map_err(|e| format!("export pattern: {e}"))?;
    Ok(())
}

pub fn resolve_library_file(app_data: &Path, id: &str) -> Result<PathBuf, String> {
    let path = library_path(app_data, id);
    if !path.exists() {
        return Err(format!("Pattern '{id}' not found"));
    }
    Ok(path)
}
