//! User filter-stack presets — JSON files in `{app_data_dir}/presets/`.

use std::fs;
use std::path::{Path, PathBuf};

use engine_project::serialize::sanitize_filename;
use engine_project::{FilterPreset, PRESET_VERSION};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresetLibraryEntry {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub path: String,
    pub filter_count: usize,
    pub palette_ids: Vec<u32>,
}

pub fn presets_dir(app_data: &Path) -> PathBuf {
    app_data.join("presets")
}

pub fn ensure_presets_dir(app_data: &Path) -> Result<PathBuf, String> {
    let dir = presets_dir(app_data);
    fs::create_dir_all(&dir).map_err(|e| format!("create presets dir: {e}"))?;
    Ok(dir)
}

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))
}

pub fn preset_path(app_data: &Path, id: &str) -> PathBuf {
    presets_dir(app_data).join(format!("{id}.json"))
}

fn unique_id(preferred: &str) -> String {
    let base = sanitize_filename(preferred, 48);
    let base = if base.is_empty() || base == "_" {
        "preset".into()
    } else {
        base
    };
    let short = &Uuid::new_v4().to_string()[..8];
    format!("{base}_{short}")
}

fn entry_from_preset(preset: &FilterPreset, path: &Path) -> PresetLibraryEntry {
    PresetLibraryEntry {
        id: preset.id.clone(),
        name: preset.name.clone(),
        created_at: preset.created_at.clone(),
        path: path.to_string_lossy().into_owned(),
        filter_count: preset.filters.len(),
        palette_ids: preset.palette_ids.clone(),
    }
}

pub fn list_library(app_data: &Path) -> Result<Vec<PresetLibraryEntry>, String> {
    let dir = presets_dir(app_data);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).map_err(|e| format!("list presets: {e}"))? {
        let entry = entry.map_err(|e| format!("list presets: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("skip preset {}: {e}", path.display());
                continue;
            }
        };
        match FilterPreset::from_json_slice(&bytes) {
            Ok(preset) => out.push(entry_from_preset(&preset, &path)),
            Err(e) => log::warn!("skip preset {}: {e}", path.display()),
        }
    }
    out.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.name.cmp(&b.name)));
    Ok(out)
}

pub fn load_preset(app_data: &Path, id: &str) -> Result<FilterPreset, String> {
    let path = preset_path(app_data, id);
    let bytes = fs::read(&path).map_err(|e| format!("read preset: {e}"))?;
    FilterPreset::from_json_slice(&bytes).map_err(|e| e.to_string())
}

pub fn save_preset(app_data: &Path, mut preset: FilterPreset) -> Result<PresetLibraryEntry, String> {
    preset.validate_known_algorithms(None).map_err(|e| e.to_string())?;
    if preset.preset_version != PRESET_VERSION {
        preset.preset_version = PRESET_VERSION;
    }
    let name = preset.name.trim();
    if name.is_empty() || name.len() > 255 {
        return Err("Name must be 1–255 characters".into());
    }
    preset.name = name.to_string();

    ensure_presets_dir(app_data)?;
    if preset.id.trim().is_empty() {
        preset.id = unique_id(&preset.name);
    }
    // Prevent path traversal via crafted ids.
    let safe = sanitize_filename(&preset.id, 64);
    if safe != preset.id || safe.contains("..") {
        return Err("Invalid preset id".into());
    }
    let path = preset_path(app_data, &preset.id);
    let json = preset.to_json_pretty().map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json.as_bytes()).map_err(|e| format!("write preset: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("rename preset: {e}"))?;
    Ok(entry_from_preset(&preset, &path))
}

pub fn delete_preset(app_data: &Path, id: &str) -> Result<(), String> {
    let path = preset_path(app_data, id);
    if path.exists() {
        fs::remove_file(&path).map_err(|e| format!("delete preset: {e}"))?;
    }
    Ok(())
}

pub fn rename_preset(
    app_data: &Path,
    id: &str,
    new_name: &str,
) -> Result<PresetLibraryEntry, String> {
    let trimmed = new_name.trim();
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err("Name must be 1–255 characters".into());
    }
    let mut preset = load_preset(app_data, id)?;
    preset.name = trimmed.to_string();
    save_preset(app_data, preset)
}

pub fn allocate_id(name: &str) -> String {
    unique_id(name)
}
