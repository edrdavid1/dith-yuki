//! Tauri commands for user filter-stack presets.

use std::collections::HashSet;
use std::sync::Arc;

use engine_project::{flatten_bottom_to_top, FilterPreset, LayerRef};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::commands::{
    emit_document_changed, layer_needs_dither_cache_reset, request_preview_refresh, AppState,
};
use crate::preset_library::{self, PresetLibraryEntry};

#[derive(Debug, Clone, Serialize)]
pub struct PresetApplyResponse {
    pub layer_id: u32,
    pub filter_count: usize,
    pub warnings: Vec<String>,
    pub missing_palette_ids: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PresetSaveRequest {
    pub doc_id: u32,
    /// Layer to snapshot; default = selected-style (first leaf with filters, else first leaf).
    #[serde(default)]
    pub layer_id: Option<u32>,
    pub name: String,
    /// When set, overwrite this preset id; otherwise allocate a new id.
    #[serde(default)]
    pub preset_id: Option<String>,
}

fn resolve_layer_id(
    snapshot: &engine_project::Document,
    layer_id: Option<u32>,
) -> Result<u32, String> {
    let leaves: Vec<&engine_project::Layer> = flatten_bottom_to_top(&snapshot.root)
        .into_iter()
        .filter_map(|r| match r {
            LayerRef::Leaf(l) => Some(l),
            _ => None,
        })
        .collect();
    if leaves.is_empty() {
        return Err("Document has no layers".into());
    }
    if let Some(id) = layer_id {
        if leaves.iter().any(|l| l.id.0 == id) {
            return Ok(id);
        }
        return Err(format!("Layer {id} not found"));
    }
    if let Some(with_filters) = leaves.iter().find(|l| !l.filters.is_empty()) {
        return Ok(with_filters.id.0);
    }
    Ok(leaves[0].id.0)
}

fn layer_filters(
    snapshot: &engine_project::Document,
    layer_id: u32,
) -> Result<Vec<engine_project::FilterInstance>, String> {
    let leaves: Vec<&engine_project::Layer> = flatten_bottom_to_top(&snapshot.root)
        .into_iter()
        .filter_map(|r| match r {
            LayerRef::Leaf(l) => Some(l),
            _ => None,
        })
        .collect();
    leaves
        .into_iter()
        .find(|l| l.id.0 == layer_id)
        .map(|l| l.filters.clone())
        .ok_or_else(|| format!("Layer {layer_id} not found"))
}

#[tauri::command]
pub fn preset_list(app: AppHandle) -> Result<Vec<PresetLibraryEntry>, String> {
    let data = preset_library::app_data_dir(&app)?;
    preset_library::list_library(&data)
}

#[tauri::command]
pub fn preset_save(
    req: PresetSaveRequest,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PresetLibraryEntry, String> {
    let session = state
        .require_session(req.doc_id)
        .map_err(|e| e.to_string())?;
    let snapshot = session.document_handle.snapshot();
    let layer_id = resolve_layer_id(&snapshot, req.layer_id)?;
    let filters = layer_filters(&snapshot, layer_id)?;
    if filters.is_empty() {
        return Err("Layer has no filters to save as a preset".into());
    }

    let id = req
        .preset_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| preset_library::allocate_id(&req.name));
    let created_at = {
        let data = preset_library::app_data_dir(&app)?;
        if let Ok(existing) = preset_library::load_preset(&data, &id) {
            existing.created_at
        } else {
            chrono_like_now()
        }
    };

    let preset = FilterPreset::from_stack(id, req.name, created_at, &filters, None)
        .map_err(|e| e.to_string())?;

    let data = preset_library::app_data_dir(&app)?;
    preset_library::save_preset(&data, preset)
}

#[tauri::command]
pub fn preset_load(preset_id: String, app: AppHandle) -> Result<FilterPreset, String> {
    let data = preset_library::app_data_dir(&app)?;
    preset_library::load_preset(&data, &preset_id)
}

#[tauri::command]
pub fn preset_delete(preset_id: String, app: AppHandle) -> Result<(), String> {
    let data = preset_library::app_data_dir(&app)?;
    preset_library::delete_preset(&data, &preset_id)
}

#[tauri::command]
pub fn preset_rename(
    preset_id: String,
    name: String,
    app: AppHandle,
) -> Result<PresetLibraryEntry, String> {
    let data = preset_library::app_data_dir(&app)?;
    preset_library::rename_preset(&data, &preset_id, &name)
}

#[tauri::command]
pub fn preset_apply_to_doc(
    doc_id: u32,
    preset_id: String,
    layer_id: Option<u32>,
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<PresetApplyResponse, String> {
    let data = preset_library::app_data_dir(&app)?;
    let preset = preset_library::load_preset(&data, &preset_id)?;

    let session = state.require_session(doc_id).map_err(|e| e.to_string())?;
    let snapshot = session.document_handle.snapshot();
    let target_layer = resolve_layer_id(&snapshot, layer_id)?;

    let available: HashSet<u32> = snapshot.palettes.iter().map(|p| p.id).collect();
    let applied = preset.apply_tolerant(&available, None);

    crate::undo::with_document_undo(&state, Some(&app), doc_id, || {
        let mut found = false;
        state
            .require_session(doc_id)?
            .document_handle
            .mutate(|doc| {
                fn replace_filters(
                    nodes: &mut [engine_project::LayerNode],
                    layer_id: u32,
                    filters: &[engine_project::FilterInstance],
                ) -> bool {
                    for node in nodes.iter_mut() {
                        match node {
                            engine_project::LayerNode::Leaf(layer) => {
                                if layer.id.0 == layer_id {
                                    layer.filters = filters.to_vec();
                                    return true;
                                }
                            }
                            engine_project::LayerNode::Group(group) => {
                                if replace_filters(&mut group.children, layer_id, filters) {
                                    return true;
                                }
                            }
                        }
                    }
                    false
                }
                found = replace_filters(&mut doc.root, target_layer, &applied.filters);
                if found {
                    doc.increment_generation();
                }
            });
        if !found {
            return Err(format!("Layer {target_layer} not found"));
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    request_preview_refresh(
        &state,
        target_layer,
        layer_needs_dither_cache_reset(
            &state
                .require_session(doc_id)
                .map_err(|e| e.to_string())?
                .document_handle
                .snapshot()
                .root,
            target_layer,
        ),
    );
    emit_document_changed(&app, "preset_applied", Some(target_layer), Some(doc_id));

    Ok(PresetApplyResponse {
        layer_id: target_layer,
        filter_count: applied.filters.len(),
        warnings: applied.warnings,
        missing_palette_ids: applied.missing_palette_ids,
    })
}

fn chrono_like_now() -> String {
    // Avoid a new chrono dependency — RFC3339-ish UTC from system time.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Minimal ISO-ish stamp; UI formats via Date.parse when possible.
    format!("{secs}")
}
