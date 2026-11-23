//! GamedevContext IPC (F1–F3): get/set, chips, tile/cell actions, frame-set apply.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use engine_project::{
    apply_frame_stable_context, cell_grid_crop_size, chip_action, chip_cache_begin_job,
    chip_cache_epoch, chip_cache_invalidate, chip_cache_put, chip_stub_for_context,
    chip_stub_pending, chip_stub_validation_failed, clear_wrap_padding_in_tree, collect_frame_paths,
    context_result_chip_hash, evaluate_gamedev_chips, evaluate_gamedev_chips_with_result,
    flatten_bottom_to_top, gamedev_cache_invalidation, period_snap_crop_size,
    process_frame_set_to_folder, seam_mode_for_layer, should_lock_pattern_phase,
    switch_filters_to_frame_stable, threshold_alpha_rgba_f32, validate_seams_rgba8, ChipCache,
    FilterParams, FrameSetReport, GamedevCacheLayer, GamedevChip, GamedevContext, GamedevJob,
    HeadlessOutputFormat, LayerRef, MaskPolicy,
};
use engine_project::filters::full_document::{
    apply_post_to_fd_result, assemble_layer_rgba_f32, full_document_result_to_rgba8,
    layer_needs_full_document, peek_full_document,
};
use engine_project::layer::{Layer, LayerNode};
use engine_project::serialize::build_processed_composite_rgba8;
use engine_project::types::{LayerKind, LayerId, TileBounds};
use engine_tiles::decompose::decompose_image_to_tiles_at_generation;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::commands::{emit_document_changed, request_preview_refresh, AppState};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetGamedevContextRequest {
    pub doc_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetGamedevContextRequest {
    pub doc_id: u32,
    pub context: GamedevContext,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetGamedevContextResponse {
    pub context: GamedevContext,
    pub invalidated: Vec<GamedevCacheLayer>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetGamedevChipsRequest {
    pub doc_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshGamedevChipsRequest {
    pub doc_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamedevChipActionRequest {
    pub doc_id: u32,
    pub action_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamedevChipActionResponse {
    pub context: GamedevContext,
    pub snap: Option<[u32; 2]>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GamedevChipsEvent {
    pub doc_id: u32,
    pub chips: Vec<GamedevChip>,
}

fn chip_caches(
    state: &AppState,
) -> Result<std::sync::MutexGuard<'_, std::collections::HashMap<u32, ChipCache>>, String> {
    state
        .gamedev_chip_caches
        .lock()
        .map_err(|e| e.to_string())
}

fn emit_chips(app: &AppHandle, doc_id: u32, chips: Vec<GamedevChip>) {
    let _ = app.emit(
        "gamedev-chips",
        GamedevChipsEvent { doc_id, chips },
    );
}

/// First bottom-most leaf with an enabled dither filter (else first leaf).
fn primary_dither_layer(doc: &engine_project::Document) -> Option<Layer> {
    let mut first_leaf: Option<Layer> = None;
    for layer_ref in flatten_bottom_to_top(&doc.root) {
        if let LayerRef::Leaf(layer) = layer_ref {
            if first_leaf.is_none() {
                first_leaf = Some(layer.clone());
            }
            let has_dither = layer.filters.iter().any(|f| {
                f.enabled
                    && matches!(
                        &f.params,
                        FilterParams::DitherV2(_) | FilterParams::Dither { .. }
                    )
            });
            if has_dither {
                return Some(layer.clone());
            }
        }
    }
    first_leaf
}

fn set_wrap_padding_in_tree(nodes: &mut [LayerNode], enabled: bool) {
    for n in nodes {
        match n {
            LayerNode::Leaf(layer) => {
                for f in &mut layer.filters {
                    if let FilterParams::DitherV2(p) = &mut f.params {
                        if p.mode.is_error_diffusion() || p.mode.is_ed1d() {
                            p.wrap_padding = enabled;
                        }
                    }
                }
            }
            LayerNode::Group(g) => set_wrap_padding_in_tree(&mut g.children, enabled),
        }
    }
}

fn switch_filters_frame_stable_in_tree(nodes: &mut [LayerNode]) {
    for n in nodes {
        match n {
            LayerNode::Leaf(layer) => switch_filters_to_frame_stable(&mut layer.filters),
            LayerNode::Group(g) => switch_filters_frame_stable_in_tree(&mut g.children),
        }
    }
}

fn sync_dither_alpha_in_tree(nodes: &mut [LayerNode], no_dither_alpha: bool) {
    for n in nodes {
        match n {
            LayerNode::Leaf(layer) => {
                for f in &mut layer.filters {
                    if let FilterParams::DitherV2(p) = &mut f.params {
                        p.dither_alpha = !no_dither_alpha;
                    }
                }
            }
            LayerNode::Group(g) => sync_dither_alpha_in_tree(&mut g.children, no_dither_alpha),
        }
    }
}

/// Threshold every raster leaf's raw pixels per `GamedevContext.alpha`.
fn threshold_document_alpha(
    state: &AppState,
    doc_id: u32,
    app: &AppHandle,
) -> Result<(), String> {
    let session = state.require_session(doc_id)?;
    let snap = session.document_handle.snapshot();
    let policy = snap.gamedev.alpha.clone();
    let w = snap.width;
    let h = snap.height;
    let mut updated: Vec<(u32, Vec<f32>)> = Vec::new();
    for layer_ref in flatten_bottom_to_top(&snap.root) {
        if let LayerRef::Leaf(layer) = layer_ref {
            if layer.kind != LayerKind::Raster {
                continue;
            }
            let mut rgba = assemble_layer_rgba_f32(
                &state.tiles.tile_cache,
                layer,
                w,
                h,
                snap.id.0,
            )
            .map_err(|e| e.to_string())?;
            threshold_alpha_rgba_f32(&mut rgba, &policy);
            updated.push((layer.id.0, rgba));
        }
    }
    drop(snap);

    for (layer_id, _) in &updated {
        state.tiles.tile_cache.evict_layer(doc_id, *layer_id);
        state.tiles.full_document.invalidate_layer(doc_id, *layer_id);
    }

    let live_gen = session
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire)
        .saturating_add(1);

    for (layer_id, rgba) in &updated {
        decompose_image_to_tiles_at_generation(
            rgba,
            w,
            h,
            doc_id,
            *layer_id,
            &state.tiles.tile_cache,
            live_gen,
        )
        .map_err(|e| format!("tile decompose after alpha threshold: {e}"))?;
    }

    session.document_handle.mutate(|doc| {
        sync_dither_alpha_in_tree(&mut doc.root, doc.gamedev.alpha.no_dither_alpha);
        for (layer_id, _) in &updated {
            let _ = doc.bump_layer_raw_rev(LayerId::new(*layer_id));
        }
        doc.increment_generation();
        doc.generations.set_document_gen(live_gen);
    });

    if let Ok(mut caches) = chip_caches(state) {
        if let Some(cache) = caches.get_mut(&doc_id) {
            chip_cache_invalidate(cache);
        }
    }

    emit_document_changed(app, "gamedev_threshold_alpha", None, Some(doc_id));
    Ok(())
}

fn crop_rgba_f32_top_left(src: &[f32], sw: u32, sh: u32, nw: u32, nh: u32) -> Vec<f32> {
    let sw = sw as usize;
    let nw = nw as usize;
    let nh = nh as usize;
    debug_assert!(nw <= sw && nh <= sh as usize);
    let mut out = vec![0.0f32; nw * nh * 4];
    for y in 0..nh {
        for x in 0..nw {
            let si = (y * sw + x) * 4;
            let di = (y * nw + x) * 4;
            out[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    out
}

fn update_layer_bounds_in_tree(nodes: &mut [LayerNode], width: u32, height: u32) {
    for n in nodes {
        match n {
            LayerNode::Leaf(layer) => {
                layer.bounds_l0 = TileBounds::full_document(width, height);
            }
            LayerNode::Group(g) => update_layer_bounds_in_tree(&mut g.children, width, height),
        }
    }
}

/// Crop document canvas top-left to `nw×nh` (raw pixels; filters preserved).
fn crop_document_to_period(
    state: &AppState,
    doc_id: u32,
    nw: u32,
    nh: u32,
    app: &AppHandle,
) -> Result<(), String> {
    if nw == 0 || nh == 0 {
        return Err("snap size must be ≥ 1".into());
    }
    let session = state.require_session(doc_id)?;
    let snap = session.document_handle.snapshot();
    if nw > snap.width || nh > snap.height {
        return Err(format!(
            "snap {nw}×{nh} exceeds canvas {}×{}",
            snap.width, snap.height
        ));
    }
    if nw == snap.width && nh == snap.height {
        return Ok(());
    }

    // Assemble + crop each leaf's raw RGBA before mutating the document.
    let mut cropped: Vec<(u32, Vec<f32>)> = Vec::new();
    for layer_ref in flatten_bottom_to_top(&snap.root) {
        if let LayerRef::Leaf(layer) = layer_ref {
            if layer.kind != LayerKind::Raster {
                continue;
            }
            let rgba = assemble_layer_rgba_f32(
                &state.tiles.tile_cache,
                layer,
                snap.width,
                snap.height,
                snap.id.0,
            )
            .map_err(|e| e.to_string())?;
            let cropped_rgba = crop_rgba_f32_top_left(&rgba, snap.width, snap.height, nw, nh);
            cropped.push((layer.id.0, cropped_rgba));
        }
    }
    drop(snap);

    for (layer_id, _) in &cropped {
        state.tiles.tile_cache.evict_layer(doc_id, *layer_id);
        state.tiles.full_document.invalidate_layer(doc_id, *layer_id);
    }

    let live_gen = session
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire)
        .saturating_add(1);

    for (layer_id, rgba) in &cropped {
        decompose_image_to_tiles_at_generation(
            rgba,
            nw,
            nh,
            doc_id,
            *layer_id,
            &state.tiles.tile_cache,
            live_gen,
        )
        .map_err(|e| format!("tile decompose after snap: {e}"))?;
    }

    session.document_handle.mutate(|doc| {
        doc.width = nw;
        doc.height = nh;
        update_layer_bounds_in_tree(&mut doc.root, nw, nh);
        for (layer_id, _) in &cropped {
            let _ = doc.bump_layer_raw_rev(LayerId::new(*layer_id));
        }
        doc.increment_generation();
        doc.generations.set_document_gen(live_gen);
    });

    if let Ok(mut caches) = chip_caches(state) {
        if let Some(cache) = caches.get_mut(&doc_id) {
            chip_cache_invalidate(cache);
        }
    }

    emit_document_changed(app, "gamedev_snap", None, Some(doc_id));
    Ok(())
}

/// Pixels for chip/seam checks — never starts a full-document compute.
///
/// Pipeline Tile / independent cells share the live preview FD cache. Calling
/// [`build_processed_composite_rgba8`] here used to invoke
/// `compute_full_document_rgba` (cache bypass) and race the preview into a
/// second 9× mosaic, which looks like an endless processor load.
fn try_chip_validation_rgba8(
    state: &AppState,
    doc: &engine_project::Document,
    layer: &Layer,
    document_gen: u64,
) -> Result<Option<Vec<u8>>, String> {
    if layer_needs_full_document(layer, doc, true) {
        let Some(peeked) = peek_full_document(
            &state.tiles.full_document,
            layer,
            doc,
            document_gen,
            true,
        ) else {
            return Ok(None);
        };
        let finished = if peeked.needs_post {
            apply_post_to_fd_result(&peeked, layer, doc, &state.tiles.tile_cache)
                .map_err(|e| format!("gamedev chip post: {e}"))?
        } else {
            peeked
        };
        if finished.width != doc.width || finished.height != doc.height {
            // Stale / mid-crop buffer — wait for the next preview settle.
            return Ok(None);
        }
        return Ok(Some(full_document_result_to_rgba8(&finished)));
    }

    // Tiled path: export-identical CPU apply (no FD race).
    let rgba = build_processed_composite_rgba8(&state.tiles.tile_cache, doc)
        .map_err(|e| format!("gamedev seam composite: {e}"))?;
    Ok(Some(rgba))
}

fn validate_and_build_chips(
    state: &AppState,
    doc_id: u32,
) -> Result<(GamedevContext, Vec<GamedevChip>, u64), String> {
    let session = state.require_session(doc_id)?;
    let doc = session.document_handle.snapshot();
    let ctx = doc.gamedev.clone();
    let w = doc.width;
    let h = doc.height;
    let gen = doc.generations.document_gen.load(Ordering::Acquire);

    let Some(layer) = primary_dither_layer(&doc) else {
        return Ok((ctx.clone(), chip_stub_for_context(&ctx), gen));
    };

    // Memory / period heuristics without needing pixels.
    let early = evaluate_gamedev_chips(&layer, &ctx, w, h, None);
    let seamless = &early[0];
    if seamless.state == engine_project::ChipState::Error
        || seamless.action_id.as_deref() == Some(chip_action::SNAP_TO_PERIOD)
    {
        return Ok((ctx, early, gen));
    }

    // Ring validation needs the preview result. If FD is still computing,
    // keep heuristic/pending chips — FE refreshes on full-document-busy:false.
    let Some(rgba) = try_chip_validation_rgba8(state, &doc, &layer, gen)? else {
        return Ok((ctx, early, gen));
    };
    let mode = seam_mode_for_layer(&layer);
    let report = validate_seams_rgba8(&rgba, w, h, mode)
        .map_err(|e| format!("validate_seams: {e}"))?;
    let chips = evaluate_gamedev_chips_with_result(&layer, &ctx, w, h, Some(&report), Some(&rgba));
    Ok((ctx, chips, gen))
}

fn store_and_emit_chips(
    state: &AppState,
    app: &AppHandle,
    doc_id: u32,
    ctx: &GamedevContext,
    chips: Vec<GamedevChip>,
    expected_gen: u64,
    expected_epoch: u64,
) -> Result<(), String> {
    // Drop stale async results so rapid toggles cannot overwrite fresher chips.
    let session = state.require_session(doc_id)?;
    let live_gen = session
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire);
    if live_gen != expected_gen {
        return Ok(());
    }
    let mut caches = chip_caches(state)?;
    let cache = caches.entry(doc_id).or_default();
    if chip_cache_epoch(cache) != expected_epoch {
        return Ok(());
    }
    let key = context_result_chip_hash(ctx, expected_gen);
    chip_cache_put(cache, key, chips.clone());
    emit_chips(app, doc_id, chips);
    Ok(())
}

fn spawn_chip_validation(state: Arc<AppState>, app: AppHandle, doc_id: u32) {
    let expected_epoch = match chip_caches(&state) {
        Ok(mut caches) => {
            let cache = caches.entry(doc_id).or_default();
            chip_cache_begin_job(cache)
        }
        Err(_) => 0,
    };
    tauri::async_runtime::spawn(async move {
        let state_for_compute = Arc::clone(&state);
        let result =
            tauri::async_runtime::spawn_blocking(move || validate_and_build_chips(&state_for_compute, doc_id))
                .await;
        match result {
            Ok(Ok((ctx, chips, gen))) => {
                let _ = store_and_emit_chips(
                    &state,
                    &app,
                    doc_id,
                    &ctx,
                    chips,
                    gen,
                    expected_epoch,
                );
            }
            Ok(Err(e)) => {
                log::warn!("gamedev chip validation failed: {e}");
                emit_chips(&app, doc_id, chip_stub_validation_failed(&e));
            }
            Err(e) => {
                log::warn!("gamedev chip validation join failed: {e}");
                emit_chips(
                    &app,
                    doc_id,
                    chip_stub_validation_failed(&format!("join: {e}")),
                );
            }
        }
    });
}

#[tauri::command]
pub fn get_gamedev_context(
    state: State<'_, Arc<AppState>>,
    req: GetGamedevContextRequest,
) -> Result<GamedevContext, String> {
    let session = state.require_session(req.doc_id)?;
    Ok(session.document_handle.snapshot().gamedev.clone())
}

#[tauri::command]
pub fn set_gamedev_context(
    state: State<'_, Arc<AppState>>,
    req: SetGamedevContextRequest,
    app_handle: AppHandle,
) -> Result<SetGamedevContextResponse, String> {
    let session = state.require_session(req.doc_id)?;
    let old = session.document_handle.snapshot().gamedev.clone();
    let new = req.context;
    let invalidated = gamedev_cache_invalidation(&old, &new);

    if invalidated.is_empty() {
        return Ok(SetGamedevContextResponse {
            context: old,
            invalidated,
        });
    }

    let tile_turned_on = !old.tile.enabled && new.tile.enabled;
    let alpha_policy_changed = old.alpha != new.alpha;
    session.document_handle.mutate(|doc| {
        doc.gamedev = new.clone();
        if tile_turned_on || new.tile.enabled {
            clear_wrap_padding_in_tree(&mut doc.root);
        }
        if alpha_policy_changed {
            sync_dither_alpha_in_tree(&mut doc.root, new.alpha.no_dither_alpha);
        }
        if invalidated.contains(&GamedevCacheLayer::Render) {
            doc.increment_generation();
        } else {
            doc.revision = doc.revision.saturating_add(1);
        }
    });

    if invalidated.contains(&GamedevCacheLayer::Render) {
        let snap = session.document_handle.snapshot();
        let primary_id = primary_dither_layer(&snap).map(|l| l.id.0);
        for layer_ref in flatten_bottom_to_top(&snap.root) {
            if let LayerRef::Leaf(layer) = layer_ref {
                state
                    .tiles
                    .full_document
                    .invalidate_layer(req.doc_id, layer.id.0);
            }
        }
        emit_document_changed(
            &app_handle,
            "gamedev_context",
            None,
            Some(req.doc_id),
        );
        // One preview wave (with busy UI) — same path as filter edits. Without
        // this, TileCanvas re-requests while chip validation used to start a
        // second uncached FD mosaic in parallel.
        if let Some(layer_id) = primary_id {
            request_preview_refresh(state.inner(), layer_id, true);
        }
    }

    if invalidated.contains(&GamedevCacheLayer::Chips) {
        let mut caches = chip_caches(state.inner())?;
        let cache = caches.entry(req.doc_id).or_default();
        chip_cache_invalidate(cache);
        let pending = chip_stub_pending();
        emit_chips(&app_handle, req.doc_id, pending);
        drop(caches);
        spawn_chip_validation(Arc::clone(state.inner()), app_handle.clone(), req.doc_id);
    }

    Ok(SetGamedevContextResponse {
        context: new,
        invalidated,
    })
}

#[tauri::command]
pub fn get_gamedev_chips(
    state: State<'_, Arc<AppState>>,
    req: GetGamedevChipsRequest,
    app_handle: AppHandle,
) -> Result<Vec<GamedevChip>, String> {
    let session = state.require_session(req.doc_id)?;
    let doc = session.document_handle.snapshot();
    let ctx = doc.gamedev.clone();
    let gen = doc.generations.document_gen.load(Ordering::Acquire);
    let key = context_result_chip_hash(&ctx, gen);
    let mut caches = chip_caches(state.inner())?;
    let cache = caches.entry(req.doc_id).or_default();
    if let Some(hits) = engine_project::chip_cache_get(cache, key) {
        return Ok(hits.to_vec());
    }
    // Return pending immediately; kick async validation.
    let pending = chip_stub_pending();
    chip_cache_put(cache, key, pending.clone());
    drop(caches);
    drop(doc);
    spawn_chip_validation(Arc::clone(state.inner()), app_handle, req.doc_id);
    Ok(pending)
}

#[tauri::command]
pub fn refresh_gamedev_chips(
    state: State<'_, Arc<AppState>>,
    req: RefreshGamedevChipsRequest,
    app_handle: AppHandle,
) -> Result<(), String> {
    let mut caches = chip_caches(state.inner())?;
    if let Some(cache) = caches.get_mut(&req.doc_id) {
        chip_cache_invalidate(cache);
    }
    drop(caches);
    emit_chips(&app_handle, req.doc_id, chip_stub_pending());
    spawn_chip_validation(Arc::clone(state.inner()), app_handle, req.doc_id);
    Ok(())
}

#[tauri::command]
pub fn gamedev_chip_action(
    state: State<'_, Arc<AppState>>,
    req: GamedevChipActionRequest,
    app_handle: AppHandle,
) -> Result<GamedevChipActionResponse, String> {
    let session = state.require_session(req.doc_id)?;
    let mut snap_size: Option<[u32; 2]> = None;

    match req.action_id.as_str() {
        chip_action::DISABLE_TILE => {
            session.document_handle.mutate(|doc| {
                doc.gamedev.tile.enabled = false;
                doc.increment_generation();
            });
            let snap = session.document_handle.snapshot();
            for layer_ref in flatten_bottom_to_top(&snap.root) {
                if let LayerRef::Leaf(layer) = layer_ref {
                    state
                        .tiles
                        .full_document
                        .invalidate_layer(req.doc_id, layer.id.0);
                }
            }
            emit_document_changed(&app_handle, "gamedev_chip_action", None, Some(req.doc_id));
        }
        chip_action::ENABLE_WRAP_PADDING => {
            session.document_handle.mutate(|doc| {
                doc.gamedev.tile.enabled = false;
                set_wrap_padding_in_tree(&mut doc.root, true);
                doc.increment_generation();
            });
            let snap = session.document_handle.snapshot();
            for layer_ref in flatten_bottom_to_top(&snap.root) {
                if let LayerRef::Leaf(layer) = layer_ref {
                    state
                        .tiles
                        .full_document
                        .invalidate_layer(req.doc_id, layer.id.0);
                }
            }
            emit_document_changed(&app_handle, "gamedev_chip_action", None, Some(req.doc_id));
        }
        chip_action::SNAP_TO_PERIOD => {
            let snap = session.document_handle.snapshot();
            let layer = primary_dither_layer(&snap)
                .ok_or_else(|| "no layer for period snap".to_string())?;
            let (nw, nh) = period_snap_crop_size(&layer, snap.width, snap.height)
                .ok_or_else(|| "canvas already period-aligned or cannot crop".to_string())?;
            drop(snap);
            crop_document_to_period(state.inner(), req.doc_id, nw, nh, &app_handle)?;
            snap_size = Some([nw, nh]);
        }
        chip_action::CROP_TO_CELL_GRID => {
            let snap = session.document_handle.snapshot();
            let cell = snap
                .gamedev
                .cell
                .as_ref()
                .ok_or_else(|| "no cell settings for grid crop".to_string())?;
            let (nw, nh) = cell_grid_crop_size(snap.width, snap.height, cell)
                .ok_or_else(|| "canvas already on cell grid or cannot crop".to_string())?;
            drop(snap);
            crop_document_to_period(state.inner(), req.doc_id, nw, nh, &app_handle)?;
            snap_size = Some([nw, nh]);
        }
        chip_action::SWITCH_TO_FRAME_STABLE => {
            session.document_handle.mutate(|doc| {
                switch_filters_frame_stable_in_tree(&mut doc.root);
                apply_frame_stable_context(&mut doc.gamedev);
                doc.increment_generation();
            });
            let snap = session.document_handle.snapshot();
            for layer_ref in flatten_bottom_to_top(&snap.root) {
                if let LayerRef::Leaf(layer) = layer_ref {
                    state
                        .tiles
                        .full_document
                        .invalidate_layer(req.doc_id, layer.id.0);
                }
            }
            emit_document_changed(&app_handle, "gamedev_chip_action", None, Some(req.doc_id));
        }
        chip_action::THRESHOLD_ALPHA => {
            threshold_document_alpha(state.inner(), req.doc_id, &app_handle)?;
        }
        other => return Err(format!("unknown gamedev chip action: {other}")),
    }

    let ctx = state
        .require_session(req.doc_id)?
        .document_handle
        .snapshot()
        .gamedev
        .clone();
    emit_chips(&app_handle, req.doc_id, chip_stub_pending());
    spawn_chip_validation(Arc::clone(state.inner()), app_handle, req.doc_id);

    Ok(GamedevChipActionResponse {
        context: ctx,
        snap: snap_size,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectGamedevFrameSetRequest {
    pub doc_id: u32,
    pub folder: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectGamedevFrameSetResponse {
    pub context: GamedevContext,
    pub frame_count: usize,
}

/// Populate `GamedevContext.frame_set` from an image folder (sorted paths).
#[tauri::command]
pub fn collect_gamedev_frame_set(
    state: State<'_, Arc<AppState>>,
    req: CollectGamedevFrameSetRequest,
    app_handle: AppHandle,
) -> Result<CollectGamedevFrameSetResponse, String> {
    let paths = collect_frame_paths(PathBuf::from(&req.folder).as_path())
        .map_err(|e| e.to_string())?;
    let count = paths.len();
    let session = state.require_session(req.doc_id)?;
    session.document_handle.mutate(|doc| {
        doc.gamedev.frame_set = paths;
        doc.gamedev.job = GamedevJob::Animation;
        if matches!(doc.gamedev.threshold_mask, MaskPolicy::Free) {
            apply_frame_stable_context(&mut doc.gamedev);
        }
        doc.increment_generation();
    });
    let ctx = session.document_handle.snapshot().gamedev.clone();
    emit_document_changed(&app_handle, "gamedev_frame_set", None, Some(req.doc_id));
    emit_chips(&app_handle, req.doc_id, chip_stub_pending());
    spawn_chip_validation(Arc::clone(state.inner()), app_handle, req.doc_id);
    Ok(CollectGamedevFrameSetResponse {
        context: ctx,
        frame_count: count,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyGamedevFrameSetRequest {
    pub doc_id: u32,
    #[serde(default)]
    pub layer_id: Option<u32>,
    pub output_dir: String,
    #[serde(default = "default_frame_name_template")]
    pub name_template: String,
    #[serde(default = "default_frame_format")]
    pub format: HeadlessOutputFormat,
}

fn default_frame_name_template() -> String {
    "{name}_dithered.png".into()
}

fn default_frame_format() -> HeadlessOutputFormat {
    HeadlessOutputFormat::Png
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyGamedevFrameSetResponse {
    pub succeeded: usize,
    pub failed: usize,
    pub report: FrameSetReportDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameSetReportDto {
    pub frame_count: usize,
    pub unique_colors: usize,
    pub shared_palette_len: usize,
    pub frames_skipped_identical: u64,
    pub palette_ok: bool,
}

impl From<&FrameSetReport> for FrameSetReportDto {
    fn from(r: &FrameSetReport) -> Self {
        Self {
            frame_count: r.frame_count,
            unique_colors: r.unique_colors,
            shared_palette_len: r.shared_palette_len,
            frames_skipped_identical: r.frames_skipped_identical,
            palette_ok: r.palette_ok(),
        }
    }
}

/// Apply document pattern to `frame_set` → output folder (F3 CTA).
#[tauri::command]
pub async fn apply_gamedev_frame_set(
    state: State<'_, Arc<AppState>>,
    req: ApplyGamedevFrameSetRequest,
) -> Result<ApplyGamedevFrameSetResponse, String> {
    let state = Arc::clone(state.inner());
    let session = state.require_session(req.doc_id)?;
    let snap = session.document_handle.snapshot();
    let frame_set = snap.gamedev.frame_set.clone();
    if frame_set.is_empty() {
        return Err("frame set is empty — add a frames folder first".into());
    }
    let lock = should_lock_pattern_phase(&snap.gamedev);
    let name_template = if req.name_template.is_empty() {
        snap.gamedev
            .export
            .naming
            .clone()
            .unwrap_or_else(default_frame_name_template)
    } else {
        req.name_template.clone()
    };
    let format = if snap.gamedev.export.png8 && matches!(req.format, HeadlessOutputFormat::Png) {
        HeadlessOutputFormat::Png8
    } else {
        req.format
    };
    let palettes = snap.palettes.clone();

    let leaves: Vec<&engine_project::Layer> = flatten_bottom_to_top(&snap.root)
        .into_iter()
        .filter_map(|r| match r {
            LayerRef::Leaf(l) => Some(l),
            _ => None,
        })
        .collect();
    let layer = if let Some(id) = req.layer_id {
        leaves
            .iter()
            .find(|l| l.id.0 == id)
            .copied()
            .ok_or_else(|| format!("Layer {id} not found"))?
    } else {
        leaves
            .iter()
            .find(|l| !l.filters.is_empty())
            .or_else(|| leaves.first())
            .copied()
            .ok_or_else(|| "Document has no layers".to_string())?
    };
    let filters = layer.filters.clone();
    drop(snap);

    let output_dir = PathBuf::from(&req.output_dir);
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state
            .batch_export_cancel
            .lock()
            .map_err(|e| e.to_string())?;
        *slot = Some(Arc::clone(&cancel));
    }

    let result = tauri::async_runtime::spawn_blocking(move || {
        process_frame_set_to_folder(
            &frame_set,
            &output_dir,
            &name_template,
            format,
            &filters,
            &palettes,
            lock,
            None,
            &cancel,
        )
        .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;

    let (results, report) = result;
    let succeeded = results.iter().filter(|r| r.error.is_none()).count();
    let failed = results.len().saturating_sub(succeeded);
    Ok(ApplyGamedevFrameSetResponse {
        succeeded,
        failed,
        report: FrameSetReportDto::from(&report),
    })
}
