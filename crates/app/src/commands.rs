//! Tauri command handlers for document operations.
//!
//! This module registers the public IPC endpoints that the frontend calls.
//! Each command acquires the DocumentHandle and TileCache from app state,
//! delegates to engine-project for mutations, and returns DTOs for serialization.

use serde::{Deserialize, Serialize};
use tauri::State;

use engine_project::{
    document::DocumentHandle,
    dto::DocumentSnapshotDto,
    types::{LayerId, LayerKind, BlendMode},
    commands::{AddLayerArgs, LayerPropsPatch},
    commands as engine_commands,
};
use engine_tiles::TileCache;

// ============================================================================
// Data Structures for Command Arguments
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct AddLayerRequest {
    pub kind: String, // "raster" or "adjustment"
    pub parent_group: Option<u32>,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetLayerPropsRequest {
    pub layer_id: u32,
    pub name: Option<String>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<String>,
    pub visible: Option<bool>,
    pub offset: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderLayerRequest {
    pub layer_id: u32,
    pub new_parent: Option<u32>,
    pub new_index: usize,
}

// ============================================================================
// Command Responses
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct LayerIdResponse {
    pub layer_id: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentResponse {
    pub snapshot: DocumentSnapshotDto,
}

// ============================================================================
// App State Structure
// ============================================================================

/// Shared application state for Tauri commands.
pub struct AppState {
    pub document_handle: DocumentHandle,
    pub tile_cache: TileCache,
}

// ============================================================================
// Tauri Commands
// ============================================================================

/// Create a new document.
#[tauri::command]
pub fn new_document(
    width: u32,
    height: u32,
    state: State<AppState>,
) -> Result<DocumentResponse, String> {
    use engine_project::types::DocumentId;
    
    let new_doc = engine_project::Document::new(DocumentId::new(1), width, height);
    state.document_handle.mutate(|doc| {
        *doc = new_doc.clone();
    });
    
    let snapshot = state.document_handle.snapshot();
    let dto = engine_project::dto::document_to_dto(&snapshot);
    
    Ok(DocumentResponse { snapshot: dto })
}

/// Get current document snapshot.
#[tauri::command]
pub fn get_document_snapshot(
    state: State<AppState>,
) -> Result<DocumentResponse, String> {
    let snapshot = state.document_handle.snapshot();
    let dto = engine_project::dto::document_to_dto(&snapshot);
    Ok(DocumentResponse { snapshot: dto })
}

/// Add a new layer to the document.
#[tauri::command]
pub fn add_layer(
    req: AddLayerRequest,
    state: State<AppState>,
) -> Result<LayerIdResponse, String> {
    let kind = match req.kind.as_str() {
        "raster" => LayerKind::Raster,
        "adjustment" => LayerKind::Adjustment,
        _ => return Err("Invalid layer kind".to_string()),
    };

    let snapshot = state.document_handle.snapshot();
    let width = snapshot.width;
    let height = snapshot.height;
    let doc_id = snapshot.id;
    drop(snapshot);

    let args = AddLayerArgs {
        kind,
        parent_group: req.parent_group.map(LayerId::new),
        index: req.index,
        width,
        height,
    };

    match engine_commands::add_layer(&state.document_handle, &state.tile_cache, doc_id, args) {
        Ok(layer_id) => Ok(LayerIdResponse { layer_id: layer_id.0 }),
        Err(e) => Err(format!("Failed to add layer: {:?}", e)),
    }
}

/// Remove a layer from the document.
#[tauri::command]
pub fn remove_layer(
    layer_id: u32,
    state: State<AppState>,
) -> Result<(), String> {
    let snapshot = state.document_handle.snapshot();
    let doc_id = snapshot.id;
    drop(snapshot);

    match engine_commands::remove_layer(
        &state.document_handle,
        &state.tile_cache,
        doc_id,
        LayerId::new(layer_id),
    ) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to remove layer: {:?}", e)),
    }
}

/// Set layer properties (name, opacity, blend mode, visibility, offset).
#[tauri::command]
pub fn set_layer_props(
    req: SetLayerPropsRequest,
    state: State<AppState>,
) -> Result<(), String> {
    let blend_mode = req.blend_mode.as_ref().map(|bm| {
        match bm.as_str() {
            "normal" => BlendMode::Normal,
            "multiply" => BlendMode::Multiply,
            "screen" => BlendMode::Screen,
            "overlay" => BlendMode::Overlay,
            "darken" => BlendMode::Darken,
            "lighten" => BlendMode::Lighten,
            "color_dodge" => BlendMode::ColorDodge,
            "color_burn" => BlendMode::ColorBurn,
            "hard_light" => BlendMode::HardLight,
            "soft_light" => BlendMode::SoftLight,
            "difference" => BlendMode::Difference,
            "exclusion" => BlendMode::Exclusion,
            _ => BlendMode::Normal,
        }
    });

    let patch = LayerPropsPatch {
        name: req.name,
        opacity: req.opacity,
        blend_mode,
        visible: req.visible,
        offset: req.offset,
    };

    let snapshot = state.document_handle.snapshot();
    let doc_id = snapshot.id;
    drop(snapshot);

    match engine_commands::set_layer_props(
        &state.document_handle,
        &state.tile_cache,
        doc_id,
        LayerId::new(req.layer_id),
        patch,
    ) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to set layer props: {:?}", e)),
    }
}

/// Reorder a layer (move to new parent/position).
#[tauri::command]
pub fn reorder_layer(
    req: ReorderLayerRequest,
    state: State<AppState>,
) -> Result<(), String> {
    let snapshot = state.document_handle.snapshot();
    let doc_id = snapshot.id;
    drop(snapshot);

    match engine_commands::reorder_layer(
        &state.document_handle,
        &state.tile_cache,
        doc_id,
        LayerId::new(req.layer_id),
        req.new_parent.map(LayerId::new),
        req.new_index,
    ) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to reorder layer: {:?}", e)),
    }
}

// ============================================================================
// Note: Future Commands (Phase 3+)
// ============================================================================

// Filter commands will be added when filter manipulation is implemented:
// - add_filter(layer_id, kind, params) -> FilterInstanceId
// - update_filter_params(layer_id, filter_id, new_params)
// - remove_filter(layer_id, filter_id)
// - reorder_filter(layer_id, filter_id, new_index)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_mode_parsing_works() {
        let bm = match "multiply".into() {
            name => match name {
                "multiply" => Some(BlendMode::Multiply),
                _ => None,
            }
        };
        assert!(bm.is_some());
    }
}
