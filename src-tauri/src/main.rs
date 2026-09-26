#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]
// Legacy cocoa/objc until objc2 migration — deprecation + macro cfg noise.
#![cfg_attr(target_os = "macos", allow(deprecated, unexpected_cfgs))]

mod commands;
mod dock_affinity;
mod document_session;
mod file_log;
mod flexlayout_persistence;
mod gpu_resident_shadow;
mod ipc_guard;
mod journal;
#[cfg(target_os = "macos")]
mod macos_first_mouse;
#[cfg(target_os = "macos")]
mod macos_title;
mod memory_budget;
mod native_menu;
#[cfg(test)]
mod preview_latency_diag;
mod recent_files;
mod services;
mod state;
mod tile_pipeline;
mod tile_protocol;
mod tile_serve;
mod undo;
mod viewport;
mod webview_debug;
mod worker;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use commands::AppState;
use engine_project::layer::LayerNode;
use engine_tiles::{Priority, RecomputeTask, TileCoord, TileKey, TILE_SIZE};
#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};
use tauri::webview::{NewWindowResponse, WebviewWindowBuilder};
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tile_protocol::{parse_tile_url, LayerTarget};

fn main() {
    // Capture Rust-side failures (GPU init, tile://, panics) to
    // `%APPDATA%\com.dither.app\logs\app.log` (or the macOS/Linux equivalent)
    // before any window exists.
    file_log::init();

    // B4c: affinity enabled on all platforms — Flex popout completes via JS mouseup,
    // not global_mouseup (which was unavailable on Linux).
    let dock_affinity_enabled = true;

    // Track D: optional GPU. Force-CPU via DITHER_FORCE_CPU=1; prefer via DITHER_GPU=1
    // (runtime or compile-time). Finder-launched .app has no shell env.
    let gpu = if engine_gpu::force_cpu() {
        eprintln!("[engine-gpu] DITHER_FORCE_CPU set — skipping adapter init (T0.1 CPU-only path)");
        log::info!("engine-gpu: DITHER_FORCE_CPU set — skipping adapter init (T0.1 CPU-only path)");
        None
    } else {
        match engine_gpu::GpuContext::try_new_blocking() {
            Some(ctx) => {
                eprintln!(
                    "[engine-gpu] device ready filters={} preview={} resident={}",
                    engine_gpu::gpu_filters_enabled(),
                    engine_gpu::gpu_preview_enabled(),
                    engine_gpu::gpu_resident_enabled()
                );
                log::info!(
                    "engine-gpu: device ready (filters={}, preview={}, resident={})",
                    engine_gpu::gpu_filters_enabled(),
                    engine_gpu::gpu_preview_enabled(),
                    engine_gpu::gpu_resident_enabled()
                );
                Some(std::sync::Arc::new(ctx))
            }
            None => {
                eprintln!("[engine-gpu] no adapter — CPU-only filters");
                log::warn!("engine-gpu: no adapter — CPU-only filters");
                None
            }
        }
    };

    // Path B: resident VRAM cache is created inside empty_process when adapter exists.
    // Track C Phase 1: RAM budget is 25% of system RAM, clamped to 512 MiB … 4 GiB.
    let ram = memory_budget::resolve_ram_budget();
    eprintln!(
        "[tile-cache] ram_budget_mib={} source={:?} total_system_ram_mib={}",
        ram.bytes / (1024 * 1024),
        ram.source,
        ram.total_system_ram / (1024 * 1024)
    );
    log::info!(
        "tile-cache: ram_budget_mib={} source={:?} total_system_ram_mib={}",
        ram.bytes / (1024 * 1024),
        ram.source,
        ram.total_system_ram / (1024 * 1024)
    );
    let mut app_state = AppState::empty_process(gpu, ram.bytes, dock_affinity_enabled);
    app_state.ram_budget_source = ram.source;

    // Wrap in Arc for sharing between Tauri state and worker threads
    let state = Arc::new(app_state);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state.clone())
        .manage(Arc::new(commands::QuitGuard {
            allow_exit: AtomicBool::new(false),
        }))
        .on_menu_event(|app, event| {
            native_menu::emit_event(app, &event);
        })
        .on_window_event(|window, event| {
            let label = window.label().to_string();
            let is_flex_popout = label.starts_with("flex-popout-");

            match event {
                WindowEvent::Resized(_) => {
                    #[cfg(target_os = "macos")]
                    {
                        // Photoshop-style: never stay in Mission Control fullscreen.
                        if window.is_fullscreen().unwrap_or(false) {
                            let _ = window.set_fullscreen(false);
                            let _ = window.maximize();
                        }
                        // Do NOT refresh traffic lights here — NSWindowDidResize
                        // already re-pins every frame. A delayed second layout
                        // after live resize was a major source of the jump.
                    }
                }
                WindowEvent::Moved(_) if is_flex_popout => {
                    let app_handle = window.app_handle().clone();
                    let state = app_handle.state::<Arc<AppState>>();
                    if let (Ok(pos), Ok(size), Ok(scale)) = (
                        window.outer_position(),
                        window.outer_size(),
                        window.scale_factor(),
                    ) {
                        let logical = dock_affinity::Rect {
                            x: pos.x as f64 / scale,
                            y: pos.y as f64 / scale,
                            width: size.width as f64 / scale,
                            height: size.height as f64 / scale,
                        };
                        commands::panels::handle_panel_moved(&app_handle, state.inner(), logical);
                    }
                }
                _ => {}
            }
        })
        .setup(move |app| {
            // Deliver first-click events to every WKWebView even when its
            // window is inactive. Without this, macOS swallows the activation
            // click and users must double-click to add a layer/effect once
            // Preview has floated into its own OS window and stolen focus.
            // See `macos_first_mouse.rs` for the full explanation.
            #[cfg(target_os = "macos")]
            macos_first_mouse::install_accepts_first_mouse_override();

            // Main window is create:false in tauri.conf — build here so we can
            // allow FlexLayout's window.open() popouts (denied by default in Tauri).
            let main_conf = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .expect("main window config missing");
            // FlexLayout popouts via window.open — create Color Lab–style windows
            // (frameless + Overlay), not default decorated OS chrome.
            // FlexLayout's screen rect (screenX + layout rect) is often wrong in
            // WKWebView/Retina; clamp like Color Lab undock so the window is visible.
            let app_for_popout = app.handle().clone();
            static FLEX_POPOUT_SEQ: AtomicU64 = AtomicU64::new(1);
            let main_window = webview_debug::apply(WebviewWindowBuilder::from_config(app, &main_conf)?)
                .on_new_window(move |url, features| {
                    let n = FLEX_POPOUT_SEQ.fetch_add(1, Ordering::Relaxed);
                    let label = format!("flex-popout-{n}");

                    let req_pos = features.position();
                    let req_size = features.size();

                    let builder = webview_debug::apply(
                        WebviewWindowBuilder::new(
                            &app_for_popout,
                            &label,
                            tauri::WebviewUrl::External(url),
                        )
                        .window_features(features)
                        .title("Dither")
                        .resizable(true)
                        .decorations(false)
                        .min_inner_size(280.0, 200.0),
                    );
                    #[cfg(target_os = "macos")]
                    let builder = builder.title_bar_style(tauri::TitleBarStyle::Overlay);

                    match builder.build() {
                        Ok(window) => {
                            let (monitors, primary) =
                                commands::panels::get_monitor_rects(&app_for_popout);

                            let width = req_size
                                .map(|s| s.width.round().max(280.0) as u32)
                                .unwrap_or(350);
                            let height = req_size
                                .map(|s| s.height.round().max(200.0) as u32)
                                .unwrap_or(500);

                            let (x, y) = match req_pos {
                                Some(p) => (p.x.round() as i32, p.y.round() as i32),
                                None => {
                                    // Fall back beside the main window.
                                    if let Some(main) = app_for_popout.get_webview_window("main") {
                                        if let (Ok(pos), Ok(scale)) =
                                            (main.outer_position(), main.scale_factor())
                                        {
                                            let lx = (pos.x as f64 / scale).round() as i32 + 40;
                                            let ly = (pos.y as f64 / scale).round() as i32 + 60;
                                            (lx, ly)
                                        } else {
                                            (80, 80)
                                        }
                                    } else {
                                        (80, 80)
                                    }
                                }
                            };

                            let raw = commands::panels::SavedBounds {
                                x,
                                y,
                                width,
                                height,
                            };
                            let fixed = commands::panels::resolve_undock_bounds(
                                "layers",
                                Some(raw),
                                &monitors,
                                primary.as_ref(),
                            );

                            let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                                fixed.width as f64,
                                fixed.height as f64,
                            )));
                            let _ = window.set_position(tauri::Position::Logical(
                                tauri::LogicalPosition::new(fixed.x as f64, fixed.y as f64),
                            ));
                            let _ = window.set_focus();

                            #[cfg(target_os = "macos")]
                            {
                                macos_title::apply_overlay_csd(&window);
                                if let Ok(ns_window) = window.ns_window() {
                                    use cocoa::appkit::NSWindow;
                                    use cocoa::base::id;
                                    let ns_window = ns_window as id;
                                    unsafe {
                                        let bg_color: id = msg_send![
                                            class!(NSColor),
                                            colorWithRed: (0xCD as f64) / 255.0
                                            green: (0xCD as f64) / 255.0
                                            blue: (0xCD as f64) / 255.0
                                            alpha: 1.0f64
                                        ];
                                        ns_window.setBackgroundColor_(bg_color);
                                    }
                                }
                            }
                            NewWindowResponse::Create { window }
                        }
                        Err(err) => {
                            eprintln!("[flex-popout] window create failed: {err}");
                            NewWindowResponse::Deny
                        }
                    }
                })
                .build()?;

            native_menu::install(app)?;
            let app_handle = app.handle().clone();
            if let Ok(mut slot) = state.app_handle.lock() {
                *slot = Some(app_handle.clone());
            }

            // Initialize FlexLayout persistence with real app_data_dir (B3)
            if let Ok(app_data_dir) = app_handle.path().app_data_dir() {
                let new_persistence =
                    crate::flexlayout_persistence::FlexLayoutPersistence::new(app_data_dir.clone());
                if let Ok(mut persistence_slot) = state.flexlayout_persistence.lock() {
                    *persistence_slot = new_persistence;
                }
                // B4a: per-side persistence
                let left_persistence =
                    crate::flexlayout_persistence::FlexLayoutPersistence::with_filename(
                        app_data_dir.clone(),
                        "flexlayout_left.json",
                    );
                let right_persistence =
                    crate::flexlayout_persistence::FlexLayoutPersistence::with_filename(
                        app_data_dir.clone(),
                        "flexlayout_right.json",
                    );
                let center_persistence =
                    crate::flexlayout_persistence::FlexLayoutPersistence::with_filename(
                        app_data_dir.clone(),
                        "flexlayout_center.json",
                    );
                if let Ok(mut slot) = state.flexlayout_left.lock() {
                    *slot = left_persistence;
                }
                if let Ok(mut slot) = state.flexlayout_right.lock() {
                    *slot = right_persistence;
                }
                if let Ok(mut slot) = state.flexlayout_center.lock() {
                    *slot = center_persistence;
                }

                // Crash-recovery journal root: {app_data}/recovery/
                crate::journal::set_recovery_dir(
                    &state,
                    crate::journal::recovery_subdir(&app_data_dir),
                );
                crate::journal::start_heartbeat(app_handle.clone());
                crate::journal::signals::install_signal_flush(app_handle.clone());
            }

            // Set native titlebar color on macOS
            #[cfg(target_os = "macos")]
            {
                use cocoa::appkit::NSWindow;
                use cocoa::base::id;

                // decorations stay true so traffic lights exist; Overlay+fullSize
                // puts them on the same row as File/Edit (not a second native strip).
                macos_title::apply_overlay_csd(&main_window);
                let ns_window = main_window.ns_window().unwrap() as id;
                unsafe {
                    // Match --color-gray (#CDCDCD) so the Overlay strip isn't a darker band.
                    let bg_color: id = msg_send![
                        class!(NSColor),
                        colorWithRed: (0xCD as f64) / 255.0
                        green: (0xCD as f64) / 255.0
                        blue: (0xCD as f64) / 255.0
                        alpha: 1.0f64
                    ];
                    ns_window.setBackgroundColor_(bg_color);
                }
            }

            #[cfg(not(target_os = "macos"))]
            {
                let _ = main_window.set_decorations(false);
            }

            // One-shot: drop leftover PanelManager `panel_state.json` (Flex owns layout).
            if let Ok(dir) = app_handle.path().app_data_dir() {
                let legacy = dir.join("panel_state.json");
                if legacy.exists() {
                    match std::fs::remove_file(&legacy) {
                        Ok(()) => log::info!("Removed legacy panel_state.json"),
                        Err(e) => log::warn!("Failed to remove legacy panel_state.json: {e}"),
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
            handle_tile_request(&state, request)
        })
        .invoke_handler(tauri::generate_handler![
            // Document commands
                    commands::allow_app_exit,
                    commands::confirm_app_quit,
                    crate::journal::commands::scan_recovery_journals,
                    crate::journal::commands::recover_journal,
                    crate::journal::commands::discard_recovery_journals,
                    crate::journal::commands::prepare_soft_discard,
            commands::new_document,
            commands::get_document_snapshot,
            commands::list_open_documents,
            commands::set_active_document,
            commands::close_document,
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
            commands::get_algorithm_schema,
            commands::list_algorithms_for_category,
            // Image / document commands
            commands::load_image,
            commands::create_document,
            commands::import_image_layer,
            commands::export_image,
            commands::export_ascii,
            commands::ascii_clipboard_text,
            commands::set_ascii_preview,
            commands::get_ascii_preview,
            commands::save_project,
            commands::save_project_as,
            commands::share_project_copy,
            commands::open_project,
            commands::export_pattern,
            commands::import_pattern,
            recent_files::get_recent_files,
            recent_files::clear_recent_files,
            commands::undo::undo,
            commands::undo::redo,
            commands::undo::is_document_dirty,
            commands::is_release_build,
            commands::get_gpu_vram_status,
            commands::get_tile_cache_status,
            // Palette commands
            commands::list_palettes,
            commands::list_builtin_palettes,
            commands::import_builtin_palette,
            commands::generate_ramp_palette,
            commands::generate_harmony_palette,
            commands::auto_interpolate_would_change,
            commands::auto_interpolate_palette,
            commands::colors_to_oklab,
            commands::get_palette_oklab,
            commands::import_palette,
            commands::add_palette,
            commands::replace_palette,
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
            commands::viewport::set_viewport,
            // Flex float-drag / dock affinity
            commands::panels::update_dock_zone,
            commands::panels::begin_float_drag,
            commands::panels::cancel_float_drag,
            commands::panels::complete_float_drag,
            // FlexLayout commands
            commands::flexlayout::save_layout,
            commands::flexlayout::load_layout,
            commands::flexlayout::reset_layout_to_default,
            // B4a: per-side FlexLayout commands
            commands::flexlayout::load_layout_left,
            commands::flexlayout::save_layout_left,
            commands::flexlayout::load_layout_right,
            commands::flexlayout::save_layout_right,
            commands::flexlayout::load_layout_center,
            commands::flexlayout::save_layout_center,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Run the application with event handling for exit/cleanup.
    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = &event {
            let gate = app_handle.state::<Arc<commands::QuitGuard>>();
            if !gate.allow_exit.load(Ordering::SeqCst) {
                api.prevent_exit();
                let _ = app_handle.emit("app-quit-requested", ());
            }
        }
    });
}

/// Build an HTTP response with CORS headers allowing any origin.
/// Required because in dev mode the webview origin is http://localhost:5173
/// and the browser enforces CORS on custom protocol fetches.
fn tile_response(
    status: u16,
    content_type: &str,
    body: Vec<u8>,
    generation: Option<u64>,
    extra: &[(&str, &str)],
) -> http::Response<Vec<u8>> {
    let mut builder = http::Response::builder()
        .status(status)
        .header(http::header::CONTENT_TYPE, content_type)
        .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(
            http::header::CACHE_CONTROL,
            "no-store, no-cache, must-revalidate",
        )
        .header(http::header::PRAGMA, "no-cache");
    if let Some(gen) = generation {
        builder = builder.header("X-Tile-Generation", gen.to_string());
    }
    for (name, value) in extra {
        builder = builder.header(*name, *value);
    }
    builder.body(body).unwrap()
}

/// Handle a tile:// protocol request.
///
/// Returns:
/// - 200 + RGBA8 if cached and clean, dirty-stale, or pyramid fallback
/// - 202 + empty body if nothing to show (schedules Immediate)
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
            log::warn!("tile:// parse error for {uri}: {e}");
            let msg = format!("400 Bad Request: {}", e);
            return tile_response(400, "text/plain", msg.into_bytes(), None, &[]);
        }
    };

    // 2. Validate document session (never substitute the active tab)
    let session = match state.session(parsed.doc_id) {
        Ok(s) => s,
        Err(_) => {
            log::warn!(
                "tile:// document {} not found (uri={uri})",
                parsed.doc_id
            );
            let msg = format!("404 Not Found: document {} not found", parsed.doc_id);
            return tile_response(404, "text/plain", msg.into_bytes(), None, &[]);
        }
    };
    let snapshot = session.document_handle.snapshot();

    // 3. Validate layer
    let layer_id = match parsed.layer {
        LayerTarget::Id(id) => {
            if !layer_exists(&snapshot.root, id) {
                let msg = format!("404 Not Found: layer {} not found", id);
                return tile_response(404, "text/plain", msg.into_bytes(), None, &[]);
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
    let grid_cols = doc_width.div_ceil(tile_size_at_level);
    let grid_rows = doc_height.div_ceil(tile_size_at_level);

    if parsed.x >= grid_cols || parsed.y >= grid_rows {
        let msg = format!(
            "404 Not Found: tile coordinate ({}, {}) out of bounds for grid {}x{} at level {}",
            parsed.x, parsed.y, grid_cols, grid_rows, parsed.level
        );
        return tile_response(404, "text/plain", msg.into_bytes(), None, &[]);
    }

    // 5. Build TileKey and check cache. Ready = !dirty && generation >= doc_gen.
    let key = TileKey {
        doc: parsed.doc_id,
        layer: layer_id,
        coord: TileCoord {
            level: parsed.level,
            x: parsed.x,
            y: parsed.y,
        },
        stage: parsed.stage,
    };

    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    let served = tile_serve::resolve_preview_tile(&state.tiles.tile_cache, key, doc_gen);
    let ready = matches!(served, tile_serve::PreviewServe::Fresh { .. });
    if !ready {
        let layer_gen = snapshot.generations.get_layer_gen(layer_id);
        let task = RecomputeTask {
            key,
            generation: doc_gen,
            layer_generation: layer_gen,
            priority: Priority::Immediate,
        };
        state.tiles.scheduler.enqueue_dedup(task);
        state.worker_wake.notify_one();
    }

    match served {
        tile_serve::PreviewServe::Fresh { rgba8, generation } => tile_response(
            200,
            "application/octet-stream",
            rgba8,
            Some(generation),
            &[],
        ),
        tile_serve::PreviewServe::Stale { rgba8, generation } => tile_response(
            200,
            "application/octet-stream",
            rgba8,
            Some(generation),
            &[("X-Dither-Tile-Stale", "1")],
        ),
        tile_serve::PreviewServe::PyramidFallback { rgba8 } => tile_response(
            200,
            "application/octet-stream",
            rgba8,
            None,
            &[("X-Dither-Tile-Fallback", "pyramid")],
        ),
        tile_serve::PreviewServe::Pending => {
            tile_response(202, "application/octet-stream", Vec::new(), None, &[])
        }
    }
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
