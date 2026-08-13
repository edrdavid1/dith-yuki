//! Remap file-local IDs to fresh runtime IDs on project open.
//!
//! Always allocates new `LayerId` / `PaletteId` / `FilterInstanceId` values and
//! rewrites internal edges (`palette_id`, `MaskStorage::External`, CustomPng
//! paths after materialize are handled by the open orchestrator).

use crate::document::Document;
use crate::filter::FilterParams;
use crate::mask::MaskStorage;
use crate::serialize::document_dto::{
    filter_from_file, layer_node_from_file, palettes_from_file, DocumentFile, FilterInstanceFile,
    LayerNodeFile, PaletteFile,
};
use crate::types::{DocumentId, FilterInstanceId, LayerId, PaletteId};
use engine_tiles::generation::GenerationTracker;
use std::collections::HashMap;

/// Tables produced while remapping a [`DocumentFile`] into a live [`Document`].
#[derive(Debug, Default)]
pub struct IdRemapTables {
    pub layers: HashMap<LayerId, LayerId>,
    pub palettes: HashMap<PaletteId, PaletteId>,
    pub filters: HashMap<FilterInstanceId, FilterInstanceId>,
}

/// Result of remapping: live document + tables (for attaching Raw tiles by old→new layer).
#[derive(Debug)]
pub struct RemappedDocument {
    pub document: Document,
    pub tables: IdRemapTables,
    /// File-local layer id → `raw_asset` basename (pre-remap keys).
    pub raw_assets: HashMap<LayerId, String>,
}

/// Remap a file document into a runtime document with fresh IDs.
///
/// - `runtime_doc_id` is typically `DocumentId(1)` (single-doc app).
/// - Palette revisions are set to 1; generations are empty; `revision = 1`.
/// - `requires_full_row` is recomputed via [`filter_from_file`].
pub fn remap_document_file(file: &DocumentFile, runtime_doc_id: DocumentId) -> RemappedDocument {
    let mut tables = IdRemapTables::default();
    let mut next_layer: u32 = 1;
    let mut next_palette: u32 = 1;
    let mut raw_assets = HashMap::new();

    // Pass 1: allocate layer + palette ids (filters allocated while rewriting).
    collect_layer_ids(&file.root, &mut |old| {
        let new_id = LayerId::new(next_layer);
        next_layer += 1;
        tables.layers.insert(old, new_id);
    });

    for p in &file.palettes {
        let old = PaletteId::new(p.id);
        let new_id = PaletteId::new(next_palette);
        next_palette += 1;
        tables.palettes.insert(old, new_id);
    }

    // Collect raw_asset keyed by file-local layer id before tree rewrite.
    collect_raw_assets(&file.root, &mut raw_assets);

    let remapped_palettes: Vec<PaletteFile> = file
        .palettes
        .iter()
        .map(|p| {
            let old = PaletteId::new(p.id);
            let new_id = *tables.palettes.get(&old).expect("palette mapped");
            PaletteFile {
                id: new_id.0,
                name: p.name.clone(),
                colors: p.colors.clone(),
            }
        })
        .collect();

    let remapped_root: Vec<LayerNodeFile> = file
        .root
        .iter()
        .map(|n| remap_layer_node(n, &mut tables))
        .collect();

    let document = Document {
        id: runtime_doc_id,
        width: file.width,
        height: file.height,
        color_profile: file.color_profile.clone(),
        root: remapped_root.iter().map(layer_node_from_file).collect(),
        palettes: palettes_from_file(&remapped_palettes),
        revision: 1,
        generations: GenerationTracker::new(),
    };

    RemappedDocument {
        document,
        tables,
        raw_assets,
    }
}

fn collect_layer_ids(nodes: &[LayerNodeFile], alloc: &mut impl FnMut(LayerId)) {
    for node in nodes {
        match node {
            LayerNodeFile::Leaf(layer) => alloc(layer.id),
            LayerNodeFile::Group(group) => {
                alloc(group.id);
                collect_layer_ids(&group.children, alloc);
            }
        }
    }
}

fn collect_raw_assets(nodes: &[LayerNodeFile], out: &mut HashMap<LayerId, String>) {
    for node in nodes {
        match node {
            LayerNodeFile::Leaf(layer) => {
                if let Some(asset) = &layer.raw_asset {
                    out.insert(layer.id, asset.clone());
                }
            }
            LayerNodeFile::Group(group) => collect_raw_assets(&group.children, out),
        }
    }
}

fn remap_layer_node(node: &LayerNodeFile, tables: &mut IdRemapTables) -> LayerNodeFile {
    match node {
        LayerNodeFile::Leaf(layer) => {
            let new_id = *tables.layers.get(&layer.id).expect("layer mapped");
            let mask = layer.mask.as_ref().map(|m| remap_mask(m, tables));
            let filters = layer
                .filters
                .iter()
                .map(|f| remap_filter(f, tables))
                .collect();
            LayerNodeFile::Leaf(crate::serialize::document_dto::LayerFile {
                id: new_id,
                name: layer.name.clone(),
                kind: layer.kind,
                blend_mode: layer.blend_mode,
                opacity: layer.opacity,
                visible: layer.visible,
                offset: layer.offset,
                mask,
                filters,
                bounds_l0: layer.bounds_l0,
                // raw_asset stays on file-local key in RemappedDocument::raw_assets
                raw_asset: layer.raw_asset.clone(),
            })
        }
        LayerNodeFile::Group(group) => {
            let new_id = *tables.layers.get(&group.id).expect("group mapped");
            let mask = group.mask.as_ref().map(|m| remap_mask(m, tables));
            let children = group
                .children
                .iter()
                .map(|c| remap_layer_node(c, tables))
                .collect();
            LayerNodeFile::Group(crate::serialize::document_dto::LayerGroupFile {
                id: new_id,
                name: group.name.clone(),
                blend_mode: group.blend_mode,
                opacity: group.opacity,
                visible: group.visible,
                mask,
                children,
            })
        }
    }
}

fn remap_mask(mask: &crate::mask::MaskRef, tables: &IdRemapTables) -> crate::mask::MaskRef {
    let storage = match &mask.storage {
        MaskStorage::External(old) => {
            let new_id = tables
                .layers
                .get(old)
                .copied()
                .expect("external mask layer mapped");
            MaskStorage::External(new_id)
        }
        other => other.clone(),
    };
    crate::mask::MaskRef {
        storage,
        enabled: mask.enabled,
        inverted: mask.inverted,
    }
}

fn remap_filter(f: &FilterInstanceFile, tables: &mut IdRemapTables) -> FilterInstanceFile {
    let new_fid = FilterInstanceId::new();
    tables.filters.insert(f.id, new_fid);
    let params = remap_filter_params(&f.params, tables);
    // Build via FilterInstance::new semantics by going through filter_from_file later;
    // here we only rewrite ids/params. requires_full_row is recomputed in filter_from_file.
    FilterInstanceFile {
        id: new_fid,
        kind: f.kind,
        params,
        enabled: f.enabled,
        opacity: f.opacity,
        blend_mode: f.blend_mode,
    }
}

fn remap_filter_params(params: &FilterParams, tables: &IdRemapTables) -> FilterParams {
    match params {
        FilterParams::PaletteQuantize {
            palette_id,
            diffusion,
        } => FilterParams::PaletteQuantize {
            palette_id: tables
                .palettes
                .get(palette_id)
                .copied()
                .expect("palette_id mapped"),
            diffusion: *diffusion,
        },
        FilterParams::DitherV2(p) => {
            let mut p = p.clone();
            if let Some(old) = p.palette_id {
                p.palette_id = Some(
                    tables
                        .palettes
                        .get(&old)
                        .copied()
                        .expect("dither palette_id mapped"),
                );
            }
            FilterParams::DitherV2(p)
        }
        other => other.clone(),
    }
}

/// Convenience: ensure a remapped filter's `requires_full_row` matches `FilterInstance::new`.
pub fn recompute_requires_full_row(f: &FilterInstanceFile) -> bool {
    filter_from_file(f).requires_full_row
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherModeV2, DitherParamsV2, FilterKind};
    use crate::mask::MaskRef;
    use crate::serialize::document_dto::{LayerFile, LayerGroupFile};
    use crate::types::{BlendMode, LayerKind, TileBounds};
    use engine_color::palette::LinearColor;

    fn sample_file() -> DocumentFile {
        let mask_layer = LayerId::new(10);
        let raster = LayerId::new(1);
        let pal_old = 7u32;
        let fid = FilterInstanceId::new();

        DocumentFile {
            id: 0,
            width: 64,
            height: 64,
            color_profile: crate::types::ColorProfileRef::SRgb,
            palettes: vec![PaletteFile {
                id: pal_old,
                name: "P".into(),
                colors: vec![LinearColor {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                }],
            }],
            root: vec![
                LayerNodeFile::Leaf(LayerFile {
                    id: mask_layer,
                    name: "Mask".into(),
                    kind: LayerKind::Raster,
                    blend_mode: BlendMode::Normal,
                    opacity: 1.0,
                    visible: true,
                    offset: (0, 0),
                    mask: None,
                    filters: vec![],
                    bounds_l0: TileBounds::full_document(64, 64),
                    raw_asset: Some("10.png".into()),
                }),
                LayerNodeFile::Leaf(LayerFile {
                    id: raster,
                    name: "Main".into(),
                    kind: LayerKind::Raster,
                    blend_mode: BlendMode::Normal,
                    opacity: 1.0,
                    visible: true,
                    offset: (0, 0),
                    mask: Some(MaskRef::external(mask_layer)),
                    filters: vec![FilterInstanceFile {
                        id: fid,
                        kind: FilterKind::Dither,
                        params: FilterParams::DitherV2(DitherParamsV2 {
                            mode: DitherModeV2::FloydSteinberg,
                            levels: 2,
                            palette_id: Some(PaletteId::new(pal_old)),
                            ..DitherParamsV2::default()
                        }),
                        enabled: true,
                        opacity: 1.0,
                        blend_mode: BlendMode::Normal,
                    }],
                    bounds_l0: TileBounds::full_document(64, 64),
                    raw_asset: Some("1.png".into()),
                }),
                LayerNodeFile::Group(LayerGroupFile {
                    id: LayerId::new(50),
                    name: "G".into(),
                    blend_mode: BlendMode::Normal,
                    opacity: 1.0,
                    visible: true,
                    mask: None,
                    children: vec![],
                }),
            ],
        }
    }

    #[test]
    fn remaps_external_mask_and_palette_id() {
        let file = sample_file();
        let old_mask = LayerId::new(10);
        let old_raster = LayerId::new(1);
        let old_pal = PaletteId::new(7);

        let remapped = remap_document_file(&file, DocumentId::new(1));
        assert_ne!(
            remapped.tables.layers.get(&old_mask).copied(),
            Some(old_mask)
        );
        assert_ne!(
            remapped.tables.layers.get(&old_raster).copied(),
            Some(old_raster)
        );
        assert_ne!(
            remapped.tables.palettes.get(&old_pal).copied(),
            Some(old_pal)
        );

        // Find remapped main layer and check mask + palette
        let new_mask = remapped.tables.layers[&old_mask];
        let new_pal = remapped.tables.palettes[&old_pal];

        let main = remapped
            .document
            .root
            .iter()
            .find_map(|n| match n {
                crate::layer::LayerNode::Leaf(l) if l.name == "Main" => Some(l),
                _ => None,
            })
            .expect("main layer");

        assert_eq!(main.mask.as_ref().unwrap().get_external_layer(), Some(new_mask));
        match &main.filters[0].params {
            FilterParams::DitherV2(p) => assert_eq!(p.palette_id, Some(new_pal)),
            other => panic!("unexpected params: {other:?}"),
        }
        assert!(main.filters[0].requires_full_row);
        assert_eq!(remapped.document.revision, 1);
        assert_eq!(remapped.document.palettes[0].revision, 1);
        assert_eq!(remapped.raw_assets.get(&old_raster).map(String::as_str), Some("1.png"));
    }

    #[test]
    fn filter_ids_are_fresh() {
        let file = sample_file();
        let old_fid = match &file.root[1] {
            LayerNodeFile::Leaf(l) => l.filters[0].id,
            _ => panic!(),
        };
        let remapped = remap_document_file(&file, DocumentId::new(1));
        let new_fid = remapped.tables.filters[&old_fid];
        assert_ne!(old_fid, new_fid);
    }
}
