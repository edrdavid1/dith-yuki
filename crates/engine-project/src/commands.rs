//! Document mutation commands.
//!
//! This module implements high-level commands for document manipulation:
//! adding/removing layers, setting properties, applying filters, etc.
//!
//! Commands are the primary interface between the Tauri frontend and the engine.

use crate::document::DocumentHandle;
use crate::error::EngineError;
use crate::invalidation::*;
use crate::layer::LayerNode;
use crate::types::{LayerId, LayerKind, BlendMode, DocumentId};
use crate::{Layer};
use engine_tiles::TileCache;

/// Mutation patch for layer properties.
///
/// All fields are optional; only set values are applied.
#[derive(Debug, Clone, Default)]
pub struct LayerPropsPatch {
    pub name: Option<String>,
    pub opacity: Option<f32>,
    pub blend_mode: Option<BlendMode>,
    pub visible: Option<bool>,
    pub offset: Option<(i32, i32)>,
}

/// Add a new layer to the document.
///
/// Creates a new layer and inserts it at the specified position in the parent.
/// Returns the ID of the created layer.
pub struct AddLayerArgs {
    pub kind: LayerKind,
    pub parent_group: Option<LayerId>,
    pub index: usize,
    pub width: u32,
    pub height: u32,
}

pub fn add_layer(
    doc_handle: &DocumentHandle,
    cache: &TileCache,
    _doc_id: DocumentId,
    args: AddLayerArgs,
) -> Result<LayerId, EngineError> {
    let mut new_layer_id = 0u32;

    doc_handle.mutate(|doc| {
        // Generate new layer ID (simple: max + 1)
        new_layer_id = generate_next_layer_id(&doc.root);
        let layer_id = LayerId::new(new_layer_id);

        // Create new layer
        let new_layer = Layer::new(layer_id, args.kind, args.width, args.height);

        // Find parent and insert
        if let Some(parent_id) = args.parent_group {
            insert_layer_into_parent(&mut doc.root, parent_id, LayerNode::Leaf(new_layer), args.index);
        } else {
            // Insert at root level
            if args.index <= doc.root.len() {
                doc.root.insert(args.index, LayerNode::Leaf(new_layer));
            } else {
                doc.root.push(LayerNode::Leaf(new_layer));
            }
        }

        doc.increment_generation();
    });

    // Invalidate Composite tiles (new layer affects composition)
    invalidate_layer_structure_changed(cache, &[LayerId::new(new_layer_id)], &[]);

    Ok(LayerId::new(new_layer_id))
}

/// Remove a layer from the document.
pub fn remove_layer(
    doc_handle: &DocumentHandle,
    cache: &TileCache,
    _doc_id: DocumentId,
    layer_id: LayerId,
) -> Result<(), EngineError> {
    doc_handle.mutate(|doc| {
        validate_document_consistency(doc, layer_id)
            .ok()
            .unwrap_or(());
        
        remove_layer_from_tree_vec(&mut doc.root, layer_id);
        doc.increment_generation();
    });

    // Invalidate (structure changed)
    invalidate_layer_structure_changed(cache, &[], &[layer_id]);

    Ok(())
}

/// Set layer properties (opacity, blend mode, visibility, offset, name).
pub fn set_layer_props(
    doc_handle: &DocumentHandle,
    cache: &TileCache,
    _doc_id: DocumentId,
    layer_id: LayerId,
    patch: LayerPropsPatch,
) -> Result<(), EngineError> {
    doc_handle.mutate(|doc| {
        // Find layer in tree
        if let Some(layer) = find_layer_mut(&mut doc.root, layer_id) {
            if let Some(name) = patch.name {
                layer.name = name;
            }
            if let Some(opacity) = patch.opacity {
                layer.opacity = opacity.clamp(0.0, 1.0);
            }
            if let Some(blend_mode) = patch.blend_mode {
                layer.blend_mode = blend_mode;
            }
            if let Some(visible) = patch.visible {
                layer.visible = visible;
            }
            if let Some(offset) = patch.offset {
                layer.offset = offset;
            }
        }
        doc.increment_generation();
    });

    // Invalidate (properties changed)
    invalidate_layer_props_changed(cache, layer_id);

    Ok(())
}

/// Reorder a layer (move to new parent/position).
pub fn reorder_layer(
    doc_handle: &DocumentHandle,
    cache: &TileCache,
    _doc_id: DocumentId,
    layer_id: LayerId,
    new_parent: Option<LayerId>,
    new_index: usize,
) -> Result<(), EngineError> {
    doc_handle.mutate(|doc| {
        // Remove from current position
        let node = remove_layer_from_tree_vec(&mut doc.root, layer_id);

        if let Some(removed_node) = node {
            // Insert at new position
            if let Some(parent_id) = new_parent {
                insert_layer_into_parent(&mut doc.root, parent_id, removed_node, new_index);
            } else if new_index <= doc.root.len() {
                doc.root.insert(new_index, removed_node);
            } else {
                doc.root.push(removed_node);
            }
        }

        doc.increment_generation();
    });

    // Invalidate (structure changed)
    invalidate_layer_structure_changed(cache, &[], &[]);

    Ok(())
}

// Helper: Generate next layer ID
fn generate_next_layer_id(nodes: &[LayerNode]) -> u32 {
    let mut max_id = 0u32;

    fn recurse(nodes: &[LayerNode], max: &mut u32) {
        for node in nodes {
            match node {
                LayerNode::Leaf(layer) => {
                    *max = (*max).max(layer.id.0);
                }
                LayerNode::Group(group) => {
                    *max = (*max).max(group.id.0);
                    recurse(&group.children, max);
                }
            }
        }
    }

    recurse(nodes, &mut max_id);
    max_id + 1
}

// Helper: Find layer mutably in tree
fn find_layer_mut(nodes: &mut [LayerNode], layer_id: LayerId) -> Option<&mut Layer> {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if layer.id == layer_id {
                    return Some(layer);
                }
            }
            LayerNode::Group(group) => {
                if group.id == layer_id {
                    return None; // Can't mutate group as Layer
                }
                if let Some(found) = find_layer_mut(&mut group.children, layer_id) {
                    return Some(found);
                }
            }
        }
    }
    None
}

// Helper: Remove layer from tree, return if found
fn remove_layer_from_tree_vec(nodes: &mut Vec<LayerNode>, layer_id: LayerId) -> Option<LayerNode> {
    for i in 0..nodes.len() {
        if let Some(node) = nodes.get(i) {
            match node {
                LayerNode::Leaf(layer) if layer.id == layer_id => {
                    return Some(nodes.remove(i));
                }
                LayerNode::Group(group) if group.id == layer_id => {
                    return Some(nodes.remove(i));
                }
                _ => {}
            }
        }
    }

    // Check in children recursively
    for node in nodes.iter_mut() {
        if let LayerNode::Group(group) = node {
            if let Some(removed) = remove_layer_from_tree_vec(&mut group.children, layer_id) {
                return Some(removed);
            }
        }
    }

    None
}

// Helper: Insert layer into parent group
#[allow(clippy::ptr_arg)]
fn insert_layer_into_parent(
    nodes: &mut Vec<LayerNode>,
    parent_id: LayerId,
    new_node: LayerNode,
    index: usize,
) -> bool {
    for node in nodes.iter_mut() {
        if let LayerNode::Group(group) = node {
            if group.id == parent_id {
                if index <= group.children.len() {
                    group.children.insert(index, new_node);
                } else {
                    group.children.push(new_node);
                }
                return true;
            }
            // Recurse into children
            if insert_layer_into_parent(&mut group.children, parent_id, new_node.clone(), index) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    #[test]
    fn generate_next_layer_id_increments() {
        let mut doc = Document::default();
        let layer1 = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        let layer2 = Layer::new(LayerId::new(2), LayerKind::Raster, 256, 256);

        doc.root.push(LayerNode::Leaf(layer1));
        doc.root.push(LayerNode::Leaf(layer2));

        let next_id = generate_next_layer_id(&doc.root);
        assert_eq!(next_id, 3);
    }

    #[test]
    fn layer_props_patch_default_empty() {
        let patch = LayerPropsPatch::default();
        assert!(patch.name.is_none());
        assert!(patch.opacity.is_none());
    }
}
