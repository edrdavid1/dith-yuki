//! Independent sprite-cell pipeline (F2).
//!
//! Each cell is dithered in isolation (fresh mini-document at origin) so ED
//! does not leak across boundaries and ordered phase resets per cell.
//! Shared by preview (`compute_full_document`) and export.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use engine_color::palette::Palette;
use engine_tiles::decompose::decompose_image_to_tiles_at_generation;
use engine_tiles::TileCache;
use rayon::prelude::*;
use thiserror::Error;

use crate::document::Document;
use crate::filter::{FilterInstance, FilterParams};
use crate::layer::{Layer, LayerNode};
use crate::serialize::pixels::build_processed_composite_rgba8;
use crate::types::{DocumentId, LayerId, LayerKind};

use super::context::CellSettings;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CellPipelineError {
    #[error("cell size must be ≥ 1")]
    BadCellSize,
    #[error("image dimensions must be ≥ 1")]
    BadDimensions,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Process(String),
}

/// Lightweight counters for incremental recompute (as-built / profiling).
#[derive(Debug, Default)]
pub struct CellPipelineMetrics {
    pub cells_computed: AtomicU64,
    pub cells_cache_hit: AtomicU64,
}

pub static CELL_PIPELINE_METRICS: CellPipelineMetrics = CellPipelineMetrics {
    cells_computed: AtomicU64::new(0),
    cells_cache_hit: AtomicU64::new(0),
};

/// Runtime per-document cell result cache (content hash → rgba8).
#[derive(Debug, Default)]
pub struct CellResultCache {
    entries: Mutex<HashMap<(u32, u32), (u64, Vec<u8>)>>,
}

impl CellResultCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.entries.lock() {
            g.clear();
        }
    }
}

/// True when Gamedev independent-cell pipeline should drive pixels.
pub fn layer_needs_independent_cells(layer: &Layer, cell: &Option<CellSettings>) -> bool {
    let Some(c) = cell else {
        return false;
    };
    if !c.independent || c.w == 0 || c.h == 0 {
        return false;
    }
    layer.filters.iter().any(|f| {
        f.enabled
            && matches!(
                &f.params,
                FilterParams::DitherV2(_) | FilterParams::Dither { .. }
            )
    })
}

/// Crop target so width/height are multiples of the cell (top-left crop).
pub fn cell_grid_crop_size(
    width: u32,
    height: u32,
    cell: &CellSettings,
) -> Option<(u32, u32)> {
    if cell.w == 0 || cell.h == 0 {
        return None;
    }
    if width.is_multiple_of(cell.w) && height.is_multiple_of(cell.h) {
        return None;
    }
    if width < cell.w || height < cell.h {
        return None;
    }
    let nw = (width / cell.w) * cell.w;
    let nh = (height / cell.h) * cell.h;
    if nw == 0 || nh == 0 || (nw == width && nh == height) {
        return None;
    }
    Some((nw, nh))
}

fn hash_cell_input(rgba: &[f32], filters: &[FilterInstance], padding: u32) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    // Quantize f32 lightly for stable hashing of raw cell contents.
    for v in rgba {
        (((*v).clamp(0.0, 1.0) * 255.0).round() as u8).hash(&mut hasher);
    }
    padding.hash(&mut hasher);
    if let Ok(bytes) = serde_json::to_vec(filters) {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
}

fn extract_cell_f32(
    src: &[f32],
    width: u32,
    x0: u32,
    y0: u32,
    cw: u32,
    ch: u32,
) -> Vec<f32> {
    let mut out = Vec::with_capacity((cw as usize) * (ch as usize) * 4);
    for y in y0..y0 + ch {
        let row = (y as usize) * (width as usize) * 4;
        let start = row + (x0 as usize) * 4;
        let end = start + (cw as usize) * 4;
        out.extend_from_slice(&src[start..end]);
    }
    out
}

/// Extract cell with optional clamp-edge padding for ED context.
fn extract_cell_padded_f32(
    src: &[f32],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    cw: u32,
    ch: u32,
    padding: u32,
) -> (Vec<f32>, u32, u32, u32, u32) {
    if padding == 0 {
        return (extract_cell_f32(src, width, x0, y0, cw, ch), cw, ch, 0, 0);
    }
    let pad = padding as i64;
    let x1 = x0 as i64 - pad;
    let y1 = y0 as i64 - pad;
    let pw = cw + 2 * padding;
    let ph = ch + 2 * padding;
    let mut out = vec![0.0f32; (pw as usize) * (ph as usize) * 4];
    for py in 0..ph as i64 {
        for px in 0..pw as i64 {
            let sx = (x1 + px).clamp(0, width as i64 - 1) as u32;
            let sy = (y1 + py).clamp(0, height as i64 - 1) as u32;
            let si = ((sy * width + sx) * 4) as usize;
            let di = ((py as usize) * (pw as usize) + (px as usize)) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    (out, pw, ph, padding, padding)
}

fn crop_rgba8(src: &[u8], sw: u32, x0: u32, y0: u32, cw: u32, ch: u32) -> Vec<u8> {
    let mut out = vec![0u8; (cw as usize) * (ch as usize) * 4];
    for y in 0..ch {
        for x in 0..cw {
            let si = (((y0 + y) as usize) * (sw as usize) + ((x0 + x) as usize)) * 4;
            let di = ((y as usize) * (cw as usize) + (x as usize)) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

fn blit_rgba8(
    dest: &mut [u8],
    width: u32,
    x0: u32,
    y0: u32,
    cw: u32,
    ch: u32,
    src: &[u8],
) {
    for y in 0..ch {
        for x in 0..cw {
            let si = ((y as usize) * (cw as usize) + (x as usize)) * 4;
            let di = (((y0 + y) as usize) * (width as usize) + ((x0 + x) as usize)) * 4;
            dest[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
}

fn process_cell_rgba8(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, CellPipelineError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(CellPipelineError::Cancelled);
    }
    let doc_id = 1u32;
    let layer_id = 1u32;
    let live_gen = 1u64;
    let budget = ((width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(16))
    .clamp(4_000_000, 128_000_000);
    let cache = TileCache::new(budget);
    decompose_image_to_tiles_at_generation(
        rgba_f32, width, height, doc_id, layer_id, &cache, live_gen,
    )
    .map_err(|e| CellPipelineError::Process(format!("tile decompose: {e}")))?;

    let mut doc = Document::new(DocumentId::new(doc_id), width, height);
    doc.generations.set_document_gen(live_gen);
    doc.palettes = palettes.to_vec();
    let mut layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, width, height);
    layer.filters = filters.to_vec();
    // Cell isolation: never wrap-pad inside a cell (would reintroduce edge bleed).
    for f in &mut layer.filters {
        if let FilterParams::DitherV2(p) = &mut f.params {
            p.wrap_padding = false;
        }
    }
    doc.root.push(LayerNode::Leaf(layer));

    build_processed_composite_rgba8(&cache, &doc)
        .map_err(|e| CellPipelineError::Process(e.to_string()))
}

/// Process the full image as independent cells → RGBA8.
pub fn process_independent_cells_rgba8(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    cell: &CellSettings,
    filters: &[FilterInstance],
    palettes: &[Palette],
    cancel: &AtomicBool,
    cache: Option<&CellResultCache>,
) -> Result<Vec<u8>, CellPipelineError> {
    if cell.w == 0 || cell.h == 0 {
        return Err(CellPipelineError::BadCellSize);
    }
    if width == 0 || height == 0 {
        return Err(CellPipelineError::BadDimensions);
    }

    let cols = width.div_ceil(cell.w);
    let rows = height.div_ceil(cell.h);
    let jobs: Vec<(u32, u32)> = (0..rows)
        .flat_map(|row| (0..cols).map(move |col| (row, col)))
        .collect();

    let results: Result<Vec<_>, CellPipelineError> = jobs
        .into_par_iter()
        .map(|(row, col)| {
            if cancel.load(Ordering::Relaxed) {
                return Err(CellPipelineError::Cancelled);
            }
            let x0 = col * cell.w;
            let y0 = row * cell.h;
            let cw = cell.w.min(width - x0);
            let ch = cell.h.min(height - y0);
            let (padded, pw, ph, pad_x, pad_y) = extract_cell_padded_f32(
                rgba_f32,
                width,
                height,
                x0,
                y0,
                cw,
                ch,
                cell.padding,
            );
            let input_hash = hash_cell_input(&padded, filters, cell.padding);
            if let Some(cache) = cache {
                if let Ok(guard) = cache.entries.lock() {
                    if let Some((h, bytes)) = guard.get(&(col, row)) {
                        if *h == input_hash {
                            CELL_PIPELINE_METRICS
                                .cells_cache_hit
                                .fetch_add(1, Ordering::Relaxed);
                            return Ok((x0, y0, cw, ch, bytes.clone()));
                        }
                    }
                }
            }
            let processed = process_cell_rgba8(&padded, pw, ph, filters, palettes, cancel)?;
            let cell_rgba = if pad_x > 0 || pad_y > 0 {
                crop_rgba8(&processed, pw, pad_x, pad_y, cw, ch)
            } else {
                processed
            };
            CELL_PIPELINE_METRICS
                .cells_computed
                .fetch_add(1, Ordering::Relaxed);
            if let Some(cache) = cache {
                if let Ok(mut guard) = cache.entries.lock() {
                    guard.insert((col, row), (input_hash, cell_rgba.clone()));
                }
            }
            Ok((x0, y0, cw, ch, cell_rgba))
        })
        .collect();

    let parts = results?;
    let mut out = vec![0u8; (width as usize) * (height as usize) * 4];
    for (x0, y0, cw, ch, bytes) in parts {
        blit_rgba8(&mut out, width, x0, y0, cw, ch, &bytes);
    }
    Ok(out)
}

/// Same as [`process_independent_cells_rgba8`] but returns display-referred f32.
pub fn process_independent_cells_rgba_f32(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    cell: &CellSettings,
    filters: &[FilterInstance],
    palettes: &[Palette],
    cancel: &AtomicBool,
    cache: Option<&CellResultCache>,
) -> Result<Vec<f32>, CellPipelineError> {
    let rgba8 = process_independent_cells_rgba8(
        rgba_f32, width, height, cell, filters, palettes, cancel, cache,
    )?;
    Ok(rgba8.iter().map(|&b| b as f32 / 255.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{
        DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
    };

    fn ed_filter() -> FilterInstance {
        let mut f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 2,
                color_mode: DitherColorMode::Grayscale,
                ..DitherParamsV2::default()
            }),
        );
        f.enabled = true;
        f
    }

    fn solid(w: u32, h: u32, v: f32) -> Vec<f32> {
        let mut rgba = vec![v; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            rgba[i * 4 + 3] = 1.0;
        }
        rgba
    }

    #[test]
    fn ed_neighbor_independence() {
        let w = 8u32;
        let h = 4u32;
        let cell = CellSettings {
            w: 4,
            h: 4,
            padding: 0,
            independent: true,
        };
        // Left cell dark, right cell mid.
        let mut src = solid(w, h, 0.2);
        for y in 0..h {
            for x in 4..w {
                let i = ((y * w + x) * 4) as usize;
                src[i] = 0.6;
                src[i + 1] = 0.6;
                src[i + 2] = 0.6;
            }
        }
        let cancel = AtomicBool::new(false);
        let out1 = process_independent_cells_rgba8(
            &src,
            w,
            h,
            &cell,
            &[ed_filter()],
            &[],
            &cancel,
            None,
        )
        .unwrap();

        // Change only the left cell.
        let mut src2 = src.clone();
        for y in 0..h {
            for x in 0..4 {
                let i = ((y * w + x) * 4) as usize;
                src2[i] = 0.9;
                src2[i + 1] = 0.9;
                src2[i + 2] = 0.9;
            }
        }
        let out2 = process_independent_cells_rgba8(
            &src2,
            w,
            h,
            &cell,
            &[ed_filter()],
            &[],
            &cancel,
            None,
        )
        .unwrap();

        // Right cell bytes must be identical.
        for y in 0..h {
            for x in 4..w {
                for c in 0..4 {
                    let i = ((y * w + x) * 4 + c) as usize;
                    assert_eq!(
                        out1[i], out2[i],
                        "right cell changed at ({x},{y},c{c})"
                    );
                }
            }
        }
        // Left cell must differ somewhere.
        assert_ne!(&out1[..16], &out2[..16]);
    }

    #[test]
    fn cache_skips_unchanged_cell() {
        let w = 4u32;
        let h = 4u32;
        let cell = CellSettings {
            w: 2,
            h: 2,
            padding: 0,
            independent: true,
        };
        let src = solid(w, h, 0.5);
        let cancel = AtomicBool::new(false);
        let cache = CellResultCache::new();
        let filters = [ed_filter()];
        let _ = process_independent_cells_rgba8(
            &src,
            w,
            h,
            &cell,
            &filters,
            &[],
            &cancel,
            Some(&cache),
        )
        .unwrap();
        assert_eq!(
            cache.entries.lock().unwrap().len(),
            4,
            "four cells cached after first pass"
        );
        let hits_before = CELL_PIPELINE_METRICS.cells_cache_hit.load(Ordering::Relaxed);
        let _ = process_independent_cells_rgba8(
            &src,
            w,
            h,
            &cell,
            &filters,
            &[],
            &cancel,
            Some(&cache),
        )
        .unwrap();
        let hits_after = CELL_PIPELINE_METRICS.cells_cache_hit.load(Ordering::Relaxed);
        assert!(
            hits_after >= hits_before + 4,
            "second pass should hit cache for all cells ({hits_before} → {hits_after})"
        );
    }

    #[test]
    fn grid_crop_suggests_multiple() {
        let cell = CellSettings {
            w: 8,
            h: 8,
            padding: 0,
            independent: true,
        };
        assert_eq!(cell_grid_crop_size(33, 32, &cell), Some((32, 32)));
        assert!(cell_grid_crop_size(32, 32, &cell).is_none());
    }
}
