//! On-disk `document.json` shapes for `.dyproj` (no runtime-only fields).
//!
//! Persisted: tree, palettes (revision reset on load), `raw_asset` per raster layer.
//! Omitted / ignored: `Document.revision`, `generations`, `requires_full_row`.
//! CustomPng paths in file form are `{content_hash}.png` basenames only.
//!
//! Forward-compat (SPEC §9.1): `extra` bags + unknown `node` tags round-trip via
//! [`crate::layer::FORWARD_COMPAT_NODE_KEY`].

use crate::filter::{FilterInstance, FilterKind, FilterParams};
use crate::layer::{Layer, LayerGroup, LayerNode, FORWARD_COMPAT_NODE_KEY};
use crate::mask::MaskRef;
use crate::types::{BlendMode, ColorProfileRef, LayerId, LayerKind, TileBounds};
use engine_color::palette::{LinearColor, Palette};
use engine_registry::AlgorithmRegistry;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

fn empty_extra() -> Map<String, Value> {
    Map::new()
}

#[allow(dead_code)] // referenced from serde `default = "empty_extra"`
fn keep_empty_extra_symbol() {
    let _ = empty_extra();
}

fn is_empty_extra(m: &Map<String, Value>) -> bool {
    m.is_empty()
}

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
    #[serde(default = "empty_extra", skip_serializing_if = "is_empty_extra")]
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Palette without relying on live revision semantics (always rewritten to 1 on load).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaletteFile {
    pub id: u32,
    pub name: String,
    pub colors: Vec<LinearColor>,
    #[serde(default = "empty_extra", skip_serializing_if = "is_empty_extra")]
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Layer or group node in the file tree (unknown `node` tags preserved).
#[derive(Debug, Clone)]
pub enum LayerNodeFile {
    Leaf(LayerFile),
    Group(LayerGroupFile),
    /// Opaque JSON object with an unrecognized `node` tag (SPEC §9.1).
    Unknown(Value),
}

impl Serialize for LayerNodeFile {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            LayerNodeFile::Leaf(l) => {
                #[derive(Serialize)]
                struct Tagged<'a> {
                    node: &'static str,
                    #[serde(flatten)]
                    inner: &'a LayerFile,
                }
                Tagged {
                    node: "leaf",
                    inner: l,
                }
                .serialize(serializer)
            }
            LayerNodeFile::Group(g) => {
                #[derive(Serialize)]
                struct Tagged<'a> {
                    node: &'static str,
                    #[serde(flatten)]
                    inner: &'a LayerGroupFile,
                }
                Tagged {
                    node: "group",
                    inner: g,
                }
                .serialize(serializer)
            }
            LayerNodeFile::Unknown(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for LayerNodeFile {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let tag = value
            .get("node")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        match tag.as_str() {
            "leaf" => {
                let mut obj = value
                    .as_object()
                    .cloned()
                    .ok_or_else(|| serde::de::Error::custom("leaf node must be an object"))?;
                obj.remove("node");
                let leaf: LayerFile = serde_json::from_value(Value::Object(obj))
                    .map_err(serde::de::Error::custom)?;
                Ok(LayerNodeFile::Leaf(leaf))
            }
            "group" => {
                let mut obj = value
                    .as_object()
                    .cloned()
                    .ok_or_else(|| serde::de::Error::custom("group node must be an object"))?;
                obj.remove("node");
                let group: LayerGroupFile = serde_json::from_value(Value::Object(obj))
                    .map_err(serde::de::Error::custom)?;
                Ok(LayerNodeFile::Group(group))
            }
            _ => Ok(LayerNodeFile::Unknown(value)),
        }
    }
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
    #[serde(default = "empty_extra", skip_serializing_if = "is_empty_extra")]
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    #[serde(default = "empty_extra", skip_serializing_if = "is_empty_extra")]
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    #[serde(default = "empty_extra", skip_serializing_if = "is_empty_extra")]
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
        let mut file = Self {
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
                    extra: Map::new(),
                })
                .collect(),
            extra: doc.extra.clone(),
        };
        file.sanitize_dangling_palette_refs();
        file
    }

    /// Drop dither `palette_id`s that are not in `palettes` so Save cannot
    /// persist the Color Lab stale-binding hole (open used to panic on remap).
    pub fn sanitize_dangling_palette_refs(&mut self) {
        let known: std::collections::HashSet<u32> =
            self.palettes.iter().map(|p| p.id).collect();
        fn walk(nodes: &mut [LayerNodeFile], known: &std::collections::HashSet<u32>) {
            for node in nodes {
                match node {
                    LayerNodeFile::Leaf(layer) => {
                        for f in &mut layer.filters {
                            if let Ok(FilterParams::DitherV2(mut p)) =
                                serde_json::from_value::<FilterParams>(f.params.clone())
                            {
                                if let Some(pid) = p.palette_id {
                                    if !known.contains(&pid.0) {
                                        p.palette_id = None;
                                        if let Ok(v) = serde_json::to_value(FilterParams::DitherV2(p))
                                        {
                                            f.params = v;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    LayerNodeFile::Group(group) => walk(&mut group.children, known),
                    LayerNodeFile::Unknown(_) => {}
                }
            }
        }
        walk(&mut self.root, &known);
    }
}

fn layer_node_to_file(
    node: &LayerNode,
    raw_asset_for: &mut impl FnMut(LayerId) -> Option<String>,
) -> LayerNodeFile {
    match node {
        LayerNode::Leaf(layer) => {
            if let Some(blob) = layer.extra.get(FORWARD_COMPAT_NODE_KEY) {
                return LayerNodeFile::Unknown(blob.clone());
            }
            LayerNodeFile::Leaf(layer_to_file(layer, raw_asset_for))
        }
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
            extra: group.extra.clone(),
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
        extra: layer.extra.clone(),
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
        extra: Map::new(),
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
            extra: layer.extra.clone(),
        }),
        LayerNodeFile::Group(group) => LayerNode::Group(LayerGroup {
            id: group.id,
            name: group.name.clone(),
            blend_mode: group.blend_mode,
            opacity: group.opacity,
            visible: group.visible,
            mask: group.mask.clone(),
            children: group.children.iter().map(layer_node_from_file).collect(),
            extra: group.extra.clone(),
        }),
        LayerNodeFile::Unknown(value) => {
            // Stub leaf: invisible adjustment; original JSON in extra for save.
            let id = value
                .get("id")
                .and_then(|v| v.as_u64())
                .map(|n| LayerId::new(n as u32))
                .unwrap_or(LayerId::new(0));
            let tag = value
                .get("node")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let mut extra = Map::new();
            extra.insert(FORWARD_COMPAT_NODE_KEY.to_string(), value.clone());
            LayerNode::Leaf(Layer {
                id,
                name: format!("Unknown ({tag})"),
                kind: LayerKind::Adjustment,
                blend_mode: BlendMode::Normal,
                opacity: 1.0,
                visible: false,
                offset: (0, 0),
                mask: None,
                filters: Vec::new(),
                bounds_l0: TileBounds {
                    min_x: 0,
                    min_y: 0,
                    max_x: 0,
                    max_y: 0,
                },
                extra,
            })
        }
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
    use crate::types::{DocumentId, FilterInstanceId};

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
        layer.filters.push(filt);
        doc.root.push(LayerNode::Leaf(layer));

        let file = DocumentFile::from_document(&doc, |_| Some("1.png".into()));
        let json = serde_json::to_value(&file).unwrap();
        let leaf = &json["root"][0];
        assert_eq!(leaf["node"], "leaf");
        assert_eq!(leaf["raw_asset"], "1.png");
        assert!(leaf["filters"][0].get("requires_full_row").is_none());
    }

    #[test]
    fn unknown_node_round_trips_via_stub() {
        let raw = serde_json::json!({
            "node": "voxel",
            "id": 9,
            "payload": { "x": 1 }
        });
        let node: LayerNodeFile = serde_json::from_value(raw.clone()).unwrap();
        assert!(matches!(node, LayerNodeFile::Unknown(_)));
        let live = layer_node_from_file(&node);
        let back = layer_node_to_file(&live, &mut |_| None);
        match back {
            LayerNodeFile::Unknown(v) => assert_eq!(v, raw),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn extra_fields_on_document_survive_json() {
        let mut file = DocumentFile {
            id: 1,
            width: 8,
            height: 8,
            color_profile: ColorProfileRef::SRgb,
            root: vec![],
            palettes: vec![],
            extra: Map::new(),
        };
        file.extra
            .insert("vendor_meta".into(), serde_json::json!({"ok": true}));
        let v = serde_json::to_value(&file).unwrap();
        assert_eq!(v["vendor_meta"]["ok"], true);
        let back: DocumentFile = serde_json::from_value(v).unwrap();
        assert_eq!(back.extra["vendor_meta"]["ok"], true);
    }

    #[test]
    fn adjustment_layer_has_no_raw_asset() {
        let mut doc = Document::new(DocumentId::new(1), 32, 32);
        let layer = Layer::new(LayerId::new(1), LayerKind::Adjustment, 32, 32);
        doc.root.push(LayerNode::Leaf(layer));
        let file = DocumentFile::from_document(&doc, |_| Some("nope.png".into()));
        match &file.root[0] {
            LayerNodeFile::Leaf(l) => assert!(l.raw_asset.is_none()),
            _ => panic!("leaf"),
        }
    }

    #[test]
    fn custom_png_basename_round_trips_in_json() {
        let params = FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::CustomPng {
                path: "aabbccddeeff00112233445566778899.png".into(),
            },
            levels: 2,
            ..DitherParamsV2::default()
        });
        let f = FilterInstanceFile {
            id: FilterInstanceId::new(),
            kind: FilterKind::Dither,
            params: file_params(params),
            enabled: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            algorithm_id: None,
            schema_version: None,
            extra: Map::new(),
        };
        let v = serde_json::to_value(&f).unwrap();
        let back: FilterInstanceFile = serde_json::from_value(v).unwrap();
        let p: FilterParams = serde_json::from_value(back.params).unwrap();
        match p {
            FilterParams::DitherV2(d) => match d.mode {
                DitherModeV2::CustomPng { path } => {
                    assert!(path.ends_with(".png"));
                }
                other => panic!("{other:?}"),
            },
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn filter_from_file_recomputes_requires_full_row() {
        let f = FilterInstanceFile {
            id: FilterInstanceId::new(),
            kind: FilterKind::Dither,
            params: file_params(FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 2,
                ..DitherParamsV2::default()
            })),
            enabled: true,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
            algorithm_id: None,
            schema_version: None,
            extra: Map::new(),
        };
        let inst = filter_from_file(&f, None);
        assert!(inst.requires_full_row);
    }

    #[test]
    fn algorithm_id_and_schema_version_roundtrip() {
        let mut inst = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2::default()),
        );
        inst.algorithm_id = Some("bayer_4x4".into());
        inst.schema_version = Some(1);
        let file = filter_to_file(&inst);
        assert_eq!(file.algorithm_id.as_deref(), Some("bayer_4x4"));
        let json = serde_json::to_value(&file).unwrap();
        assert_eq!(json["algorithm_id"], "bayer_4x4");
        let loaded: FilterInstanceFile = serde_json::from_value(json).unwrap();
        let back = filter_from_file(&loaded, None);
        assert_eq!(back.algorithm_id.as_deref(), Some("bayer_4x4"));
    }

    #[test]
    fn filter_instance_file_missing_opacity_blend_defaults() {
        let id = FilterInstanceId::new();
        let v = serde_json::json!({
            "id": id,
            "kind": "Levels",
            "params": { "Levels": {
                "input_black": 0.0, "input_white": 1.0, "gamma": 1.0,
                "output_black": 0.0, "output_white": 1.0
            }},
            "enabled": true
        });
        let f: FilterInstanceFile = serde_json::from_value(v).unwrap();
        assert_eq!(f.opacity, 1.0);
        assert_eq!(f.blend_mode, BlendMode::Normal);
    }

    #[test]
    fn mask_external_serializes() {
        let mask = MaskRef {
            storage: MaskStorage::External(LayerId::new(3)),
            enabled: true,
            inverted: false,
        };
        let v = serde_json::to_value(&mask).unwrap();
        assert!(v.get("storage").is_some() || v.is_object());
        let _back: MaskRef = serde_json::from_value(v).unwrap();
    }
}
