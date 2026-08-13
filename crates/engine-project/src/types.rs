//! Core types for document model.

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

/// Unique identifier for a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub u32);

impl DocumentId {
    pub fn new(id: u32) -> Self {
        DocumentId(id)
    }
}

impl fmt::Display for DocumentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "doc_{}", self.0)
    }
}

/// Unique, stable identifier for a layer within a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LayerId(pub u32);

impl LayerId {
    pub fn new(id: u32) -> Self {
        LayerId(id)
    }

    /// Maximum value, reserved for document-level composite
    pub const MAX: u32 = u32::MAX;
}

impl fmt::Display for LayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "layer_{}", self.0)
    }
}

/// Unique identifier for a filter instance within a layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FilterInstanceId(pub Uuid);

impl FilterInstanceId {
    pub fn new() -> Self {
        FilterInstanceId(Uuid::new_v4())
    }
}

impl Default for FilterInstanceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for FilterInstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for a palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PaletteId(pub u32);

impl PaletteId {
    pub fn new(id: u32) -> Self {
        PaletteId(id)
    }
}

impl fmt::Display for PaletteId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "palette_{}", self.0)
    }
}

/// Reference to a color profile (placeholder for now).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorProfileRef {
    /// sRGB standard
    #[default]
    SRgb,
    /// Placeholder for other profiles
    Other(String),
}

/// Layer kind: whether it stores raster pixels or applies filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerKind {
    /// Stores raster pixel data
    Raster,
    /// Applies filters to layers below
    Adjustment,
}

/// Blend mode for layer compositing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord, Default)]
pub enum BlendMode {
    #[default]
    Normal = 0,
    Multiply = 1,
    Screen = 2,
    Overlay = 3,
    Darken = 4,
    Lighten = 5,
    ColorDodge = 6,
    ColorBurn = 7,
    HardLight = 8,
    SoftLight = 9,
    Difference = 10,
    Exclusion = 11,
    // 4 reserved for future
    Reserved12 = 12,
    Reserved13 = 13,
    Reserved14 = 14,
    Reserved15 = 15,
}

impl BlendMode {
    /// Reserved slots are not selectable (Track I: reject on set).
    pub fn is_reserved(self) -> bool {
        matches!(
            self,
            BlendMode::Reserved12
                | BlendMode::Reserved13
                | BlendMode::Reserved14
                | BlendMode::Reserved15
        )
    }

    /// Parse a UI / IPC blend name. Accepts Display names and common aliases.
    /// Reserved and unknown names return `None` (caller must reject).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim() {
            "Normal" | "normal" => Some(Self::Normal),
            "Multiply" | "multiply" => Some(Self::Multiply),
            "Screen" | "screen" => Some(Self::Screen),
            "Overlay" | "overlay" => Some(Self::Overlay),
            "Darken" | "darken" => Some(Self::Darken),
            "Lighten" | "lighten" => Some(Self::Lighten),
            "ColorDodge" | "color_dodge" | "colordodge" => Some(Self::ColorDodge),
            "ColorBurn" | "color_burn" | "colorburn" => Some(Self::ColorBurn),
            "HardLight" | "hard_light" | "hardlight" => Some(Self::HardLight),
            "SoftLight" | "soft_light" | "softlight" => Some(Self::SoftLight),
            "Difference" | "difference" => Some(Self::Difference),
            "Exclusion" | "exclusion" => Some(Self::Exclusion),
            _ => None,
        }
    }
}

impl fmt::Display for BlendMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiply",
            BlendMode::Screen => "Screen",
            BlendMode::Overlay => "Overlay",
            BlendMode::Darken => "Darken",
            BlendMode::Lighten => "Lighten",
            BlendMode::ColorDodge => "ColorDodge",
            BlendMode::ColorBurn => "ColorBurn",
            BlendMode::HardLight => "HardLight",
            BlendMode::SoftLight => "SoftLight",
            BlendMode::Difference => "Difference",
            BlendMode::Exclusion => "Exclusion",
            _ => "Reserved",
        };
        write!(f, "{}", name)
    }
}

/// Bounding box for a layer in tiles at MipLevel 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileBounds {
    /// Minimum tile coordinate (inclusive)
    pub min_x: u32,
    pub min_y: u32,
    /// Maximum tile coordinate (inclusive)
    pub max_x: u32,
    pub max_y: u32,
}

impl TileBounds {
    pub fn new(min_x: u32, min_y: u32, max_x: u32, max_y: u32) -> Self {
        TileBounds {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Check if a tile coordinate is within bounds
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Create bounds for entire document at given resolution
    pub fn full_document(width: u32, height: u32) -> Self {
        const TILE_SIZE: u32 = 256;
        let max_x = width.saturating_sub(1) / TILE_SIZE;
        let max_y = height.saturating_sub(1) / TILE_SIZE;
        TileBounds {
            min_x: 0,
            min_y: 0,
            max_x,
            max_y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_id_display() {
        let id = DocumentId(42);
        assert_eq!(id.to_string(), "doc_42");
    }

    #[test]
    fn layer_id_display() {
        let id = LayerId(100);
        assert_eq!(id.to_string(), "layer_100");
    }

    #[test]
    fn filter_instance_id_is_unique() {
        let id1 = FilterInstanceId::new();
        let id2 = FilterInstanceId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn palette_id_serializes() {
        let id = PaletteId(5);
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: PaletteId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[test]
    fn blend_mode_default_is_normal() {
        assert_eq!(BlendMode::default(), BlendMode::Normal);
    }

    #[test]
    fn blend_mode_from_name_accepts_ui_and_rejects_reserved() {
        assert_eq!(BlendMode::from_name("Normal"), Some(BlendMode::Normal));
        assert_eq!(BlendMode::from_name("color_dodge"), Some(BlendMode::ColorDodge));
        assert_eq!(BlendMode::from_name("ColorDodge"), Some(BlendMode::ColorDodge));
        assert!(BlendMode::from_name("Reserved").is_none());
        assert!(BlendMode::from_name("Reserved12").is_none());
        assert!(BlendMode::Reserved12.is_reserved());
        assert!(!BlendMode::Multiply.is_reserved());
    }

    #[test]
    fn tile_bounds_contains() {
        let bounds = TileBounds::new(5, 10, 15, 20);
        assert!(bounds.contains(10, 15));
        assert!(!bounds.contains(4, 10));
        assert!(!bounds.contains(16, 20));
    }

    #[test]
    fn tile_bounds_full_document() {
        let bounds = TileBounds::full_document(512, 512);
        assert_eq!(bounds.min_x, 0);
        assert_eq!(bounds.min_y, 0);
        assert_eq!(bounds.max_x, 1);
        assert_eq!(bounds.max_y, 1);
    }
}
