//! Error types for the engine-project module.

use crate::types::{DocumentId, FilterInstanceId, LayerId, PaletteId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Engine error type for all document/layer/filter operations.
#[derive(Error, Debug, Clone, Serialize, Deserialize)]
pub enum EngineError {
    #[error("Layer not found: {layer_id}")]
    LayerNotFound { layer_id: LayerId },

    #[error("Document not found: {doc_id}")]
    DocumentNotFound { doc_id: DocumentId },

    #[error("Filter not found: {filter_id}")]
    FilterNotFound { filter_id: FilterInstanceId },

    #[error("Palette not found: {palette_id}")]
    PaletteNotFound { palette_id: PaletteId },

    #[error("Palette in use: {palette_id}, referenced by filters: {references:?}")]
    PaletteInUse {
        palette_id: PaletteId,
        references: Vec<FilterInstanceId>,
    },

    #[error("Invalid layer kind: {reason}")]
    InvalidLayerKind { reason: String },

    #[error("Invalid filter params: {reason}")]
    InvalidFilterParams { reason: String },

    #[error("IO error: {reason}")]
    IoError { reason: String },

    #[error("Invalid state: {reason}")]
    InvalidState { reason: String },

    /// Scheduler / worker should park or retry later — not a hard failure.
    #[error("ED prefix not yet computed")]
    EdPrefixPending,

    /// ED Processed dequeued before left/top/diag (or Raw) ready.
    #[error("ED dependencies not ready")]
    EdDependenciesPending,

    /// Display pyramid parent waiting on full-res Composite children.
    #[error("Pyramid children not yet computed")]
    PyramidChildrenPending,

    #[error("Operation not supported: {reason}")]
    NotSupported { reason: String },
}

impl EngineError {
    /// Create a LayerNotFound error
    pub fn layer_not_found(layer_id: LayerId) -> Self {
        EngineError::LayerNotFound { layer_id }
    }

    /// Create a DocumentNotFound error
    pub fn document_not_found(doc_id: DocumentId) -> Self {
        EngineError::DocumentNotFound { doc_id }
    }

    /// Create a FilterNotFound error
    pub fn filter_not_found(filter_id: FilterInstanceId) -> Self {
        EngineError::FilterNotFound { filter_id }
    }

    /// Create an InvalidLayerKind error
    pub fn invalid_layer_kind(reason: impl Into<String>) -> Self {
        EngineError::InvalidLayerKind {
            reason: reason.into(),
        }
    }

    /// Create an InvalidFilterParams error
    pub fn invalid_filter_params(reason: impl Into<String>) -> Self {
        EngineError::InvalidFilterParams {
            reason: reason.into(),
        }
    }

    /// Create an IoError
    pub fn io_error(reason: impl Into<String>) -> Self {
        EngineError::IoError {
            reason: reason.into(),
        }
    }

    /// Create an InvalidState error
    pub fn invalid_state(reason: impl Into<String>) -> Self {
        EngineError::InvalidState {
            reason: reason.into(),
        }
    }

    /// Create a PaletteNotFound error
    pub fn palette_not_found(palette_id: PaletteId) -> Self {
        EngineError::PaletteNotFound { palette_id }
    }

    /// Create a PaletteInUse error
    pub fn palette_in_use(palette_id: PaletteId, references: Vec<FilterInstanceId>) -> Self {
        EngineError::PaletteInUse {
            palette_id,
            references,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_serializes_to_json() {
        let error = EngineError::invalid_layer_kind("test reason");
        let json = serde_json::to_string(&error).unwrap();
        let deserialized: EngineError = serde_json::from_str(&json).unwrap();

        if let EngineError::InvalidLayerKind { reason } = deserialized {
            assert_eq!(reason, "test reason");
        } else {
            panic!("Expected InvalidLayerKind variant");
        }
    }

    #[test]
    fn error_display_works() {
        let error = EngineError::invalid_filter_params("bad params");
        let msg = error.to_string();
        assert!(msg.contains("bad params"));
    }
}
