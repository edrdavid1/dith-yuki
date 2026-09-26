//! FlexLayout float-drag / dock-affinity commands + monitor bounds helpers.
//!
//! Layers / Effect / Color Lab / Preview float as OS `flex-popout-*` windows.
//! Titlebar drag is JS `setPosition` + in-WebView `mouseup` → `complete_float_drag`
//! (no OS `startDragging`, no platform mouseup hook). Affinity hit-test stays in
//! `dock_affinity` (zones from the main window; positions via `WindowEvent::Moved`).
//!
//! Preferences and Help are main-window dialogs — not dock panels and not float windows.
//!
//! Float-drag: `begin_float_drag` / `cancel_float_drag` / `complete_float_drag`
//! + `update_dock_zone`. Hit-test: `dock_affinity.rs`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::AppState;
use crate::dock_affinity::{DockAffinityEvent, DockZone, SidebarSide};

// ============================================================================
// Monitor bounds correction (Flex popout placement)
// ============================================================================

/// Logical window bounds used when placing / correcting Flex popouts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SavedBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

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

    bounds.x < mon_right && win_right > monitor.x && bounds.y < mon_bottom && win_bottom > monitor.y
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
        "layers" => (480, 800),
        "effect" => (520, 800),
        "preview" => (1600, 1200),
        _ => (900, 900),
    }
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
// Dock Affinity / float-drag
// ============================================================================

fn emit_dock_affinity(app_handle: &AppHandle, event: &DockAffinityEvent) {
    let _ = app_handle.emit("dock-affinity", event);
}

fn is_flex_layout_panel(panel_id: &str) -> bool {
    matches!(panel_id, "layers" | "effect" | "colorlab" | "preview")
}

/// FlexLayout popouts use `flex-popout-N`.
fn resolve_float_window(app_handle: &AppHandle, _panel_id: &str) -> Option<tauri::WebviewWindow> {
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
pub fn begin_float_drag(
    panel_id: String,
    app_handle: AppHandle,
    state: State<Arc<AppState>>,
) -> Result<(), String> {
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
pub fn cancel_float_drag(app_handle: AppHandle, state: State<Arc<AppState>>) -> Result<(), String> {
    let mut ctrl = state.dock_affinity.lock().map_err(|e| e.to_string())?;
    if let Some(ev) = ctrl.cancel() {
        emit_dock_affinity(&app_handle, &ev);
    }
    Ok(())
}

/// Finish JS-driven float drag: poll last rect, then request Flex redock if armed.
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

    let Some((panel_id, armed, _insert_index, armed_side)) = snapshot else {
        return;
    };

    if armed && is_flex_layout_panel(&panel_id) {
        let side_str = match armed_side.unwrap_or(SidebarSide::Right) {
            SidebarSide::Left => "left",
            SidebarSide::Right => "right",
        };
        let _ = app_handle.emit(
            "flex-panel-dock-request",
            serde_json::json!({
                "panelId": panel_id,
                "side": side_str,
            }),
        );
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
        MonitorRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn bounds_on_screen_returned_unchanged() {
        let bounds = SavedBounds {
            x: 100,
            y: 100,
            width: 400,
            height: 600,
        };
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
        let bounds = SavedBounds {
            x: 5000,
            y: 100,
            width: 400,
            height: 600,
        };
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
        let bounds = SavedBounds {
            x: -5000,
            y: -3000,
            width: 400,
            height: 600,
        };
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
        let bounds = SavedBounds {
            x: 1800,
            y: 900,
            width: 400,
            height: 600,
        };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, 1800);
        assert_eq!(result.y, 900);
    }

    #[test]
    fn multi_monitor_on_second_screen_unchanged() {
        let bounds = SavedBounds {
            x: 2000,
            y: 100,
            width: 400,
            height: 600,
        };
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
        let bounds = SavedBounds {
            x: 10000,
            y: 5000,
            width: 400,
            height: 600,
        };
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
        let bounds = SavedBounds {
            x: 5000,
            y: 5000,
            width: 400,
            height: 600,
        };
        let monitors: Vec<MonitorRect> = vec![];

        let result = correct_bounds_for_monitors(&bounds, &monitors, None);
        assert_eq!(result.x, 5000);
        assert_eq!(result.y, 5000);
    }

    #[test]
    fn no_primary_falls_back_to_first_monitor() {
        let bounds = SavedBounds {
            x: 10000,
            y: 10000,
            width: 300,
            height: 500,
        };
        let monitors = vec![make_monitor(0, 0, 2560, 1440)];

        let result = correct_bounds_for_monitors(&bounds, &monitors, None);
        assert_eq!(result.x, 1130);
        assert_eq!(result.y, 470);
        assert_eq!(result.width, 300);
        assert_eq!(result.height, 500);
    }

    #[test]
    fn preserves_width_and_height_when_correcting() {
        let bounds = SavedBounds {
            x: -9999,
            y: -9999,
            width: 450,
            height: 700,
        };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.width, 450);
        assert_eq!(result.height, 700);
    }

    #[test]
    fn window_exactly_touching_monitor_edge_is_on_screen() {
        let bounds = SavedBounds {
            x: -400,
            y: 0,
            width: 400,
            height: 600,
        };
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
        let bounds = SavedBounds {
            x: -399,
            y: 0,
            width: 400,
            height: 600,
        };
        let monitors = vec![make_monitor(0, 0, 1920, 1080)];
        let primary = monitors[0].clone();

        let result = correct_bounds_for_monitors(&bounds, &monitors, Some(&primary));
        assert_eq!(result.x, -399);
        assert_eq!(result.y, 0);
    }
}
