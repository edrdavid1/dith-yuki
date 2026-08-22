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

    pub fn set_viewport(
        &self,
        zoom: f64,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
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

        let snapshot = self
            .state
            .active_session()
            .map_err(|e| AppError::Generic(e.to_string()))?
            .document_handle
            .snapshot();
        let doc_id = snapshot.id.0;
        let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
        drop(snapshot);

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
                self.state.scheduler.enqueue(task);
                self.state.worker_wake.notify_one();
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
                self.state.scheduler.enqueue(task);
                self.state.worker_wake.notify_one();
            }
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

        *self.state.ui.viewport.lock().unwrap() = new_viewport;

        Ok(SetViewportResponse { level, tile_count })
    }
}
