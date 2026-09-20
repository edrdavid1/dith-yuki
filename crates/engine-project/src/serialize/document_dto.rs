//! On-disk `document.json` shapes for `.dyproj` (no runtime-only fields).
//!
//! Persisted: tree, palettes (revision reset on load), `raw_asset` per raster layer.
//! Omitted / ignored: `Document.revision`, `generations`, `requires_full_row`.
//! CustomPng paths in file form are `{content_hash}.png` basenames only.

use crate::filter::{FilterInstance, FilterKind, FilterParams};
use crate::layer::{Layer, LayerGroup, LayerNode};
use crate::mask::MaskRef;
use crate::types::{BlendMode, ColorProfileRef, LayerId, LayerKind, TileBounds};
use engine_color::palette::{LinearColor, Palette};
use engine_registry::AlgorithmRegistry;
use serde::{Deserialize, Serialize};

/// Root of `document.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentFile {
    /// File-local document id (remap key only; runtime uses doc_id=1).
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub color_profile: ColorProfileRef,
    pub root: Vec<LayerNodeFile>,
    pub palettes: Vec<PaletteFile>,
}

/// Palette without relying on live revision semantics (always rewritten to 1 on open).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaletteFile {
    pub id: u32,
    pub name: String,
    pub colors: Vec<LinearColor>,
}

/// Layer or group node in the file tree.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum LayerNodeFile {
    Leaf(LayerFile),
    Group(LayerGroupFile),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerFile {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,
    pub blend_mode: BlendMode,
    pub opacity: f32,
    pub visible: bool,
    pub offset: (i32, i32),
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<MaskRef>,
    pub filters: Vec<FilterInstanceFile>,
    pub bounds_l0: TileBounds,
    /// Basename under `layers/` (e.g. `"3.png"`). Absent for adjustment layers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_asset: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerGroupFile {
    pub id: LayerId,
    pub name: String,
    pub blend_mode: BlendMode,
    pub opacity: f32,
    pub visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<MaskRef>,
    pub children: Vec<LayerNodeFile>,
}

fn default_filter_opacity() -> f32 {
    1.0
}

/// Filter instance without `requires_full_row` (recomputed on load).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterInstanceFile {
    pub id: crate::types::FilterInstanceId,
    pub kind: FilterKind,
    /// Tagged `FilterParams` JSON, or verbatim unknown-algorithm params (task 5.2).
    pub params: serde_json::Value,
    pub enabled: bool,
    #[serde(default = "default_filter_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub blend_mode: BlendMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
}

impl DocumentFile {
    /// Build file DTO from a live document.
    ///
    /// `raw_asset_for` maps each raster `LayerId` to its `layers/` basename.
    /// Adjustment layers get `raw_asset: None`. CustomPng paths must already be
    /// rewritten to `{hash}.png` basenames by the caller before serialize.
    pub fn from_document(
        doc: &crate::document::Document,
        mut raw_asset_for: impl FnMut(LayerId) -> Option<String>,
    ) -> Self {
        Self {
            id: doc.id.0,
            width: doc.width,
            height: doc.height,
            color_profile: doc.color_profile.clone(),
            root: doc
                .root
                .iter()
                .map(|n| layer_node_to_file(n, &mut raw_asset_for))
                .collect(),
            palettes: doc
                .palettes
                .iter()
                .map(|p| PaletteFile {
                    id: p.id,
                    name: p.name.clone(),
                    colors: p.colors.clone(),
                })
                .collect(),
        }
    }
}

fn layer_node_to_file(
    node: &LayerNode,
    raw_asset_for: &mut impl FnMut(LayerId) -> Option<String>,
) -> LayerNodeFile {
    match node {
        LayerNode::Leaf(layer) => LayerNodeFile::Leaf(layer_to_file(layer, raw_asset_for)),
        LayerNode::Group(group) => LayerNodeFile::Group(LayerGroupFile {
            id: group.id,
            name: group.name.clone(),
            blend_mode: group.blend_mode,
            opacity: group.opacity,
            visible: group.visible,
            mask: group.mask.clone(),
            children: group
                .children
                .iter()
                .map(|c| layer_node_to_file(c, raw_asset_for))
                .collect(),
        }),
    }
}

fn layer_to_file(
    layer: &Layer,
    raw_asset_for: &mut impl FnMut(LayerId) -> Option<String>,
) -> LayerFile {
    let raw_asset = match layer.kind {
        LayerKind::Raster => raw_asset_for(layer.id),
        LayerKind::Adjustment => None,
    };
    LayerFile {
        id: layer.id,
        name: layer.name.clone(),
        kind: layer.kind,
        blend_mode: layer.blend_mode,
        opacity: layer.opacity,
        visible: layer.visible,
        offset: layer.offset,
        mask: layer.mask.clone(),
        filters: layer.filters.iter().map(filter_to_file).collect(),
        bounds_l0: layer.bounds_l0,
        raw_asset,
    }
}

pub fn filter_to_file(f: &FilterInstance) -> FilterInstanceFile {
    let params = match &f.params {
        FilterParams::Placeholder(p) if p.raw_params.is_some() => p.raw_params.clone().unwrap(),
        other => serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
    };
    FilterInstanceFile {
        id: f.id,
        kind: f.kind,
        params,
        enabled: f.enabled,
        opacity: f.opacity,
        blend_mode: f.blend_mode,
        algorithm_id: f.algorithm_id.clone(),
        schema_version: f.schema_version,
    }
}

/// Rebuild a runtime [`FilterInstance`] with fresh `requires_full_row` from kind/params.
///
/// When `algorithm_id` is unknown, the instance becomes a disabled Placeholder
/// that round-trips `params` verbatim (Req 8.1, 8.4).
pub fn filter_from_file(
    f: &FilterInstanceFile,
    registry: Option<&AlgorithmRegistry>,
) -> FilterInstance {
    let registry = registry.unwrap_or_else(|| crate::algorithms::builtin_registry());
    let mut params_json = f.params.clone();

    if let Some(id) = f.algorithm_id.as_deref() {
        if registry.get_by_str(id).is_none() {
            let mut inst = FilterInstance::new(
                FilterKind::Placeholder,
                FilterParams::Placeholder(crate::filter::PlaceholderParams {
                    label: id.to_string(),
                    raw_params: Some(f.params.clone()),
                }),
            );
            inst.id = f.id;
            inst.enabled = false;
            inst.opacity = f.opacity;
            inst.blend_mode = f.blend_mode;
            inst.algorithm_id = Some(id.to_string());
            inst.schema_version = f.schema_version;
            return inst;
        }
        if let Some(algo) = registry.get_by_str(id) {
            let mut inner = inner_params_json(&params_json);
            let before = inner.clone();
            let old_ver = f.schema_version.unwrap_or(1);
            algo.migrate_params(old_ver, &mut inner);
            if inner != before {
                params_json = rewrap_params_json(&f.params, inner);
            }
        }
    }

    let params = serde_json::from_value::<FilterParams>(params_json.clone()).unwrap_or_else(|_| {
        FilterParams::Placeholder(crate::filter::PlaceholderParams {
            label: f.algorithm_id.clone().unwrap_or_else(|| "invalid".into()),
            raw_params: Some(params_json),
        })
    });
    let mut inst = FilterInstance::new(f.kind, params);
    inst.id = f.id;
    inst.enabled = f.enabled;
    inst.opacity = f.opacity;
    inst.blend_mode = f.blend_mode;
    inst.algorithm_id = f.algorithm_id.clone();
    inst.schema_version = f.schema_version;
    inst
}

fn inner_params_json(params: &serde_json::Value) -> serde_json::Value {
    if let Ok(fp) = serde_json::from_value::<FilterParams>(params.clone()) {
        crate::filter::filter_params_to_json(&fp).unwrap_or_else(|_| params.clone())
    } else {
        params.clone()
    }
}

fn rewrap_params_json(original: &serde_json::Value, inner: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = original.as_object() {
        if obj.len() == 1 {
            if let Some(tag) = obj.keys().next() {
                return serde_json::json!({ tag: inner });
            }
        }
    }
    inner
}

/// Convert file layer node to runtime (ids already remapped).
pub fn layer_node_from_file(node: &LayerNodeFile) -> LayerNode {
    match node {
        LayerNodeFile::Leaf(layer) => LayerNode::Leaf(Layer {
            id: layer.id,
            name: layer.name.clone(),
            kind: layer.kind,
            blend_mode: layer.blend_mode,
            opacity: layer.opacity,
            visible: layer.visible,
            offset: layer.offset,
            mask: layer.mask.clone(),
            filters: layer
                .filters
                .iter()
                .map(|f| filter_from_file(f, None))
                .collect(),
            bounds_l0: layer.bounds_l0,
        }),
        LayerNodeFile::Group(group) => LayerNode::Group(LayerGroup {
            id: group.id,
            name: group.name.clone(),
            blend_mode: group.blend_mode,
            opacity: group.opacity,
            visible: group.visible,
            mask: group.mask.clone(),
            children: group.children.iter().map(layer_node_from_file).collect(),
        }),
    }
}

/// Convert file palettes to runtime with `revision = 1`.
pub fn palettes_from_file(palettes: &[PaletteFile]) -> Vec<Palette> {
    palettes
        .iter()
        .map(|p| Palette {
            id: p.id,
            name: p.name.clone(),
            colors: p.colors.clone(),
            revision: 1,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::filter::{DitherModeV2, DitherParamsV2};
    use crate::mask::MaskStorage;
    use crate::types::{DocumentId, FilterInstanceId, PaletteId};

    fn file_params(p: FilterParams) -> serde_json::Value {
        serde_json::to_value(p).unwrap()
    }

    #[test]
    fn document_file_omits_requires_full_row_and_sets_raw_asset() {
        let mut doc = Document::new(DocumentId::new(1), 64, 64);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 64, 64);
        let mut filt = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 2,
                ..DitherParamsV2::default()
            }),
        );
        assert!(filt.requires_full_row);
        filt.id = FilterInstanceId::new();
        layer.filters.push(filt);
        doc.root.push(LayerNode::Leaf(layer));

        let file = DocumentFile::from_document(&doc, |id| Some(format!("{}.png", id.0)));
        let json = serde_json::to_string(&file).unwrap();
        assert!(!json.contains("requires_full_row"));
        assert!(json.contains("\"raw_asset\":\"1.png\""));

        match &file.root[0] {
            LayerNodeFile::Leaf(l) => {
                assert_eq!(l.raw_asset.as_deref(), Some("1.png"));
                assert_eq!(l.filters.len(), 1);
            }
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn adjustment_layer_has_no_raw_asset() {
        let mut doc = Document::new(DocumentId::new(1), 32, 32);
        let layer = Layer::new(LayerId::new(2), LayerKind::Adjustment, 32, 32);
        doc.root.push(LayerNode::Leaf(layer));
        let file = DocumentFile::from_document(&doc, |_| Some("should_not.png".into()));
        match &file.root[0] {
            LayerNodeFile::Leaf(l) => assert!(l.raw_asset.is_none()),
            _ => panic!("expected leaf"),
        }
    }

    #[test]
    fn filter_from_file_recomputes_requires_full_row() {
        let f = FilterInstanceFile {
            id: FilterInstanceId::new(),
            kind: FilterKind::Dither,
            params: file_params(FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Atkinson,
                levels: 4,
                ..DitherParamsV2::default()
            })),
            enabled: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            algorithm_id: None,
            schema_version: None,
        };
        let inst = filter_from_file(&f, None);
        assert!(inst.requires_full_row);
        assert_eq!(inst.id, f.id);
        assert_eq!(inst.opacity, 1.0);
        assert_eq!(inst.blend_mode, BlendMode::Normal);
    }

    #[test]
    fn filter_instance_file_missing_opacity_blend_defaults() {
        let f = FilterInstanceFile {
            id: FilterInstanceId::new(),
            kind: FilterKind::Dither,
            params: file_params(FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Atkinson,
                levels: 4,
                ..DitherParamsV2::default()
            })),
            enabled: true,
            opacity: 0.5,
            blend_mode: BlendMode::Multiply,
            algorithm_id: None,
            schema_version: None,
        };
        let mut value = serde_json::to_value(&f).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("opacity");
        obj.remove("blend_mode");
        let restored: FilterInstanceFile = serde_json::from_value(value).unwrap();
        assert_eq!(restored.opacity, 1.0);
        assert_eq!(restored.blend_mode, BlendMode::Normal);
        let inst = filter_from_file(&restored, None);
        assert_eq!(inst.opacity, 1.0);
        assert_eq!(inst.blend_mode, BlendMode::Normal);
    }

    #[test]
    fn custom_png_basename_round_trips_in_json() {
        let f = FilterInstanceFile {
            id: FilterInstanceId::new(),
            kind: FilterKind::Dither,
            params: file_params(FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::CustomPng {
                    path: "abcdef0123456789abcdef0123456789.png".into(),
                },
                levels: 2,
                ..DitherParamsV2::default()
            })),
            enabled: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            algorithm_id: None,
            schema_version: None,
        };
        let s = serde_json::to_string(&f).unwrap();
        assert!(s.contains("abcdef0123456789abcdef0123456789.png"));
        assert!(!s.contains('/'));
    }

    #[test]
    fn algorithm_id_and_schema_version_roundtrip() {
        let mut inst = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                ..DitherParamsV2::default()
            }),
        );
        inst.algorithm_id = Some("bayer_4x4".into());
        inst.schema_version = Some(1);
        let file = filter_to_file(&inst);
        assert_eq!(file.algorithm_id.as_deref(), Some("bayer_4x4"));
        assert_eq!(file.schema_version, Some(1));
        let json = serde_json::to_value(&file).unwrap();
        assert_eq!(json["algorithm_id"], "bayer_4x4");
        assert_eq!(json["schema_version"], 1);
        let restored: FilterInstanceFile = serde_json::from_value(json).unwrap();
        let loaded = filter_from_file(&restored, None);
        assert_eq!(loaded.algorithm_id.as_deref(), Some("bayer_4x4"));
        assert_eq!(loaded.schema_version, Some(1));
    }

    #[test]
    fn mask_external_serializes() {
        let layer = LayerFile {
            id: LayerId::new(1),
            name: "L".into(),
            kind: LayerKind::Raster,
            blend_mode: BlendMode::Normal,
            opacity: 1.0,
            visible: true,
            offset: (0, 0),
            mask: Some(MaskRef {
                storage: MaskStorage::External(LayerId::new(99)),
                enabled: true,
                inverted: false,
            }),
            filters: vec![],
            bounds_l0: TileBounds::full_document(16, 16),
            raw_asset: Some("1.png".into()),
        };
        let back: LayerFile =
            serde_json::from_str(&serde_json::to_string(&layer).unwrap()).unwrap();
        assert_eq!(
            back.mask.unwrap().storage,
            MaskStorage::External(LayerId::new(99))
        );
        let _ = PaletteId::new(1);
    }
}
