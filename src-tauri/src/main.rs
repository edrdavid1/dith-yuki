#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

mod commands;
mod diffusion_waiters;
mod dock_affinity;
mod global_mouseup;
mod panel_commands;
mod panel_manager;
mod panel_persistence;
mod recent_files;
mod tile_pipeline;
mod tile_protocol;
mod undo;
mod viewport;
mod worker;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use engine_project::document::DocumentHandle;
use engine_project::layer::LayerNode;
use engine_tiles::{
    Priority, RecomputeTask, Scheduler, TileCache, TileCoord, TileKey, TILE_SIZE,
};
use commands::{AppState, ViewportState};
use panel_manager::PanelManager;
use worker::WorkerWake;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri::webview::WebviewWindowBuilder;
#[cfg(target_os = "macos")]
use objc::{msg_send, sel, sel_impl, class};
use tile_protocol::{f32_tile_to_rgba8, parse_tile_url, LayerTarget};

fn main() {
    // Initialize app state
    use engine_project::types::DocumentId;
    
    let document = engine_project::Document::new(DocumentId::new(1), 800, 600);
    let doc_handle = DocumentHandle::new(document);
    let tile_cache = TileCache::new(256 * 1024 * 1024); // 256 MB budget
    let scheduler = Scheduler::new();
    
    let dock_affinity_enabled = global_mouseup::mouseup_backend_available();
    if !dock_affinity_enabled {
        log::warn!("Dock affinity unavailable on this platform (no mouseup backend)");
    }

    // Track D: optional GPU. Force-CPU via DITHER_FORCE_CPU=1; prefer via DITHER_GPU=1.
    let gpu = if engine_gpu::force_cpu() {
        log::info!("engine-gpu: DITHER_FORCE_CPU set — skipping adapter init");
        None
    } else {
        match engine_gpu::GpuContext::try_new_blocking() {
            Some(ctx) => {
                log::info!("engine-gpu: device ready (map_timeouts=0)");
                Some(std::sync::Arc::new(ctx))
            }
            None => {
                log::warn!("engine-gpu: no adapter — CPU-only filters");
                None
            }
        }
    };

    let app_state = AppState {
        document_handle: doc_handle,
        tile_cache,
        scheduler,
        viewport: Mutex::new(ViewportState::default()),
        worker_wake: WorkerWake::new(),
        palette_cache: engine_color::palette_cache::PaletteKdCache::new(),
        palette_lut_cache: engine_color::palette_lut::PaletteLutCache::new(),
        threshold_cache: engine_color::threshold_map::ThresholdMapCache::new(),
        error_residuals: engine_project::filters::ErrorResidualsStore::new(),
        block_representatives: engine_tiles::BlockRepresentativeCache::new(),
        diffusion_skip_counter: diffusion_waiters::DiffusionSkipCounter::new(),
        pending_diffusion_waiters: diffusion_waiters::PendingDiffusionWaiters::new(),
        gpu,
        panel_manager: Mutex::new(PanelManager::new()),
        selection: Mutex::new(commands::SelectionState::default()),
        dock_affinity: Mutex::new(dock_affinity::DockAffinityController::new(
            dock_affinity_enabled,
        )),
        float_drag_mouseup_cancel: Arc::new(AtomicBool::new(true)),
        float_drag_mouseup_hook: Mutex::new(None),
        project_path: Mutex::new(None),
        undo_manager: Mutex::new(crate::undo::UndoManager::new()),
    };

    // Wrap in Arc for sharing between Tauri state and worker threads
    let state = Arc::new(app_state);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_os::init())
        .manage(state.clone())
        .on_window_event(|window, event| {
            let label = window.label().to_string();
            let is_panel = label.starts_with("panel-");

            match event {
                WindowEvent::Moved(_) if is_panel => {
                    let app_handle = window.app_handle().clone();
                    let state = app_handle.state::<Arc<AppState>>();
                    if let (Ok(pos), Ok(size), Ok(scale)) =
                        (window.outer_position(), window.outer_size(), window.scale_factor())
                    {
                        let logical = dock_affinity::Rect {
                            x: pos.x as f64 / scale,
                            y: pos.y as f64 / scale,
                            width: size.width as f64 / scale,
                            height: size.height as f64 / scale,
                        };
                        panel_commands::handle_panel_moved(&app_handle, state.inner(), logical);
                    }
                }
                WindowEvent::CloseRequested { api, .. } => {
                    // Only intercept close on panel windows (label pattern: "panel-{id}")
                    if let Some(panel_id) = label.strip_prefix("panel-") {
                        let panel_id = panel_id.to_string();

                        // Prevent the default close — we'll dock the panel instead,
                        // which will close the window as part of dock logic.
                        api.prevent_close();

                        let app_handle = window.app_handle().clone();
                        let state = app_handle.state::<Arc<AppState>>();

                        // Drop any in-flight float-drag session.
                        state
                            .float_drag_mouseup_cancel
                            .store(true, Ordering::SeqCst);
                        if let Ok(mut ctrl) = state.dock_affinity.lock() {
                            let _ = ctrl.cancel();
                        }

                        // Save current window bounds before docking.
                        if let Ok(position) = window.outer_position() {
                            if let Ok(size) = window.inner_size() {
                                let mut pm = state.panel_manager.lock().unwrap();
                                let _ = pm.update_bounds(
                                    &panel_id,
                                    panel_manager::SavedBounds {
                                        x: position.x,
                                        y: position.y,
                                        width: size.width,
                                        height: size.height,
                                    },
                                );
                            }
                        }

                        // Dock the panel (sets docked=true, clears window_label).
                        let panels_snapshot = {
                            let mut pm = state.panel_manager.lock().unwrap();
                            let side = pm.remembered_dock_side(&panel_id);
                            let _ = pm.dock(&panel_id, side, usize::MAX);
                            pm.get_state_with_orders()
                        };

                        // Close the window now that dock logic is done.
                        let _ = window.destroy();

                        // Emit state change to all remaining windows.
                        let payload = panel_manager::SerializedPanelState {
                            panels: panels_snapshot.0,
                            left_order: panels_snapshot.1,
                            right_order: panels_snapshot.2,
                        };
                        let _ = app_handle.emit("panel-state-changed", payload);
                    }
                }
                _ => {}
            }
        })
        .setup(move |app| {
            let app_handle = app.handle().clone();

            // Set native titlebar color on macOS
            #[cfg(target_os = "macos")]
            {
                use cocoa::appkit::NSWindow;
                use cocoa::base::id;

                let main_window = app.get_webview_window("main").unwrap();
                let ns_window = main_window.ns_window().unwrap() as id;
                unsafe {
                    // #999999 → RGB (0.6, 0.6, 0.6)
                    let bg_color: id = msg_send![
                        class!(NSColor),
                        colorWithRed: 0.6f64
                        green: 0.6f64
                        blue: 0.6f64
                        alpha: 1.0f64
                    ];
                    ns_window.setBackgroundColor_(bg_color);
                }
            }

            // Load persisted panel state (if available) and replace the default.
            // v1 files migrate to a single-stack on `fallback_side` (legacy shell
            // sidebarSide is applied on the frontend in task 5; Rust defaults to right).
            let fallback_side = panel_manager::DockSide::Right;
            if let Some(loaded) =
                panel_persistence::load_panel_state(&app_handle, fallback_side)
            {
                let mut pm = state.panel_manager.lock().unwrap();
                *pm = PanelManager::from_persisted(
                    loaded.panels,
                    Some(loaded.left_order),
                    Some(loaded.right_order),
                );
            }

            // Restore floating windows for panels that were undocked at last exit.
            {
                let pm = state.panel_manager.lock().unwrap();
                let panels = pm.get_state();

                // Get monitor info for off-screen bounds correction.
                let (monitors, primary) = panel_commands::get_monitor_rects(&app_handle);

                for panel in &panels {
                    if !panel.docked && panel.visible {
                        let label = panel
                            .window_label
                            .clone()
                            .unwrap_or_else(|| format!("panel-{}", panel.id));
                        let url_path = format!("index.html?panel={}", panel.id);
                        let url = tauri::WebviewUrl::App(url_path.into());

                        // Correct bounds for off-screen positions (logical px / Retina-safe).
                        let bounds = panel_commands::resolve_undock_bounds(
                            &panel.id,
                            panel.saved_bounds.clone(),
                            &monitors,
                            primary.as_ref(),
                        );

                        let title = format!(
                            "Dither – {}",
                            match panel.id.as_str() {
                                "effect" => "Effect Settings",
                                "layers" => "Layers",
                                "colorlab" => "Color Lab",
                                "preview" => "Preview",
                                "preferences" => "Preferences",
                                _ => "Panel",
                            }
                        );

                        // All panels use custom titlebar with decorations disabled
                        // and Overlay title bar style (for macOS traffic lights).
                        let builder =
                            WebviewWindowBuilder::new(&app_handle, &label, url)
                                .title(&title)
                                .inner_size(bounds.width as f64, bounds.height as f64)
                                .position(bounds.x as f64, bounds.y as f64)
                                .resizable(true)
                                .decorations(false)
                                .title_bar_style(tauri::TitleBarStyle::Overlay)
                                .min_inner_size(280.0, 200.0);

                        if let Err(e) = builder.build() {
                            log::warn!(
                                "Failed to restore floating window for panel '{}': {}",
                                panel.id,
                                e
                            );
                        }
                    }
                }
            }

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
            commands::reorder_filter,
            
            // Image / document commands
            commands::load_image,
            commands::create_document,
            commands::export_image,
            commands::save_project,
            commands::save_project_as,
            commands::open_project,
            commands::export_pattern,
            commands::import_pattern,
            recent_files::get_recent_files,
            crate::undo::undo,
            crate::undo::redo,
            
            // Palette commands
            commands::list_palettes,
            commands::list_builtin_palettes,
            commands::import_builtin_palette,
            commands::generate_ramp_palette,
            commands::generate_harmony_palette,
            commands::colors_to_oklab,
            commands::get_palette_oklab,
            commands::import_palette,
            commands::add_palette,
            commands::generate_palette,
            commands::remove_palette,
            commands::rename_palette,
            commands::create_palette,
            commands::export_palette,
            commands::add_color_to_palette,
            commands::update_palette_color,
            commands::remove_palette_color,
            commands::reorder_palette_color,
            commands::delete_palette,
            
            // Selection commands
            commands::set_selection,
            commands::get_selection,
            
            // Viewport commands
            viewport::set_viewport,
            
            // Panel commands
            panel_commands::get_panels_state,
            panel_commands::undock_panel,
            panel_commands::undock_panel_with_size,
            panel_commands::dock_panel,
            panel_commands::hide_panel,
            panel_commands::show_panel,
            panel_commands::save_panel_bounds,
            panel_commands::reorder_sidebar,
            panel_commands::move_panel_to_side,
            panel_commands::move_all_panels_to_side,
            panel_commands::update_dock_zone,
            panel_commands::begin_float_drag,
            panel_commands::cancel_float_drag,
            panel_commands::dock_panel_at,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Run the application with event handling for exit/cleanup.
    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { .. } = &event {
            // Save full dual-sidebar panel state (panels + side orders) before exit.
            let state = app_handle.state::<Arc<AppState>>();
            let snapshot = {
                let pm = state.panel_manager.lock().unwrap();
                pm.serialize()
            };
            panel_persistence::save_panel_state(app_handle, &snapshot);

            // Close all floating panel windows.
            let windows = app_handle.webview_windows();
            for (label, win) in &windows {
                if label.starts_with("panel-") {
                    let _ = win.destroy();
                }
            }
        }
    });
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
    state.worker_wake.notify_one();

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
