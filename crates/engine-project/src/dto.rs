//! Data Transfer Objects for serialization over Tauri IPC.

use crate::document::Document;
use crate::filter::FilterInstance;
use crate::layer::{LayerNode};
use crate::types::{DocumentId, FilterInstanceId, LayerId};
use serde::{Deserialize, Serialize};

/// Document snapshot sent to frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSnapshotDto {
    pub id: DocumentId,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub layers: Vec<LayerNodeDto>,
    pub palettes: Vec<crate::types::PaletteId>,
}

/// Layer or group node in the DTO representation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum LayerNodeDto {
    #[serde(rename = "raster")]
    Raster(LayerDto),
    #[serde(rename = "adjustment")]
    Adjustment(LayerDto),
    #[serde(rename = "group")]
    Group(LayerGroupDto),
}

/// Single raster or adjustment layer DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerDto {
    pub id: LayerId,
    pub name: String,
    pub blend_mode: String,
    pub opacity: f32,
    pub visible: bool,
    pub offset: (i32, i32),
    pub has_mask: bool,
    pub filters: Vec<FilterInstanceDto>,
    pub thumbnail_url: String,
}

/// Layer group DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerGroupDto {
    pub id: LayerId,
    pub name: String,
    pub blend_mode: String,
    pub opacity: f32,
    pub visible: bool,
    pub has_mask: bool,
    pub children: Vec<LayerNodeDto>,
}

/// Filter instance DTO.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterInstanceDto {
    pub id: FilterInstanceId,
    pub kind: String,
    pub params: serde_json::Value,
    pub enabled: bool,
}

/// Convert a Document to a DocumentSnapshotDto.
pub fn document_to_dto(doc: &Document) -> DocumentSnapshotDto {
    let layers = doc.root.iter().map(layer_node_to_dto).collect();

    DocumentSnapshotDto {
        id: doc.id,
        width: doc.width,
        height: doc.height,
        revision: doc.revision,
        layers,
        palettes: doc.palettes.clone(),
    }
}

/// Convert a LayerNode to a LayerNodeDto.
fn layer_node_to_dto(node: &LayerNode) -> LayerNodeDto {
    match node {
        LayerNode::Leaf(layer) => {
            let kind_str = match layer.kind {
                crate::types::LayerKind::Raster => "raster",
                crate::types::LayerKind::Adjustment => "adjustment",
            };

            let layer_dto = LayerDto {
                id: layer.id,
                name: layer.name.clone(),
                blend_mode: layer.blend_mode.to_string(),
                opacity: layer.opacity,
                visible: layer.visible,
                offset: layer.offset,
                has_mask: layer.mask.is_some(),
                filters: layer.filters.iter().map(filter_to_dto).collect(),
                thumbnail_url: format!(
                    "tile://doc/{}/layer/{}/stage/composite/l/8/0/0",
                    doc_id_to_u32(layer.id),
                    layer.id.0
                ),
            };

            match kind_str {
                "raster" => LayerNodeDto::Raster(layer_dto),
                "adjustment" => LayerNodeDto::Adjustment(layer_dto),
                _ => LayerNodeDto::Raster(layer_dto),
            }
        }
        LayerNode::Group(group) => {
            let group_dto = LayerGroupDto {
                id: group.id,
                name: group.name.clone(),
                blend_mode: group.blend_mode.to_string(),
                opacity: group.opacity,
                visible: group.visible,
                has_mask: group.mask.is_some(),
                children: group.children.iter().map(layer_node_to_dto).collect(),
            };
            LayerNodeDto::Group(group_dto)
        }
    }
}

/// Convert a FilterInstance to a FilterInstanceDto.
fn filter_to_dto(filter: &FilterInstance) -> FilterInstanceDto {
    let params = serde_json::to_value(&filter.params).unwrap_or(serde_json::json!(null));

    FilterInstanceDto {
        id: filter.id,
        kind: filter.kind.to_string(),
        params,
        enabled: filter.enabled,
    }
}

/// Helper: convert LayerId to u32 for URL generation
fn doc_id_to_u32(layer_id: LayerId) -> u32 {
    layer_id.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::Layer;
    use crate::types::LayerKind;

    #[test]
    fn document_to_dto_empty() {
        let doc = Document::default();
        let dto = document_to_dto(&doc);

        assert_eq!(dto.width, doc.width);
        assert_eq!(dto.height, doc.height);
        assert_eq!(dto.revision, doc.revision);
        assert!(dto.layers.is_empty());
    }

    #[test]
    fn document_to_dto_with_layer() {
        let mut doc = Document::default();
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        doc.root.push(LayerNode::Leaf(layer));

        let dto = document_to_dto(&doc);

        assert_eq!(dto.layers.len(), 1);
        matches!(dto.layers[0], LayerNodeDto::Raster(_));
    }

    #[test]
    fn filter_to_dto_serializes() {
        let filter = crate::filter::FilterInstance::new(
            crate::filter::FilterKind::Curves,
            crate::filter::FilterParams::Curves { curve: vec![], channel: crate::filters::curves::CurveChannel::All },
        );
        let dto = filter_to_dto(&filter);

        assert_eq!(dto.kind, "Curves");
        assert!(dto.enabled);
    }

    #[test]
    fn dto_round_trip_json() {
        let doc = Document::default();
        let dto = document_to_dto(&doc);
        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: DocumentSnapshotDto = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, dto.id);
        assert_eq!(deserialized.width, dto.width);
    }
}
