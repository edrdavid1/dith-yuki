//! GPU-resident Path B: shadow enqueue + G10 preview authorship.
//!
//! - `DITHER_GPU_RESIDENT=1` — shadow only (CPU `tile_cache` remains SoT).
//! - `DITHER_GPU_PREVIEW=1` — exclusive L0 Composite publish via demote/download
//!   when the viewport stack is fully `is_gpu_only()` and flat (no mask/group).
//!   Any checkpoint / mask / group → return false and leave CPU schedule alone.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use engine_gpu::{
    GpuCompositeFrameJob, GpuCompositeLayerOp, GpuCompositeTileWork, GpuFrameJob, GpuTileWork,
    GraphNode,
};
use engine_project::filters::gpu_graph::compile_layer_graph;
use engine_project::layer::{Layer, LayerNode};
use engine_tiles::{CacheStage, TileCoord, TileKey};
use tauri::Emitter;

use crate::commands::AppState;
use crate::worker::TileReadyPayload;

const FRAME_BATCH: usize = 64;

/// Shadow path: fire-and-forget GPU work alongside CPU schedule.
pub fn enqueue_resident_shadow_viewport(state: &AppState) {
    // Preview mode owns L0; don't dual-submit.
    if engine_gpu::gpu_preview_enabled() {
        return;
    }
    if !engine_gpu::gpu_resident_enabled() {
        return;
    }
    let Some(executor_mtx) = state.gpu_executor.as_ref() else {
        return;
    };
    let Ok(session) = state.active_session() else {
        return;
    };

    let snapshot = session.document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let doc = snapshot.id.0;
    let viewport = state.viewport.lock().unwrap().clone();

    let mut layers: Vec<&Layer> = Vec::new();
    collect_visible_layers(&snapshot.root, &mut layers);

    for layer in &layers {
        let graph = match compile_layer_graph(&layer.filters) {
            Ok(g) if g.is_gpu_only() => Arc::new(g),
            _ => continue,
        };

        let mut tiles = Vec::new();
        for coord in &viewport.visible_tiles {
            if coord.level != 0 {
                continue;
            }
            let raw_key = TileKey {
                doc,
                layer: layer.id.0,
                coord: *coord,
                stage: CacheStage::Raw,
            };
            let Some(raw) = state.tile_cache.get_entry(raw_key) else {
                continue;
            };
            let processed_key = TileKey {
                stage: CacheStage::Processed,
                ..raw_key
            };
            tiles.push(GpuTileWork {
                key: processed_key,
                coord: *coord,
                generation: doc_gen,
                pixels: raw,
            });
        }

        if tiles.is_empty() {
            continue;
        }

        for chunk in tiles.chunks(FRAME_BATCH) {
            let job = GpuFrameJob {
                doc_gen,
                graph: Arc::clone(&graph),
                tiles: chunk.to_vec(),
            };
            if let Ok(executor) = executor_mtx.try_lock() {
                executor.submit_frame(job);
            }
        }
    }

    if let Some(job) = build_composite_job(state, doc, doc_gen, &layers, &viewport.visible_tiles) {
        for chunk in chunk_composite_job(job) {
            if let Ok(executor) = executor_mtx.try_lock() {
                executor.submit_composite(chunk);
            }
        }
    }
}

/// G10: try to author dirty L0 Composite tiles on GPU.
///
/// Returns the set of L0 coords successfully published (caller skips CPU for those).
/// Empty set → full CPU schedule for all dirty tiles.
pub fn try_publish_gpu_preview_viewport(state: &AppState) -> std::collections::HashSet<TileCoord> {
    use std::collections::HashSet;

    let empty = HashSet::new();
    if !engine_gpu::gpu_preview_enabled() {
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
    let viewport = state.viewport.lock().unwrap().clone();

    let mut layers: Vec<&Layer> = Vec::new();
    collect_visible_layers(&snapshot.root, &mut layers);

    if !preview_stack_eligible(&layers, &snapshot.root) {
        return empty;
    }

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
            match state.tile_cache.entries.get(&key) {
                Some(entry) => entry.dirty.load(Ordering::Acquire),
                None => true,
            }
        })
        .collect();

    if dirty_l0.is_empty() {
        return empty;
    }

    for coord in &dirty_l0 {
        for layer in &layers {
            let raw_key = TileKey {
                doc,
                layer: layer.id.0,
                coord: *coord,
                stage: CacheStage::Raw,
            };
            if state.tile_cache.get_entry(raw_key).is_none() {
                return empty;
            }
        }
    }

    let Ok(executor) = executor_mtx.lock() else {
        return empty;
    };

    for layer in &layers {
        if layer.filters.is_empty() {
            continue;
        }
        let graph = match compile_layer_graph(&layer.filters) {
            Ok(g) if g.is_gpu_only() => Arc::new(g),
            _ => return empty,
        };
        let mut tiles = Vec::new();
        for coord in &dirty_l0 {
            let raw_key = TileKey {
                doc,
                layer: layer.id.0,
                coord: *coord,
                stage: CacheStage::Raw,
            };
            let Some(raw) = state.tile_cache.get_entry(raw_key) else {
                return empty;
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
            };
            if executor.submit_frame_blocking(job).is_err() {
                return empty;
            }
        }
    }

    let Some(composite_job) =
        build_composite_job(state, doc, doc_gen, &layers, &dirty_l0)
    else {
        return empty;
    };

    let mut pending = Vec::new();
    for chunk in chunk_composite_job(composite_job) {
        if executor.submit_composite_blocking(chunk.clone()).is_err() {
            return empty;
        }
        let live_gen = session
            .document_handle
            .snapshot()
            .generations
            .document_gen
            .load(Ordering::Acquire);
        if live_gen != doc_gen {
            return empty;
        }
        for work in &chunk.tiles {
            let Ok(Some(tile)) = gpu_cache.download(ctx, &work.composite_key) else {
                return empty;
            };
            pending.push((work.composite_key, tile));
        }
    }
    drop(executor);

    let viewport_level = state.viewport.lock().unwrap().level;
    let app = state.app_handle.lock().ok().and_then(|g| g.clone());
    let mut published = HashSet::new();

    for (key, tile) in pending {
        let inserted = state
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
fn preview_stack_eligible(layers: &[&Layer], root: &[LayerNode]) -> bool {
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
        match compile_layer_graph(&layer.filters) {
            Ok(g) if g.is_gpu_only() => {}
            _ => return false,
        }
    }
    true
}

fn chunk_composite_job(job: GpuCompositeFrameJob) -> Vec<GpuCompositeFrameJob> {
    if job.tiles.len() <= FRAME_BATCH {
        return vec![job];
    }
    let doc_gen = job.doc_gen;
    job.tiles
        .chunks(FRAME_BATCH)
        .map(|chunk| GpuCompositeFrameJob {
            doc_gen,
            tiles: chunk.to_vec(),
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
            let pixels = state
                .tile_cache
                .get_entry(processed_key)
                .or_else(|| {
                    state.tile_cache.get_entry(TileKey {
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
    Some(GpuCompositeFrameJob { doc_gen, tiles })
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
        assert!(!preview_stack_eligible(
            &[],
            &[]
        ));
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
        assert!(!preview_stack_eligible(&[&layer], &root));
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
        assert!(preview_stack_eligible(&[&layer], &root));
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
