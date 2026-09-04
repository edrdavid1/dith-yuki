//! GPU-resident Path B: A1 authorship + A2 speculative warm-up.
//!
//! - Auto-dispatch: warm Composite slots → download even when opt-in is off.
//! - Opt-in: cold eligible L0 may GPU-compute (per-tile).
//! - A2: low-priority promote of prefetch L0 without evicting viewport VRAM.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use engine_gpu::{
    cap_warmup_coords, decide_tile_dispatch, select_warmup_coords, slots_per_warmup_coord,
    viewport_vram_reserve, warmup_slot_budget, GpuCompositeFrameJob, GpuCompositeLayerOp,
    GpuCompositeTileWork, GpuFrameJob, GpuTileWork, GraphNode, TileDispatch, TileDispatchInput,
};
use engine_project::filters::gpu_graph::{
    compile_layer_graph, compile_layer_graph_with_palettes, PaletteGraphCtx,
};
use engine_project::layer::{Layer, LayerNode};
use engine_tiles::{CacheStage, TileCoord, TileKey};
use tauri::Emitter;

use crate::commands::AppState;
use crate::worker::TileReadyPayload;

const FRAME_BATCH: usize = 64;
const WARMUP_BATCH: usize = 8;

/// A2: fire-and-forget GPU promote. Skips while CPU preview is urgent; never evicts.
pub fn enqueue_resident_shadow_viewport(state: &AppState) {
    if !engine_gpu::gpu_warmup_enabled() {
        return;
    }
    // TZ A2: do not steal the GPU while CPU Immediate / ViewportCenter (or any
    // in-flight preview pass) is still authoring the current frame.
    if state.tiles.scheduler.has_urgent_work()
        || state.tiles.scheduler.queued_len() > 0
        || state.preview_pass_inflight.load(Ordering::Acquire) > 0
    {
        return;
    }
    let Some(gpu_cache) = state.gpu_resident.as_ref() else {
        return;
    };
    let Some(executor_mtx) = state.gpu_executor.as_ref() else {
        return;
    };
    let Ok(session) = state.active_session() else {
        return;
    };

    let snapshot = session.document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let doc = snapshot.id.0;
    let viewport = state.ui.viewport.lock().unwrap().clone();

    let mut layers: Vec<&Layer> = Vec::new();
    collect_visible_layers(&snapshot.root, &mut layers);
    let palettes = PaletteGraphCtx {
        document: snapshot.as_ref(),
        lut_cache: &state.tiles.palette_lut_cache,
        kd_cache: &state.tiles.palette_cache,
    };
    if !preview_stack_eligible(&layers, &snapshot.root, Some(&palettes)) {
        return;
    }

    let visible_l0: Vec<TileCoord> = viewport
        .visible_tiles
        .iter()
        .copied()
        .filter(|c| c.level == 0)
        .collect();
    let reserve = viewport_vram_reserve(visible_l0.len(), layers.len());
    // Visible L0 is CPU or opt-in compute — never dual-submit with speculative promote.
    let per = slots_per_warmup_coord(layers.len());
    let free = gpu_cache.free_slot_count();
    let pre_budget = warmup_slot_budget(free, reserve);
    if pre_budget == 0 {
        return;
    }
    let prefetch = select_warmup_coords(&[], &viewport.prefetch_tiles, false);
    let planned = cap_warmup_coords(&prefetch, pre_budget, per);

    let coords: Vec<TileCoord> = planned
        .into_iter()
        .filter(|coord| {
            let composite_key = TileKey {
                doc,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            };
            if gpu_cache.get_slot(&composite_key, doc_gen).is_some() {
                return false;
            }
            layers.iter().all(|layer| {
                state
                    .tiles
                    .tile_cache
                    .get_entry(TileKey {
                        doc,
                        layer: layer.id.0,
                        coord: *coord,
                        stage: CacheStage::Raw,
                    })
                    .is_some()
            })
        })
        .collect();

    if coords.is_empty() {
        return;
    }

    let Ok(executor) = executor_mtx.try_lock() else {
        return;
    };

    for layer in &layers {
        if layer.filters.is_empty() {
            continue;
        }
        let graph = match compile_layer_graph_with_palettes(&layer.filters, Some(&palettes)) {
            Ok(g) if g.is_gpu_only() => Arc::new(g),
            _ => continue,
        };
        let mut tiles = Vec::new();
        for coord in &coords {
            let raw_key = TileKey {
                doc,
                layer: layer.id.0,
                coord: *coord,
                stage: CacheStage::Raw,
            };
            let Some(raw) = state.tiles.tile_cache.get_entry(raw_key) else {
                continue;
            };
            tiles.push(GpuTileWork {
                key: TileKey {
                    stage: CacheStage::Processed,
                    ..raw_key
                },
                coord: *coord,
                generation: doc_gen,
                pixels: raw,
            });
        }
        for chunk in tiles.chunks(WARMUP_BATCH) {
            executor.submit_frame(GpuFrameJob {
                doc_gen,
                graph: Arc::clone(&graph),
                tiles: chunk.to_vec(),
                speculative: true,
            });
        }
    }

    if let Some(mut job) = build_composite_job(state, doc, doc_gen, &layers, &coords) {
        job.speculative = true;
        for chunk in chunk_composite_job(job, WARMUP_BATCH) {
            executor.submit_composite(chunk);
        }
    }
}

/// Classify one dirty L0 coord (unit-testable; no wgpu).
fn classify_dirty_l0(
    stack_eligible: bool,
    opt_in: bool,
    warm: bool,
    raws_ready: bool,
) -> TileDispatch {
    let decision = decide_tile_dispatch(TileDispatchInput {
        force_cpu: false,
        gpu_available: true,
        preview_opt_in: opt_in,
        stack_eligible,
        level: 0,
        composite_slot_warm: warm,
    });
    match decision {
        TileDispatch::GpuCompute if !raws_ready => TileDispatch::Cpu,
        other => other,
    }
}

/// Author dirty L0 Composite tiles from GPU where the per-tile decision allows it.
///
/// Returns coords successfully published (caller skips CPU for those only).
pub fn try_publish_gpu_preview_viewport(state: &AppState) -> std::collections::HashSet<TileCoord> {
    use std::collections::HashSet;

    let empty = HashSet::new();
    if engine_gpu::force_cpu() {
        return empty;
    }
    let Some(ctx) = state.gpu.as_ref() else {
        return empty;
    };
    let Some(gpu_cache) = state.gpu_resident.as_ref() else {
        return empty;
    };
    let Some(executor_mtx) = state.gpu_executor.as_ref() else {
        return empty;
    };
    let Ok(session) = state.active_session() else {
        return empty;
    };

    let snapshot = session.document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let doc = snapshot.id.0;
    let viewport = state.ui.viewport.lock().unwrap().clone();

    let mut layers: Vec<&Layer> = Vec::new();
    collect_visible_layers(&snapshot.root, &mut layers);

    let palettes = PaletteGraphCtx {
        document: snapshot.as_ref(),
        lut_cache: &state.tiles.palette_lut_cache,
        kd_cache: &state.tiles.palette_cache,
    };
    let stack_eligible = preview_stack_eligible(&layers, &snapshot.root, Some(&palettes));
    let opt_in = engine_gpu::gpu_preview_enabled();

    let dirty_l0: Vec<TileCoord> = viewport
        .visible_tiles
        .iter()
        .copied()
        .filter(|c| c.level == 0)
        .filter(|coord| {
            let key = TileKey {
                doc,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            };
            match state.tiles.tile_cache.entries.get(&key) {
                Some(entry) => entry.dirty.load(Ordering::Acquire),
                None => true,
            }
        })
        .collect();

    if dirty_l0.is_empty() {
        return empty;
    }

    let mut download_coords = Vec::new();
    let mut compute_coords = Vec::new();
    for coord in dirty_l0 {
        let composite_key = TileKey {
            doc,
            layer: 0,
            coord,
            stage: CacheStage::Composite,
        };
        let warm = gpu_cache.get_slot(&composite_key, doc_gen).is_some();
        let raws_ready = layers.iter().all(|layer| {
            state
                .tiles
                .tile_cache
                .get_entry(TileKey {
                    doc,
                    layer: layer.id.0,
                    coord,
                    stage: CacheStage::Raw,
                })
                .is_some()
        });
        match classify_dirty_l0(stack_eligible, opt_in, warm, raws_ready) {
            TileDispatch::GpuDownload => download_coords.push(coord),
            TileDispatch::GpuCompute => compute_coords.push(coord),
            TileDispatch::Cpu => {}
        }
    }

    if download_coords.is_empty() && compute_coords.is_empty() {
        return empty;
    }

    let Ok(executor) = executor_mtx.lock() else {
        return empty;
    };

    let mut compute_ok = !compute_coords.is_empty();
    if compute_ok {
        for layer in &layers {
            if layer.filters.is_empty() {
                continue;
            }
            let graph = match compile_layer_graph_with_palettes(&layer.filters, Some(&palettes)) {
                Ok(g) if g.is_gpu_only() => Arc::new(g),
                _ => {
                    compute_ok = false;
                    break;
                }
            };
            let mut tiles = Vec::new();
            for coord in &compute_coords {
                let raw_key = TileKey {
                    doc,
                    layer: layer.id.0,
                    coord: *coord,
                    stage: CacheStage::Raw,
                };
                let Some(raw) = state.tiles.tile_cache.get_entry(raw_key) else {
                    continue;
                };
                tiles.push(GpuTileWork {
                    key: TileKey {
                        stage: CacheStage::Processed,
                        ..raw_key
                    },
                    coord: *coord,
                    generation: doc_gen,
                    pixels: raw,
                });
            }
            for chunk in tiles.chunks(FRAME_BATCH) {
                let job = GpuFrameJob {
                    doc_gen,
                    graph: Arc::clone(&graph),
                    tiles: chunk.to_vec(),
                    speculative: false,
                };
                if executor.submit_frame_blocking(job).is_err() {
                    compute_ok = false;
                    break;
                }
            }
            if !compute_ok {
                break;
            }
        }
    }

    let mut pending_keys: Vec<TileKey> = download_coords
        .iter()
        .map(|coord| TileKey {
            doc,
            layer: 0,
            coord: *coord,
            stage: CacheStage::Composite,
        })
        .collect();

    if compute_ok {
        if let Some(composite_job) =
            build_composite_job(state, doc, doc_gen, &layers, &compute_coords)
        {
            for chunk in chunk_composite_job(composite_job, FRAME_BATCH) {
                if executor.submit_composite_blocking(chunk.clone()).is_err() {
                    break;
                }
                let live_gen = session
                    .document_handle
                    .snapshot()
                    .generations
                    .document_gen
                    .load(Ordering::Acquire);
                if live_gen != doc_gen {
                    break;
                }
                for work in &chunk.tiles {
                    pending_keys.push(work.composite_key);
                }
            }
        }
    }
    drop(executor);

    let mut pending = Vec::new();
    for key in pending_keys {
        let Ok(Some(tile)) = gpu_cache.download(ctx, &key) else {
            continue;
        };
        pending.push((key, tile));
    }

    let viewport_level = state.ui.viewport.lock().unwrap().level;
    let app = state.app_handle.lock().ok().and_then(|g| g.clone());
    let mut published = HashSet::new();

    for (key, tile) in pending {
        let inserted = state
            .tiles
            .tile_cache
            .insert_fresh_gen(key, Arc::new(tile), doc_gen);
        if !inserted {
            continue;
        }
        published.insert(key.coord);
        state.evict_for_pressure_if_needed();
        if key.coord.level == viewport_level {
            if let Some(ref handle) = app {
                let payload = TileReadyPayload {
                    doc_id: doc,
                    layer_id: key.layer,
                    stage: "composite".to_string(),
                    level: key.coord.level,
                    x: key.coord.x,
                    y: key.coord.y,
                };
                let _ = handle.emit_to(tauri::EventTarget::Any, "tile-ready", payload);
            }
        }
    }

    published
}

/// True when every visible leaf is GPU-previewable (flat, no checkpoint).
fn preview_stack_eligible(
    layers: &[&Layer],
    root: &[LayerNode],
    palettes: Option<&PaletteGraphCtx<'_>>,
) -> bool {
    if layers.is_empty() {
        return false;
    }
    if layers.iter().any(|l| l.mask.is_some()) {
        return false;
    }
    if root_has_groups(root) {
        return false;
    }
    for layer in layers {
        if layer.filters.is_empty() {
            continue;
        }
        match compile_layer_graph_with_palettes(&layer.filters, palettes) {
            Ok(g) if g.is_gpu_only() => {}
            _ => return false,
        }
    }
    true
}

fn chunk_composite_job(job: GpuCompositeFrameJob, batch: usize) -> Vec<GpuCompositeFrameJob> {
    if job.tiles.len() <= batch {
        return vec![job];
    }
    let doc_gen = job.doc_gen;
    let speculative = job.speculative;
    job.tiles
        .chunks(batch)
        .map(|chunk| GpuCompositeFrameJob {
            doc_gen,
            tiles: chunk.to_vec(),
            speculative,
        })
        .collect()
}

/// Build a composite frame from visible leaf layers (document order).
fn build_composite_job(
    state: &AppState,
    doc: u32,
    doc_gen: u64,
    layers: &[&Layer],
    visible: &[TileCoord],
) -> Option<GpuCompositeFrameJob> {
    if layers.is_empty() {
        return None;
    }
    if layers.iter().any(|l| l.mask.is_some()) {
        return None;
    }
    let Ok(session) = state.active_session() else {
        return None;
    };
    let snapshot = session.document_handle.snapshot();
    if root_has_groups(&snapshot.root) {
        return None;
    }

    let mut tiles = Vec::new();
    for coord in visible {
        if coord.level != 0 {
            continue;
        }
        let mut ops = Vec::with_capacity(layers.len());
        let mut incomplete = false;
        for layer in layers {
            let processed_key = TileKey {
                doc,
                layer: layer.id.0,
                coord: *coord,
                stage: CacheStage::Processed,
            };
            let pixels = state.tiles.tile_cache.get_entry(processed_key).or_else(|| {
                state.tiles.tile_cache.get_entry(TileKey {
                    stage: CacheStage::Raw,
                    ..processed_key
                })
            });
            let Some(pixels) = pixels else {
                incomplete = true;
                break;
            };
            ops.push(GpuCompositeLayerOp {
                processed_key,
                blend_mode: layer.blend_mode as u32,
                opacity: layer.opacity,
                pixels: Some(pixels),
            });
        }
        if incomplete || ops.is_empty() {
            continue;
        }
        tiles.push(GpuCompositeTileWork {
            coord: *coord,
            composite_key: TileKey {
                doc,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            },
            generation: doc_gen,
            layers: ops,
        });
    }

    if tiles.is_empty() {
        return None;
    }
    Some(GpuCompositeFrameJob {
        doc_gen,
        tiles,
        speculative: false,
    })
}

fn root_has_groups(nodes: &[LayerNode]) -> bool {
    nodes.iter().any(|n| matches!(n, LayerNode::Group(_)))
}

fn collect_visible_layers<'a>(nodes: &'a [LayerNode], out: &mut Vec<&'a Layer>) {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) if layer.visible => out.push(layer),
            LayerNode::Group(group) if group.visible => {
                collect_visible_layers(&group.children, out);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_project::types::BlendMode;

    #[test]
    fn shadow_skips_graph_with_checkpoint() {
        use engine_gpu::CpuCheckpointKind;
        use engine_project::filter::{
            DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
        };

        let ed = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                ..Default::default()
            }),
        );
        let graph = compile_layer_graph(&[ed]).unwrap();
        assert!(matches!(
            graph.nodes.first(),
            Some(GraphNode::CpuCheckpoint(CpuCheckpointKind::ErrorDiffusion))
        ));
        assert!(!graph.is_gpu_only());
        assert!(!preview_stack_eligible(&[], &[], None));
    }

    #[test]
    fn preview_rejects_ed_layer() {
        use engine_project::filter::{
            DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
        };
        use engine_project::types::{LayerId, LayerKind};

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                ..Default::default()
            }),
        ));
        let root = vec![LayerNode::Leaf(layer.clone())];
        assert!(!preview_stack_eligible(&[&layer], &root, None));
    }

    #[test]
    fn preview_accepts_bayer_only() {
        use engine_project::filter::{
            DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
        };
        use engine_project::types::{LayerId, LayerKind};

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                ..Default::default()
            }),
        ));
        let root = vec![LayerNode::Leaf(layer.clone())];
        assert!(preview_stack_eligible(&[&layer], &root, None));
    }

    #[test]
    fn classify_hybrid_warm_and_cold() {
        assert_eq!(
            classify_dirty_l0(true, false, true, true),
            TileDispatch::GpuDownload
        );
        assert_eq!(
            classify_dirty_l0(true, false, false, true),
            TileDispatch::Cpu
        );
        assert_eq!(
            classify_dirty_l0(true, true, false, true),
            TileDispatch::GpuCompute
        );
        assert_eq!(
            classify_dirty_l0(true, true, false, false),
            TileDispatch::Cpu
        );
        assert_eq!(
            classify_dirty_l0(false, true, true, true),
            TileDispatch::Cpu
        );
    }

    #[test]
    fn classify_missing_raw_does_not_block_warm_neighbor() {
        let a = classify_dirty_l0(true, true, true, false);
        let b = classify_dirty_l0(true, true, false, true);
        assert_eq!(a, TileDispatch::GpuDownload);
        assert_eq!(b, TileDispatch::GpuCompute);
    }

    #[test]
    fn blend_mode_discriminants_match_gpu() {
        assert_eq!(BlendMode::Normal as u32, 0);
        assert_eq!(BlendMode::Multiply as u32, 1);
        assert_eq!(BlendMode::Screen as u32, 2);
        assert_eq!(BlendMode::SoftLight as u32, 9);
        assert_eq!(BlendMode::Exclusion as u32, 11);
    }

    #[test]
    fn root_has_groups_detects_nested() {
        use engine_project::layer::LayerGroup;
        use engine_project::types::{LayerId, LayerKind};

        let leaf = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        assert!(!root_has_groups(&[LayerNode::Leaf(leaf.clone())]));
        let group = LayerGroup {
            id: LayerId::new(10),
            name: "g".into(),
            blend_mode: BlendMode::Normal,
            opacity: 1.0,
            visible: true,
            mask: None,
            children: vec![LayerNode::Leaf(leaf)],
        };
        assert!(root_has_groups(&[LayerNode::Group(group)]));
    }
}
