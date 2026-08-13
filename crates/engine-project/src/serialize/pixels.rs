//! Assemble Raw tiles → PNG8 and decode PNG → f32 for project persistence.
//!
//! # Lossless caveat (importer assumption)
//!
//! Encoding quantizes Raw f32 channels to 8-bit RGBA PNG (`round(v * 255)` clamped).
//! Round-trip is bit-preserving **only when Raw tiles originated from 8-bit sources**
//! (today’s `load_image` path). A future 16-bit (or higher) import must either store a
//! wider container or warn — never quietly ship lossy round-trip as “lossless”.

use crate::layer::{Layer, LayerNode};
use crate::serialize::migrate::{ProjectError, SOFT_SIZE_WARN_BYTES};
use crate::types::{LayerId, LayerKind, TileBounds};
use engine_tiles::{
    CacheStage, TileCache, TileCoord, TileKey, HALO, TILE_SIZE,
};
use image::RgbaImage;
use std::io::Cursor;

/// Uncompressed RGBA estimate for one full-document raster layer.
pub fn uncompressed_layer_bytes(width: u32, height: u32) -> u64 {
    (width as u64) * (height as u64) * 4
}

/// Sum uncompressed estimates for all raster layers; `true` if ≥ soft warn threshold.
pub fn soft_size_warning(doc_width: u32, doc_height: u32, raster_layer_count: usize) -> bool {
    let total = uncompressed_layer_bytes(doc_width, doc_height)
        .saturating_mul(raster_layer_count as u64);
    total >= SOFT_SIZE_WARN_BYTES
}

/// Count raster leaves in a layer tree.
pub fn count_raster_layers(nodes: &[LayerNode]) -> usize {
    let mut n = 0;
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) if layer.kind == LayerKind::Raster => n += 1,
            LayerNode::Group(g) => n += count_raster_layers(&g.children),
            _ => {}
        }
    }
    n
}

/// Assemble level-0 Raw tiles for a raster layer into an RGBA8 document-sized buffer,
/// then encode PNG.
///
/// Missing any Raw tile covering `layer.bounds_l0` → [`ProjectError::IncompleteRaw`].
pub fn assemble_layer_png(
    cache: &TileCache,
    layer: &Layer,
    doc_width: u32,
    doc_height: u32,
) -> Result<Vec<u8>, ProjectError> {
    let rgba = assemble_layer_rgba8(cache, layer, doc_width, doc_height)?;
    encode_rgba8_png(&rgba, doc_width, doc_height)
}

/// Blit Raw tiles into a transparent document canvas (RGBA8).
pub fn assemble_layer_rgba8(
    cache: &TileCache,
    layer: &Layer,
    doc_width: u32,
    doc_height: u32,
) -> Result<Vec<u8>, ProjectError> {
    if layer.kind != LayerKind::Raster {
        return Err(ProjectError::InvalidArchive(
            "assemble_layer_rgba8 called on non-raster layer".into(),
        ));
    }

    let mut canvas = vec![0u8; (doc_width as usize) * (doc_height as usize) * 4];
    let bounds = layer.bounds_l0;
    let (off_x, off_y) = layer.offset;

    for ty in bounds.min_y..=bounds.max_y {
        for tx in bounds.min_x..=bounds.max_x {
            let key = TileKey {
                layer: layer.id.0,
                coord: TileCoord {
                    level: 0,
                    x: tx,
                    y: ty,
                },
                stage: CacheStage::Raw,
            };
            let tile = cache.get_entry(key).ok_or(ProjectError::IncompleteRaw {
                layer_id: layer.id.0,
            })?;

            for ly in 0..TILE_SIZE {
                for lx in 0..TILE_SIZE {
                    let gx = off_x + (tx * TILE_SIZE + lx) as i32;
                    let gy = off_y + (ty * TILE_SIZE + ly) as i32;
                    if gx < 0 || gy < 0 || gx >= doc_width as i32 || gy >= doc_height as i32 {
                        continue;
                    }
                    let dst = ((gy as usize) * (doc_width as usize) + (gx as usize)) * 4;
                    let sx = HALO + lx;
                    let sy = HALO + ly;
                    canvas[dst] = f32_to_u8(tile.at(sx, sy, 0));
                    canvas[dst + 1] = f32_to_u8(tile.at(sx, sy, 1));
                    canvas[dst + 2] = f32_to_u8(tile.at(sx, sy, 2));
                    canvas[dst + 3] = f32_to_u8(tile.at(sx, sy, 3));
                }
            }
        }
    }

    Ok(canvas)
}

/// Quantize linear-ish [0,1] float to u8 (see module lossless caveat).
fn f32_to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Encode RGBA8 bytes as PNG.
pub fn encode_rgba8_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, ProjectError> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(ProjectError::Codec(format!(
            "RGBA buffer size {} != {}×{}×4",
            rgba.len(),
            width,
            height
        )));
    }
    let img = RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        ProjectError::Codec("failed to wrap RGBA buffer as image".into())
    })?;
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| ProjectError::Codec(e.to_string()))?;
    Ok(buf.into_inner())
}

/// Decode PNG → f32 RGBA (same path as `load_image`).
pub fn decode_png_to_f32(png_bytes: &[u8]) -> Result<(u32, u32, Vec<f32>), ProjectError> {
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| ProjectError::Codec(e.to_string()))?
        .to_rgba8();
    let width = img.width();
    let height = img.height();
    let mut rgba_f32 = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for pixel in img.pixels() {
        rgba_f32.push(pixel[0] as f32 / 255.0);
        rgba_f32.push(pixel[1] as f32 / 255.0);
        rgba_f32.push(pixel[2] as f32 / 255.0);
        rgba_f32.push(pixel[3] as f32 / 255.0);
    }
    Ok((width, height, rgba_f32))
}

/// Walk tree and collect raster layers (for save).
pub fn collect_raster_layers(nodes: &[LayerNode]) -> Vec<&Layer> {
    let mut out = Vec::new();
    fn walk<'a>(nodes: &'a [LayerNode], out: &mut Vec<&'a Layer>) {
        for node in nodes {
            match node {
                LayerNode::Leaf(layer) if layer.kind == LayerKind::Raster => out.push(layer),
                LayerNode::Group(g) => walk(&g.children, out),
                _ => {}
            }
        }
    }
    walk(nodes, &mut out);
    out
}

/// Helper used by tests: drop a Raw tile by key without LRU eviction.
pub fn force_drop_raw_tile(cache: &TileCache, layer_id: LayerId, x: u32, y: u32) {
    let key = TileKey {
        layer: layer_id.0,
        coord: TileCoord { level: 0, x, y },
        stage: CacheStage::Raw,
    };
    cache.entries.remove(&key);
}

/// Tiles required for a bounds box (inclusive).
pub fn tile_keys_for_bounds(layer_id: u32, bounds: TileBounds) -> Vec<TileKey> {
    let mut keys = Vec::new();
    for ty in bounds.min_y..=bounds.max_y {
        for tx in bounds.min_x..=bounds.max_x {
            keys.push(TileKey {
                layer: layer_id,
                coord: TileCoord {
                    level: 0,
                    x: tx,
                    y: ty,
                },
                stage: CacheStage::Raw,
            });
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LayerKind;
    use engine_tiles::decompose::decompose_image_to_tiles;

    fn solid_f32(w: u32, h: u32, rgba: [f32; 4]) -> Vec<f32> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            v.extend_from_slice(&rgba);
        }
        v
    }

    #[test]
    fn assemble_round_trip_png8() {
        let w = 64u32;
        let h = 48u32;
        let buf = solid_f32(w, h, [1.0, 0.0, 0.0, 1.0]);
        let cache = TileCache::new(50_000_000);
        decompose_image_to_tiles(&buf, w, h, 1, &cache).unwrap();

        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        let png = assemble_layer_png(&cache, &layer, w, h).unwrap();
        let (dw, dh, f32buf) = decode_png_to_f32(&png).unwrap();
        assert_eq!((dw, dh), (w, h));
        // Red channel should round-trip exactly from 8-bit source path
        assert!((f32buf[0] - 1.0).abs() < 1e-6);
        assert!((f32buf[1]).abs() < 1e-6);
        assert!((f32buf[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn incomplete_raw_after_force_drop() {
        let w = 300u32;
        let h = 300u32;
        let buf = solid_f32(w, h, [0.5, 0.5, 0.5, 1.0]);
        let cache = TileCache::new(50_000_000);
        decompose_image_to_tiles(&buf, w, h, 1, &cache).unwrap();

        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        // 300×300 → 2×2 tiles; drop one
        force_drop_raw_tile(&cache, LayerId::new(1), 1, 1);

        let err = assemble_layer_png(&cache, &layer, w, h).unwrap_err();
        assert_eq!(err, ProjectError::IncompleteRaw { layer_id: 1 });
    }

    #[test]
    fn soft_size_helper() {
        // One 8192² layer ≈ 256 MiB
        assert!(soft_size_warning(8192, 8192, 1));
        assert!(!soft_size_warning(64, 64, 1));
    }
}
