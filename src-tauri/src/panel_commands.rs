//! Tauri command handlers for panel docking/undocking operations.
//!
//! Each command acquires the PanelManager mutex from AppState, performs the
//! requested operation, and emits a `panel-state-changed` event to all windows.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};
use tauri::webview::WebviewWindowBuilder;

use crate::commands::AppState;
use crate::panel_manager::{DockSide, PanelInfo, SavedBounds, SerializedPanelState};

// ============================================================================
// Monitor bounds correction
// ============================================================================

/// Abstracted monitor rectangle for bounds correction logic.
/// This is decoupled from Tauri's Monitor type to allow unit testing.
#[derive(Debug, Clone)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Check if a window rectangle intersects with a monitor rectangle.
/// Uses the standard AABB overlap test.
fn rect_intersects_monitor(bounds: &SavedBounds, monitor: &MonitorRect) -> bool {
    let win_right = bounds.x.saturating_add(bounds.width as i32);
    let win_bottom = bounds.y.saturating_add(bounds.height as i32);
    let mon_right = monitor.x.saturating_add(monitor.width as i32);
    let mon_bottom = monitor.y.saturating_add(monitor.height as i32);

    bounds.x < mon_right
        && win_right > monitor.x
        && bounds.y < mon_bottom
        && win_bottom > monitor.y
}

/// Correct saved bounds if the window falls entirely outside all available monitors.
///
/// - If the window rectangle intersects at least one monitor, returns bounds unchanged.
/// - If it doesn't intersect any monitor, repositions to the center of the primary
///   monitor while preserving the original width and height.
/// - `primary_monitor` is the monitor to center on if correction is needed.
///   If None, falls back to the first monitor in the list. If the list is empty,
///   returns bounds unchanged (no correction possible).
pub fn correct_bounds_for_monitors(
    bounds: &SavedBounds,
    monitors: &[MonitorRect],
    primary_monitor: Option<&MonitorRect>,
) -> SavedBounds {
    // If no monitors are available, we can't correct — return as-is.
    if monitors.is_empty() {
        return bounds.clone();
    }

    // Check if bounds intersects at least one monitor.
    let on_screen = monitors.iter().any(|m| rect_intersects_monitor(bounds, m));
    if on_screen {
        return bounds.clone();
    }

    // Off-screen: center on primary monitor (or first available).
    let target = primary_monitor.unwrap_or(&monitors[0]);
    let new_x = target.x + (target.width as i32 - bounds.width as i32) / 2;
    let new_y = target.y + (target.height as i32 - bounds.height as i32) / 2;

    SavedBounds {
        x: new_x,
        y: new_y,
        width: bounds.width,
        height: bounds.height,
    }
}

/// Query Tauri for all available monitors and the primary monitor, returning
/// them as **logical-pixel** `MonitorRect` values matching frontend-saved bounds.
pub fn get_monitor_rects(app_handle: &AppHandle) -> (Vec<MonitorRect>, Option<MonitorRect>) {
    let to_logical = |m: &tauri::Monitor| -> MonitorRect {
        let scale = m.scale_factor();
        let pos = m.position();
        let size = m.size();
        MonitorRect {
            x: (pos.x as f64 / scale).round() as i32,
            y: (pos.y as f64 / scale).round() as i32,
            width: (size.width as f64 / scale).round().max(1.0) as u32,
            height: (size.height as f64 / scale).round().max(1.0) as u32,
        }
    };

    let monitors: Vec<MonitorRect> = app_handle
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(to_logical)
        .collect();

    let primary = app_handle
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| to_logical(&m));

    (monitors, primary)
}

/// Default floating-window size per panel (logical px).
fn panel_default_size(id: &str) -> (u32, u32) {
    match id {
        "preferences" => (420, 360),
        "colorlab" => (560, 640),
        "preview" => (800, 600),
        "layers" => (350, 500),
        "effect" => (400, 600),
        _ => (332, 400),
    }
}

/// Hard cap so a bad saved/maximized size cannot open a panel at monitor size.
fn panel_max_size(id: &str) -> (u32, u32) {
    match id {
        "colorlab" => (640, 760),
        "preferences" => (560, 520),
        "layers" => (480, 800),
        "effect" => (520, 800),
        "preview" => (1600, 1200),
        _ => (900, 900),
    }
}

/// Max inner size for the OS window (logical px).
pub fn panel_max_inner_size(id: &str) -> (f64, f64) {
    let (w, h) = panel_max_size(id);
    (w as f64, h as f64)
}

/// Center a window of the given size on the primary (or first) monitor.
fn centered_bounds(
    width: u32,
    height: u32,
    monitors: &[MonitorRect],
    primary: Option<&MonitorRect>,
) -> SavedBounds {
    let target = primary.or_else(|| monitors.first());
    match target {
        Some(t) => {
            let w = width.min(t.width.saturating_sub(40).max(280));
            let h = height.min(t.height.saturating_sub(40).max(200));
            SavedBounds {
                x: t.x + (t.width as i32 - w as i32) / 2,
                y: t.y + (t.height as i32 - h as i32) / 2,
                width: w,
                height: h,
            }
        }
        None => SavedBounds {
            x: 80,
            y: 80,
            width,
            height,
        },
    }
}

/// Clamp size to the target monitor and ensure the window stays fully visible
/// when possible (after off-screen correction).
fn clamp_bounds_to_monitor(
    panel_id: &str,
    bounds: &SavedBounds,
    monitor: &MonitorRect,
) -> SavedBounds {
    let (cap_w, cap_h) = panel_max_size(panel_id);
    let max_w = monitor.width.saturating_sub(40).max(280).min(cap_w);
    let max_h = monitor.height.saturating_sub(80).max(200).min(cap_h);
    let width = bounds.width.min(max_w).max(280);
    let height = bounds.height.min(max_h).max(200);
    let max_x = monitor.x + monitor.width as i32 - width as i32;
    let max_y = monitor.y + monitor.height as i32 - height as i32;
    SavedBounds {
        x: bounds.x.clamp(monitor.x, max_x.max(monitor.x)),
        y: bounds.y.clamp(monitor.y, max_y.max(monitor.y)),
        width,
        height,
    }
}

/// Focus an already-floating panel; if it's off-screen (e.g. Retina mismatch),
/// pull it back onto the primary monitor.
fn focus_floating_panel(app_handle: &AppHandle, panel_id: &str, window_label: &str) {
    let Some(win) = app_handle.get_webview_window(window_label) else {
        return;
    };
    let (monitors, primary) = get_monitor_rects(app_handle);
    if let (Ok(pos), Ok(size), Ok(scale)) =
        (win.outer_position(), win.outer_size(), win.scale_factor())
    {
        let logical = SavedBounds {
            x: (pos.x as f64 / scale).round() as i32,
            y: (pos.y as f64 / scale).round() as i32,
            width: (size.width as f64 / scale).round().max(1.0) as u32,
            height: (size.height as f64 / scale).round().max(1.0) as u32,
        };
        let fixed = resolve_undock_bounds(panel_id, Some(logical.clone()), &monitors, primary.as_ref());
        if fixed.x != logical.x
            || fixed.y != logical.y
            || fixed.width != logical.width
            || fixed.height != logical.height
        {
            let _ = win.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
                fixed.width as f64,
                fixed.height as f64,
            )));
            let _ = win.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
                fixed.x as f64,
                fixed.y as f64,
            )));
        }
    }
    let _ = win.unminimize();
    let _ = win.set_focus();
}

/// Resolve final window bounds for undock: correct off-screen, clamp to monitor,
/// or center with panel defaults when nothing was saved.
pub fn resolve_undock_bounds(
    panel_id: &str,
    saved: Option<SavedBounds>,
    monitors: &[MonitorRect],
    primary: Option<&MonitorRect>,
) -> SavedBounds {
    let (default_w, default_h) = panel_default_size(panel_id);
    let target = primary.or_else(|| monitors.first());

    let raw = match saved {
        Some(b) => correct_bounds_for_monitors(&b, monitors, primary),
        None => centered_bounds(default_w, default_h, monitors, primary),
    };

    match target {
        Some(m) => clamp_bounds_to_monitor(panel_id, &raw, m),
        None => {
            let (cap_w, cap_h) = panel_max_size(panel_id);
            SavedBounds {
                x: raw.x,
                y: raw.y,
                width: raw.width.min(cap_w).max(280),
                height: raw.height.min(cap_h).max(200),
            }
        }
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Map a panel ID to its user-facing display name.
fn panel_display_name(id: &str) -> &str {
    match id {
        "effect" => "Effect Settings",
        "layers" => "Layers",
        "colorlab" => "Color Lab",
        "preview" => "Preview",
        "preferences" => "Preferences",
        _ => "Panel",
    }
}

/// Emit the `panel-state-changed` event with a full dual-sidebar snapshot.
fn emit_panel_state(
    app_handle: &AppHandle,
    panels: Vec<PanelInfo>,
    left_order: Vec<String>,
    right_order: Vec<String>,
) {
    // Fire-and-forget: if emit fails (e.g. no listeners), we silently ignore.
    let payload = SerializedPanelState {
        panels,
        left_order,
        right_order,
    };
    let _ = app_handle.emit("panel-state-changed", payload);
}

fn parse_dock_side(side: &str) -> Result<DockSide, String> {
    match side {
        "left" => Ok(DockSide::Left),
        "right" => Ok(DockSide::Right),
        other => Err(format!("Invalid dock side: {other}")),
    }
}

// ============================================================================
// Commands
// ============================================================================

/// Get full dual-sidebar panel snapshot.
#[tauri::command]
pub fn get_panels_state(state: State<Arc<AppState>>) -> Result<SerializedPanelState, String> {
    let pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
    Ok(pm.serialize())
}

/// Undock a panel into a floating window.
#[tauri::command]
pub fn undock_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let (result, panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        let result = pm.undock(&panel_id).map_err(|e| e.to_string())?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        (result, snapshot, left, right)
    };

    if result.already_floating {
        focus_floating_panel(&app_handle, &panel_id, &result.window_label);
        return Ok(());
    }

    // Resolve position/size in logical px (Retina-safe) — always place on-screen.
    let (monitors, primary) = get_monitor_rects(&app_handle);
    let bounds = resolve_undock_bounds(
        &panel_id,
        result.bounds,
        &monitors,
        primary.as_ref(),
    );

    let title = format!("Dither – {}", panel_display_name(&panel_id));
    let url = tauri::WebviewUrl::App(result.url.into());

    // All panels use custom titlebar with decorations disabled
    // and Overlay title bar style (for macOS traffic lights).
    let builder = WebviewWindowBuilder::new(&app_handle, &result.window_label, url)
        .title(&title)
        .inner_size(bounds.width as f64, bounds.height as f64)
        .position(bounds.x as f64, bounds.y as f64)
        .resizable(true)
        .decorations(false)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .min_inner_size(280.0, 200.0);
    let (max_w, max_h) = panel_max_inner_size(&panel_id);
    let builder = builder.max_inner_size(max_w, max_h);

    let revert_side = result.previous_dock_side.unwrap_or(DockSide::Right);
    builder.build().map_err(|e| {
        // Revert the undock in panel state since window creation failed.
        let mut pm = state.panel_manager.lock().unwrap();
        let _ = pm.dock(&panel_id, revert_side, usize::MAX);
        format!("Window creation failed: {}", e)
    })?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

/// Dock a floating panel back into the sidebar.
/// Uses the panel's last dock side when remembered; otherwise defaults to `right`.
#[tauri::command]
pub fn dock_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let (old_label, panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        let side = pm.remembered_dock_side(&panel_id);
        let old_label = pm
            .dock(&panel_id, side, usize::MAX)
            .map_err(|e| e.to_string())?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        (old_label, snapshot, left, right)
    };

    // Close the floating window if one existed.
    if let Some(label) = old_label {
        if let Some(win) = app_handle.get_webview_window(&label) {
            let _ = win.close();
        }
        emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    }
    // If old_label is None, the panel was already docked — no-op, no event.

    Ok(())
}

/// Hide a panel without destroying it.
#[tauri::command]
pub fn hide_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let (changed, window_label, panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        let changed = pm.hide(&panel_id).map_err(|e| e.to_string())?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        // Grab window_label to check if we need to hide the OS window.
        let window_label = snapshot
            .iter()
            .find(|p| p.id == panel_id)
            .and_then(|p| p.window_label.clone());
        (changed, window_label, snapshot, left, right)
    };

    if changed {
        // If panel is floating, hide the OS window.
        if let Some(label) = window_label {
            if let Some(win) = app_handle.get_webview_window(&label) {
                let _ = win.hide();
            }
        }
        emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    }

    Ok(())
}

/// Show a hidden panel.
#[tauri::command]
pub fn show_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let (changed, window_label, panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        let changed = pm.show(&panel_id).map_err(|e| e.to_string())?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        let window_label = snapshot
            .iter()
            .find(|p| p.id == panel_id)
            .and_then(|p| p.window_label.clone());
        (changed, window_label, snapshot, left, right)
    };

    if changed {
        // If panel is floating, show the OS window.
        if let Some(label) = window_label {
            if let Some(win) = app_handle.get_webview_window(&label) {
                let _ = win.show();
            }
        }
        emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    }

    Ok(())
}

/// Reorder docked panels within one sidebar.
/// `side` is `"left"` or `"right"`; `order` must be a permutation of that side's members.
#[tauri::command]
pub fn reorder_sidebar(
    side: String,
    order: Vec<String>,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let dock_side = parse_dock_side(&side)?;
    let (panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        pm.reorder_side(dock_side, order)
            .map_err(|e| e.to_string())?;
        pm.get_state_with_orders()
    };

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

/// Move a docked panel to another sidebar (or same side at a new index).
/// `insert_index` defaults to append when omitted / null.
#[tauri::command]
pub fn move_panel_to_side(
    panel_id: String,
    side: String,
    insert_index: Option<usize>,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let dock_side = parse_dock_side(&side)?;
    let (panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        let index = insert_index.unwrap_or(usize::MAX);
        pm.move_to_side(&panel_id, dock_side, index)
            .map_err(|e| e.to_string())?;
        pm.get_state_with_orders()
    };

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

/// Move all currently docked dockable panels onto one side (single-stack).
#[tauri::command]
pub fn move_all_panels_to_side(
    side: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let dock_side = parse_dock_side(&side)?;
    let (panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        pm.move_all_to_side(dock_side)
            .map_err(|e| e.to_string())?;
        pm.get_state_with_orders()
    };

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

/// Undock a panel into a floating window with explicit size and position.
/// Used by drag-to-undock where the frontend provides the measured panel dimensions
/// and the cursor's screen coordinates at release.
#[tauri::command]
pub fn undock_panel_with_size(
    panel_id: String,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let (result, panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        let result = pm.undock(&panel_id).map_err(|e| e.to_string())?;
        let (snapshot, left, right) = pm.get_state_with_orders();
        (result, snapshot, left, right)
    };

    if result.already_floating {
        focus_floating_panel(&app_handle, &panel_id, &result.window_label);
        return Ok(());
    }

    // Apply off-screen correction + clamp using the provided position and size.
    let provided_bounds = SavedBounds { x, y, width, height };
    let (monitors, primary) = get_monitor_rects(&app_handle);
    let corrected_bounds =
        resolve_undock_bounds(&panel_id, Some(provided_bounds), &monitors, primary.as_ref());

    let title = format!("Dither – {}", panel_display_name(&panel_id));
    let url = tauri::WebviewUrl::App(result.url.into());

    // All panels use custom titlebar with decorations disabled
    // and Overlay title bar style (for macOS traffic lights).
    let builder = WebviewWindowBuilder::new(&app_handle, &result.window_label, url)
        .title(&title)
        .inner_size(corrected_bounds.width as f64, corrected_bounds.height as f64)
        .position(corrected_bounds.x as f64, corrected_bounds.y as f64)
        .resizable(true)
        .decorations(false)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .min_inner_size(280.0, 200.0);
    let (max_w, max_h) = panel_max_inner_size(&panel_id);
    let builder = builder.max_inner_size(max_w, max_h);

    let revert_side = result.previous_dock_side.unwrap_or(DockSide::Right);
    builder.build().map_err(|e| {
        // Revert the undock in panel state since window creation failed.
        let mut pm = state.panel_manager.lock().unwrap();
        let _ = pm.dock(&panel_id, revert_side, usize::MAX);
        format!("Window creation failed: {}", e)
    })?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

/// Save panel bounds (called from frontend on window move/resize).
/// This is a silent position save — no event is emitted.
#[tauri::command]
pub fn save_panel_bounds(
    panel_id: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
    pm.update_bounds(&panel_id, SavedBounds { x, y, width, height })
        .map_err(|e| e.to_string())
}

// ============================================================================
// Dock Affinity
// ============================================================================

use crate::dock_affinity::{DockAffinityEvent, DockZone, SidebarSide};
use crate::global_mouseup;
use std::sync::atomic::Ordering;

fn emit_dock_affinity(app_handle: &AppHandle, event: &DockAffinityEvent) {
    eprintln!(
        "[dock-affinity] emit panel={} armed={} insert={:?} side={:?}",
        event.panel_id, event.armed, event.insert_index, event.side
    );
    let _ = app_handle.emit("dock-affinity", event);
}

fn sidebar_side_to_dock(side: SidebarSide) -> DockSide {
    match side {
        SidebarSide::Left => DockSide::Left,
        SidebarSide::Right => DockSide::Right,
    }
}

fn parse_sidebar_side(side: &str) -> Result<SidebarSide, String> {
    match side {
        "left" => Ok(SidebarSide::Left),
        "right" => Ok(SidebarSide::Right),
        other => Err(format!("Invalid dock side: {other}")),
    }
}

/// Main window reports a sidebar dock zone (+ slot midpoints) for one side.
/// Pass `zone: null` to clear that side's zone.
#[tauri::command]
pub fn update_dock_zone(
    side: String,
    zone: Option<DockZone>,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let sidebar_side = parse_sidebar_side(&side)?;
    let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
    match &zone {
        Some(z) => eprintln!(
            "[dock-affinity] zone side={:?} x={:.0} y={:.0} w={:.0} h={:.0} slots={}",
            sidebar_side,
            z.x,
            z.y,
            z.width,
            z.height,
            z.slots.len()
        ),
        None => eprintln!("[dock-affinity] zone cleared side={:?}", sidebar_side),
    }
    ctrl.set_dock_zone(sidebar_side, zone);
    Ok(())
}

/// Atomic dock + insert at index among docked+visible panels on `side`.
#[tauri::command]
pub fn dock_panel_at(
    panel_id: String,
    side: String,
    insert_index: usize,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let dock_side = parse_dock_side(&side)?;
    dock_panel_at_inner(&panel_id, dock_side, insert_index, &app_handle, state.inner())
}

fn dock_panel_at_inner(
    panel_id: &str,
    side: DockSide,
    index: usize,
    app_handle: &AppHandle,
    state: &Arc<AppState>,
) -> Result<(), String> {
    let (old_label, panels_snapshot, left_order, right_order) = {
        let mut pm = state.panel_manager.lock().map_err(|e| e.to_string())?;
        let already_docked = pm
            .get_state()
            .iter()
            .find(|p| p.id == panel_id)
            .map(|p| p.docked)
            .unwrap_or(false);
        let old_label = if already_docked {
            pm.move_to_dock_insert_index(panel_id, side, index)
                .map_err(|e| e.to_string())?;
            None
        } else {
            pm.dock(panel_id, side, index).map_err(|e| e.to_string())?
        };
        let (snapshot, left, right) = pm.get_state_with_orders();
        (old_label, snapshot, left, right)
    };

    if let Some(label) = old_label {
        if let Some(win) = app_handle.get_webview_window(&label) {
            let _ = win.close();
        }
    }
    emit_panel_state(app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

/// Begin a float-drag session (call before `startDragging`).
#[tauri::command]
pub fn begin_float_drag(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    // Tear down any previous hook/session (removeMonitor must be on main).
    {
        let mut hook = state.float_drag_mouseup_hook.lock().map_err(|e| e.to_string())?;
        if let Some(h) = hook.take() {
            let app = app_handle.clone();
            let _ = app.run_on_main_thread(move || h.cancel());
        }
    }
    state
        .float_drag_mouseup_cancel
        .store(true, Ordering::SeqCst);

    let started = {
        let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
        if !ctrl.enabled {
            log::warn!("begin_float_drag: dock affinity disabled");
            return Ok(());
        }
        ctrl.begin(&panel_id)
    };

    if !started {
        log::debug!("begin_float_drag: rejected for panel '{}'", panel_id);
        return Ok(());
    }

    if !global_mouseup::mouseup_backend_available() {
        let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
        ctrl.enabled = false;
        if let Some(ev) = ctrl.cancel() {
            emit_dock_affinity(&app_handle, &ev);
        }
        log::warn!("dock affinity disabled: no mouseup backend on this platform");
        return Ok(());
    }

    // Ensure we have a dock zone even if the JS reporter hasn't flushed yet.
    ensure_fallback_dock_zone(&app_handle, &state);
    // Ask main window to re-report precise left/right zones (overrides fallbacks).
    let _ = app_handle.emit("dock-zones-refresh", ());

    state
        .float_drag_mouseup_cancel
        .store(false, Ordering::SeqCst);

    let app_handle_watch = app_handle.clone();
    let state_arc: Arc<AppState> = Arc::clone(state.inner());
    let panel_label_tick = format!("panel-{}", panel_id);

    eprintln!("[dock-affinity] begin session for '{}'", panel_id);

    let app_for_install = app_handle.clone();
    let hook = global_mouseup::install_left_mouseup_hook(
        move |install_fn| {
            let _ = app_for_install.run_on_main_thread(move || {
                install_fn();
            });
        },
        {
            let app = app_handle_watch.clone();
            let state = state_arc.clone();
            move || {
                eprintln!("[dock-affinity] NSEvent mouseup");
                // MUST defer: removeMonitor inside the NSEvent callback crashes/hangs.
                let app2 = app.clone();
                let state2 = state.clone();
                let _ = app.run_on_main_thread(move || {
                    if let Some((pid, _, _, _)) = state2
                        .dock_affinity
                        .lock()
                        .ok()
                        .and_then(|c| c.session_snapshot())
                    {
                        poll_panel_affinity(&app2, &state2, &format!("panel-{}", pid));
                    }
                    complete_float_drag(&app2, &state2);
                });
            }
        },
    );

    // Position polling for affinity while the hook is alive.
    let tick_cancel = hook.cancel_flag();
    global_mouseup::spawn_tick_loop(tick_cancel.clone(), {
        let app = app_handle_watch.clone();
        let state = state_arc.clone();
        move || {
            poll_panel_affinity(&app, &state, &panel_label_tick);
        }
    });

    // HID backup if NSEvent mouseUp is swallowed after OS window drag.
    global_mouseup::spawn_hid_mouseup_backup(tick_cancel, {
        let app = app_handle_watch;
        let state = state_arc;
        move || {
            eprintln!("[dock-affinity] HID backup mouseup");
            if let Some((pid, _, _, _)) = state
                .dock_affinity
                .lock()
                .ok()
                .and_then(|c| c.session_snapshot())
            {
                poll_panel_affinity(&app, &state, &format!("panel-{}", pid));
            }
            let app2 = app.clone();
            let state2 = state.clone();
            if let Err(e) = app.run_on_main_thread(move || {
                complete_float_drag(&app2, &state2);
            }) {
                eprintln!("[dock-affinity] run_on_main_thread failed: {e}");
                complete_float_drag(&app, &state);
            }
        }
    });

    *state
        .float_drag_mouseup_hook
        .lock()
        .map_err(|e| e.to_string())? = Some(hook);

    Ok(())
}

/// Ensure each dock side has a zone. Frontend reporters are preferred; when a
/// side is missing (empty column, HMR, or reporter lag), derive an edge strip
/// from the main window so float→dock still works.
fn ensure_fallback_dock_zone(app_handle: &AppHandle, state: &AppState) {
    let mut ctrl = match state.dock_affinity.lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    let need_left = !ctrl.zones.contains_key(&SidebarSide::Left);
    let need_right = !ctrl.zones.contains_key(&SidebarSide::Right);
    if !need_left && !need_right {
        eprintln!(
            "[dock-affinity] zones ok left={:?} right={:?}",
            ctrl.zones.get(&SidebarSide::Left).map(|z| (z.x, z.y, z.width, z.height, z.slots.len())),
            ctrl.zones.get(&SidebarSide::Right).map(|z| (z.x, z.y, z.width, z.height, z.slots.len())),
        );
        return;
    }
    let Some(main) = app_handle.get_webview_window("main") else {
        return;
    };
    let Ok(pos) = main.outer_position() else {
        return;
    };
    let Ok(size) = main.outer_size() else {
        return;
    };
    let Ok(scale) = main.scale_factor() else {
        return;
    };
    let x = pos.x as f64 / scale;
    let y = pos.y as f64 / scale;
    let w = size.width as f64 / scale;
    let h = size.height as f64 / scale;
    // Thin magnet strip — side-aligned float titlebar probe can hit this.
    let edge = 96.0_f64.min(w * 0.2).max(64.0);

    if need_left {
        let zone = crate::dock_affinity::DockZone {
            x,
            y,
            width: edge,
            height: h,
            scale_factor: scale,
            side: SidebarSide::Left,
            slots: vec![],
        };
        eprintln!(
            "[dock-affinity] fallback LEFT zone x={:.0} y={:.0} w={:.0} h={:.0}",
            zone.x, zone.y, zone.width, zone.height
        );
        ctrl.set_dock_zone(SidebarSide::Left, Some(zone));
    }

    if need_right {
        // Wider strip when right was never reported (classic single-stack UX).
        let strip = 320.0_f64.min(w * 0.4).max(edge);
        let zone = crate::dock_affinity::DockZone {
            x: x + w - strip,
            y,
            width: strip,
            height: h,
            scale_factor: scale,
            side: SidebarSide::Right,
            slots: vec![],
        };
        eprintln!(
            "[dock-affinity] fallback RIGHT zone x={:.0} y={:.0} w={:.0} h={:.0}",
            zone.x, zone.y, zone.width, zone.height
        );
        ctrl.set_dock_zone(SidebarSide::Right, Some(zone));
    }
}

fn poll_panel_affinity(app_handle: &AppHandle, state: &Arc<AppState>, window_label: &str) {
    let Some(win) = app_handle.get_webview_window(window_label) else {
        return;
    };
    let Ok(pos) = win.outer_position() else {
        return;
    };
    let Ok(size) = win.outer_size() else {
        return;
    };
    let Ok(scale) = win.scale_factor() else {
        return;
    };
    let logical = crate::dock_affinity::Rect {
        x: pos.x as f64 / scale,
        y: pos.y as f64 / scale,
        width: size.width as f64 / scale,
        height: size.height as f64 / scale,
    };
    handle_panel_moved(app_handle, state, logical);
}

/// Cancel an in-progress float-drag session (Escape / unmount).
#[tauri::command]
pub fn cancel_float_drag(
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    state
        .float_drag_mouseup_cancel
        .store(true, Ordering::SeqCst);
    if let Ok(mut hook) = state.float_drag_mouseup_hook.lock() {
        if let Some(h) = hook.take() {
            h.cancel();
        }
    }
    let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
    if let Some(ev) = ctrl.cancel() {
        emit_dock_affinity(&app_handle, &ev);
    }
    Ok(())
}

/// Complete gesture on mouseup: dock if armed, always end session.
pub fn complete_float_drag(app_handle: &AppHandle, state: &Arc<AppState>) {
    state
        .float_drag_mouseup_cancel
        .store(true, Ordering::SeqCst);

    // Take the hook but delay removeMonitor — never call it re-entrantly from
    // an NSEvent monitor callback (even via run_on_main_thread sync paths).
    let pending_hook = state
        .float_drag_mouseup_hook
        .lock()
        .ok()
        .and_then(|mut h| h.take());

    let snapshot = {
        let ctrl = match state.dock_affinity.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        ctrl.session_snapshot()
    };

    let Some((panel_id, armed, insert_index, armed_side)) = snapshot else {
        log::debug!("complete_float_drag: no active session");
        if let Some(h) = pending_hook {
            defer_hook_cancel(app_handle, h);
        }
        return;
    };

    eprintln!(
        "[dock-affinity] complete panel='{}' armed={} insert={} side={:?}",
        panel_id, armed, insert_index, armed_side
    );

    if armed {
        let side = armed_side
            .map(sidebar_side_to_dock)
            .unwrap_or(DockSide::Right);
        if let Err(e) = dock_panel_at_inner(&panel_id, side, insert_index, app_handle, state) {
            log::warn!("dock_panel_at failed during affinity release: {}", e);
            let _ = app_handle.emit(
                "panel-error",
                format!("Failed to dock panel: {}", e),
            );
        }
    }

    let end_ev = {
        let mut ctrl = match state.dock_affinity.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        ctrl.end_session()
    };
    if let Some(ev) = end_ev {
        emit_dock_affinity(app_handle, &ev);
    }

    if let Some(h) = pending_hook {
        defer_hook_cancel(app_handle, h);
    }
}

fn defer_hook_cancel(app_handle: &AppHandle, hook: global_mouseup::MouseUpHook) {
    let app = app_handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(16));
        let _ = app.run_on_main_thread(move || {
            hook.cancel();
        });
    });
}

/// Feed a floating panel outer rect (logical px) into the affinity controller.
pub fn handle_panel_moved(
    app_handle: &AppHandle,
    state: &Arc<AppState>,
    logical: crate::dock_affinity::Rect,
) {
    let event = {
        let mut ctrl = match state.dock_affinity.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        if ctrl.session.is_none() {
            return;
        }
        ctrl.on_moved(logical)
    };
    if let Some(ev) = event {
        emit_dock_affinity(app_handle, &ev);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_monitor(x: i32, y: i32, width: u32, height: u32) -> MonitorRect {
        MonitorRect { x, y, width, height }
    }

    #[test]
    fn bounds_on_screen_returned_unchanged() {
        let bounds = SavedBounds { x: 100, y: 100, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, 100);
        assert_eq!(result.y, 100);
        assert_eq!(result.width, 400);
        assert_eq!(result.height, 600);
    }

    #[test]
    fn bounds_off_screen_right_gets_centered() {
        // Window is far to the right, beyond any monitor
        let bounds = SavedBounds { x: 5000, y: 100, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        // Centered: x = 0 + (1920 - 400) / 2 = 760
        //           y = 0 + (1080 - 600) / 2 = 240
        assert_eq!(result.x, 760);
        assert_eq!(result.y, 240);
        assert_eq!(result.width, 400);
        assert_eq!(result.height, 600);
    }

    #[test]
    fn bounds_off_screen_left_gets_centered() {
        // Window is far to the left
        let bounds = SavedBounds { x: -5000, y: -3000, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, 760);
        assert_eq!(result.y, 240);
        assert_eq!(result.width, 400);
        assert_eq!(result.height, 600);
    }

    #[test]
    fn bounds_partially_on_screen_returned_unchanged() {
        // Window is partially off-screen but still overlaps with the monitor
        let bounds = SavedBounds { x: 1800, y: 900, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        // Still intersects the monitor (1800 < 1920 and 1800+400 > 0)
        assert_eq!(result.x, 1800);
        assert_eq!(result.y, 900);
    }

    #[test]
    fn multi_monitor_on_second_screen_unchanged() {
        // Window on second monitor (positioned to the right)
        let bounds = SavedBounds { x: 2000, y: 100, width: 400, height: 600 };
        let monitors = vec![
            make_monitor(0, 0, 1920, 1080),
            make_monitor(1920, 0, 2560, 1440),
        ];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        // Intersects the second monitor — unchanged
        assert_eq!(result.x, 2000);
        assert_eq!(result.y, 100);
    }

    #[test]
    fn multi_monitor_off_all_screens_centers_on_primary() {
        // Window is off both monitors
        let bounds = SavedBounds { x: 10000, y: 5000, width: 400, height: 600 };
        let monitors = vec![
            make_monitor(0, 0, 1920, 1080),
            make_monitor(1920, 0, 2560, 1440),
        ];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        // Centers on primary (monitor[0]): x = (1920-400)/2 = 760, y = (1080-600)/2 = 240
        assert_eq!(result.x, 760);
        assert_eq!(result.y, 240);
        assert_eq!(result.width, 400);
        assert_eq!(result.height, 600);
    }

    #[test]
    fn no_monitors_returns_bounds_unchanged() {
        let bounds = SavedBounds { x: 5000, y: 5000, width: 400, height: 600 };
        let monitors: Vec<MonitorRect> = vec![];

        let result = correct_bounds_for_monitors(&bounds, &monitors, None);
        assert_eq!(result.x, 5000);
        assert_eq!(result.y, 5000);
    }

    #[test]
    fn no_primary_falls_back_to_first_monitor() {
        let bounds = SavedBounds { x: 10000, y: 10000, width: 300, height: 500 };
        let monitors = vec![make_monitor(0, 0, 2560, 1440)];

        let result = correct_bounds_for_monitors(&bounds, &monitors, None);
        // Centers on first monitor: x = (2560-300)/2 = 1130, y = (1440-500)/2 = 470
        assert_eq!(result.x, 1130);
        assert_eq!(result.y, 470);
        assert_eq!(result.width, 300);
        assert_eq!(result.height, 500);
    }

    #[test]
    fn preserves_width_and_height_when_correcting() {
        let bounds = SavedBounds { x: -9999, y: -9999, width: 450, height: 700 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.width, 450);
        assert_eq!(result.height, 700);
    }

    #[test]
    fn window_exactly_touching_monitor_edge_is_on_screen() {
        // Window right edge touches monitor left edge: they share a boundary
        // According to our intersection test: x < mx + mw && x + w > mx
        // bounds.x = -400, bounds.x + bounds.width = 0, monitor.x = 0
        // So: -400 < 1920 (true) && 0 > 0 (false) → NOT intersecting
        let bounds = SavedBounds { x: -400, y: 0, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        // Not intersecting (just touching edge), so it gets corrected
        assert_eq!(result.x, 760);
        assert_eq!(result.y, 240);
    }

    #[test]
    fn colorlab_huge_saved_bounds_are_capped() {
        let bounds = SavedBounds {
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
        };
        let monitors = vec![make_monitor(0, 0, 2560, 1440)];
        let primary = monitors[0].clone();
        let result = resolve_undock_bounds("colorlab", Some(bounds), &monitors, Some(&primary));
        assert!(result.width <= 640, "width {}", result.width);
        assert!(result.height <= 760, "height {}", result.height);
    }

    #[test]
    fn window_one_pixel_overlap_stays_on_screen() {
        // Window overlaps by 1 pixel
        let bounds = SavedBounds { x: -399, y: 0, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        // -399 < 1920 (true) && (-399 + 400 = 1) > 0 (true) → intersecting
        assert_eq!(result.x, -399);
        assert_eq!(result.y, 0);
    }
}
