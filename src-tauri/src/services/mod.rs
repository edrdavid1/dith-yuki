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

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Generic(s)
    }
}

pub mod filter_service;
pub use filter_service::FilterService;

pub mod layer_service;
pub use layer_service::LayerService;

pub mod panel_service;
pub use panel_service::PanelService;

pub mod undo_service;
pub use undo_service::UndoService;

pub mod viewport_service;
pub use viewport_service::ViewportService;

pub mod palette_service;
pub use palette_service::PaletteService;

pub mod document_service;
pub use document_service::DocumentService;
