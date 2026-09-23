//! Full-document filter execution (monolithic pass).
//!
//! Used by algorithms with [`ExecutionScope::FullDocument`] (currently
//! Riemersma). Result is one document-sized RGBA f32 block, later sliced into
//! Processed tiles for display.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use engine_registry::ExecutionScope;
use engine_tiles::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};

use crate::algorithms::builtin_registry;
use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{
    filter_params_to_json, resolve_algorithm_id, DitherParamsV2, FilterInstance, FilterParams,
};
use crate::filters::riemersma::{apply_riemersma_rgba, RiemersmaError};
use crate::layer::Layer;
use crate::serialize::pixels::assemble_layer_rgba8;

/// Cache key for a full-document filter result (§0.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FullDocumentKey {
    pub doc: u32,
    pub layer: u32,
    pub params_hash: u64,
    pub document_gen: u64,
}

/// One ready full-document RGBA f32 result.
#[derive(Clone)]
pub struct FullDocumentResult {
    pub rgba_f32: Arc<Vec<f32>>,
    pub width: u32,
    pub height: u32,
}

struct CacheSlot {
    /// Generation token; bump to cancel an in-flight job for this layer.
    job_token: u64,
    cancel: Arc<AtomicBool>,
    last_key: Option<FullDocumentKey>,
    result: Option<FullDocumentResult>,
}

/// Process-wide-ish cache of full-document filter results (one per AppState).
pub struct FullDocumentCache {
    inner: Mutex<HashMap<(u32, u32), CacheSlot>>,
    next_token: AtomicU64,
}

impl FullDocumentCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            next_token: AtomicU64::new(1),
        }
    }

    pub fn clear(&self) {
        let mut guard = self.inner.lock().expect("full_document cache lock");
        for slot in guard.values_mut() {
            slot.cancel.store(true, Ordering::Relaxed);
            slot.result = None;
        }
        guard.clear();
    }

    pub fn invalidate_layer(&self, doc: u32, layer: u32) {
        let mut guard = self.inner.lock().expect("full_document cache lock");
        if let Some(slot) = guard.get_mut(&(doc, layer)) {
            slot.cancel.store(true, Ordering::Relaxed);
            slot.result = None;
            slot.last_key = None;
            slot.job_token = self.next_token.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Return a cached result if it matches `key`.
    pub fn get_if_fresh(&self, key: &FullDocumentKey) -> Option<FullDocumentResult> {
        let guard = self.inner.lock().expect("full_document cache lock");
        let slot = guard.get(&(key.doc, key.layer))?;
        if slot.last_key.as_ref() == Some(key) {
            slot.result.clone()
        } else {
            None
        }
    }
}

impl Default for FullDocumentCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash enabled filter params for cache keying.
pub fn hash_layer_filter_params(layer: &Layer) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for filter in layer.filters.iter().filter(|f| f.enabled) {
        resolve_algorithm_id(filter).hash(&mut hasher);
        let json = filter_params_to_json(&filter.params).unwrap_or(serde_json::Value::Null);
        json.to_string().hash(&mut hasher);
        filter.opacity.to_bits().hash(&mut hasher);
        format!("{:?}", filter.blend_mode).hash(&mut hasher);
    }
    hasher.finish()
}

/// Whether any enabled filter on `layer` needs a full-document pass.
pub fn layer_has_full_document_filter(layer: &Layer) -> bool {
    let reg = builtin_registry();
    layer.filters.iter().any(|f| {
        if !f.enabled {
            return false;
        }
        resolve_algorithm_id(f)
            .and_then(|id| reg.get_by_str(&id).map(|a| a.execution_scope()))
            .map(|s| s == ExecutionScope::FullDocument)
            .unwrap_or(false)
    })
}

/// Assemble Raw tiles → f32 RGBA (linear 0..1 from 8-bit assemble path).
pub fn assemble_layer_rgba_f32(
    cache: &TileCache,
    layer: &Layer,
    doc_width: u32,
    doc_height: u32,
    doc_id: u32,
) -> Result<Vec<f32>, EngineError> {
    let rgba8 = assemble_layer_rgba8(cache, layer, doc_width, doc_height, doc_id).map_err(|e| {
        EngineError::invalid_state(format!("assemble raw for full-document: {e}"))
    })?;
    Ok(rgba8
        .into_iter()
        .map(|b| b as f32 / 255.0)
        .collect())
}

/// Run the full-document filter stack for `layer`, returning RGBA f32.
///
/// Currently supports stacks whose FullDocument member is Riemersma. Other
/// enabled filters in the same stack are applied via a tile-roundtrip before
/// / after the monolithic pass when present — v1 requires Riemersma to be the
/// only enabled FullDocument filter; preceding/following tiled filters are
/// applied by decomposing the buffer into temporary tiles.
pub fn compute_full_document_rgba(
    cache: &TileCache,
    layer: &Layer,
    doc: &Document,
    should_cancel: &AtomicBool,
) -> Result<FullDocumentResult, EngineError> {
    let mut rgba = assemble_layer_rgba_f32(cache, layer, doc.width, doc.height, doc.id.0)?;
    let w = doc.width;
    let h = doc.height;

    // Bottom-up: last vec entry first (same as tiled apply).
    let enabled: Vec<&FilterInstance> = layer.filters.iter().rev().filter(|f| f.enabled).collect();
    let reg = builtin_registry();

    for filter in enabled {
        if should_cancel.load(Ordering::Relaxed) {
            return Err(EngineError::invalid_state("full-document job cancelled"));
        }
        let Some(id) = resolve_algorithm_id(filter) else {
            continue;
        };
        let Some(algo) = reg.get_by_str(&id) else {
            return Err(EngineError::unknown_algorithm(id));
        };

        match algo.execution_scope() {
            ExecutionScope::FullDocument => {
                apply_full_document_algorithm(&mut rgba, w, h, filter, should_cancel)?;
            }
            ExecutionScope::Tiled => {
                apply_tiled_filters_on_buffer(&mut rgba, w, h, layer, filter, doc, cache)?;
            }
        }
    }

    Ok(FullDocumentResult {
        rgba_f32: Arc::new(rgba),
        width: w,
        height: h,
    })
}

fn apply_full_document_algorithm(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    filter: &FilterInstance,
    should_cancel: &AtomicBool,
) -> Result<(), EngineError> {
    let params = match &filter.params {
        FilterParams::DitherV2(p) => p.clone(),
        other => {
            // Deserialise via JSON so AlgorithmId-only instances still work.
            let json = filter_params_to_json(other)?;
            serde_json::from_value::<DitherParamsV2>(json).map_err(|e| {
                EngineError::invalid_filter_params(format!("riemersma params: {e}"))
            })?
        }
    };
    apply_riemersma_rgba(rgba, width, height, &params, should_cancel).map_err(|e| match e {
        RiemersmaError::Cancelled => EngineError::invalid_state("riemersma cancelled"),
        RiemersmaError::InvalidBuffer => {
            EngineError::invalid_filter_params("riemersma invalid buffer")
        }
    })
}

/// Apply a single tiled filter by walking document tiles (no cross-tile ED
/// residuals — acceptable for non-ED filters that may sit beside Riemersma).
fn apply_tiled_filters_on_buffer(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    layer: &Layer,
    filter: &FilterInstance,
    doc: &Document,
    _raw_cache: &TileCache,
) -> Result<(), EngineError> {
    use crate::filters::apply::apply_filter_to_tile;
    use engine_color::palette_cache::PaletteKdCache;
    use engine_color::palette_lut::PaletteLutCache;
    use engine_color::threshold_map::ThresholdMapCache;

    // Build a one-filter layer view.
    let mut layer_one = layer.clone();
    layer_one.filters = vec![filter.clone()];

    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();

    let cols = (width + TILE_SIZE - 1) / TILE_SIZE;
    let rows = (height + TILE_SIZE - 1) / TILE_SIZE;
    for ty in 0..rows {
        for tx in 0..cols {
            let tile = extract_tile_from_rgba(rgba, width, height, tx, ty);
            let coord = TileCoord {
                level: 0,
                x: tx,
                y: ty,
            };
            let processed = apply_filter_to_tile(
                &tile,
                &layer_one,
                coord,
                &palette_cache,
                &lut_cache,
                &threshold_cache,
                doc,
            )?;
            blit_tile_into_rgba(rgba, width, height, tx, ty, &processed);
        }
    }
    Ok(())
}

/// Ensure a fresh full-document result for `layer`, computing if needed.
///
/// Cancels any in-flight job for the same layer when params/gen change.
pub fn ensure_full_document(
    fd_cache: &FullDocumentCache,
    tile_cache: &TileCache,
    layer: &Layer,
    doc: &Document,
    document_gen: u64,
) -> Result<FullDocumentResult, EngineError> {
    let key = FullDocumentKey {
        doc: doc.id.0,
        layer: layer.id.0,
        params_hash: hash_layer_filter_params(layer),
        document_gen,
    };

    if let Some(hit) = fd_cache.get_if_fresh(&key) {
        return Ok(hit);
    }

    let token = fd_cache.next_token.fetch_add(1, Ordering::Relaxed);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = fd_cache.inner.lock().expect("full_document cache lock");
        if let Some(prev) = guard.get_mut(&(key.doc, key.layer)) {
            prev.cancel.store(true, Ordering::Relaxed);
            prev.cancel = Arc::clone(&cancel);
            prev.job_token = token;
            prev.last_key = None;
            prev.result = None;
        } else {
            guard.insert(
                (key.doc, key.layer),
                CacheSlot {
                    job_token: token,
                    cancel: Arc::clone(&cancel),
                    last_key: None,
                    result: None,
                },
            );
        }
    }

    let result = compute_full_document_rgba(tile_cache, layer, doc, &cancel)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(EngineError::invalid_state("full-document job cancelled"));
    }

    let mut guard = fd_cache.inner.lock().expect("full_document cache lock");
    if let Some(slot) = guard.get_mut(&(key.doc, key.layer)) {
        if slot.job_token == token {
            slot.last_key = Some(key);
            slot.result = Some(result.clone());
            return Ok(result);
        }
    }
    Err(EngineError::invalid_state(
        "full-document result superseded",
    ))
}

/// Slice one Processed-sized `PixelTile` out of a full-document result.
pub fn slice_processed_tile(
    result: &FullDocumentResult,
    coord: TileCoord,
) -> PixelTile {
    extract_tile_from_rgba(
        &result.rgba_f32,
        result.width,
        result.height,
        coord.x,
        coord.y,
    )
}

/// Insert all level-0 Processed tiles for a full-document result into the cache.
pub fn publish_processed_tiles(
    tile_cache: &TileCache,
    result: &FullDocumentResult,
    doc_id: u32,
    layer_id: u32,
    generation: u64,
) {
    let cols = (result.width + TILE_SIZE - 1) / TILE_SIZE;
    let rows = (result.height + TILE_SIZE - 1) / TILE_SIZE;
    for ty in 0..rows {
        for tx in 0..cols {
            let tile = extract_tile_from_rgba(&result.rgba_f32, result.width, result.height, tx, ty);
            let key = TileKey {
                doc: doc_id,
                layer: layer_id,
                coord: TileCoord {
                    level: 0,
                    x: tx,
                    y: ty,
                },
                stage: CacheStage::Processed,
            };
            let _ = tile_cache.insert_fresh_gen(key, Arc::new(tile), generation);
        }
    }
}

fn extract_tile_from_rgba(
    buffer: &[f32],
    img_width: u32,
    img_height: u32,
    tile_col: u32,
    tile_row: u32,
) -> PixelTile {
    let mut tile = PixelTile::new();
    let origin_x = (tile_col * TILE_SIZE) as i64;
    let origin_y = (tile_row * TILE_SIZE) as i64;
    let stride = (TILE_SIZE + 2 * HALO) as i64;

    for ty in 0..stride {
        for tx in 0..stride {
            let ix = origin_x + tx - HALO as i64;
            let iy = origin_y + ty - HALO as i64;
            if ix >= 0 && iy >= 0 && ix < img_width as i64 && iy < img_height as i64 {
                let src = ((iy as usize) * (img_width as usize) + (ix as usize)) * 4;
                let dx = tx as u32;
                let dy = ty as u32;
                tile.set(dx, dy, 0, buffer[src]);
                tile.set(dx, dy, 1, buffer[src + 1]);
                tile.set(dx, dy, 2, buffer[src + 2]);
                tile.set(dx, dy, 3, buffer[src + 3]);
            }
        }
    }
    tile
}

fn blit_tile_into_rgba(
    buffer: &mut [f32],
    img_width: u32,
    img_height: u32,
    tile_col: u32,
    tile_row: u32,
    tile: &PixelTile,
) {
    let origin_x = (tile_col * TILE_SIZE) as i32;
    let origin_y = (tile_row * TILE_SIZE) as i32;
    for ly in 0..TILE_SIZE {
        for lx in 0..TILE_SIZE {
            let gx = origin_x + lx as i32;
            let gy = origin_y + ly as i32;
            if gx < 0 || gy < 0 || gx >= img_width as i32 || gy >= img_height as i32 {
                continue;
            }
            let dst = ((gy as usize) * (img_width as usize) + (gx as usize)) * 4;
            let sx = HALO + lx;
            let sy = HALO + ly;
            buffer[dst] = tile.at(sx, sy, 0);
            buffer[dst + 1] = tile.at(sx, sy, 1);
            buffer[dst + 2] = tile.at(sx, sy, 2);
            buffer[dst + 3] = tile.at(sx, sy, 3);
        }
    }
}
