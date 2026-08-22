//! Layer compositor for blending visible layers bottom-to-top.
//!
//! This module implements the tile compositing pipeline:
//! - Walk layer tree bottom-to-top
//! - For each visible leaf layer: fetch Processed tile, apply mask, blend into composite
//! - Handle group isolation: push/pop composite stack at GroupStart/GroupEnd
//! - Return fully transparent tile if no visible layers contribute content

use crate::error::EngineError;
use crate::layer::LayerNode;
use crate::mask::MaskRef;
use crate::types::BlendMode;
use engine_tiles::cache::TileCache;
use engine_tiles::tile::PixelTile;
use engine_tiles::types::{CacheStage, TileCoord, TileKey};
use engine_tiles::{HALO, TILE_SIZE};

/// Composite all visible layers at a tile coordinate.
///
/// Walks the layer tree bottom-to-top, blending each visible layer's
/// Processed tile into the running composite using its blend mode and opacity.
/// Groups are handled with isolation: children are composited within the group
/// first, then the group result is blended into the parent composite.
///
/// `doc` must be the runtime `DocumentId` — TileCache keys are namespaced by it.
///
/// Returns a fully transparent tile if no visible layers contribute content.
pub fn composite_tile(
    root: &[LayerNode],
    doc: u32,
    coord: TileCoord,
    cache: &TileCache,
) -> Result<PixelTile, EngineError> {
    let mut composite = PixelTile::new(); // starts fully transparent
    composite_nodes(root, doc, coord, cache, &mut composite)?;
    Ok(composite)
}

/// Recursively composite a slice of layer nodes into the destination tile.
/// Processes nodes in order (bottom-to-top as stored in the document).
fn composite_nodes(
    nodes: &[LayerNode],
    doc: u32,
    coord: TileCoord,
    cache: &TileCache,
    dst: &mut PixelTile,
) -> Result<(), EngineError> {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if !layer.visible {
                    continue;
                }
                // Get Processed tile for this layer from cache
                let processed = get_processed_tile(doc, layer.id.0, coord, cache);
                // Apply mask if present
                let masked = apply_layer_mask(&layer.mask, &processed, doc, coord, cache);
                // Blend into composite
                blend_tile(dst, &masked, layer.blend_mode, layer.opacity);
            }
            LayerNode::Group(group) => {
                if !group.visible {
                    // Skip invisible groups and all their descendants
                    continue;
                }
                // Group isolation: composite children into a fresh tile
                let mut group_composite = PixelTile::new();
                composite_nodes(&group.children, doc, coord, cache, &mut group_composite)?;
                // Apply group mask if present
                let masked = apply_layer_mask(
                    &group.mask,
                    &group_composite,
                    doc,
                    coord,
                    cache,
                );
                // Blend group result into parent composite
                blend_tile(dst, &masked, group.blend_mode, group.opacity);
            }
        }
    }
    Ok(())
}

/// Fetch the Processed-stage tile for a layer from the cache.
/// Falls back to the Raw-stage tile if no Processed tile exists
/// (this handles the case where filters haven't been applied yet).
/// If neither exists, returns a fully transparent tile.
fn get_processed_tile(doc: u32, layer_id: u32, coord: TileCoord, cache: &TileCache) -> PixelTile {
    let processed_key = TileKey {
        doc,
        layer: layer_id,
        coord,
        stage: CacheStage::Processed,
    };
    if let Some(tile) = cache.get_entry(processed_key) {
        let mut result = PixelTile::new();
        result.data.copy_from_slice(&tile.data);
        return result;
    }

    // Fallback: use Raw tile if Processed isn't available yet
    let raw_key = TileKey {
        doc,
        layer: layer_id,
        coord,
        stage: CacheStage::Raw,
    };
    match cache.get_entry(raw_key) {
        Some(tile) => {
            let mut result = PixelTile::new();
            result.data.copy_from_slice(&tile.data);
            result
        }
        None => PixelTile::new(), // fully transparent
    }
}

/// Apply a layer mask to a tile.
/// Multiplies the tile's alpha by the mask's luminance (or 1-luminance if inverted).
/// If mask is None or disabled, returns the tile unchanged (by reference pattern).
fn apply_layer_mask(
    mask: &Option<MaskRef>,
    tile: &PixelTile,
    doc: u32,
    coord: TileCoord,
    cache: &TileCache,
) -> PixelTile {
    let mask_ref = match mask {
        Some(m) if m.enabled => m,
        _ => {
            // No mask or disabled: clone tile data
            let mut result = PixelTile::new();
            result.data.copy_from_slice(&tile.data);
            return result;
        }
    };

    // Get the mask tile from cache (using the external layer ID)
    let mask_layer_id = match mask_ref.get_external_layer() {
        Some(id) => id.0,
        None => {
            // Non-external mask: return tile unchanged
            let mut result = PixelTile::new();
            result.data.copy_from_slice(&tile.data);
            return result;
        }
    };

    let mask_key = TileKey {
        doc,
        layer: mask_layer_id,
        coord,
        stage: CacheStage::Processed,
    };
    let mask_tile = match cache.get_entry(mask_key) {
        Some(t) => t,
        None => {
            // Mask tile not available: return tile unchanged
            let mut result = PixelTile::new();
            result.data.copy_from_slice(&tile.data);
            return result;
        }
    };

    let mut result = PixelTile::new();
    result.data.copy_from_slice(&tile.data);

    for y in HALO..(HALO + TILE_SIZE) {
        for x in HALO..(HALO + TILE_SIZE) {
            // Luminance = 0.2126*R + 0.7152*G + 0.0722*B
            let lum = 0.2126 * mask_tile.at(x, y, 0)
                + 0.7152 * mask_tile.at(x, y, 1)
                + 0.0722 * mask_tile.at(x, y, 2);
            let mask_value = if mask_ref.inverted { 1.0 - lum } else { lum };
            let alpha = result.at(x, y, 3) * mask_value;
            result.set(x, y, 3, alpha);
        }
    }
    result
}

/// Per-pixel blending of src tile onto dst tile using blend mode and opacity.
/// Operates in linear f32 RGBA color space with Porter-Duff "over" compositing.
/// Uses row-based SIMD processing for the main tile region.
pub fn blend_tile(dst: &mut PixelTile, src: &PixelTile, mode: BlendMode, opacity: f32) {
    use crate::simd::blend_row_simd;

    let size = (TILE_SIZE + 2 * HALO) as usize; // 260
    for y in HALO..(HALO + TILE_SIZE) {
        let row_start = (y as usize * size + HALO as usize) * 4;
        let row_end = row_start + (TILE_SIZE as usize) * 4;
        blend_row_simd(
            &mut dst.data[row_start..row_end],
            &src.data[row_start..row_end],
            mode,
            opacity,
        );
    }
}

/// Apply a single blend mode formula per channel.
/// All formulas operate on linear f32 values in [0, 1].
#[cfg(test)]
fn apply_blend_mode(mode: BlendMode, src: f32, dst: f32) -> f32 {
    match mode {
        BlendMode::Normal => src,
        BlendMode::Multiply => src * dst,
        BlendMode::Screen => src + dst - src * dst,
        BlendMode::Overlay => {
            if dst < 0.5 {
                2.0 * src * dst
            } else {
                1.0 - 2.0 * (1.0 - src) * (1.0 - dst)
            }
        }
        BlendMode::Darken => src.min(dst),
        BlendMode::Lighten => src.max(dst),
        BlendMode::ColorDodge => {
            if src >= 1.0 {
                1.0
            } else {
                (dst / (1.0 - src)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if src <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - dst) / src).min(1.0)
            }
        }
        BlendMode::HardLight => {
            if src < 0.5 {
                2.0 * src * dst
            } else {
                1.0 - 2.0 * (1.0 - src) * (1.0 - dst)
            }
        }
        BlendMode::SoftLight => {
            let d = if dst <= 0.25 {
                ((16.0 * dst - 12.0) * dst + 4.0) * dst
            } else {
                dst.sqrt()
            };
            if src <= 0.5 {
                dst - (1.0 - 2.0 * src) * dst * (1.0 - dst)
            } else {
                dst + (2.0 * src - 1.0) * (d - dst)
            }
        }
        BlendMode::Difference => (src - dst).abs(),
        BlendMode::Exclusion => src + dst - 2.0 * src * dst,
        // Reserved modes default to Normal behavior
        _ => src,
    }
}

/// Reference implementation of `blend_tile` preserved for property-based testing.
/// This is an exact copy of the current `blend_tile` implementation at the time of snapshotting.
/// Used to verify that optimized versions produce identical output.
#[cfg(test)]
pub fn reference_blend_tile(dst: &mut PixelTile, src: &PixelTile, mode: BlendMode, opacity: f32) {
    for y in HALO..(HALO + TILE_SIZE) {
        for x in HALO..(HALO + TILE_SIZE) {
            let src_a = src.at(x, y, 3) * opacity;
            if src_a < 1e-6 {
                continue; // fully transparent source pixel, skip
            }

            let dst_a = dst.at(x, y, 3);

            for c in 0..3 {
                // RGB channels
                let s = src.at(x, y, c);
                let d = dst.at(x, y, c);
                let blended = apply_blend_mode(mode, s, d);
                // Porter-Duff "over" compositing
                let out = blended * src_a + d * dst_a * (1.0 - src_a);
                dst.set(x, y, c, out);
            }
            // Alpha channel: standard "over"
            let out_a = src_a + dst_a * (1.0 - src_a);
            dst.set(x, y, 3, out_a);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{Layer, LayerGroup, LayerNode};
    use crate::mask::MaskRef;
    use crate::types::{BlendMode, LayerId, LayerKind};
    use std::sync::Arc;

    /// Helper: create a solid-color tile (main region filled, halo left zero)
    fn make_solid_tile(r: f32, g: f32, b: f32, a: f32) -> PixelTile {
        let mut tile = PixelTile::new();
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                tile.set(x, y, 0, r);
                tile.set(x, y, 1, g);
                tile.set(x, y, 2, b);
                tile.set(x, y, 3, a);
            }
        }
        tile
    }

    fn make_coord() -> TileCoord {
        TileCoord { level: 0, x: 0, y: 0 }
    }

    #[test]
    fn empty_layer_tree_returns_transparent_tile() {
        let cache = TileCache::new(100_000_000);
        let result = composite_tile(&[], 1, make_coord(), &cache).unwrap();
        // All pixels should be zero (transparent)
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                assert_eq!(result.at(x, y, 3), 0.0);
            }
        }
    }

    #[test]
    fn invisible_layer_is_skipped() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Insert a red tile in cache for layer 1
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        let red_tile = Arc::new(make_solid_tile(1.0, 0.0, 0.0, 1.0));
        cache.insert_fresh(key, red_tile);

        // Create an invisible layer
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.visible = false;

        let nodes = vec![LayerNode::Leaf(layer)];
        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // Should be transparent since layer is invisible
        assert_eq!(result.at(HALO, HALO, 3), 0.0);
    }

    #[test]
    fn single_visible_layer_composites_correctly() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Insert a red tile for layer 1
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        let red_tile = Arc::new(make_solid_tile(1.0, 0.0, 0.0, 1.0));
        cache.insert_fresh(key, red_tile);

        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        let nodes = vec![LayerNode::Leaf(layer)];
        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // Should be solid red
        assert_eq!(result.at(HALO, HALO, 0), 1.0);
        assert_eq!(result.at(HALO, HALO, 1), 0.0);
        assert_eq!(result.at(HALO, HALO, 2), 0.0);
        assert_eq!(result.at(HALO, HALO, 3), 1.0);
    }

    #[test]
    fn opacity_reduces_contribution() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        let key = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        let red_tile = Arc::new(make_solid_tile(1.0, 0.0, 0.0, 1.0));
        cache.insert_fresh(key, red_tile);

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.opacity = 0.5;

        let nodes = vec![LayerNode::Leaf(layer)];
        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // With 50% opacity over transparent: alpha = 0.5, red = 1.0 * 0.5 = 0.5
        assert!((result.at(HALO, HALO, 0) - 0.5).abs() < 1e-5);
        assert!((result.at(HALO, HALO, 3) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn two_layers_blend_normal() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Bottom layer: green, full opacity
        let key1 = TileKey {
            doc: 1,
            layer: 1,
            coord,
            stage: CacheStage::Processed,
        };
        cache.insert_fresh(key1, Arc::new(make_solid_tile(0.0, 1.0, 0.0, 1.0)));

        // Top layer: red, full opacity
        let key2 = TileKey {
            doc: 1,
            layer: 2,
            coord,
            stage: CacheStage::Processed,
        };
        cache.insert_fresh(key2, Arc::new(make_solid_tile(1.0, 0.0, 0.0, 1.0)));

        let layer1 = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        let layer2 = Layer::new(LayerId::new(2), LayerKind::Raster, 256, 256);
        let nodes = vec![LayerNode::Leaf(layer1), LayerNode::Leaf(layer2)];

        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // Top red layer fully covers green layer
        assert!((result.at(HALO, HALO, 0) - 1.0).abs() < 1e-5);
        assert!((result.at(HALO, HALO, 1) - 0.0).abs() < 1e-5);
        assert!((result.at(HALO, HALO, 3) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn blend_mode_multiply() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Bottom: white (1,1,1,1)
        let key1 = TileKey { doc: 1, layer: 1, coord, stage: CacheStage::Processed };
        cache.insert_fresh(key1, Arc::new(make_solid_tile(1.0, 1.0, 1.0, 1.0)));

        // Top: 50% gray with Multiply
        let key2 = TileKey { doc: 1, layer: 2, coord, stage: CacheStage::Processed };
        cache.insert_fresh(key2, Arc::new(make_solid_tile(0.5, 0.5, 0.5, 1.0)));

        let layer1 = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        let mut layer2 = Layer::new(LayerId::new(2), LayerKind::Raster, 256, 256);
        layer2.blend_mode = BlendMode::Multiply;

        let nodes = vec![LayerNode::Leaf(layer1), LayerNode::Leaf(layer2)];
        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // Multiply: 0.5 * 1.0 = 0.5, over dst_a=1 with src_a=1:
        // out = 0.5 * 1.0 + 1.0 * 1.0 * (1.0 - 1.0) = 0.5
        assert!((result.at(HALO, HALO, 0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn blend_mode_screen() {
        // Screen: src + dst - src*dst
        let result = apply_blend_mode(BlendMode::Screen, 0.5, 0.5);
        // 0.5 + 0.5 - 0.25 = 0.75
        assert!((result - 0.75).abs() < 1e-6);
    }

    #[test]
    fn blend_mode_overlay() {
        // dst < 0.5: 2*src*dst
        let r1 = apply_blend_mode(BlendMode::Overlay, 0.5, 0.3);
        assert!((r1 - 2.0 * 0.5 * 0.3).abs() < 1e-6);

        // dst >= 0.5: 1 - 2*(1-src)*(1-dst)
        let r2 = apply_blend_mode(BlendMode::Overlay, 0.5, 0.7);
        let expected = 1.0 - 2.0 * 0.5 * 0.3;
        assert!((r2 - expected).abs() < 1e-6);
    }

    #[test]
    fn blend_mode_darken_lighten() {
        assert_eq!(apply_blend_mode(BlendMode::Darken, 0.3, 0.7), 0.3);
        assert_eq!(apply_blend_mode(BlendMode::Lighten, 0.3, 0.7), 0.7);
    }

    #[test]
    fn blend_mode_color_dodge() {
        // src >= 1.0 → 1.0
        assert_eq!(apply_blend_mode(BlendMode::ColorDodge, 1.0, 0.5), 1.0);
        // Normal case: min(1.0, dst / (1 - src))
        let r = apply_blend_mode(BlendMode::ColorDodge, 0.5, 0.4);
        assert!((r - (0.4_f32 / 0.5).min(1.0)).abs() < 1e-6);
    }

    #[test]
    fn blend_mode_color_burn() {
        // src <= 0.0 → 0.0
        assert_eq!(apply_blend_mode(BlendMode::ColorBurn, 0.0, 0.5), 0.0);
        // Normal case: 1 - min(1.0, (1-dst)/src)
        let r = apply_blend_mode(BlendMode::ColorBurn, 0.5, 0.4);
        let expected = 1.0 - ((1.0 - 0.4_f32) / 0.5).min(1.0);
        assert!((r - expected).abs() < 1e-6);
    }

    #[test]
    fn blend_mode_hard_light() {
        // src < 0.5: 2*src*dst
        let r1 = apply_blend_mode(BlendMode::HardLight, 0.3, 0.5);
        assert!((r1 - 2.0 * 0.3 * 0.5).abs() < 1e-6);
        // src >= 0.5: 1 - 2*(1-src)*(1-dst)
        let r2 = apply_blend_mode(BlendMode::HardLight, 0.7, 0.5);
        let expected = 1.0 - 2.0 * 0.3 * 0.5;
        assert!((r2 - expected).abs() < 1e-6);
    }

    #[test]
    fn blend_mode_soft_light() {
        // src <= 0.5: dst - (1 - 2*src) * dst * (1 - dst)
        let r = apply_blend_mode(BlendMode::SoftLight, 0.3, 0.6);
        let expected = 0.6 - (1.0 - 2.0 * 0.3) * 0.6 * (1.0 - 0.6);
        assert!((r - expected).abs() < 1e-6);

        // src > 0.5, dst > 0.25: d = sqrt(dst)
        let r2 = apply_blend_mode(BlendMode::SoftLight, 0.8, 0.6);
        let d = 0.6_f32.sqrt();
        let expected2 = 0.6 + (2.0 * 0.8 - 1.0) * (d - 0.6);
        assert!((r2 - expected2).abs() < 1e-6);
    }

    #[test]
    fn blend_mode_difference_exclusion() {
        let r = apply_blend_mode(BlendMode::Difference, 0.8, 0.3);
        assert!((r - 0.5).abs() < 1e-6);

        let r2 = apply_blend_mode(BlendMode::Exclusion, 0.5, 0.5);
        // 0.5 + 0.5 - 2*0.5*0.5 = 0.5
        assert!((r2 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn invisible_group_skips_descendants() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Put a red tile in cache for layer 10
        let key = TileKey { doc: 1, layer: 10, coord, stage: CacheStage::Processed };
        cache.insert_fresh(key, Arc::new(make_solid_tile(1.0, 0.0, 0.0, 1.0)));

        // Create a group with a visible child, but group itself is invisible
        let child = Layer::new(LayerId::new(10), LayerKind::Raster, 256, 256);
        let mut group = LayerGroup::new(LayerId::new(100));
        group.visible = false;
        group.children.push(LayerNode::Leaf(child));

        let nodes = vec![LayerNode::Group(group)];
        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // Should be transparent since group is invisible
        assert_eq!(result.at(HALO, HALO, 3), 0.0);
    }

    #[test]
    fn group_isolation_blends_children_first() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Background layer (layer 1): white
        let key1 = TileKey { doc: 1, layer: 1, coord, stage: CacheStage::Processed };
        cache.insert_fresh(key1, Arc::new(make_solid_tile(1.0, 1.0, 1.0, 1.0)));

        // Group child (layer 10): red
        let key10 = TileKey { doc: 1, layer: 10, coord, stage: CacheStage::Processed };
        cache.insert_fresh(key10, Arc::new(make_solid_tile(1.0, 0.0, 0.0, 1.0)));

        // Background layer
        let bg = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);

        // Group with 50% opacity
        let child = Layer::new(LayerId::new(10), LayerKind::Raster, 256, 256);
        let mut group = LayerGroup::new(LayerId::new(100));
        group.opacity = 0.5;
        group.children.push(LayerNode::Leaf(child));

        let nodes = vec![LayerNode::Leaf(bg), LayerNode::Group(group)];
        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // Red over white at 50% opacity:
        // blended_r = 1.0 (Normal blend), out_r = 1.0 * 0.5 + 1.0 * 1.0 * 0.5 = 1.0
        // blended_g = 0.0, out_g = 0.0 * 0.5 + 1.0 * 1.0 * 0.5 = 0.5
        assert!((result.at(HALO, HALO, 0) - 1.0).abs() < 1e-5);
        assert!((result.at(HALO, HALO, 1) - 0.5).abs() < 1e-5);
        assert!((result.at(HALO, HALO, 2) - 0.5).abs() < 1e-5);
        assert!((result.at(HALO, HALO, 3) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn missing_tile_treated_as_transparent() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Layer with no tile in cache
        let layer = Layer::new(LayerId::new(99), LayerKind::Raster, 256, 256);
        let nodes = vec![LayerNode::Leaf(layer)];
        let result = composite_tile(&nodes, 1, coord, &cache).unwrap();

        // All transparent since tile not in cache
        assert_eq!(result.at(HALO, HALO, 3), 0.0);
    }


    #[test]
    fn composite_uses_requested_doc_not_doc_one() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Doc 1 = red, doc 2 = green — same layer id / coord.
        cache.insert_fresh(
            TileKey { doc: 1, layer: 1, coord, stage: CacheStage::Raw },
            Arc::new(make_solid_tile(1.0, 0.0, 0.0, 1.0)),
        );
        cache.insert_fresh(
            TileKey { doc: 2, layer: 1, coord, stage: CacheStage::Raw },
            Arc::new(make_solid_tile(0.0, 1.0, 0.0, 1.0)),
        );

        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        let nodes = vec![LayerNode::Leaf(layer)];
        let result = composite_tile(&nodes, 2, coord, &cache).unwrap();

        assert!(
            (result.at(HALO, HALO, 1) - 1.0).abs() < 1e-5,
            "doc=2 composite must read green Raw, got r={} g={}",
            result.at(HALO, HALO, 0),
            result.at(HALO, HALO, 1),
        );
        assert!((result.at(HALO, HALO, 0)).abs() < 1e-5);
    }

    // --- Tests for apply_layer_mask (Task 12.2) ---

    #[test]
    fn mask_none_returns_tile_unchanged() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();
        let tile = make_solid_tile(1.0, 0.0, 0.0, 0.8);

        let result = apply_layer_mask(&None, &tile, 1, coord, &cache);

        // Alpha should be unchanged
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                assert_eq!(result.at(x, y, 3), 0.8);
                assert_eq!(result.at(x, y, 0), 1.0);
            }
        }
    }

    #[test]
    fn mask_disabled_returns_tile_unchanged() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();
        let tile = make_solid_tile(0.0, 1.0, 0.0, 0.6);

        let mut mask_ref = MaskRef::external(LayerId::new(50));
        mask_ref.enabled = false;

        let result = apply_layer_mask(&Some(mask_ref), &tile, 1, coord, &cache);

        // Alpha should be unchanged since mask is disabled
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                assert_eq!(result.at(x, y, 3), 0.6);
                assert_eq!(result.at(x, y, 1), 1.0);
            }
        }
    }

    #[test]
    fn mask_50_percent_gray_halves_alpha() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Source tile: red, alpha = 1.0
        let tile = make_solid_tile(1.0, 0.0, 0.0, 1.0);

        // Mask tile: 50% gray → luminance = 0.2126*0.5 + 0.7152*0.5 + 0.0722*0.5 = 0.5
        let mask_tile = make_solid_tile(0.5, 0.5, 0.5, 1.0);
        let mask_key = TileKey {
            doc: 1,
            layer: 50,
            coord,
            stage: CacheStage::Processed,
        };
        cache.insert_fresh(mask_key, Arc::new(mask_tile));

        let mask_ref = MaskRef::external(LayerId::new(50));
        let result = apply_layer_mask(&Some(mask_ref), &tile, 1, coord, &cache);

        // Luminance of (0.5, 0.5, 0.5) = 0.5
        // alpha = 1.0 * 0.5 = 0.5
        let expected_lum = 0.2126 * 0.5 + 0.7152 * 0.5 + 0.0722 * 0.5;
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                assert!((result.at(x, y, 3) - expected_lum).abs() < 1e-5,
                    "Expected alpha ~{}, got {} at ({}, {})",
                    expected_lum, result.at(x, y, 3), x, y);
                // RGB unchanged
                assert_eq!(result.at(x, y, 0), 1.0);
            }
        }
    }

    #[test]
    fn mask_inverted_uses_one_minus_luminance() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Source tile: blue, alpha = 1.0
        let tile = make_solid_tile(0.0, 0.0, 1.0, 1.0);

        // Mask tile: 80% gray → luminance = 0.2126*0.8 + 0.7152*0.8 + 0.0722*0.8 = 0.8
        let mask_tile = make_solid_tile(0.8, 0.8, 0.8, 1.0);
        let mask_key = TileKey {
            doc: 1,
            layer: 60,
            coord,
            stage: CacheStage::Processed,
        };
        cache.insert_fresh(mask_key, Arc::new(mask_tile));

        let mut mask_ref = MaskRef::external(LayerId::new(60));
        mask_ref.inverted = true;

        let result = apply_layer_mask(&Some(mask_ref), &tile, 1, coord, &cache);

        // Luminance of (0.8, 0.8, 0.8) = 0.8
        // Inverted mask_value = 1.0 - 0.8 = 0.2
        // alpha = 1.0 * 0.2 = 0.2
        let lum = 0.2126 * 0.8 + 0.7152 * 0.8 + 0.0722 * 0.8;
        let expected_alpha = 1.0 - lum;
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                assert!((result.at(x, y, 3) - expected_alpha).abs() < 1e-5,
                    "Expected alpha ~{}, got {} at ({}, {})",
                    expected_alpha, result.at(x, y, 3), x, y);
                // RGB unchanged
                assert_eq!(result.at(x, y, 2), 1.0);
            }
        }
    }

    #[test]
    fn mask_with_missing_cache_tile_returns_unchanged() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Source tile with alpha 0.9
        let tile = make_solid_tile(0.5, 0.5, 0.5, 0.9);

        // Mask references layer 70, but no tile inserted in cache
        let mask_ref = MaskRef::external(LayerId::new(70));
        let result = apply_layer_mask(&Some(mask_ref), &tile, 1, coord, &cache);

        // Alpha should be unchanged since mask tile is not in cache
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                assert_eq!(result.at(x, y, 3), 0.9);
            }
        }
    }

    #[test]
    fn mask_with_colored_tile_uses_luminance_weights() {
        let cache = TileCache::new(100_000_000);
        let coord = make_coord();

        // Source tile: white, alpha = 1.0
        let tile = make_solid_tile(1.0, 1.0, 1.0, 1.0);

        // Mask tile: pure red (1,0,0) → luminance = 0.2126*1 + 0.7152*0 + 0.0722*0 = 0.2126
        let mask_tile = make_solid_tile(1.0, 0.0, 0.0, 1.0);
        let mask_key = TileKey {
            doc: 1,
            layer: 80,
            coord,
            stage: CacheStage::Processed,
        };
        cache.insert_fresh(mask_key, Arc::new(mask_tile));

        let mask_ref = MaskRef::external(LayerId::new(80));
        let result = apply_layer_mask(&Some(mask_ref), &tile, 1, coord, &cache);

        // Luminance of pure red = 0.2126
        // alpha = 1.0 * 0.2126 = 0.2126
        let expected_alpha = 0.2126;
        let px = result.at(HALO, HALO, 3);
        assert!((px - expected_alpha).abs() < 1e-5,
            "Expected alpha ~{}, got {}", expected_alpha, px);
    }
}
