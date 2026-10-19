use std::sync::atomic::Ordering;
use std::sync::Arc;

use engine_tiles::{CacheStage, Priority, RecomputeTask, TileKey};

use crate::commands::AppState;
use crate::services::AppError;
use crate::viewport::{
    classify_priority, compute_max_level, compute_prefetch_ring, compute_pyramid_level,
    compute_visible_tiles, needs_recompute, sort_tiles_center_out, SetViewportResponse,
    ViewportState,
};

pub struct ViewportService {
    state: Arc<AppState>,
}

impl ViewportService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    #[allow(dead_code)] // IPC uses set_viewport_traced; thin wrapper kept for callers
    pub fn set_viewport(
        &self,
        zoom: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    ) -> Result<SetViewportResponse, AppError> {
        self.set_viewport_traced(zoom, x, y, width, height, None)
    }

    pub fn set_viewport_traced(
        &self,
        zoom: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        trace_seq: Option<u64>,
    ) -> Result<SetViewportResponse, AppError> {
        let zoom = zoom.clamp(0.01, 64.0);

        let Ok(session) = self.state.active_session() else {
            return Ok(SetViewportResponse {
                level: 0,
                tile_count: 0,
            });
        };

        let snapshot = session.document_handle.snapshot();
        let doc_width = snapshot.width;
        let doc_height = snapshot.height;
        drop(snapshot);

        let max_level = compute_max_level(doc_width, doc_height);
        let level = compute_pyramid_level(zoom, max_level);

        let mut visible =
            compute_visible_tiles(zoom, x, y, width, height, level, doc_width, doc_height);
        sort_tiles_center_out(&mut visible);

        let prefetch = compute_prefetch_ring(&visible, level, doc_width, doc_height);
        let tile_count = visible.len();
        let (recv_hash, x0, y0, x1, y1) =
            crate::tile_gen_trace::vp_hash_from_coords(level, &visible);

        if crate::tile_gen_trace::enabled() {
            crate::tile_gen_trace::emit_simple(
                "vp_received",
                serde_json::json!({
                    "zoom": zoom,
                    "x": x,
                    "y": y,
                    "width": width,
                    "height": height,
                    "level": level,
                    "x0": x0, "y0": y0, "x1": x1, "y1": y1,
                    "tile_count": tile_count,
                    "vp_hash": recv_hash,
                    "trace_seq": trace_seq,
                }),
            );
        }

        let snapshot = self
            .state
            .active_session()
            .map_err(|e| AppError::Generic(e.to_string()))?
            .document_handle
            .snapshot();
        let doc_id = snapshot.id.0;
        let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
        drop(snapshot);

        let diag = crate::preview_diag::level();
        let t_sched = (diag != 0).then(std::time::Instant::now);
        for coord in &visible {
            let key = TileKey {
                doc: doc_id,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            };
            if needs_recompute(&self.state, &key) {
                let priority = classify_priority(coord, &visible);
                let task = RecomputeTask {
                    key,
                    generation: doc_gen,
                    layer_generation: 0,
                    priority,
                };
                self.state.tiles.scheduler.enqueue(task);
                self.state.worker_wake.notify_one();
                if crate::tile_gen_trace::enabled() {
                    crate::tile_gen_trace::emit(
                        "tile_enqueued",
                        None,
                        Some(doc_gen),
                        None,
                        Some(crate::tile_gen_trace::tile(coord.level, coord.x, coord.y)),
                        Some(doc_gen),
                        serde_json::json!({
                            "reason": "viewport_change",
                            "set": "visible",
                            "priority": match priority {
                                Priority::Immediate => "Immediate",
                                Priority::ViewportCenter => "ViewportCenter",
                                Priority::ViewportEdge => "ViewportEdge",
                                Priority::Prefetch => "Prefetch",
                            },
                            "vp_hash": recv_hash,
                        }),
                    );
                }
            }
        }

        for coord in &prefetch {
            let key = TileKey {
                doc: doc_id,
                layer: 0,
                coord: *coord,
                stage: CacheStage::Composite,
            };
            if needs_recompute(&self.state, &key) {
                let task = RecomputeTask {
                    key,
                    generation: doc_gen,
                    layer_generation: 0,
                    priority: Priority::Prefetch,
                };
                self.state.tiles.scheduler.enqueue(task);
                self.state.worker_wake.notify_one();
                if crate::tile_gen_trace::enabled() {
                    crate::tile_gen_trace::emit(
                        "tile_enqueued",
                        None,
                        Some(doc_gen),
                        None,
                        Some(crate::tile_gen_trace::tile(coord.level, coord.x, coord.y)),
                        Some(doc_gen),
                        serde_json::json!({
                            "reason": "viewport_change",
                            "set": "prefetch",
                            "priority": "Prefetch",
                            "vp_hash": recv_hash,
                        }),
                    );
                }
            }
        }
        if let Some(t0) = t_sched {
            crate::preview_diag::record_schedule(doc_id, doc_gen, t0.elapsed());
        }

        let new_viewport = ViewportState {
            zoom,
            x,
            y,
            width,
            height,
            level,
            visible_tiles: visible,
            prefetch_tiles: prefetch,
        };
        let (applied_hash, _, _, _, _) =
            crate::tile_gen_trace::vp_hash_from_coords(level, &new_viewport.visible_tiles);
        let matches_recv = applied_hash == recv_hash;
        if crate::tile_gen_trace::enabled() {
            crate::tile_gen_trace::emit_simple(
                "vp_state_applied",
                serde_json::json!({
                    "zoom": zoom,
                    "x": x,
                    "y": y,
                    "width": width,
                    "height": height,
                    "level": level,
                    "x0": x0, "y0": y0, "x1": x1, "y1": y1,
                    "tile_count": tile_count,
                    "vp_hash": applied_hash,
                    "matches_received": matches_recv,
                    "trace_seq": trace_seq,
                }),
            );
        }
        let vp_protect: std::collections::HashSet<_> =
            new_viewport.visible_tiles.iter().copied().collect();

        *self.state.ui.viewport.lock().unwrap() = new_viewport;

        self.state.sync_gpu_evict_policy(&vp_protect);

        crate::gpu_resident_shadow::enqueue_resident_shadow_viewport(&self.state);

        Ok(SetViewportResponse { level, tile_count })
    }
}
