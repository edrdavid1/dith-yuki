//! Full-document filter execution (monolithic pass).
//!
//! Used by algorithms with [`ExecutionScope::FullDocument`] (currently
//! Riemersma and ASCII). Result is one document-sized RGBA f32 block, later
//! sliced into Processed tiles for display.
//!
//! ## Single-flight (§0.4 / ASCII instability fix)
//!
//! At most one in-flight job per `(doc, layer)` for a given
//! [`FullDocumentKey`]. Concurrent callers that want the **same** key wait for
//! that job instead of cancelling it. Cancellation happens only when a request
//! arrives with a **different** key (params / `document_gen` / preview mode),
//! or on explicit [`FullDocumentCache::invalidate_layer`] / [`clear`].

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use engine_registry::ExecutionScope;
use engine_tiles::{CacheStage, PixelTile, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};

use crate::algorithms::builtin_registry;
use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{
    filter_params_to_json, resolve_algorithm_id, AsciiParams, DitherParamsV2, FilterInstance,
    FilterParams,
};
use crate::filters::ascii_job::{apply_ascii_rgba, run_ascii_job};
use crate::filters::riemersma::{apply_riemersma_rgba, RiemersmaError};
use crate::layer::Layer;
use crate::serialize::pixels::assemble_layer_rgba8;

/// Test-only: sleep at the start of [`compute_full_document_rgba_ex`] so concurrent
/// single-flight / cancel behaviour can be asserted without relying on ASCII cost.
#[cfg(test)]
pub static FD_TEST_COMPUTE_DELAY_MS: AtomicU64 = AtomicU64::new(0);

/// Cache key for a full-document filter result (§0.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FullDocumentKey {
    pub doc: u32,
    pub layer: u32,
    pub params_hash: u64,
    pub document_gen: u64,
    /// When false, Ascii filters are omitted from the pass (Image preview mode).
    pub include_ascii: bool,
}

/// One ready full-document RGBA f32 result.
#[derive(Clone)]
pub struct FullDocumentResult {
    pub rgba_f32: Arc<Vec<f32>>,
    pub width: u32,
    pub height: u32,
}

struct CacheSlot {
    /// Cancels the *current owner* job when a different key takes over.
    cancel: Arc<AtomicBool>,
    job_token: u64,
    /// Key currently being computed (owner holds this until publish or drop).
    inflight_key: Option<FullDocumentKey>,
    /// Last successfully published key + result.
    ready_key: Option<FullDocumentKey>,
    result: Option<FullDocumentResult>,
}

impl CacheSlot {
    fn empty() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            job_token: 0,
            inflight_key: None,
            ready_key: None,
            result: None,
        }
    }
}

/// Process-wide-ish cache of full-document filter results (one per AppState).
pub struct FullDocumentCache {
    inner: Mutex<HashMap<(u32, u32), CacheSlot>>,
    /// Wakes waiters when a flight completes, is cancelled, or is replaced.
    cv: Condvar,
    next_token: AtomicU64,
}

impl FullDocumentCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            cv: Condvar::new(),
            next_token: AtomicU64::new(1),
        }
    }

    pub fn clear(&self) {
        let mut guard = self.inner.lock().expect("full_document cache lock");
        for slot in guard.values_mut() {
            slot.cancel.store(true, Ordering::Relaxed);
            slot.inflight_key = None;
            slot.ready_key = None;
            slot.result = None;
        }
        guard.clear();
        self.cv.notify_all();
    }

    pub fn invalidate_layer(&self, doc: u32, layer: u32) {
        let mut guard = self.inner.lock().expect("full_document cache lock");
        if let Some(slot) = guard.get_mut(&(doc, layer)) {
            slot.cancel.store(true, Ordering::Relaxed);
            slot.inflight_key = None;
            slot.ready_key = None;
            slot.result = None;
            slot.job_token = self.next_token.fetch_add(1, Ordering::Relaxed);
        }
        self.cv.notify_all();
    }

    /// Return a cached result if it matches `key`.
    pub fn get_if_fresh(&self, key: &FullDocumentKey) -> Option<FullDocumentResult> {
        let guard = self.inner.lock().expect("full_document cache lock");
        let slot = guard.get(&(key.doc, key.layer))?;
        if slot.ready_key.as_ref() == Some(key) {
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
    hash_layer_filter_params_ex(layer, true)
}

/// Hash enabled filters; when `include_ascii` is false, Ascii filters are omitted
/// so Image-mode preview does not invalidate when only ASCII params change.
pub fn hash_layer_filter_params_ex(layer: &Layer, include_ascii: bool) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    include_ascii.hash(&mut hasher);
    for filter in layer.filters.iter().filter(|f| f.enabled) {
        if !include_ascii && is_ascii_filter(filter) {
            continue;
        }
        resolve_algorithm_id(filter).hash(&mut hasher);
        let json = filter_params_to_json(&filter.params).unwrap_or(serde_json::Value::Null);
        json.to_string().hash(&mut hasher);
        filter.opacity.to_bits().hash(&mut hasher);
        format!("{:?}", filter.blend_mode).hash(&mut hasher);
    }
    hasher.finish()
}

fn is_ascii_filter(filter: &FilterInstance) -> bool {
    matches!(filter.params, FilterParams::Ascii(_))
        || resolve_algorithm_id(filter).as_deref() == Some("ascii")
}

/// Whether any enabled filter on `layer` needs a full-document pass.
pub fn layer_has_full_document_filter(layer: &Layer) -> bool {
    layer_has_full_document_filter_ex(layer, true)
}

/// Like [`layer_has_full_document_filter`], optionally ignoring Ascii filters
/// (Image preview mode still needs this when Riemersma etc. remain).
pub fn layer_has_full_document_filter_ex(layer: &Layer, include_ascii: bool) -> bool {
    let reg = builtin_registry();
    layer.filters.iter().any(|f| {
        if !f.enabled {
            return false;
        }
        if !include_ascii && is_ascii_filter(f) {
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
/// Currently supports FullDocument members Riemersma and ASCII. Other enabled
/// filters in the same stack are applied via a tile-roundtrip before / after
/// the monolithic pass when present.
pub fn compute_full_document_rgba(
    cache: &TileCache,
    layer: &Layer,
    doc: &Document,
    should_cancel: &AtomicBool,
) -> Result<FullDocumentResult, EngineError> {
    compute_full_document_rgba_ex(cache, layer, doc, should_cancel, true)
}

/// Like [`compute_full_document_rgba`]; when `include_ascii` is false, Ascii
/// filters are skipped (Image preview vs ASCII preview).
pub fn compute_full_document_rgba_ex(
    cache: &TileCache,
    layer: &Layer,
    doc: &Document,
    should_cancel: &AtomicBool,
    include_ascii: bool,
) -> Result<FullDocumentResult, EngineError> {
    #[cfg(test)]
    {
        let ms = FD_TEST_COMPUTE_DELAY_MS.load(Ordering::Relaxed);
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            if should_cancel.load(Ordering::Relaxed) {
                return Err(EngineError::invalid_state("full-document job cancelled"));
            }
        }
    }

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
        if !include_ascii && is_ascii_filter(filter) {
            continue;
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

/// Run the layer filter stack and capture the ASCII job result (grid + atlas).
///
/// Preceding filters are applied first; the Ascii filter produces the grid.
/// If the layer has no Ascii filter, returns an error.
pub fn compute_ascii_job(
    cache: &TileCache,
    layer: &Layer,
    doc: &Document,
    should_cancel: &AtomicBool,
) -> Result<crate::filters::ascii_job::AsciiJobResult, EngineError> {
    let mut rgba = assemble_layer_rgba_f32(cache, layer, doc.width, doc.height, doc.id.0)?;
    let w = doc.width;
    let h = doc.height;
    let enabled: Vec<&FilterInstance> = layer.filters.iter().rev().filter(|f| f.enabled).collect();
    let reg = builtin_registry();
    let mut ascii_params: Option<AsciiParams> = None;

    for filter in enabled {
        if should_cancel.load(Ordering::Relaxed) {
            return Err(EngineError::invalid_state("ascii job cancelled"));
        }
        if let FilterParams::Ascii(ref p) = filter.params {
            ascii_params = Some(p.clone());
            break;
        }
        if resolve_algorithm_id(filter).as_deref() == Some("ascii") {
            let json = filter_params_to_json(&filter.params)?;
            ascii_params = Some(serde_json::from_value(json).map_err(|e| {
                EngineError::invalid_filter_params(format!("ascii params: {e}"))
            })?);
            break;
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

    let params = ascii_params.ok_or_else(|| {
        EngineError::invalid_state("no Ascii filter on layer for ASCII export")
    })?;
    run_ascii_job(&rgba, w, h, &params, should_cancel)
}

fn apply_full_document_algorithm(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    filter: &FilterInstance,
    should_cancel: &AtomicBool,
) -> Result<(), EngineError> {
    // ASCII is its own FilterParams arm (not a dither mode).
    if let FilterParams::Ascii(ref p) = filter.params {
        return apply_ascii_rgba(rgba, width, height, p, should_cancel);
    }
    if resolve_algorithm_id(filter).as_deref() == Some("ascii") {
        let json = filter_params_to_json(&filter.params)?;
        let p: AsciiParams = serde_json::from_value(json).map_err(|e| {
            EngineError::invalid_filter_params(format!("ascii params: {e}"))
        })?;
        return apply_ascii_rgba(rgba, width, height, &p, should_cancel);
    }

    let params = match &filter.params {
        FilterParams::DitherV2(p) => p.clone(),
        other => {
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
/// Single-flight: concurrent callers for the **same** key wait for one job.
/// An in-flight job is cancelled only when a request arrives with a **different**
/// key (or on [`FullDocumentCache::invalidate_layer`] / [`clear`]).
pub fn ensure_full_document(
    fd_cache: &FullDocumentCache,
    tile_cache: &TileCache,
    layer: &Layer,
    doc: &Document,
    document_gen: u64,
) -> Result<FullDocumentResult, EngineError> {
    ensure_full_document_ex(fd_cache, tile_cache, layer, doc, document_gen, true)
}

/// Like [`ensure_full_document`] with Image|ASCII preview control.
pub fn ensure_full_document_ex(
    fd_cache: &FullDocumentCache,
    tile_cache: &TileCache,
    layer: &Layer,
    doc: &Document,
    document_gen: u64,
    include_ascii: bool,
) -> Result<FullDocumentResult, EngineError> {
    let key = FullDocumentKey {
        doc: doc.id.0,
        layer: layer.id.0,
        params_hash: hash_layer_filter_params_ex(layer, include_ascii),
        document_gen,
        include_ascii,
    };

    // Acquire ownership or wait for an existing same-key flight.
    let (token, cancel) = {
        let mut guard = fd_cache.inner.lock().expect("full_document cache lock");
        let mut was_waiting = false;
        loop {
            let slot = guard
                .entry((key.doc, key.layer))
                .or_insert_with(CacheSlot::empty);

            if slot.ready_key.as_ref() == Some(&key) {
                if let Some(ref result) = slot.result {
                    return Ok(result.clone());
                }
            }

            match slot.inflight_key.as_ref() {
                Some(inflight) if inflight == &key => {
                    // Same flight in progress — wait; do not cancel.
                    was_waiting = true;
                    guard = fd_cache
                        .cv
                        .wait(guard)
                        .expect("full_document cache condvar");
                    continue;
                }
                Some(_) if was_waiting => {
                    // We were waiting on our key; a newer key took the slot.
                    return Err(EngineError::invalid_state(
                        "full-document job superseded by newer key",
                    ));
                }
                Some(_) => {
                    // Fresh request with a different key — cancel the old flight.
                    slot.cancel.store(true, Ordering::Relaxed);
                }
                None => {}
            }

            let token = fd_cache.next_token.fetch_add(1, Ordering::Relaxed);
            let cancel = Arc::new(AtomicBool::new(false));
            slot.job_token = token;
            slot.cancel = Arc::clone(&cancel);
            slot.inflight_key = Some(key.clone());
            slot.ready_key = None;
            slot.result = None;
            fd_cache.cv.notify_all();
            break (token, cancel);
        }
    };

    let compute = compute_full_document_rgba_ex(
        tile_cache,
        layer,
        doc,
        &cancel,
        include_ascii,
    );

    let mut guard = fd_cache.inner.lock().expect("full_document cache lock");
    let slot = guard
        .get_mut(&(key.doc, key.layer))
        .expect("full_document slot must exist after ownership");

    let still_owner = slot.job_token == token;
    if still_owner {
        slot.inflight_key = None;
    }

    if !still_owner || cancel.load(Ordering::Relaxed) {
        fd_cache.cv.notify_all();
        return Err(EngineError::invalid_state(
            "full-document job cancelled or superseded",
        ));
    }

    match compute {
        Ok(result) => {
            slot.ready_key = Some(key);
            slot.result = Some(result.clone());
            fd_cache.cv.notify_all();
            Ok(result)
        }
        Err(e) => {
            fd_cache.cv.notify_all();
            Err(e)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::filter::{AsciiParams, FilterKind, FilterParams};
    use crate::layer::{Layer, LayerNode};
    use crate::types::{DocumentId, LayerId, LayerKind};
    use engine_tiles::decompose::decompose_image_to_tiles;
    use std::sync::Barrier;

    fn solid_white(w: u32, h: u32) -> Vec<f32> {
        vec![1.0f32; (w * h * 4) as usize]
    }

    fn doc_with_ascii(w: u32, h: u32) -> (Document, Layer, TileCache) {
        let mut doc = Document::new(DocumentId::new(1), w, h);
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        let mut filt = FilterInstance::new(
            FilterKind::Ascii,
            FilterParams::Ascii(AsciiParams {
                font: "departure_mono".into(),
                size_mode: "px".into(),
                font_px: 11.0,
                symbol_set: "bourke_10".into(),
                match_mode: "tone".into(),
                ..AsciiParams::default()
            }),
        );
        filt.algorithm_id = Some("ascii".into());
        layer.filters.push(filt);
        doc.root.push(LayerNode::Leaf(layer.clone()));

        let cache = TileCache::new(50_000_000);
        let buf = solid_white(w, h);
        decompose_image_to_tiles(&buf, w, h, doc.id.0, layer.id.0, &cache).unwrap();
        (doc, layer, cache)
    }

    #[test]
    fn concurrent_same_key_all_succeed_single_flight() {
        let w = 28u32;
        let h = 28u32;
        let (doc, layer, cache) = doc_with_ascii(w, h);
        let fd = FullDocumentCache::new();
        FD_TEST_COMPUTE_DELAY_MS.store(80, Ordering::Relaxed);

        let n = 8usize;
        let barrier = Arc::new(Barrier::new(n));
        let fd = Arc::new(fd);
        let cache = Arc::new(cache);
        let doc = Arc::new(doc);
        let layer = Arc::new(layer);

        let mut handles = Vec::new();
        for _ in 0..n {
            let fd = Arc::clone(&fd);
            let cache = Arc::clone(&cache);
            let doc = Arc::clone(&doc);
            let layer = Arc::clone(&layer);
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                barrier.wait();
                ensure_full_document_ex(&fd, &cache, &layer, &doc, 1, true)
            }));
        }

        let mut oks = 0usize;
        for h in handles {
            if h.join().unwrap().is_ok() {
                oks += 1;
            }
        }
        FD_TEST_COMPUTE_DELAY_MS.store(0, Ordering::Relaxed);
        assert_eq!(oks, n, "all waiters must receive the single-flight result");
        assert!(fd.get_if_fresh(&FullDocumentKey {
            doc: 1,
            layer: 1,
            params_hash: hash_layer_filter_params_ex(&layer, true),
            document_gen: 1,
            include_ascii: true,
        })
        .is_some());
    }

    #[test]
    fn newer_key_cancels_in_flight_job() {
        let w = 28u32;
        let h = 28u32;
        let (doc, layer, cache) = doc_with_ascii(w, h);
        let fd = Arc::new(FullDocumentCache::new());
        let cache = Arc::new(cache);
        let doc = Arc::new(doc);
        let layer = Arc::new(layer);

        FD_TEST_COMPUTE_DELAY_MS.store(120, Ordering::Relaxed);

        let fd_a = Arc::clone(&fd);
        let cache_a = Arc::clone(&cache);
        let doc_a = Arc::clone(&doc);
        let layer_a = Arc::clone(&layer);
        let first = std::thread::spawn(move || {
            ensure_full_document_ex(&fd_a, &cache_a, &layer_a, &doc_a, 1, true)
        });

        // Let the first thread become owner and enter the delayed compute.
        std::thread::sleep(std::time::Duration::from_millis(30));

        let second = ensure_full_document_ex(&fd, &cache, &layer, &doc, 1, false);
        let first_res = first.join().unwrap();

        FD_TEST_COMPUTE_DELAY_MS.store(0, Ordering::Relaxed);

        assert!(
            second.is_ok(),
            "Image-mode request should own the flight"
        );
        assert!(
            first_res.is_err(),
            "ASCII-mode job must be cancelled by newer key"
        );
        assert!(fd
            .get_if_fresh(&FullDocumentKey {
                doc: 1,
                layer: 1,
                params_hash: hash_layer_filter_params_ex(&layer, false),
                document_gen: 1,
                include_ascii: false,
            })
            .is_some());
    }
}
