//! Core data model types for the Dither engine.
//!
//! Types defined here: Layer, Document, FilterInstance, BlendMode.
//! Full API specification: ../../../tauri-api-document-model.md
//!
//! Phase 0: Stub definitions only. Full implementation in Phase 2.

/// Represents a single layer in a document.
///
/// TODO: fill in Phase 2
/// - Fields: name, opacity, blend_mode, visible, pixels/reference
#[derive(Clone, Debug)]
pub struct Layer {
    // TODO: fill in Phase 2
}

/// Represents a complete image document with multiple layers.
///
/// TODO: fill in Phase 2
/// - Fields: layers, width, height, metadata
#[derive(Clone, Debug)]
pub struct Document {
    // TODO: fill in Phase 2
}

/// Represents a filter or effect applied to a layer or document.
///
/// TODO: fill in Phase 2
/// - Fields: filter_type, parameters, enabled
#[derive(Clone, Debug)]
pub struct FilterInstance {
    // TODO: fill in Phase 2
}

/// Blend mode for compositing layers.
///
/// TODO: fill in Phase 2
/// - Variants: Normal, Multiply, Screen, Overlay, ColorBurn, LinearBurn, etc.
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub enum BlendMode {
    // TODO: fill in Phase 2
}

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}
