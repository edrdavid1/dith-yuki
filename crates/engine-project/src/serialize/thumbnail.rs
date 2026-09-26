//! System-preview `thumbnail.png` writer (SPEC_dither_previews_full §3).
//!
//! Contract is frozen: name, PNG format, and meaning must not change. Improvements
//! go through new archive entries with different names.

use crate::document::Document;
use crate::filter::{FilterInstance, FilterParams};
use crate::filters::apply::apply_filter_to_tile_with_caches;
use crate::filters::dither_residuals::ErrorResidualsStore;
use crate::layer::{Layer, LayerNode};
use crate::serialize::migrate::ProjectError;
use crate::types::{DocumentId, LayerId, LayerKind};
use engine_color::palette::Palette;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};
use image::imageops::FilterType;
use image::RgbaImage;
use std::sync::OnceLock;

/// Long-side cap for written thumbnails (no upscale).
pub const THUMBNAIL_MAX_SIDE: u32 = 1024;

/// Hard byte budget for encoded `thumbnail.png` (SPEC §3.1).
pub const THUMBNAIL_MAX_BYTES: usize = 3 * 1024 * 1024;

/// Pattern-preview sample size (SPEC §3.2).
pub const PATTERN_SAMPLE_WIDTH: u32 = 1024;
pub const PATTERN_SAMPLE_HEIGHT: u32 = 768;

/// Side lengths tried when the encoded PNG exceeds [`THUMBNAIL_MAX_BYTES`].
const SIDE_STEPS: [u32; 4] = [1024, 768, 512, 384];

/// Deterministic PNG encoder knobs (bump golden tests when changing).
const PNG_COMPRESSION: png::Compression = png::Compression::Fast;
const PNG_FILTER: png::FilterType = png::FilterType::Sub;

/// Process-wide cache: blake3(composite RGBA) → encoded thumbnail bytes.
static THUMB_CACHE: OnceLock<std::sync::Mutex<lru_thumb::ThumbCache>> = OnceLock::new();

mod lru_thumb {
    use std::collections::HashMap;

    const CAP: usize = 8;

    pub struct ThumbCache {
        map: HashMap<[u8; 32], Vec<u8>>,
        order: Vec<[u8; 32]>,
    }

    impl ThumbCache {
        pub fn new() -> Self {
            Self {
                map: HashMap::new(),
                order: Vec::new(),
            }
        }

        pub fn get(&self, key: &[u8; 32]) -> Option<Vec<u8>> {
            self.map.get(key).cloned()
        }

        pub fn insert(&mut self, key: [u8; 32], value: Vec<u8>) {
            if let std::collections::hash_map::Entry::Occupied(mut e) = self.map.entry(key) {
                e.insert(value);
                return;
            }
            if self.order.len() >= CAP {
                if let Some(old) = self.order.first().copied() {
                    self.order.remove(0);
                    self.map.remove(&old);
                }
            }
            self.order.push(key);
            self.map.insert(key, value);
        }
    }
}

fn thumb_cache() -> &'static std::sync::Mutex<lru_thumb::ThumbCache> {
    THUMB_CACHE.get_or_init(|| std::sync::Mutex::new(lru_thumb::ThumbCache::new()))
}

/// Encode RGBA8 as a critical-chunks-only PNG with fixed filter/compression.
pub fn encode_thumbnail_png_deterministic(
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ProjectError> {
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| ProjectError::Codec("thumbnail dimensions overflow".into()))?;
    if rgba.len() != expected {
        return Err(ProjectError::Codec(format!(
            "thumbnail RGBA size {} != {}×{}×4",
            rgba.len(),
            width,
            height
        )));
    }
    if width == 0 || height == 0 {
        return Err(ProjectError::Codec(
            "thumbnail requires non-zero size".into(),
        ));
    }

    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut buf, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(PNG_COMPRESSION);
        encoder.set_filter(PNG_FILTER);
        encoder.set_adaptive_filter(png::AdaptiveFilterType::NonAdaptive);
        let mut writer = encoder
            .write_header()
            .map_err(|e| ProjectError::Codec(format!("thumbnail PNG header: {e}")))?;
        writer
            .write_image_data(rgba)
            .map_err(|e| ProjectError::Codec(format!("thumbnail PNG data: {e}")))?;
    }
    Ok(buf)
}

/// 1×1 fully transparent placeholder (valid PNG, no ancillary chunks).
pub fn neutral_thumbnail_png() -> Vec<u8> {
    encode_thumbnail_png_deterministic(&[0, 0, 0, 0], 1, 1)
        .expect("1×1 transparent PNG must encode")
}

/// Resize (no upscale) with Lanczos3 in the `image` crate's gamma-ish space,
/// then encode with step-down until ≤ [`THUMBNAIL_MAX_BYTES`].
pub fn build_thumbnail_png(
    rgba: &[u8],
    width: u32,
    height: u32,
    max_side: u32,
) -> Result<Vec<u8>, ProjectError> {
    if width == 0 || height == 0 {
        return Err(ProjectError::Codec(
            "thumbnail requires non-zero size".into(),
        ));
    }
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(ProjectError::Codec(format!(
            "thumbnail RGBA size {} != {}×{}×4",
            rgba.len(),
            width,
            height
        )));
    }

    let img = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| ProjectError::Codec("failed to wrap RGBA for thumbnail".into()))?;

    let cap = max_side.clamp(1, THUMBNAIL_MAX_SIDE);
    let mut sides: Vec<u32> = SIDE_STEPS.iter().copied().filter(|&s| s <= cap).collect();
    if sides.is_empty() || sides[0] != cap {
        sides.insert(0, cap);
        sides.sort_by(|a, b| b.cmp(a));
        sides.dedup();
    }

    let mut last_err = None;
    for side in sides {
        match encode_resized(&img, width, height, side) {
            Ok(bytes) if bytes.len() <= THUMBNAIL_MAX_BYTES => return Ok(bytes),
            Ok(bytes) => {
                last_err = Some(ProjectError::Codec(format!(
                    "thumbnail {} bytes exceeds {} after side {}",
                    bytes.len(),
                    THUMBNAIL_MAX_BYTES,
                    side
                )));
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap_or_else(|| ProjectError::Codec("thumbnail encode failed".into())))
}

fn encode_resized(
    img: &RgbaImage,
    width: u32,
    height: u32,
    max_side: u32,
) -> Result<Vec<u8>, ProjectError> {
    let long = width.max(height);
    let (tw, th) = if long <= max_side {
        (width, height)
    } else {
        let scale = max_side as f32 / long as f32;
        let tw = ((width as f32) * scale).round().max(1.0) as u32;
        let th = ((height as f32) * scale).round().max(1.0) as u32;
        (tw, th)
    };

    let resized = if tw == width && th == height {
        img.clone()
    } else {
        // Decision: Lanczos3 in image crate's working space (approx. gamma).
        // Recorded in FORMAT_DECISIONS.md — keep identical on all platforms.
        image::imageops::resize(img, tw, th, FilterType::Lanczos3)
    };
    encode_thumbnail_png_deterministic(resized.as_raw(), tw, th)
}

/// Build thumbnail from composite RGBA, reusing a process cache keyed by blake3.
///
/// On any failure returns [`neutral_thumbnail_png`] so callers never fail a save.
pub fn build_thumbnail_png_cached(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    if width == 0 || height == 0 || rgba.is_empty() {
        return neutral_thumbnail_png();
    }
    let key = blake3::hash(rgba);
    let key_bytes = *key.as_bytes();
    if let Ok(guard) = thumb_cache().lock() {
        if let Some(hit) = guard.get(&key_bytes) {
            return hit;
        }
    }

    match build_thumbnail_png(rgba, width, height, THUMBNAIL_MAX_SIDE) {
        Ok(bytes) => {
            if let Ok(mut guard) = thumb_cache().lock() {
                guard.insert(key_bytes, bytes.clone());
            }
            bytes
        }
        Err(e) => {
            // No file paths — codec/size only.
            log::warn!("thumbnail generation failed ({e}); writing neutral placeholder");
            neutral_thumbnail_png()
        }
    }
}

/// Temporary pattern-preview sample (TODO(design): replace with final art).
///
/// Deterministic gradient + fine checker + low-frequency noise so dither/
/// quantize effects remain visible at thumbnail sizes.
pub fn pattern_preview_sample_rgba8() -> (u32, u32, Vec<u8>) {
    let w = PATTERN_SAMPLE_WIDTH;
    let h = PATTERN_SAMPLE_HEIGHT;
    let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / (w - 1) as f32;
            let fy = y as f32 / (h - 1) as f32;
            let checker = if ((x / 8) + (y / 8)) % 2 == 0 {
                18u8
            } else {
                0u8
            };
            // Cheap deterministic hash noise.
            let n = ((x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263))
                .wrapping_mul(1274126177)
                >> 24) as u8
                & 31;
            let r = ((fx * 220.0) as u8)
                .saturating_add(checker / 2)
                .saturating_add(n / 4);
            let g = ((fy * 200.0) as u8)
                .saturating_add(40)
                .saturating_add(checker / 3);
            let b = (((1.0 - fx) * 180.0 + fy * 60.0) as u8).saturating_add(n / 3);
            let i = ((y * w + x) * 4) as usize;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = 255;
        }
    }
    (w, h, rgba)
}

fn rgba8_to_f32(rgba: &[u8]) -> Vec<f32> {
    rgba.iter().map(|&c| c as f32 / 255.0).collect()
}

/// Apply pattern filters to the embedded sample and return RGBA8 preview pixels.
pub fn render_pattern_preview_rgba(
    filters: &[FilterInstance],
    palettes: &[Palette],
) -> Result<(u32, u32, Vec<u8>), ProjectError> {
    let (w, h, sample) = pattern_preview_sample_rgba8();
    if filters.is_empty() {
        return Ok((w, h, sample));
    }

    let f32buf = rgba8_to_f32(&sample);
    let cache = TileCache::new(64 * 1024 * 1024);
    let doc_id = DocumentId::new(1);
    let layer_id = LayerId::new(1);
    decompose_image_to_tiles(&f32buf, w, h, doc_id.0, layer_id.0, &cache)
        .map_err(|e| ProjectError::Codec(format!("pattern preview decompose: {e}")))?;

    let mut layer = Layer::new(layer_id, LayerKind::Raster, w, h);
    layer.filters = filters.to_vec();
    let mut doc = Document::new(doc_id, w, h);
    doc.palettes = palettes.to_vec();
    doc.root.push(LayerNode::Leaf(layer.clone()));

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    // Shared residuals across tiles — same as export / live preview. Isolated
    // per-tile stores draw a 256px ED seam grid into pattern thumbnails.
    let residuals_store = ErrorResidualsStore::new();
    let block_cache = BlockRepresentativeCache::new();
    if layer.dither_is_first_applied() {
        for filter in &layer.filters {
            if !filter.enabled {
                continue;
            }
            let ps = match &filter.params {
                FilterParams::DitherV2(p) => p.pixel_size,
                FilterParams::Dither { .. } => 1,
                _ => continue,
            };
            if ps > 1 {
                block_cache
                    .ensure_populated_from_tiles(&cache, doc_id.0, layer_id.0, ps as u32, w, h);
            }
        }
    }

    let cols = w.div_ceil(TILE_SIZE);
    let rows = h.div_ceil(TILE_SIZE);
    let mut out = vec![0u8; (w as usize) * (h as usize) * 4];

    for ty in 0..rows {
        for tx in 0..cols {
            let coord = TileCoord {
                level: 0,
                x: tx,
                y: ty,
            };
            let key = TileKey {
                doc: doc_id.0,
                layer: layer_id.0,
                coord,
                stage: CacheStage::Raw,
            };
            let raw = cache
                .get_entry(key)
                .ok_or_else(|| ProjectError::Codec("pattern preview missing Raw tile".into()))?;
            let processed = apply_filter_to_tile_with_caches(
                raw.as_ref(),
                &layer,
                coord,
                &palette_cache,
                &lut_cache,
                &threshold_cache,
                &doc,
                &residuals_store,
                &block_cache,
                None,
            )
            .map_err(|e| ProjectError::Codec(format!("pattern preview filter: {e}")))?;
            blit_tile_to_rgba8(&processed, tx, ty, w, h, &mut out);
        }
    }

    Ok((w, h, out))
}

fn blit_tile_to_rgba8(
    tile: &PixelTile,
    tile_x: u32,
    tile_y: u32,
    doc_w: u32,
    doc_h: u32,
    out: &mut [u8],
) {
    let origin_x = tile_x * TILE_SIZE;
    let origin_y = tile_y * TILE_SIZE;
    for ly in 0..TILE_SIZE {
        let gy = origin_y + ly;
        if gy >= doc_h {
            break;
        }
        for lx in 0..TILE_SIZE {
            let gx = origin_x + lx;
            if gx >= doc_w {
                break;
            }
            let sx = lx + HALO;
            let sy = ly + HALO;
            let r = (tile.at(sx, sy, 0).clamp(0.0, 1.0) * 255.0).round() as u8;
            let g = (tile.at(sx, sy, 1).clamp(0.0, 1.0) * 255.0).round() as u8;
            let b = (tile.at(sx, sy, 2).clamp(0.0, 1.0) * 255.0).round() as u8;
            let a = (tile.at(sx, sy, 3).clamp(0.0, 1.0) * 255.0).round() as u8;
            let i = ((gy * doc_w + gx) * 4) as usize;
            out[i] = r;
            out[i + 1] = g;
            out[i + 2] = b;
            out[i + 3] = a;
        }
    }
}

/// Prefer a real pattern preview; fall back to sample, then neutral.
pub fn build_pattern_thumbnail_png(filters: &[FilterInstance], palettes: &[Palette]) -> Vec<u8> {
    let rgba_result = render_pattern_preview_rgba(filters, palettes).or_else(|e| {
        log::warn!("pattern preview render failed ({e}); using raw sample");
        let (w, h, sample) = pattern_preview_sample_rgba8();
        Ok::<_, ProjectError>((w, h, sample))
    });
    match rgba_result {
        Ok((w, h, rgba)) => build_thumbnail_png_cached(&rgba, w, h),
        Err(_) => neutral_thumbnail_png(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherModeV2, DitherParamsV2, FilterKind, FilterParams};

    #[test]
    fn neutral_is_tiny_and_valid() {
        let png = neutral_thumbnail_png();
        assert!(png.len() < 128);
        assert_eq!(&png[1..4], b"PNG");
    }

    #[test]
    fn deterministic_same_input_same_bytes() {
        let (w, _h, rgba) = pattern_preview_sample_rgba8();
        // Use a small crop for speed.
        let tw = 32u32;
        let th = 24u32;
        let mut small = vec![0u8; (tw * th * 4) as usize];
        for y in 0..th {
            for x in 0..tw {
                let si = ((y * w + x) * 4) as usize;
                let di = ((y * tw + x) * 4) as usize;
                small[di..di + 4].copy_from_slice(&rgba[si..si + 4]);
            }
        }
        let a = build_thumbnail_png(&small, tw, th, 32).unwrap();
        let b = build_thumbnail_png(&small, tw, th, 32).unwrap();
        assert_eq!(a, b);
        assert!(a.len() <= THUMBNAIL_MAX_BYTES);
    }

    #[test]
    fn no_ancillary_chunks() {
        let rgba = vec![
            10u8, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
        ];
        let png = encode_thumbnail_png_deterministic(&rgba, 2, 2).unwrap();
        // Scan for known ancillary type codes (lowercase second letter).
        let forbidden = [
            b"tEXt", b"iTXt", b"zTXt", b"eXIf", b"iCCP", b"pHYs", b"gAMA", b"sRGB", b"cHRM",
        ];
        for tag in forbidden {
            let mut found = false;
            for w in png.windows(4) {
                if w == tag {
                    found = true;
                    break;
                }
            }
            assert!(
                !found,
                "found forbidden chunk {:?}",
                std::str::from_utf8(tag)
            );
        }
    }

    #[test]
    fn pattern_preview_runs_bayer() {
        let filters = vec![FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                ..DitherParamsV2::default()
            }),
        )];
        let (w, h, out) = render_pattern_preview_rgba(&filters, &[]).unwrap();
        assert_eq!((w, h), (PATTERN_SAMPLE_WIDTH, PATTERN_SAMPLE_HEIGHT));
        assert_eq!(out.len(), (w * h * 4) as usize);
        // Must differ from raw sample somewhere (dither changes pixels).
        let (_, _, sample) = pattern_preview_sample_rgba8();
        assert_ne!(out, sample);
    }

    #[test]
    fn cached_thumbnail_stable() {
        let rgba = vec![200u8; 16 * 16 * 4];
        let a = build_thumbnail_png_cached(&rgba, 16, 16);
        let b = build_thumbnail_png_cached(&rgba, 16, 16);
        assert_eq!(a, b);
    }
}
