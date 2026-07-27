//! Error types for the engine-project module.

use crate::types::{DocumentId, FilterInstanceId, LayerId};
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

    #[error("Invalid layer kind: {reason}")]
    InvalidLayerKind { reason: String },

    #[error("Invalid filter params: {reason}")]
    InvalidFilterParams { reason: String },

    #[error("IO error: {reason}")]
    IoError { reason: String },

    #[error("Invalid state: {reason}")]
    InvalidState { reason: String },

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
