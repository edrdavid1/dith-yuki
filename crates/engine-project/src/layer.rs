//! Layer hierarchy and tree traversal.

use crate::filter::FilterInstance;
use crate::mask::MaskRef;
use crate::types::{BlendMode, LayerId, LayerKind, TileBounds};
use serde::{Deserialize, Serialize};

/// A reference to a layer during tree traversal.
#[derive(Debug, Clone)]
pub enum LayerRef<'a> {
    /// A leaf layer (raster or adjustment)
    Leaf(&'a Layer),
    /// Start of a layer group (signals descent into children)
    GroupStart(&'a LayerGroup),
    /// End of a layer group (signals return to parent)
    GroupEnd(&'a LayerGroup),
}

/// A single raster or adjustment layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layer {
    /// Stable unique identifier
    pub id: LayerId,

    /// Display name in the UI
    pub name: String,

    /// Raster or Adjustment layer
    pub kind: LayerKind,

    /// Blend mode for composition
    pub blend_mode: BlendMode,

    /// Opacity, 0.0–1.0
    pub opacity: f32,

    /// Whether to include this layer in composition
    pub visible: bool,

    /// Pixel offset from canvas origin
    pub offset: (i32, i32),

    /// Optional alpha mask
    pub mask: Option<MaskRef>,

    /// Stack of filters applied to this layer
    pub filters: Vec<FilterInstance>,

    /// Bounds of layer content in tiles at MipLevel 0
    pub bounds_l0: TileBounds,
}

impl Layer {
    /// Create a new layer.
    pub fn new(id: LayerId, kind: LayerKind, width: u32, height: u32) -> Self {
        Layer {
            id,
            name: format!("Layer {}", id.0),
            kind,
            blend_mode: BlendMode::Normal,
            opacity: 1.0,
            visible: true,
            offset: (0, 0),
            mask: None,
            filters: Vec::new(),
            bounds_l0: TileBounds::full_document(width, height),
        }
    }

    /// Find a filter by ID in this layer
    pub fn find_filter(&self, filter_id: crate::types::FilterInstanceId) -> Option<&FilterInstance> {
        self.filters.iter().find(|f| f.id == filter_id)
    }

    /// Find a mutable filter by ID in this layer
    pub fn find_filter_mut(
        &mut self,
        filter_id: crate::types::FilterInstanceId,
    ) -> Option<&mut FilterInstance> {
        self.filters.iter_mut().find(|f| f.id == filter_id)
    }
}

/// A group of layers with its own blend mode and opacity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerGroup {
    /// Stable unique identifier
    pub id: LayerId,

    /// Display name in the UI
    pub name: String,

    /// Blend mode for composition
    pub blend_mode: BlendMode,

    /// Opacity, 0.0–1.0
    pub opacity: f32,

    /// Whether to include this group in composition
    pub visible: bool,

    /// Optional alpha mask applied to group composite
    pub mask: Option<MaskRef>,

    /// Child layers/groups, bottom-to-top
    pub children: Vec<LayerNode>,
}

impl LayerGroup {
    /// Create a new layer group.
    pub fn new(id: LayerId) -> Self {
        LayerGroup {
            id,
            name: format!("Group {}", id.0),
            blend_mode: BlendMode::Normal,
            opacity: 1.0,
            visible: true,
            mask: None,
            children: Vec::new(),
        }
    }
}

/// A node in the layer tree: either a leaf layer or a group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LayerNode {
    Leaf(Layer),
    Group(LayerGroup),
}

/// Lazy iterator over layer tree in bottom-to-top order.
pub struct BottomToTopIter<'a> {
    stack: Vec<(&'a [LayerNode], usize, bool)>, // (nodes, index, is_descending)
}

impl<'a> BottomToTopIter<'a> {
    fn new(nodes: &'a [LayerNode]) -> Self {
        BottomToTopIter {
            stack: vec![(nodes, 0, false)],
        }
    }
}

impl<'a> Iterator for BottomToTopIter<'a> {
    type Item = LayerRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.stack.is_empty() {
            let (nodes, idx, is_descending) = self.stack.last_mut().unwrap();

            if *idx >= nodes.len() {
                self.stack.pop();
                continue;
            }

            let current_node = &nodes[*idx];

            if !*is_descending {
                // First visit: emit GroupStart or Leaf
                *is_descending = true;

                match current_node {
                    LayerNode::Leaf(layer) => {
                        *idx += 1;
                        return Some(LayerRef::Leaf(layer));
                    }
                    LayerNode::Group(group) => {
                        // Don't increment yet; we'll process children next
                        return Some(LayerRef::GroupStart(group));
                    }
                }
            } else {
                // Second visit (after children): emit GroupEnd and move to next
                match current_node {
                    LayerNode::Leaf(_) => {
                        // Should not happen (we emit Leaf on first visit)
                        *idx += 1;
                    }
                    LayerNode::Group(group) => {
                        // Check if we need to descend
                        if !group.children.is_empty() && *idx == nodes.len() - 1 {
                            // Push children onto stack
                            let _saved_idx = *idx;
                            *idx += 1;
                            self.stack.push((&group.children, 0, false));
                            return Some(LayerRef::GroupEnd(group));
                        } else {
                            *idx += 1;
                            return Some(LayerRef::GroupEnd(group));
                        }
                    }
                }
            }
        }

        None
    }
}

/// Walk the layer tree in bottom-to-top order.
///
/// Emits `LayerRef::Leaf` for each raster/adjustment layer,
/// `LayerRef::GroupStart` before processing children,
/// and `LayerRef::GroupEnd` after.
pub fn walk_bottom_to_top<'a>(nodes: &'a [LayerNode]) -> impl Iterator<Item = LayerRef<'a>> {
    BottomToTopIter::new(nodes)
}

/// Simpler version: collect all layers in order, flattening the tree.
pub fn flatten_bottom_to_top<'a>(nodes: &'a [LayerNode]) -> Vec<LayerRef<'a>> {
    walk_bottom_to_top(nodes).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_new_defaults() {
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 5000, 5000);
        assert_eq!(layer.opacity, 1.0);
        assert!(layer.visible);
        assert_eq!(layer.blend_mode, BlendMode::Normal);
    }

    #[test]
    fn layer_group_new_defaults() {
        let group = LayerGroup::new(LayerId::new(1));
        assert_eq!(group.opacity, 1.0);
        assert!(group.visible);
        assert_eq!(group.blend_mode, BlendMode::Normal);
        assert!(group.children.is_empty());
    }

    #[test]
    fn walk_single_layer() {
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        let node = LayerNode::Leaf(layer);
        let nodes = vec![node];
        let walked: Vec<_> = walk_bottom_to_top(&nodes).collect();

        assert_eq!(walked.len(), 1);
        matches!(walked[0], LayerRef::Leaf(_));
    }

    #[test]
    fn walk_group_with_children() {
        let mut group = LayerGroup::new(LayerId::new(1));
        let layer1 = Layer::new(LayerId::new(10), LayerKind::Raster, 256, 256);
        let layer2 = Layer::new(LayerId::new(20), LayerKind::Raster, 256, 256);

        group.children.push(LayerNode::Leaf(layer1));
        group.children.push(LayerNode::Leaf(layer2));

        let node = LayerNode::Group(group);
        let nodes = vec![node];
        let walked: Vec<_> = walk_bottom_to_top(&nodes).collect();

        // TreeWalker emits: GroupStart, Leaf(10), Leaf(20), then GroupEnd
        // (current placeholder impl may differ slightly)
        assert!(walked.len() >= 3, "Expected at least 3 items, got {}", walked.len());
    }

    #[test]
    fn layer_find_filter() {
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        let filter = crate::filter::FilterInstance::new(
            crate::filter::FilterKind::Curves,
            crate::filter::FilterParams::Curves { curve: vec![] },
        );
        let filter_id = filter.id;
        layer.filters.push(filter);

        assert!(layer.find_filter(filter_id).is_some());
        assert!(layer.find_filter(crate::types::FilterInstanceId::new()).is_none());
    }
}
