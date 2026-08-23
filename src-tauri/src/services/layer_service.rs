use std::sync::Arc;
use tauri::AppHandle;

use engine_project::commands as engine_commands;
use engine_project::layer::LayerNode;
use engine_project::types::{BlendMode, LayerId, LayerKind};
use engine_project::commands::{AddLayerArgs, LayerPropsPatch};
use serde::{Deserialize, Serialize};

use crate::commands::{emit_document_changed, schedule_dirty_viewport_tiles, AppState};
use crate::services::AppError;

#[derive(Debug, Clone, Deserialize)]
pub struct AddLayerRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub kind: String, // "raster" or "adjustment"
    pub parent_group: Option<u32>,
    pub index: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SetLayerPropsRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub name: Option<String>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<String>,
    pub visible: Option<bool>,
    pub offset: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReorderLayerRequest {
    #[serde(alias = "docId")]
    pub doc_id: u32,
    pub layer_id: u32,
    pub new_parent: Option<u32>,
    pub new_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerPropsPatchDto {
    pub name: Option<String>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<String>,
    pub visible: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerNodeDto {
    pub id: u32,
    pub name: String,
    pub kind: String, // "raster" | "adjustment" | "group"
    pub blend_mode: String,
    pub opacity: f32,
    pub visible: bool,
    pub children: Option<Vec<LayerNodeDto>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LayerIdResponse {
    pub layer_id: u32,
}

pub fn layer_nodes_to_dto(nodes: &[LayerNode]) -> Vec<LayerNodeDto> {
    nodes.iter().map(layer_node_to_flat_dto).collect()
}

fn layer_node_to_flat_dto(node: &LayerNode) -> LayerNodeDto {
    match node {
        LayerNode::Leaf(layer) => {
            let kind = match layer.kind {
                LayerKind::Raster => "raster",
                LayerKind::Adjustment => "adjustment",
            };
            LayerNodeDto {
                id: layer.id.0,
                name: layer.name.clone(),
                kind: kind.to_string(),
                blend_mode: layer.blend_mode.to_string(),
                opacity: layer.opacity,
                visible: layer.visible,
                children: None,
            }
        }
        LayerNode::Group(group) => {
            let children = layer_nodes_to_dto(&group.children);
            LayerNodeDto {
                id: group.id.0,
                name: group.name.clone(),
                kind: "group".to_string(),
                blend_mode: group.blend_mode.to_string(),
                opacity: group.opacity,
                visible: group.visible,
                children: Some(children),
            }
        }
    }
}

pub struct LayerService {
    state: Arc<AppState>,
}

impl LayerService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub fn get_layer_tree(&self) -> Result<Vec<LayerNodeDto>, AppError> {
        let Ok(session) = self.state.active_session() else {
            return Ok(vec![]);
        };
        let snapshot = session.document_handle.snapshot();
        Ok(layer_nodes_to_dto(&snapshot.root))
    }

    pub fn add_layer(
        &self,
        app_handle: &AppHandle,
        req: AddLayerRequest,
    ) -> Result<LayerIdResponse, AppError> {
        let doc_id = req.doc_id;
        let kind = match req.kind.as_str() {
            "raster" => LayerKind::Raster,
            "adjustment" => LayerKind::Adjustment,
            _ => return Err(AppError::InvalidOperation("Invalid layer kind".to_string())),
        };

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let width = snapshot.width;
            let height = snapshot.height;
            let engine_doc_id = snapshot.id;
            drop(snapshot);

            let args = AddLayerArgs {
                kind,
                parent_group: req.parent_group.map(LayerId::new),
                index: req.index,
                width,
                height,
            };

            match engine_commands::add_layer(
                &self.state.require_session(doc_id)?.document_handle,
                &self.state.tiles.tile_cache,
                engine_doc_id,
                args,
            ) {
                Ok(layer_id) => {
                    emit_document_changed(app_handle, "layer_added", Some(layer_id.0), Some(doc_id));
                    Ok(LayerIdResponse { layer_id: layer_id.0 })
                }
                Err(e) => Err(format!("Failed to add layer: {:?}", e)),
            }
        })
        .map_err(AppError::Generic)
    }

    pub fn remove_layer(
        &self,
        app_handle: &AppHandle,
        doc_id: u32,
        layer_id: u32,
    ) -> Result<(), AppError> {
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let engine_doc_id = snapshot.id;
            drop(snapshot);

            match engine_commands::remove_layer(
                &self.state.require_session(doc_id)?.document_handle,
                &self.state.tiles.tile_cache,
                engine_doc_id,
                LayerId::new(layer_id),
            ) {
                Ok(_) => {
                    emit_document_changed(app_handle, "layer_removed", Some(layer_id), Some(doc_id));
                    Ok(())
                }
                Err(e) => Err(format!("Failed to remove layer: {:?}", e)),
            }
        })
        .map_err(AppError::Generic)
    }

    pub fn set_layer_props(
        &self,
        app_handle: &AppHandle,
        req: SetLayerPropsRequest,
    ) -> Result<(), AppError> {
        let doc_id = req.doc_id;
        let blend_mode = req.blend_mode.as_ref().map(|bm| match bm.as_str() {
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
        });

        let is_visual_change =
            req.opacity.is_some() || req.blend_mode.is_some() || req.visible.is_some();
        let layer_id = req.layer_id;

        let patch = LayerPropsPatch {
            name: req.name,
            opacity: req.opacity,
            blend_mode,
            visible: req.visible,
            offset: req.offset,
        };

        let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
        let engine_doc_id = snapshot.id;
        drop(snapshot);

        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            match engine_commands::set_layer_props(
                &self.state.require_session(doc_id)?.document_handle,
                &self.state.tiles.tile_cache,
                engine_doc_id,
                LayerId::new(layer_id),
                patch,
            ) {
                Ok(_) => {
                    if is_visual_change && self.state.active_id() == Some(doc_id) {
                        schedule_dirty_viewport_tiles(&self.state);
                    }
                    emit_document_changed(app_handle, "layer_changed", Some(layer_id), Some(doc_id));
                    Ok(())
                }
                Err(e) => Err(format!("Failed to set layer props: {:?}", e)),
            }
        })
        .map_err(AppError::Generic)
    }

    pub fn reorder_layer(
        &self,
        app_handle: &AppHandle,
        req: ReorderLayerRequest,
    ) -> Result<(), AppError> {
        let doc_id = req.doc_id;
        crate::undo::with_document_undo(&self.state, Some(app_handle), doc_id, || {
            let snapshot = self.state.require_session(doc_id)?.document_handle.snapshot();
            let engine_doc_id = snapshot.id;
            drop(snapshot);

            match engine_commands::reorder_layer(
                &self.state.require_session(doc_id)?.document_handle,
                &self.state.tiles.tile_cache,
                engine_doc_id,
                LayerId::new(req.layer_id),
                req.new_parent.map(LayerId::new),
                req.new_index,
            ) {
                Ok(_) => {
                    emit_document_changed(app_handle, "layer_reordered", None, Some(doc_id));
                    Ok(())
                }
                Err(e) => Err(format!("Failed to reorder layer: {:?}", e)),
            }
        })
        .map_err(AppError::Generic)
    }
}
