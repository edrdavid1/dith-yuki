#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;
mod tile_pipeline;
mod tile_protocol;
mod viewport;
mod worker;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use engine_project::document::DocumentHandle;
use engine_project::layer::LayerNode;
use engine_tiles::{
    Priority, RecomputeTask, Scheduler, TileCache, TileCoord, TileKey, TILE_SIZE,
};
use commands::{AppState, ViewportState};
use tauri::Manager;
use tile_protocol::{f32_tile_to_rgba8, parse_tile_url, LayerTarget};

fn main() {
    // Initialize app state
    use engine_project::types::DocumentId;
    
    let document = engine_project::Document::new(DocumentId::new(1), 800, 600);
    let doc_handle = DocumentHandle::new(document);
    let tile_cache = TileCache::new(256 * 1024 * 1024); // 256 MB budget
    let scheduler = Scheduler::new();
    
    let app_state = AppState {
        document_handle: doc_handle,
        tile_cache,
        scheduler,
        viewport: Mutex::new(ViewportState::default()),
    };

    // Wrap in Arc for sharing between Tauri state and worker threads
    let state = Arc::new(app_state);

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(state.clone())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let num_workers = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4);
            for _ in 0..num_workers {
                let state_clone = state.clone();
                let handle_clone = app_handle.clone();
                std::thread::spawn(move || {
                    worker::tile_worker_loop(state_clone, handle_clone);
                });
            }
            Ok(())
        })
        .register_uri_scheme_protocol("tile", |ctx, request| {
            let state = ctx.app_handle().state::<Arc<AppState>>();
            handle_tile_request(&*state, request)
        })
        .invoke_handler(tauri::generate_handler![
            // Document commands
            commands::new_document,
            commands::get_document_snapshot,
            
            // Layer commands
            commands::add_layer,
            commands::remove_layer,
            commands::set_layer_props,
            commands::reorder_layer,
            commands::get_layer_tree,
            
            // Filter commands
            commands::add_filter,
            commands::update_filter,
            commands::remove_filter,
            
            // Image commands
            commands::load_image,
            commands::export_image,
            
            // Viewport commands
            viewport::set_viewport,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

/// Build an HTTP response with CORS headers allowing any origin.
/// Required because in dev mode the webview origin is http://localhost:5173
/// and the browser enforces CORS on custom protocol fetches.
fn tile_response(status: u16, content_type: &str, body: Vec<u8>) -> http::Response<Vec<u8>> {
    http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, content_type)
        .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body)
        .unwrap()
}

/// Handle a tile:// protocol request.
///
/// Returns:
/// - 200 + 262,144 bytes (RGBA8) if tile is cached and clean
/// - 202 + empty body if tile needs recomputation (schedules Immediate task)
/// - 400 if URL is malformed
/// - 404 if doc/layer/coord is invalid
fn handle_tile_request(
    state: &AppState,
    request: http::Request<Vec<u8>>,
) -> http::Response<Vec<u8>> {
    // 1. Parse the URL
    let uri = request.uri().to_string();
    let parsed = match parse_tile_url(&uri) {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("400 Bad Request: {}", e);
            return tile_response(400, "text/plain", msg.into_bytes());
        }
    };

    // 2. Validate document
    let snapshot = state.document_handle.snapshot();
    if snapshot.id.0 != parsed.doc_id {
        let msg = format!("404 Not Found: document {} not found", parsed.doc_id);
        return tile_response(404, "text/plain", msg.into_bytes());
    }

    // 3. Validate layer
    let layer_id = match parsed.layer {
        LayerTarget::Id(id) => {
            if !layer_exists(&snapshot.root, id) {
                let msg = format!("404 Not Found: layer {} not found", id);
                return tile_response(404, "text/plain", msg.into_bytes());
            }
            id
        }
        LayerTarget::Composite => {
            // Composite uses layer 0 as a sentinel; always valid if doc exists
            0
        }
    };

    // 4. Validate coordinate bounds
    let doc_width = snapshot.width;
    let doc_height = snapshot.height;
    let scale = 1u32.checked_shl(parsed.level as u32).unwrap_or(u32::MAX);
    let tile_size_at_level = TILE_SIZE.saturating_mul(scale);
    let grid_cols = (doc_width + tile_size_at_level - 1) / tile_size_at_level;
    let grid_rows = (doc_height + tile_size_at_level - 1) / tile_size_at_level;

    if parsed.x >= grid_cols || parsed.y >= grid_rows {
        let msg = format!(
            "404 Not Found: tile coordinate ({}, {}) out of bounds for grid {}x{} at level {}",
            parsed.x, parsed.y, grid_cols, grid_rows, parsed.level
        );
        return tile_response(404, "text/plain", msg.into_bytes());
    }

    // 5. Build TileKey and check cache
    let key = TileKey {
        layer: layer_id,
        coord: TileCoord {
            level: parsed.level,
            x: parsed.x,
            y: parsed.y,
        },
        stage: parsed.stage,
    };

    // Check cache: if entry exists and is not dirty, serve it
    if let Some(entry) = state.tile_cache.entries.get(&key) {
        if !entry.dirty.load(Ordering::Acquire) {
            let rgba8 = f32_tile_to_rgba8(&entry.tile);
            return tile_response(200, "application/octet-stream", rgba8);
        }
    }

    // 6. Cache miss or dirty: schedule Immediate task and return 202
    // Use the current generation values so the worker doesn't discard the task as stale.
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let layer_gen = snapshot.generations.get_layer_gen(layer_id);
    let task = RecomputeTask {
        key,
        generation: doc_gen,
        layer_generation: layer_gen,
        priority: Priority::Immediate,
    };
    state.scheduler.enqueue(task);

    tile_response(202, "application/octet-stream", Vec::new())
}

/// Check if a layer with the given ID exists in the document tree.
fn layer_exists(nodes: &[LayerNode], layer_id: u32) -> bool {
    for node in nodes {
        match node {
            LayerNode::Leaf(layer) => {
                if layer.id.0 == layer_id {
                    return true;
                }
            }
            LayerNode::Group(group) => {
                if group.id.0 == layer_id {
                    return true;
                }
                if layer_exists(&group.children, layer_id) {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}
