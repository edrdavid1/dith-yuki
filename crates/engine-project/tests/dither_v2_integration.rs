//! End-to-end integration tests for the DitherV2 filter pipeline.
//!
//! Tests the full path: Document → Layer → DitherV2 filter → apply_filter_to_tile → verify output.
//!
//! **Validates: Requirements 9.1, 9.2, 9.3, 9.4**

use engine_color::palette::LinearColor;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filter::{
    DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::apply::apply_filter_to_tile;
use engine_project::{Document, DocumentHandle, Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};

/// Helper: create a tile filled with a uniform color (all pixels same RGBA).
fn make_uniform_tile(r: f32, g: f32, b: f32, a: f32) -> PixelTile {
    let mut tile = PixelTile::new();
    let full_size = TILE_SIZE + 2 * HALO; // 260
    for y in 0..full_size {
        for x in 0..full_size {
            tile.set(x, y, 0, r);
            tile.set(x, y, 1, g);
            tile.set(x, y, 2, b);
            tile.set(x, y, 3, a);
        }
    }
    tile
}

/// Helper: create a tile with a gradient pattern (for more interesting dither output).
fn make_gradient_tile() -> PixelTile {
    let mut tile = PixelTile::new();
    let full_size = TILE_SIZE + 2 * HALO;
    for y in 0..full_size {
        for x in 0..full_size {
            let val = x as f32 / full_size as f32;
            tile.set(x, y, 0, val);
            tile.set(x, y, 1, val * 0.7);
            tile.set(x, y, 2, 1.0 - val);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

/// Compute the set of valid quantization levels for a given `levels` parameter.
/// Valid values are: {k / (levels - 1) : k = 0, 1, ..., levels - 1}
fn valid_uniform_levels(levels: u16) -> Vec<f32> {
    let l = levels as f32;
    (0..levels).map(|k| k as f32 / (l - 1.0)).collect()
}

/// Check if a value matches one of the valid quantized levels (with tolerance).
fn is_valid_level(value: f32, valid_levels: &[f32]) -> bool {
    valid_levels.iter().any(|&level| (value - level).abs() < 1e-5)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 1: Ordered dithering (Bayer4x4) + Uniform quantization
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ordered_bayer4x4_uniform_quantization() {
    // Set up document with a layer containing a DitherV2 filter
    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);
    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);

    let dither_filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::Bayer4x4,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }),
    );
    layer.filters.push(dither_filter);
    doc.root.push(LayerNode::Leaf(layer));

    // Create caches
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();

    // Create a gradient tile for interesting dither patterns
    let tile = make_gradient_tile();
    let coord = TileCoord { level: 0, x: 0, y: 0 };

    // Apply the filter
    let layer_ref = match &doc.root[0] {
        LayerNode::Leaf(l) => l,
        _ => panic!("Expected leaf layer"),
    };
    let result = apply_filter_to_tile(&tile, layer_ref, coord, &palette_cache, &lut_cache, &threshold_cache, &doc);
    assert!(result.is_ok(), "apply_filter_to_tile failed: {:?}", result.err());

    let output = result.unwrap();

    // Verify output pixels are valid quantized levels (levels=4 → {0.0, 1/3, 2/3, 1.0})
    let valid_levels = valid_uniform_levels(4);

    // Check a sampling of pixels in the core area
    for y in (HALO..(HALO + TILE_SIZE)).step_by(16) {
        for x in (HALO..(HALO + TILE_SIZE)).step_by(16) {
            let r = output.at(x, y, 0);
            let g = output.at(x, y, 1);
            let b = output.at(x, y, 2);
            let a = output.at(x, y, 3);

            assert!(
                is_valid_level(r, &valid_levels),
                "Pixel ({}, {}): R={} not a valid level for levels=4. Valid: {:?}",
                x, y, r, valid_levels
            );
            assert!(
                is_valid_level(g, &valid_levels),
                "Pixel ({}, {}): G={} not a valid level for levels=4",
                x, y, g
            );
            assert!(
                is_valid_level(b, &valid_levels),
                "Pixel ({}, {}): B={} not a valid level for levels=4",
                x, y, b
            );
            // Alpha should be preserved
            assert_eq!(a, 1.0, "Pixel ({}, {}): alpha not preserved", x, y);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 2: Error diffusion (Floyd-Steinberg) + Uniform quantization
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn error_diffusion_floyd_steinberg_uniform_quantization() {
    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);
    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);

    let dither_filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }),
    );
    layer.filters.push(dither_filter);
    doc.root.push(LayerNode::Leaf(layer));

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();

    let tile = make_gradient_tile();
    let coord = TileCoord { level: 0, x: 0, y: 0 };

    let layer_ref = match &doc.root[0] {
        LayerNode::Leaf(l) => l,
        _ => panic!("Expected leaf layer"),
    };
    let result = apply_filter_to_tile(&tile, layer_ref, coord, &palette_cache, &lut_cache, &threshold_cache, &doc);
    assert!(result.is_ok(), "apply_filter_to_tile failed: {:?}", result.err());

    let output = result.unwrap();

    // Verify output pixels are valid quantized levels in the core area
    // Error diffusion may produce slightly different results at edges due to
    // boundary conditions, so we check a central region
    let valid_levels = valid_uniform_levels(4);
    let margin = HALO + 10; // skip boundary pixels where error seeding effects dominate

    for y in (margin..(HALO + TILE_SIZE - 10)).step_by(16) {
        for x in (margin..(HALO + TILE_SIZE - 10)).step_by(16) {
            let r = output.at(x, y, 0);
            let g = output.at(x, y, 1);
            let b = output.at(x, y, 2);
            let a = output.at(x, y, 3);

            assert!(
                is_valid_level(r, &valid_levels),
                "Pixel ({}, {}): R={} not a valid level for levels=4. Valid: {:?}",
                x, y, r, valid_levels
            );
            assert!(
                is_valid_level(g, &valid_levels),
                "Pixel ({}, {}): G={} not a valid level for levels=4",
                x, y, g
            );
            assert!(
                is_valid_level(b, &valid_levels),
                "Pixel ({}, {}): B={} not a valid level for levels=4",
                x, y, b
            );
            assert_eq!(a, 1.0, "Pixel ({}, {}): alpha not preserved", x, y);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 3: Ordered dithering (Bayer8x8) + Palette-constrained quantization
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn ordered_bayer8x8_palette_constrained() {
    let mut doc = Document::new(DocumentId::new(1), 512, 512);

    // Add a palette to the document
    let palette_colors = vec![
        LinearColor { r: 0.0, g: 0.0, b: 0.0 },   // black
        LinearColor { r: 1.0, g: 0.0, b: 0.0 },   // red
        LinearColor { r: 0.0, g: 1.0, b: 0.0 },   // green
        LinearColor { r: 0.0, g: 0.0, b: 1.0 },   // blue
        LinearColor { r: 1.0, g: 1.0, b: 1.0 },   // white
        LinearColor { r: 1.0, g: 1.0, b: 0.0 },   // yellow
    ];
    let palette_id = doc.add_palette("Test Palette".to_string(), palette_colors.clone());

    let layer_id = LayerId::new(1);
    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);

    let dither_filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::Bayer8x8,
            levels: 4, // ignored when palette_id is set
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: Some(palette_id),
            ..Default::default()
        }),
    );
    layer.filters.push(dither_filter);
    doc.root.push(LayerNode::Leaf(layer));

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();

    let tile = make_gradient_tile();
    let coord = TileCoord { level: 0, x: 0, y: 0 };

    let layer_ref = match &doc.root[0] {
        LayerNode::Leaf(l) => l,
        _ => panic!("Expected leaf layer"),
    };
    let result = apply_filter_to_tile(&tile, layer_ref, coord, &palette_cache, &lut_cache, &threshold_cache, &doc);
    assert!(result.is_ok(), "apply_filter_to_tile failed: {:?}", result.err());

    let output = result.unwrap();

    // Verify all output pixels are members of the palette
    for y in (HALO..(HALO + TILE_SIZE)).step_by(8) {
        for x in (HALO..(HALO + TILE_SIZE)).step_by(8) {
            let r = output.at(x, y, 0);
            let g = output.at(x, y, 1);
            let b = output.at(x, y, 2);
            let a = output.at(x, y, 3);

            let is_palette_member = palette_colors.iter().any(|c| {
                (c.r - r).abs() < 1e-5 && (c.g - g).abs() < 1e-5 && (c.b - b).abs() < 1e-5
            });
            assert!(
                is_palette_member,
                "Pixel ({}, {}): ({}, {}, {}) is not a member of the palette",
                x, y, r, g, b
            );
            assert_eq!(a, 1.0, "Pixel ({}, {}): alpha not preserved", x, y);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 4: Error diffusion (Floyd-Steinberg) + Palette-constrained quantization
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn error_diffusion_floyd_steinberg_palette_constrained() {
    let mut doc = Document::new(DocumentId::new(1), 512, 512);

    // Add a palette
    let palette_colors = vec![
        LinearColor { r: 0.0, g: 0.0, b: 0.0 },   // black
        LinearColor { r: 1.0, g: 0.0, b: 0.0 },   // red
        LinearColor { r: 0.0, g: 1.0, b: 0.0 },   // green
        LinearColor { r: 0.0, g: 0.0, b: 1.0 },   // blue
        LinearColor { r: 1.0, g: 1.0, b: 1.0 },   // white
    ];
    let palette_id = doc.add_palette("Diffusion Palette".to_string(), palette_colors.clone());

    let layer_id = LayerId::new(1);
    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);

    let dither_filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 2, // ignored when palette_id is set
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: Some(palette_id),
            ..Default::default()
        }),
    );
    layer.filters.push(dither_filter);
    doc.root.push(LayerNode::Leaf(layer));

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();

    let tile = make_uniform_tile(0.5, 0.3, 0.7, 1.0);
    let coord = TileCoord { level: 0, x: 0, y: 0 };

    let layer_ref = match &doc.root[0] {
        LayerNode::Leaf(l) => l,
        _ => panic!("Expected leaf layer"),
    };
    let result = apply_filter_to_tile(&tile, layer_ref, coord, &palette_cache, &lut_cache, &threshold_cache, &doc);
    assert!(result.is_ok(), "apply_filter_to_tile failed: {:?}", result.err());

    let output = result.unwrap();

    // Verify output pixels in the core area are palette members
    let margin = HALO + 5;
    for y in (margin..(HALO + TILE_SIZE - 5)).step_by(8) {
        for x in (margin..(HALO + TILE_SIZE - 5)).step_by(8) {
            let r = output.at(x, y, 0);
            let g = output.at(x, y, 1);
            let b = output.at(x, y, 2);
            let a = output.at(x, y, 3);

            let is_palette_member = palette_colors.iter().any(|c| {
                (c.r - r).abs() < 1e-5 && (c.g - g).abs() < 1e-5 && (c.b - b).abs() < 1e-5
            });
            assert!(
                is_palette_member,
                "Pixel ({}, {}): ({}, {}, {}) is not a palette member",
                x, y, r, g, b
            );
            assert_eq!(a, 1.0, "Alpha not preserved at ({}, {})", x, y);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 5: Legacy auto-migration (FilterParams::Dither → DitherV2)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn legacy_dither_auto_migration() {
    use engine_project::filter::DitherMode;

    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);
    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);

    // Create a legacy Dither filter (not DitherV2)
    let legacy_filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::Dither {
            mode: DitherMode::Bayer { matrix_size: 4 },
            color_depth: 3, // → levels = 2^3 = 8
        },
    );
    layer.filters.push(legacy_filter);
    doc.root.push(LayerNode::Leaf(layer));

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();

    let tile = make_gradient_tile();
    let coord = TileCoord { level: 0, x: 0, y: 0 };

    let layer_ref = match &doc.root[0] {
        LayerNode::Leaf(l) => l,
        _ => panic!("Expected leaf layer"),
    };

    // The filter dispatcher should auto-migrate the legacy filter to V2 and process it
    let result = apply_filter_to_tile(&tile, layer_ref, coord, &palette_cache, &lut_cache, &threshold_cache, &doc);
    assert!(
        result.is_ok(),
        "Legacy dither auto-migration failed: {:?}",
        result.err()
    );

    let output = result.unwrap();

    // Verify output pixels are valid for levels=8 (2^3)
    let valid_levels = valid_uniform_levels(8);

    for y in (HALO..(HALO + TILE_SIZE)).step_by(32) {
        for x in (HALO..(HALO + TILE_SIZE)).step_by(32) {
            let r = output.at(x, y, 0);
            let g = output.at(x, y, 1);
            let b = output.at(x, y, 2);

            assert!(
                is_valid_level(r, &valid_levels),
                "Legacy migration: Pixel ({}, {}): R={} not valid for levels=8",
                x, y, r
            );
            assert!(
                is_valid_level(g, &valid_levels),
                "Legacy migration: Pixel ({}, {}): G={} not valid for levels=8",
                x, y, g
            );
            assert!(
                is_valid_level(b, &valid_levels),
                "Legacy migration: Pixel ({}, {}): B={} not valid for levels=8",
                x, y, b
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 6: Legacy error diffusion auto-migration
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn legacy_error_diffusion_auto_migration() {
    use engine_project::filter::{DiffusionKernel, DitherMode};

    let mut doc = Document::new(DocumentId::new(1), 512, 512);
    let layer_id = LayerId::new(1);
    let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);

    // Legacy error diffusion filter
    let legacy_filter = FilterInstance::new(
        FilterKind::Dither,
        FilterParams::Dither {
            mode: DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::FloydSteinberg,
            },
            color_depth: 2, // → levels = 2^2 = 4
        },
    );
    layer.filters.push(legacy_filter);
    doc.root.push(LayerNode::Leaf(layer));

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();

    let tile = make_uniform_tile(0.6, 0.4, 0.8, 1.0);
    let coord = TileCoord { level: 0, x: 0, y: 0 };

    let layer_ref = match &doc.root[0] {
        LayerNode::Leaf(l) => l,
        _ => panic!("Expected leaf layer"),
    };

    let result = apply_filter_to_tile(&tile, layer_ref, coord, &palette_cache, &lut_cache, &threshold_cache, &doc);
    assert!(
        result.is_ok(),
        "Legacy error diffusion auto-migration failed: {:?}",
        result.err()
    );

    let output = result.unwrap();

    // Verify output pixels are valid for levels=4
    let valid_levels = valid_uniform_levels(4);
    let margin = HALO + 10;

    for y in (margin..(HALO + TILE_SIZE - 10)).step_by(16) {
        for x in (margin..(HALO + TILE_SIZE - 10)).step_by(16) {
            let r = output.at(x, y, 0);
            let g = output.at(x, y, 1);
            let b = output.at(x, y, 2);

            assert!(
                is_valid_level(r, &valid_levels),
                "Legacy FS: Pixel ({}, {}): R={} not valid for levels=4",
                x, y, r
            );
            assert!(
                is_valid_level(g, &valid_levels),
                "Legacy FS: Pixel ({}, {}): G={} not valid for levels=4",
                x, y, g
            );
            assert!(
                is_valid_level(b, &valid_levels),
                "Legacy FS: Pixel ({}, {}): B={} not valid for levels=4",
                x, y, b
            );
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Test 7: Full pipeline via DocumentHandle (thread-safe)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn full_pipeline_via_document_handle() {
    let doc = Document::new(DocumentId::new(1), 512, 512);
    let handle = DocumentHandle::new(doc);

    // Add a layer with DitherV2 filter through the handle
    handle.mutate(|doc| {
        let layer_id = LayerId::new(1);
        let mut layer = Layer::new(layer_id, LayerKind::Raster, 512, 512);

        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer2x2,
                levels: 2, // binary output
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: None,
            ..Default::default()
            }),
        );
        layer.filters.push(filter);
        doc.root.push(LayerNode::Leaf(layer));
        doc.increment_generation();
    });

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let tile = make_uniform_tile(0.5, 0.5, 0.5, 1.0);
    let coord = TileCoord { level: 0, x: 0, y: 0 };

    // Get snapshot and apply filter
    let snapshot = handle.snapshot();
    let layer_ref = match &snapshot.root[0] {
        LayerNode::Leaf(l) => l,
        _ => panic!("Expected leaf layer"),
    };

    let result = apply_filter_to_tile(&tile, layer_ref, coord, &palette_cache, &lut_cache, &threshold_cache, &snapshot);
    assert!(result.is_ok(), "Full pipeline failed: {:?}", result.err());

    let output = result.unwrap();

    // With levels=2, output should be binary: 0.0 or 1.0
    let valid_levels = valid_uniform_levels(2);
    for y in (HALO..(HALO + TILE_SIZE)).step_by(16) {
        for x in (HALO..(HALO + TILE_SIZE)).step_by(16) {
            let r = output.at(x, y, 0);
            assert!(
                is_valid_level(r, &valid_levels),
                "Binary dither: Pixel ({}, {}): R={} should be 0.0 or 1.0",
                x, y, r
            );
        }
    }
}
