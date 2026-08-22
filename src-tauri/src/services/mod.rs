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

pub mod panel_service;
pub use panel_service::PanelService;

pub mod undo_service;
pub use undo_service::UndoService;

pub mod viewport_service;
pub use viewport_service::ViewportService;
