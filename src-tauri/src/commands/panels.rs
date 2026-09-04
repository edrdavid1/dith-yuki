//! Tauri command handlers for panel docking/undocking operations.
//!
//! # IMPORTANT: B4c — Flex redock without global_mouseup
//!
//! Layers / Effect / Color Lab float as OS `flex-popout-*` windows. Titlebar drag
//! is JS `setPosition` + in-WebView `mouseup` → `complete_float_drag` (no OS
//! `startDragging`, no platform mouseup hook). Affinity hit-test stays in
//! `dock_affinity` (zones from the main window; positions via `WindowEvent::Moved`).
//!
//! Preview / Preferences still use PanelManager + OS `startDragging` for move-only
//! (FLOATING_ONLY — affinity never arms).
//!
//! ## Commands still used
//! DO NOT remove Preview/PanelManager undock/dock paths here.
//!
//! Float-drag (Flex): `begin_float_drag` / `cancel_float_drag` / `complete_float_drag`
//! + `update_dock_zone`. Hit-test: `dock_affinity.rs` (no `global_mouseup`).

use std::sync::Arc;

use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::dock_affinity::{DockAffinityEvent, DockZone, SidebarSide};
use crate::panel_manager::{DockSide, PanelInfo, SavedBounds, SerializedPanelState};
use crate::services::PanelService;

// ============================================================================
// Monitor bounds correction
// ============================================================================

/// Abstracted monitor rectangle for bounds correction logic.
#[derive(Debug, Clone)]
pub struct MonitorRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

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

pub fn correct_bounds_for_monitors(
    bounds: &SavedBounds,
    monitors: &[MonitorRect],
    primary_monitor: Option<&MonitorRect>,
) -> SavedBounds {
    if monitors.is_empty() {
        return bounds.clone();
    }

    let on_screen = monitors.iter().any(|m| rect_intersects_monitor(bounds, m));
    if on_screen {
        return bounds.clone();
    }

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

pub fn panel_max_inner_size(id: &str) -> (f64, f64) {
    let (w, h) = panel_max_size(id);
    (w as f64, h as f64)
}

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

fn emit_panel_state(
    app_handle: &AppHandle,
    panels: Vec<PanelInfo>,
    left_order: Vec<String>,
    right_order: Vec<String>,
) {
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

#[tauri::command]
pub fn get_panels_state(state: State<Arc<AppState>>) -> Result<SerializedPanelState, String> {
    PanelService::new(state.inner().clone())
        .get_panels_state()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn undock_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let service = PanelService::new(state.inner().clone());
    let (result, panels_snapshot, left_order, right_order) =
        service.undock(&panel_id).map_err(|e| e.to_string())?;

    if result.already_floating {
        focus_floating_panel(&app_handle, &panel_id, &result.window_label);
        return Ok(());
    }

    let (monitors, primary) = get_monitor_rects(&app_handle);
    let bounds = resolve_undock_bounds(
        &panel_id,
        result.bounds,
        &monitors,
        primary.as_ref(),
    );

    let title = format!("Dither – {}", panel_display_name(&panel_id));
    let url = tauri::WebviewUrl::App(result.url.into());

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
        let _ = service.dock_at(&panel_id, revert_side, usize::MAX);
        format!("Window creation failed: {}", e)
    })?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

#[tauri::command]
pub fn dock_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let service = PanelService::new(state.inner().clone());
    let (old_label, panels_snapshot, left_order, right_order) =
        service.dock(&panel_id).map_err(|e| e.to_string())?;

    if let Some(label) = old_label {
        if let Some(win) = app_handle.get_webview_window(&label) {
            let _ = win.close();
        }
        emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    }

    Ok(())
}

#[tauri::command]
pub fn hide_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let service = PanelService::new(state.inner().clone());
    let (changed, window_label, panels_snapshot, left_order, right_order) =
        service.hide(&panel_id).map_err(|e| e.to_string())?;

    if changed {
        if let Some(label) = window_label {
            if let Some(win) = app_handle.get_webview_window(&label) {
                let _ = win.hide();
            }
        }
        emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    }

    Ok(())
}

#[tauri::command]
pub fn show_panel(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let service = PanelService::new(state.inner().clone());
    let (changed, window_label, panels_snapshot, left_order, right_order) =
        service.show(&panel_id).map_err(|e| e.to_string())?;

    if changed {
        if let Some(label) = window_label {
            if let Some(win) = app_handle.get_webview_window(&label) {
                let _ = win.show();
            }
        }
        emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    }

    Ok(())
}

#[tauri::command]
pub fn reorder_sidebar(
    side: String,
    order: Vec<String>,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let dock_side = parse_dock_side(&side)?;
    let service = PanelService::new(state.inner().clone());
    let (panels_snapshot, left_order, right_order) =
        service.reorder_side(dock_side, order).map_err(|e| e.to_string())?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

#[tauri::command]
pub fn move_panel_to_side(
    panel_id: String,
    side: String,
    insert_index: Option<usize>,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let dock_side = parse_dock_side(&side)?;
    let index = insert_index.unwrap_or(usize::MAX);
    let service = PanelService::new(state.inner().clone());
    let (panels_snapshot, left_order, right_order) = service
        .move_to_side(&panel_id, dock_side, index)
        .map_err(|e| e.to_string())?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

#[tauri::command]
pub fn move_all_panels_to_side(
    side: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let dock_side = parse_dock_side(&side)?;
    let service = PanelService::new(state.inner().clone());
    let (panels_snapshot, left_order, right_order) =
        service.move_all_to_side(dock_side).map_err(|e| e.to_string())?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

#[tauri::command]
pub fn swap_sidebars(
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let service = PanelService::new(state.inner().clone());
    let (panels_snapshot, left_order, right_order) =
        service.swap_sidebars().map_err(|e| e.to_string())?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

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
    let service = PanelService::new(state.inner().clone());
    let (result, panels_snapshot, left_order, right_order) =
        service.undock(&panel_id).map_err(|e| e.to_string())?;

    if result.already_floating {
        focus_floating_panel(&app_handle, &panel_id, &result.window_label);
        return Ok(());
    }

    let provided_bounds = SavedBounds { x, y, width, height };
    let (monitors, primary) = get_monitor_rects(&app_handle);
    let corrected_bounds =
        resolve_undock_bounds(&panel_id, Some(provided_bounds), &monitors, primary.as_ref());

    let title = format!("Dither – {}", panel_display_name(&panel_id));
    let url = tauri::WebviewUrl::App(result.url.into());

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
        let _ = service.dock_at(&panel_id, revert_side, usize::MAX);
        format!("Window creation failed: {}", e)
    })?;

    emit_panel_state(&app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

#[tauri::command]
pub fn save_panel_bounds(
    panel_id: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    PanelService::new(state.inner().clone())
        .save_bounds(&panel_id, SavedBounds { x, y, width, height })
        .map_err(|e| e.to_string())
}

// ============================================================================
// Dock Affinity
// ============================================================================

fn emit_dock_affinity(app_handle: &AppHandle, event: &DockAffinityEvent) {
    let _ = app_handle.emit("dock-affinity", event);
}

fn sidebar_side_to_dock(side: SidebarSide) -> DockSide {
    match side {
        SidebarSide::Left => DockSide::Left,
        SidebarSide::Right => DockSide::Right,
    }
}

/// Layers + Effect + Color Lab live in FlexLayout — redock must not go through PanelManager.
fn is_flex_layout_panel(panel_id: &str) -> bool {
    matches!(panel_id, "layers" | "effect" | "colorlab")
}

/// Color Lab uses `panel-{id}`; FlexLayout popouts use `flex-popout-N`.
fn resolve_float_window(
    app_handle: &AppHandle,
    panel_id: &str,
) -> Option<tauri::WebviewWindow> {
    let panel_label = format!("panel-{}", panel_id);
    if let Some(win) = app_handle.get_webview_window(&panel_label) {
        return Some(win);
    }
    let windows = app_handle.webview_windows();
    let mut flex: Vec<_> = windows
        .into_iter()
        .filter(|(label, _)| label.starts_with("flex-popout-"))
        .map(|(_, w)| w)
        .collect();
    if flex.is_empty() {
        return None;
    }
    if let Some(focused) = flex.iter().find(|w| w.is_focused().unwrap_or(false)) {
        return Some(focused.clone());
    }
    flex.pop()
}

fn poll_panel_affinity(app_handle: &AppHandle, state: &Arc<AppState>, panel_id: &str) {
    let Some(win) = resolve_float_window(app_handle, panel_id) else {
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

fn parse_sidebar_side(side: &str) -> Result<SidebarSide, String> {
    match side {
        "left" => Ok(SidebarSide::Left),
        "right" => Ok(SidebarSide::Right),
        other => Err(format!("Invalid dock side: {other}")),
    }
}

#[tauri::command]
pub fn update_dock_zone(
    side: String,
    zone: Option<DockZone>,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let sidebar_side = parse_sidebar_side(&side)?;
    let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
    ctrl.set_dock_zone(sidebar_side, zone);
    Ok(())
}

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
    let service = PanelService::new(state.clone());
    let (old_label, panels_snapshot, left_order, right_order) =
        service.dock_at(panel_id, side, index).map_err(|e| e.to_string())?;

    if let Some(label) = old_label {
        if let Some(win) = app_handle.get_webview_window(&label) {
            let _ = win.close();
        }
    }
    emit_panel_state(app_handle, panels_snapshot, left_order, right_order);
    Ok(())
}

#[tauri::command]
pub fn begin_float_drag(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    // Cancel any prior session (JS drag restart / Escape).
    {
        let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
        if let Some(ev) = ctrl.cancel() {
            emit_dock_affinity(&app_handle, &ev);
        }
    }

    let started = {
        let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
        if !ctrl.enabled {
            return Ok(());
        }
        ctrl.begin(&panel_id)
    };

    if !started {
        return Ok(());
    }

    ensure_fallback_dock_zone(&app_handle, &state);
    let _ = app_handle.emit("dock-zones-refresh", ());
    Ok(())
}

fn ensure_fallback_dock_zone(app_handle: &AppHandle, state: &AppState) {
    let mut ctrl = match state.dock_affinity.lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    let need_left = !ctrl.zones.contains_key(&SidebarSide::Left);
    let need_right = !ctrl.zones.contains_key(&SidebarSide::Right);
    if !need_left && !need_right {
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
        ctrl.set_dock_zone(SidebarSide::Left, Some(zone));
    }

    if need_right {
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
        ctrl.set_dock_zone(SidebarSide::Right, Some(zone));
    }
}

#[tauri::command]
pub fn cancel_float_drag(
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
    if let Some(ev) = ctrl.cancel() {
        emit_dock_affinity(&app_handle, &ev);
    }
    Ok(())
}

/// Finish JS-driven float drag: poll last rect, then dock if armed.
#[tauri::command]
pub fn complete_float_drag(
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
    if let Some((pid, _, _, _)) = state
        .dock_affinity
        .lock()
        .ok()
        .and_then(|c| c.session_snapshot())
    {
        poll_panel_affinity(&app_handle, state.inner(), &pid);
    }
    finish_float_drag(&app_handle, state.inner());
    Ok(())
}

pub fn finish_float_drag(app_handle: &AppHandle, state: &Arc<AppState>) {
    let snapshot = {
        let ctrl = match state.dock_affinity.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        ctrl.session_snapshot()
    };

    let Some((panel_id, armed, insert_index, armed_side)) = snapshot else {
        return;
    };

    if armed {
        let side = armed_side
            .map(sidebar_side_to_dock)
            .unwrap_or(DockSide::Right);
        if is_flex_layout_panel(&panel_id) {
            // Frontend LayoutContext unfloats / cross-moves; do not touch PanelManager.
            let side_str = match side {
                DockSide::Left => "left",
                DockSide::Right => "right",
            };
            let _ = app_handle.emit(
                "flex-panel-dock-request",
                serde_json::json!({
                    "panelId": panel_id,
                    "side": side_str,
                }),
            );
        } else if let Err(e) = dock_panel_at_inner(&panel_id, side, insert_index, app_handle, state)
        {
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
}

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
        let bounds = SavedBounds { x: 5000, y: 100, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, 760);
        assert_eq!(result.y, 240);
        assert_eq!(result.width, 400);
        assert_eq!(result.height, 600);
    }

    #[test]
    fn bounds_off_screen_left_gets_centered() {
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
        let bounds = SavedBounds { x: 1800, y: 900, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, 1800);
        assert_eq!(result.y, 900);
    }

    #[test]
    fn multi_monitor_on_second_screen_unchanged() {
        let bounds = SavedBounds { x: 2000, y: 100, width: 400, height: 600 };
        let monitors = vec![
            make_monitor(0, 0, 1920, 1080),
            make_monitor(1920, 0, 2560, 1440),
        ];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, 2000);
        assert_eq!(result.y, 100);
    }

    #[test]
    fn multi_monitor_off_all_screens_centers_on_primary() {
        let bounds = SavedBounds { x: 10000, y: 5000, width: 400, height: 600 };
        let monitors = vec![
            make_monitor(0, 0, 1920, 1080),
            make_monitor(1920, 0, 2560, 1440),
        ];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
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
        let bounds = SavedBounds { x: -400, y: 0, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
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
        let bounds = SavedBounds { x: -399, y: 0, width: 400, height: 600 };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, -399);
        assert_eq!(result.y, 0);
    }
}
