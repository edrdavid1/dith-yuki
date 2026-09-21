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
use engine_tiles::{CacheStage, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};
use image::RgbaImage;
use std::io::Cursor;

/// Uncompressed RGBA estimate for one full-document raster layer.
pub fn uncompressed_layer_bytes(width: u32, height: u32) -> u64 {
    (width as u64) * (height as u64) * 4
}

/// Sum uncompressed estimates for all raster layers; `true` if ≥ soft warn threshold.
pub fn soft_size_warning(doc_width: u32, doc_height: u32, raster_layer_count: usize) -> bool {
    let total =
        uncompressed_layer_bytes(doc_width, doc_height).saturating_mul(raster_layer_count as u64);
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
/// `doc_id` is the runtime [`crate::types::DocumentId`] — Raw keys are namespaced by doc.
/// Missing any Raw tile covering `layer.bounds_l0` → [`ProjectError::IncompleteRaw`].
pub fn assemble_layer_png(
    cache: &TileCache,
    layer: &Layer,
    doc_width: u32,
    doc_height: u32,
    doc_id: u32,
) -> Result<Vec<u8>, ProjectError> {
    let rgba = assemble_layer_rgba8(cache, layer, doc_width, doc_height, doc_id)?;
    encode_rgba8_png(&rgba, doc_width, doc_height)
}

/// Blit Raw tiles into a transparent document canvas (RGBA8).
pub fn assemble_layer_rgba8(
    cache: &TileCache,
    layer: &Layer,
    doc_width: u32,
    doc_height: u32,
    doc_id: u32,
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
                doc: doc_id,
                layer: layer.id.0,
                coord: TileCoord {
                    level: 0,
                    x: tx,
                    y: ty,
                },
                stage: CacheStage::Raw,
            };
            let tile = cache.get_entry(key).ok_or(ProjectError::IncompleteRaw {
                doc_id,
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

/// Build a document-sized flat composite from Raw raster layers (insurance PNG).
///
/// Walks leaves in tree order (bottom → top). Visible rasters are alpha-composited
/// with Porter-Duff **over** using each layer's opacity. Filter stacks and non-Normal
/// blend modes are intentionally skipped — composite is a compatibility flat, not a
/// full export render (see `docs/FORMAT_DECISIONS.md` Stage 4).
pub fn build_composite_rgba8(
    cache: &TileCache,
    nodes: &[LayerNode],
    doc_width: u32,
    doc_height: u32,
    doc_id: u32,
) -> Result<Vec<u8>, ProjectError> {
    let mut canvas = vec![0u8; (doc_width as usize) * (doc_height as usize) * 4];
    paint_composite_nodes(nodes, cache, doc_width, doc_height, doc_id, &mut canvas)?;
    Ok(canvas)
}

fn paint_composite_nodes(
    nodes: &[LayerNode],
    cache: &TileCache,
    doc_width: u32,
    doc_height: u32,
    doc_id: u32,
    canvas: &mut [u8],
) -> Result<(), ProjectError> {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if !layer.visible || layer.kind != LayerKind::Raster {
                    continue;
                }
                let src = assemble_layer_rgba8(cache, layer, doc_width, doc_height, doc_id)?;
                blend_over_rgba(canvas, &src, layer.opacity);
            }
            LayerNode::Group(g) => {
                if !g.visible {
                    continue;
                }
                // Group isolation / group opacity deferred; paint children flat.
                paint_composite_nodes(&g.children, cache, doc_width, doc_height, doc_id, canvas)?;
            }
        }
    }
    Ok(())
}

fn blend_over_rgba(dst: &mut [u8], src: &[u8], opacity: f32) {
    let opacity = opacity.clamp(0.0, 1.0);
    let n = dst.len().min(src.len()) / 4;
    for i in 0..n {
        let o = i * 4;
        let sa = (src[o + 3] as f32 / 255.0) * opacity;
        if sa <= 0.0 {
            continue;
        }
        let da = dst[o + 3] as f32 / 255.0;
        let out_a = sa + da * (1.0 - sa);
        if out_a <= 0.0 {
            dst[o] = 0;
            dst[o + 1] = 0;
            dst[o + 2] = 0;
            dst[o + 3] = 0;
            continue;
        }
        for c in 0..3 {
            let s = src[o + c] as f32 / 255.0;
            let d = dst[o + c] as f32 / 255.0;
            let out = (s * sa + d * da * (1.0 - sa)) / out_a;
            dst[o + c] = (out.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
        dst[o + 3] = (out_a.clamp(0.0, 1.0) * 255.0).round() as u8;
    }
}

/// Encode [`build_composite_rgba8`] as PNG.
pub fn build_composite_png(
    cache: &TileCache,
    nodes: &[LayerNode],
    doc_width: u32,
    doc_height: u32,
    doc_id: u32,
) -> Result<Vec<u8>, ProjectError> {
    let rgba = build_composite_rgba8(cache, nodes, doc_width, doc_height, doc_id)?;
    encode_rgba8_png(&rgba, doc_width, doc_height)
}

// Thumbnail writer lives in `serialize::thumbnail` (preview SPEC §3).
pub use crate::serialize::thumbnail::build_thumbnail_png;

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
    let img = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| ProjectError::Codec("failed to wrap RGBA buffer as image".into()))?;
    let mut buf = Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| ProjectError::Codec(e.to_string()))?;
    Ok(buf.into_inner())
}

/// Decode PNG → re-encode as clean RGBA8 PNG (drops EXIF/XMP/iCCP/text chunks).
pub fn reencode_png_clean(png_bytes: &[u8]) -> Result<Vec<u8>, ProjectError> {
    let img = image::load_from_memory(png_bytes)
        .map_err(|e| ProjectError::Codec(format!("PNG re-encode load: {e}")))?
        .to_rgba8();
    let (w, h) = img.dimensions();
    encode_rgba8_png(img.as_raw(), w, h)
}

/// Decode PNG → f32 RGBA with resource limits (SPEC §7.6).
///
/// Reads IHDR through `png::Decoder` **before** allocating the float buffer;
/// rejects oversized dimensions and caps decoder allocations via `png::Limits`.
pub fn decode_png_to_f32(png_bytes: &[u8]) -> Result<(u32, u32, Vec<f32>), ProjectError> {
    decode_png_to_f32_with_limits(png_bytes, PngDecodeLimits::default())
}

/// Limits for archive PNG decode (document layers / composites).
#[derive(Debug, Clone, Copy)]
pub struct PngDecodeLimits {
    pub max_width: u32,
    pub max_height: u32,
    /// `width × height` cap (SPEC §7.3: 16384²).
    pub max_pixels: u64,
    /// Budget for `png::Limits::bytes` (decoded intermediate allocations).
    pub max_decoder_bytes: usize,
}

impl Default for PngDecodeLimits {
    fn default() -> Self {
        Self {
            max_width: 65_535,
            max_height: 65_535,
            max_pixels: 268_435_456,
            max_decoder_bytes: 1024 * 1024 * 1024, // 1 GiB matches dyproj PNG entry cap
        }
    }
}

/// Stricter limits for threshold-map PNGs (SPEC §7.4).
pub fn threshold_map_png_limits() -> PngDecodeLimits {
    PngDecodeLimits {
        max_width: 4096,
        max_height: 4096,
        max_pixels: 4096 * 4096,
        max_decoder_bytes: 64 * 1024 * 1024,
    }
}

pub fn decode_png_to_f32_with_limits(
    png_bytes: &[u8],
    limits: PngDecodeLimits,
) -> Result<(u32, u32, Vec<f32>), ProjectError> {
    let (width, height) = peek_png_dimensions(png_bytes, limits)?;
    let pixels = (width as u64)
        .checked_mul(height as u64)
        .ok_or_else(|| ProjectError::Corrupt(format!("PNG dimensions overflow {width}×{height}")))?;
    if pixels == 0 || pixels > limits.max_pixels {
        return Err(ProjectError::Corrupt(format!(
            "PNG pixel count {pixels} outside 1..={}",
            limits.max_pixels
        )));
    }
    if width > limits.max_width || height > limits.max_height {
        return Err(ProjectError::Corrupt(format!(
            "PNG size {width}×{height} exceeds {}×{}",
            limits.max_width, limits.max_height
        )));
    }

    // Decode with the same budget so compressed bombs cannot expand past the cap.
    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_limits(png::Limits {
        bytes: limits.max_decoder_bytes,
    });
    // Expand to 8-bit and add alpha so output is predictable.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder
        .read_info()
        .map_err(|e| ProjectError::Corrupt(format!("PNG header: {e}")))?;
    let info = reader.info();
    if info.width != width || info.height != height {
        return Err(ProjectError::Corrupt("PNG IHDR mismatch on re-read".into()));
    }

    let output_size = reader.output_buffer_size();
    let max_rgba = pixels
        .checked_mul(4)
        .ok_or_else(|| ProjectError::Corrupt("PNG RGBA size overflow".into()))?;
    if (output_size as u64) > max_rgba.saturating_mul(2).max(64) {
        return Err(ProjectError::Corrupt(format!(
            "PNG output buffer {output_size} implausible for {width}×{height}"
        )));
    }
    if output_size > limits.max_decoder_bytes {
        return Err(ProjectError::Corrupt(format!(
            "PNG output buffer {output_size} exceeds decoder budget"
        )));
    }

    let mut buf = vec![0u8; output_size];
    let frame = reader
        .next_frame(&mut buf)
        .map_err(|e| ProjectError::Corrupt(format!("PNG decode: {e}")))?;
    let used = &buf[..frame.buffer_size()];

    let mut rgba_f32 = Vec::new();
    rgba_f32
        .try_reserve_exact((pixels as usize).saturating_mul(4))
        .map_err(|_| ProjectError::Corrupt("PNG float buffer alloc failed".into()))?;

    match frame.color_type {
        png::ColorType::Rgba => {
            if used.len() < (pixels as usize) * 4 {
                return Err(ProjectError::Corrupt("PNG RGBA truncated".into()));
            }
            for px in used.chunks_exact(4).take(pixels as usize) {
                rgba_f32.push(px[0] as f32 / 255.0);
                rgba_f32.push(px[1] as f32 / 255.0);
                rgba_f32.push(px[2] as f32 / 255.0);
                rgba_f32.push(px[3] as f32 / 255.0);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            if used.len() < (pixels as usize) * 2 {
                return Err(ProjectError::Corrupt("PNG GrayAlpha truncated".into()));
            }
            for px in used.chunks_exact(2).take(pixels as usize) {
                let g = px[0] as f32 / 255.0;
                rgba_f32.push(g);
                rgba_f32.push(g);
                rgba_f32.push(g);
                rgba_f32.push(px[1] as f32 / 255.0);
            }
        }
        other => {
            return Err(ProjectError::Corrupt(format!(
                "unsupported PNG color type after expand: {other:?}"
            )));
        }
    }

    Ok((width, height, rgba_f32))
}

fn peek_png_dimensions(
    png_bytes: &[u8],
    limits: PngDecodeLimits,
) -> Result<(u32, u32), ProjectError> {
    let mut decoder = png::Decoder::new(Cursor::new(png_bytes));
    decoder.set_limits(png::Limits {
        bytes: limits.max_decoder_bytes,
    });
    let reader = decoder
        .read_info()
        .map_err(|e| ProjectError::Corrupt(format!("PNG header: {e}")))?;
    let info = reader.info();
    Ok((info.width, info.height))
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
pub fn force_drop_raw_tile(cache: &TileCache, doc_id: u32, layer_id: LayerId, x: u32, y: u32) {
    let key = TileKey {
        doc: doc_id,
        layer: layer_id.0,
        coord: TileCoord { level: 0, x, y },
        stage: CacheStage::Raw,
    };
    cache.entries.remove(&key);
}

/// Tiles required for a bounds box (inclusive).
pub fn tile_keys_for_bounds(doc_id: u32, layer_id: u32, bounds: TileBounds) -> Vec<TileKey> {
    let mut keys = Vec::new();
    for ty in bounds.min_y..=bounds.max_y {
        for tx in bounds.min_x..=bounds.max_x {
            keys.push(TileKey {
                doc: doc_id,
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
        decompose_image_to_tiles(&buf, w, h, 1, 1, &cache).unwrap();

        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        let png = assemble_layer_png(&cache, &layer, w, h, 1).unwrap();
        let (dw, dh, f32buf) = decode_png_to_f32(&png).unwrap();
        assert_eq!((dw, dh), (w, h));
        assert!((f32buf[0] - 1.0).abs() < 1e-6);
        assert!((f32buf[1]).abs() < 1e-6);
        assert!((f32buf[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn assemble_requires_matching_doc_id() {
        let w = 64u32;
        let h = 64u32;
        let buf = solid_f32(w, h, [0.0, 1.0, 0.0, 1.0]);
        let cache = TileCache::new(50_000_000);
        // Tiles live under doc=2 only.
        decompose_image_to_tiles(&buf, w, h, 2, 1, &cache).unwrap();
        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);

        let err = assemble_layer_png(&cache, &layer, w, h, 1).unwrap_err();
        assert_eq!(
            err,
            ProjectError::IncompleteRaw {
                doc_id: 1,
                layer_id: 1
            }
        );
        assert!(assemble_layer_png(&cache, &layer, w, h, 2).is_ok());
    }

    #[test]
    fn incomplete_raw_after_force_drop() {
        let w = 300u32;
        let h = 300u32;
        let buf = solid_f32(w, h, [0.5, 0.5, 0.5, 1.0]);
        let cache = TileCache::new(50_000_000);
        decompose_image_to_tiles(&buf, w, h, 1, 1, &cache).unwrap();

        let layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        force_drop_raw_tile(&cache, 1, LayerId::new(1), 1, 1);

        let err = assemble_layer_png(&cache, &layer, w, h, 1).unwrap_err();
        assert_eq!(
            err,
            ProjectError::IncompleteRaw {
                doc_id: 1,
                layer_id: 1
            }
        );
    }

    #[test]
    fn soft_size_helper() {
        assert!(soft_size_warning(8192, 8192, 1));
        assert!(!soft_size_warning(64, 64, 1));
    }
}
