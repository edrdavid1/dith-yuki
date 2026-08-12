//! Palette generation integration: generate palettes from layer pixel data.
//!
//! This module provides utility functions to generate palettes from a layer's
//! pixel content and store the result in the Document.

use engine_color::palette::generate::{generate_palette, PaletteGenMethod};
use engine_color::palette::LinearColor;

use crate::document::Document;
use crate::error::EngineError;
use crate::layer::LayerNode;
use crate::types::{LayerId, PaletteId};

/// Find a layer by ID in the document tree (recursive).
fn find_layer_in_nodes<'a>(
    nodes: &'a [LayerNode],
    layer_id: LayerId,
) -> Option<&'a crate::layer::Layer> {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if layer.id == layer_id {
                    return Some(layer);
                }
            }
            LayerNode::Group(group) => {
                if let Some(found) = find_layer_in_nodes(&group.children, layer_id) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Format a palette name from the layer name and generation method.
/// Truncates to 64 characters.
fn format_palette_name(layer_name: &str, method: PaletteGenMethod) -> String {
    let method_str = match method {
        PaletteGenMethod::MedianCut => "MedianCut",
        PaletteGenMethod::KMeans => "KMeans",
    };
    let full_name = format!("{}_{}", layer_name, method_str);
    if full_name.len() > 64 {
        full_name[..64].to_string()
    } else {
        full_name
    }
}

/// Generate a palette from a layer's pixel content and store it in the document.
///
/// This function accepts an iterator of pixels (with alpha) extracted from the
/// layer's tiles. It filters out fully transparent pixels (alpha == 0.0), calls
/// the palette generation algorithm, and stores the result in the document.
///
/// # Arguments
/// * `document` - Mutable reference to the document
/// * `layer_id` - The layer to generate a palette from (used for name and validation)
/// * `pixels` - Iterator of `(LinearColor, f32)` tuples where f32 is the alpha value
/// * `target_count` - Number of colors to generate (2–256)
/// * `method` - Generation algorithm (MedianCut or KMeans)
///
/// # Returns
/// The `PaletteId` of the newly created palette.
///
/// # Errors
/// Returns `EngineError::LayerNotFound` if the layer ID is not in the document.
/// Returns `EngineError::InvalidState` if the layer has no non-transparent pixels.
pub fn generate_palette_from_layer(
    document: &mut Document,
    layer_id: LayerId,
    pixels: impl Iterator<Item = (LinearColor, f32)>,
    target_count: u16,
    method: PaletteGenMethod,
) -> Result<PaletteId, EngineError> {
    // 1. Find the layer to get its name and validate it exists
    let layer_name = find_layer_in_nodes(&document.root, layer_id)
        .ok_or_else(|| EngineError::layer_not_found(layer_id))?
        .name
        .clone();

    // 2. Filter out fully transparent pixels (alpha == 0.0)
    let opaque_pixels: Vec<LinearColor> = pixels
        .filter(|(_, alpha)| *alpha > 0.0)
        .map(|(color, _)| color)
        .collect();

    // 3. Error if no non-transparent pixels
    if opaque_pixels.is_empty() {
        return Err(EngineError::invalid_state(
            "layer has no non-transparent pixels for palette generation",
        ));
    }

    // 4. Call generate_palette
    let colors = generate_palette(opaque_pixels.into_iter(), target_count, method).map_err(
        |e| EngineError::invalid_state(format!("palette generation failed: {}", e)),
    )?;

    // 5. Format name as "{layer_name}_{method}" truncated to 64 chars
    let palette_name = format_palette_name(&layer_name, method);

    // 6. Store in document
    let palette_id = document.add_palette(palette_name, colors);

    Ok(palette_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::layer::{Layer, LayerNode};
    use crate::types::{DocumentId, LayerId, LayerKind};

    fn make_doc_with_layer(layer_id: u32, layer_name: &str) -> Document {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let mut layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, 256, 256);
        layer.name = layer_name.to_string();
        doc.root.push(LayerNode::Leaf(layer));
        doc
    }

    #[test]
    fn test_generate_palette_from_layer_basic() {
        let mut doc = make_doc_with_layer(1, "Background");

        // Create some pixels: reds and blues with alpha > 0
        let pixels: Vec<(LinearColor, f32)> = (0..100)
            .map(|_| (LinearColor { r: 1.0, g: 0.0, b: 0.0 }, 1.0))
            .chain(
                (0..100).map(|_| (LinearColor { r: 0.0, g: 0.0, b: 1.0 }, 1.0)),
            )
            .collect();

        let result = generate_palette_from_layer(
            &mut doc,
            LayerId::new(1),
            pixels.into_iter(),
            2,
            PaletteGenMethod::MedianCut,
        );

        assert!(result.is_ok());
        let palette_id = result.unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        assert_eq!(palette.name, "Background_MedianCut");
        assert_eq!(palette.colors.len(), 2);
    }

    #[test]
    fn test_generate_palette_from_layer_kmeans() {
        let mut doc = make_doc_with_layer(1, "MyLayer");

        let pixels: Vec<(LinearColor, f32)> = (0..50)
            .map(|_| (LinearColor { r: 0.9, g: 0.0, b: 0.0 }, 1.0))
            .chain(
                (0..50).map(|_| (LinearColor { r: 0.0, g: 0.9, b: 0.0 }, 1.0)),
            )
            .collect();

        let result = generate_palette_from_layer(
            &mut doc,
            LayerId::new(1),
            pixels.into_iter(),
            2,
            PaletteGenMethod::KMeans,
        );

        assert!(result.is_ok());
        let palette_id = result.unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        assert_eq!(palette.name, "MyLayer_KMeans");
        assert!(palette.colors.len() <= 2);
    }

    #[test]
    fn test_generate_palette_filters_transparent_pixels() {
        let mut doc = make_doc_with_layer(1, "Layer1");

        // Mix of transparent and opaque pixels
        let pixels: Vec<(LinearColor, f32)> = vec![
            (LinearColor { r: 1.0, g: 0.0, b: 0.0 }, 0.0), // transparent, should be skipped
            (LinearColor { r: 0.0, g: 1.0, b: 0.0 }, 1.0), // opaque
            (LinearColor { r: 0.0, g: 0.0, b: 1.0 }, 0.5), // semi-transparent, should be included
        ];

        let result = generate_palette_from_layer(
            &mut doc,
            LayerId::new(1),
            pixels.into_iter(),
            4,
            PaletteGenMethod::MedianCut,
        );

        assert!(result.is_ok());
        let palette_id = result.unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        // Should have at most 2 colors (only 2 non-transparent pixels)
        assert!(palette.colors.len() <= 2);
    }

    #[test]
    fn test_generate_palette_layer_not_found() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);

        let pixels = vec![(LinearColor { r: 1.0, g: 0.0, b: 0.0 }, 1.0)];

        let result = generate_palette_from_layer(
            &mut doc,
            LayerId::new(999),
            pixels.into_iter(),
            4,
            PaletteGenMethod::MedianCut,
        );

        assert!(matches!(result, Err(EngineError::LayerNotFound { .. })));
    }

    #[test]
    fn test_generate_palette_no_opaque_pixels() {
        let mut doc = make_doc_with_layer(1, "EmptyLayer");

        // All pixels are fully transparent
        let pixels: Vec<(LinearColor, f32)> = vec![
            (LinearColor { r: 1.0, g: 0.0, b: 0.0 }, 0.0),
            (LinearColor { r: 0.0, g: 1.0, b: 0.0 }, 0.0),
        ];

        let result = generate_palette_from_layer(
            &mut doc,
            LayerId::new(1),
            pixels.into_iter(),
            4,
            PaletteGenMethod::MedianCut,
        );

        assert!(matches!(result, Err(EngineError::InvalidState { .. })));
    }

    #[test]
    fn test_generate_palette_name_truncation() {
        // Layer name that when combined with method exceeds 64 chars
        let long_name = "A".repeat(60); // 60 chars + "_MedianCut" = 70 chars > 64
        let mut doc = make_doc_with_layer(1, &long_name);

        let pixels: Vec<(LinearColor, f32)> =
            vec![(LinearColor { r: 1.0, g: 0.0, b: 0.0 }, 1.0); 10];

        let result = generate_palette_from_layer(
            &mut doc,
            LayerId::new(1),
            pixels.into_iter(),
            4,
            PaletteGenMethod::MedianCut,
        );

        assert!(result.is_ok());
        let palette_id = result.unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        assert!(palette.name.len() <= 64);
        assert_eq!(palette.name.len(), 64);
    }

    #[test]
    fn test_format_palette_name_normal() {
        let name = format_palette_name("Background", PaletteGenMethod::MedianCut);
        assert_eq!(name, "Background_MedianCut");

        let name = format_palette_name("Layer1", PaletteGenMethod::KMeans);
        assert_eq!(name, "Layer1_KMeans");
    }

    #[test]
    fn test_format_palette_name_truncation() {
        let long_name = "X".repeat(60);
        let name = format_palette_name(&long_name, PaletteGenMethod::MedianCut);
        assert_eq!(name.len(), 64);
        assert!(name.starts_with("XXXX"));
    }

    #[test]
    fn test_generate_palette_stores_revision_1() {
        let mut doc = make_doc_with_layer(1, "TestLayer");

        let pixels: Vec<(LinearColor, f32)> =
            vec![(LinearColor { r: 0.5, g: 0.5, b: 0.5 }, 1.0); 20];

        let result = generate_palette_from_layer(
            &mut doc,
            LayerId::new(1),
            pixels.into_iter(),
            4,
            PaletteGenMethod::MedianCut,
        );

        let palette_id = result.unwrap();
        let palette = doc.get_palette(palette_id).unwrap();
        assert_eq!(palette.revision, 1);
    }
}
