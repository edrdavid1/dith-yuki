use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("{0}")]
    Generic(String),
    #[error("Document not found: {0}")]
    DocumentNotFound(u32),
    #[error("Layer not found: {0}")]
    LayerNotFound(u32),
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
