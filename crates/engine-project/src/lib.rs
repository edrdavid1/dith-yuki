//! Document model, layer hierarchy, filter pipeline, and mask system.
//!
//! This module implements the application-level data structures that bridge
//! the UI with the Phase 1 tile engine. It provides:
//!
//! - Document: Main project structure
//! - Layer & LayerGroup: Recursive layer hierarchy
//! - FilterInstance: Filter stack with parameters
//! - MaskRef: Alpha masks (via external layers)
//! - DocumentHandle: Thread-safe access via arc-swap
//! - Invalidation: Cache coordination
//! - Commands: Document mutation operations

pub mod commands;
pub mod compositor;
pub mod document;
pub mod dto;
pub mod error;
pub mod filter;
pub mod filters;
pub mod invalidation;
pub mod layer;
pub mod mask;
pub mod types;

// Public API re-exports
pub use commands::{add_layer, remove_layer, set_layer_props, reorder_layer, LayerPropsPatch};
pub use compositor::{blend_tile, composite_tile};
pub use document::{Document, DocumentHandle};
pub use error::EngineError;
pub use filter::{apply_filter_to_tile, FilterInstance, FilterKind, FilterParams};
pub use invalidation::{
    invalidate_layer_filter_changed, invalidate_layer_props_changed,
    invalidate_layer_structure_changed, validate_document_consistency,
};
pub use layer::{flatten_bottom_to_top, walk_bottom_to_top, Layer, LayerGroup, LayerNode, LayerRef};
pub use mask::{apply_mask, MaskRef, MaskStorage};
pub use types::{
    BlendMode, ColorProfileRef, DocumentId, FilterInstanceId, LayerId, LayerKind, PaletteId,
    TileBounds,
};

/// Library version
pub const VERSION: &str = "0.1.0";
