//! Disk persistence for panel state (schema v2).
//!
//! Saves and loads panel configuration (docked/floating, visibility, window bounds,
//! dock_side, and per-side orders) to a JSON file in the app data directory.
//! Designed for graceful degradation: missing or corrupt files result in default
//! state; save failures are logged but never propagate errors to callers.
//!
//! v1 files (panels only, no sides/orders) are migrated on load using a fallback
//! dock side (typically from legacy shell `sidebarSide`, else `right`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Manager;

use crate::panel_manager::{DockSide, PanelInfo, SerializedPanelState};

/// Current schema version for the persisted panel state file.
const PANEL_STATE_VERSION: u32 = 2;

/// Dockable panel IDs in the legacy relative order (v1 had no persisted order).
const DOCKABLE_RELATIVE_ORDER: &[&str] = &["effect", "layers", "colorlab"];

/// Versioned wrapper for the persisted panel state JSON (v2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct PersistedPanelStateV2 {
    version: u32,
    panels: Vec<PanelInfo>,
    left_order: Vec<String>,
    right_order: Vec<String>,
}

/// Minimal v1 shape (panels only). `dock_side` may be absent in old files.
#[derive(Debug, Clone, Deserialize)]
struct PersistedPanelStateV1 {
    version: u32,
    panels: Vec<PanelInfo>,
}

/// Result of a successful load (already migrated to dual-sidebar shape).
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedPanelState {
    pub panels: Vec<PanelInfo>,
    pub left_order: Vec<String>,
    pub right_order: Vec<String>,
}

impl LoadedPanelState {
    pub fn into_serialized(self) -> SerializedPanelState {
        SerializedPanelState {
            panels: self.panels,
            left_order: self.left_order,
            right_order: self.right_order,
        }
    }
}

/// Get the path to the panel state file.
///
/// Located at: `{app_data_dir}/panel_state.json`
pub fn panel_state_path(app_handle: &tauri::AppHandle) -> PathBuf {
    let data_dir = app_handle
        .path()
        .app_data_dir()
        .expect("failed to resolve app data directory");
    data_dir.join("panel_state.json")
}

/// Load panel state from disk, migrating v1 → v2 when needed.
///
/// `fallback_side` is used when upgrading v1 files that have no `dock_side`:
/// all docked dockable panels are assigned to that side (legacy single-sidebar).
/// Pass the migrated shell `sidebarSide` when known; otherwise `DockSide::Right`.
///
/// Returns `None` if:
/// - The file does not exist (fresh install / first run)
/// - The file cannot be read (permissions, I/O error)
/// - The file contents cannot be parsed / unsupported version / corrupt v2
///
/// In all failure cases a warning is logged (except missing file, which is silent).
/// Callers should keep the default dual layout when this returns `None`.
pub fn load_panel_state(
    app_handle: &tauri::AppHandle,
    fallback_side: DockSide,
) -> Option<LoadedPanelState> {
    let path = panel_state_path(app_handle);

    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!(
                    "Failed to read panel state file at {}: {}",
                    path.display(),
                    e
                );
            }
            return None;
        }
    };

    match parse_panel_state_json(&contents, fallback_side) {
        Ok(loaded) => Some(loaded),
        Err(reason) => {
            log::warn!(
                "Failed to load panel state from {}: {}; using default dual layout",
                path.display(),
                reason
            );
            None
        }
    }
}

/// Parse persisted JSON into a dual-sidebar snapshot.
///
/// Pure function for unit tests (no filesystem).
pub fn parse_panel_state_json(
    contents: &str,
    fallback_side: DockSide,
) -> Result<LoadedPanelState, String> {
    let value: Value =
        serde_json::from_str(contents).map_err(|e| format!("invalid JSON: {e}"))?;

    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing version field".to_string())? as u32;

    match version {
        1 => {
            let v1: PersistedPanelStateV1 = serde_json::from_value(value)
                .map_err(|e| format!("v1 schema parse error: {e}"))?;
            Ok(migrate_v1_to_v2(v1.panels, fallback_side))
        }
        2 => {
            let v2: PersistedPanelStateV2 = serde_json::from_value(value)
                .map_err(|e| format!("v2 schema parse error: {e}"))?;
            if v2.version != 2 {
                return Err(format!("inconsistent version field {}", v2.version));
            }
            Ok(LoadedPanelState {
                panels: v2.panels,
                left_order: v2.left_order,
                right_order: v2.right_order,
            })
        }
        other => Err(format!(
            "unsupported panel state version {other} (expected 1 or 2)"
        )),
    }
}

/// Migrate v1 panels (no sides / orders) to a single-stack dual snapshot.
///
/// All docked dockable panels go to `fallback_side` in legacy relative order;
/// the opposite side is empty. Floating and floating-only panels stay out of orders.
pub fn migrate_v1_to_v2(mut panels: Vec<PanelInfo>, fallback_side: DockSide) -> LoadedPanelState {
    let mut side_order = Vec::new();

    for &id in DOCKABLE_RELATIVE_ORDER {
        if let Some(panel) = panels.iter_mut().find(|p| p.id == id) {
            if panel.docked {
                panel.dock_side = Some(fallback_side);
                side_order.push(id.to_string());
            } else {
                panel.dock_side = None;
            }
        }
    }

    // Clear dock_side on anything else (floating-only / unknown leftovers).
    for panel in &mut panels {
        if !DOCKABLE_RELATIVE_ORDER.contains(&panel.id.as_str()) {
            panel.dock_side = None;
        }
    }

    let (left_order, right_order) = match fallback_side {
        DockSide::Left => (side_order, Vec::new()),
        DockSide::Right => (Vec::new(), side_order),
    };

    LoadedPanelState {
        panels,
        left_order,
        right_order,
    }
}

/// Save full dual-sidebar panel state to disk (always writes schema v2).
///
/// Creates the app data directory if it does not exist.
/// On any failure a warning is logged but the error is not propagated.
pub fn save_panel_state(app_handle: &tauri::AppHandle, snapshot: &SerializedPanelState) {
    let path = panel_state_path(app_handle);

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!(
                "Failed to create panel state directory {}: {}",
                parent.display(),
                e
            );
            return;
        }
    }

    let json = match serialize_panel_state(snapshot) {
        Ok(j) => j,
        Err(e) => {
            log::warn!("Failed to serialize panel state: {}", e);
            return;
        }
    };

    if let Err(e) = std::fs::write(&path, json) {
        log::warn!(
            "Failed to write panel state file at {}: {}",
            path.display(),
            e
        );
    }
}

/// Serialize a snapshot to pretty v2 JSON (pure; for tests).
pub fn serialize_panel_state(snapshot: &SerializedPanelState) -> Result<String, String> {
    let state = PersistedPanelStateV2 {
        version: PANEL_STATE_VERSION,
        panels: snapshot.panels.clone(),
        left_order: snapshot.left_order.clone(),
        right_order: snapshot.right_order.clone(),
    };
    serde_json::to_string_pretty(&state).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel_manager::{PanelManager, SavedBounds};

    fn sample_v2_snapshot() -> SerializedPanelState {
        SerializedPanelState {
            panels: vec![
                PanelInfo {
                    id: "effect".to_string(),
                    docked: true,
                    visible: true,
                    window_label: None,
                    saved_bounds: None,
                    dock_side: Some(DockSide::Right),
                },
                PanelInfo {
                    id: "layers".to_string(),
                    docked: true,
                    visible: true,
                    window_label: None,
                    saved_bounds: None,
                    dock_side: Some(DockSide::Left),
                },
                PanelInfo {
                    id: "colorlab".to_string(),
                    docked: true,
                    visible: false,
                    window_label: None,
                    saved_bounds: Some(SavedBounds {
                        x: 500,
                        y: 200,
                        width: 450,
                        height: 700,
                    }),
                    dock_side: Some(DockSide::Right),
                },
            ],
            left_order: vec!["layers".into()],
            right_order: vec!["effect".into(), "colorlab".into()],
        }
    }

    #[test]
    fn v2_round_trip() {
        let snapshot = sample_v2_snapshot();
        let json = serialize_panel_state(&snapshot).unwrap();
        let loaded = parse_panel_state_json(&json, DockSide::Right).unwrap();
        assert_eq!(loaded.left_order, snapshot.left_order);
        assert_eq!(loaded.right_order, snapshot.right_order);
        assert_eq!(loaded.panels.len(), 3);
        assert_eq!(loaded.panels[1].dock_side, Some(DockSide::Left));
        assert!(!loaded.panels[2].visible);

        let pm = PanelManager::from_persisted(
            loaded.panels,
            Some(loaded.left_order),
            Some(loaded.right_order),
        );
        assert_eq!(pm.get_left_order(), &["layers"]);
        assert_eq!(pm.get_right_order(), &["effect", "colorlab"]);
    }

    #[test]
    fn v1_migrates_all_docked_to_fallback_right() {
        let v1 = r#"{
            "version": 1,
            "panels": [
                {
                    "id": "effect",
                    "docked": true,
                    "visible": true,
                    "window_label": null,
                    "saved_bounds": null
                },
                {
                    "id": "layers",
                    "docked": true,
                    "visible": false,
                    "window_label": null,
                    "saved_bounds": null
                },
                {
                    "id": "colorlab",
                    "docked": false,
                    "visible": true,
                    "window_label": "panel-colorlab",
                    "saved_bounds": { "x": 1, "y": 2, "width": 3, "height": 4 }
                }
            ]
        }"#;

        let loaded = parse_panel_state_json(v1, DockSide::Right).unwrap();
        assert!(loaded.left_order.is_empty());
        assert_eq!(loaded.right_order, vec!["effect", "layers"]);
        let effect = loaded.panels.iter().find(|p| p.id == "effect").unwrap();
        assert_eq!(effect.dock_side, Some(DockSide::Right));
        let layers = loaded.panels.iter().find(|p| p.id == "layers").unwrap();
        assert_eq!(layers.dock_side, Some(DockSide::Right));
        assert!(!layers.visible);
        let colorlab = loaded.panels.iter().find(|p| p.id == "colorlab").unwrap();
        assert!(colorlab.dock_side.is_none());
        assert!(!colorlab.docked);

        let pm = PanelManager::from_persisted(
            loaded.panels,
            Some(loaded.left_order),
            Some(loaded.right_order),
        );
        assert!(pm.get_left_order().is_empty());
        assert_eq!(pm.get_right_order(), &["effect", "layers"]);
    }

    #[test]
    fn v1_migrates_to_left_when_fallback_left() {
        let v1 = r#"{
            "version": 1,
            "panels": [
                { "id": "effect", "docked": true, "visible": true, "window_label": null, "saved_bounds": null },
                { "id": "layers", "docked": true, "visible": true, "window_label": null, "saved_bounds": null },
                { "id": "colorlab", "docked": true, "visible": true, "window_label": null, "saved_bounds": null }
            ]
        }"#;

        let loaded = parse_panel_state_json(v1, DockSide::Left).unwrap();
        assert_eq!(loaded.left_order, vec!["effect", "layers", "colorlab"]);
        assert!(loaded.right_order.is_empty());
        for id in ["effect", "layers", "colorlab"] {
            let p = loaded.panels.iter().find(|p| p.id == id).unwrap();
            assert_eq!(p.dock_side, Some(DockSide::Left));
        }
    }

    #[test]
    fn v1_preserves_legacy_relative_order() {
        // Panels listed out of canonical order — migration uses DOCKABLE_RELATIVE_ORDER.
        let v1 = r#"{
            "version": 1,
            "panels": [
                { "id": "colorlab", "docked": true, "visible": true, "window_label": null, "saved_bounds": null },
                { "id": "effect", "docked": true, "visible": true, "window_label": null, "saved_bounds": null },
                { "id": "layers", "docked": true, "visible": true, "window_label": null, "saved_bounds": null }
            ]
        }"#;
        let loaded = parse_panel_state_json(v1, DockSide::Right).unwrap();
        assert_eq!(loaded.right_order, vec!["effect", "layers", "colorlab"]);
    }

    #[test]
    fn corrupt_json_returns_err() {
        let err = parse_panel_state_json("{ this is not valid json }", DockSide::Right).unwrap_err();
        assert!(err.contains("invalid JSON"));
    }

    #[test]
    fn unsupported_version_returns_err() {
        let json = r#"{ "version": 99, "panels": [], "left_order": [], "right_order": [] }"#;
        let err = parse_panel_state_json(json, DockSide::Right).unwrap_err();
        assert!(err.contains("unsupported"));
    }

    #[test]
    fn v2_missing_orders_field_returns_err() {
        let json = r#"{ "version": 2, "panels": [] }"#;
        let err = parse_panel_state_json(json, DockSide::Right).unwrap_err();
        assert!(err.contains("v2 schema"));
    }

    #[test]
    fn serialize_writes_version_2() {
        let json = serialize_panel_state(&sample_v2_snapshot()).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["version"], 2);
        assert!(value.get("left_order").is_some());
        assert!(value.get("right_order").is_some());
        assert!(value.get("panel_order").is_none());
    }

    #[test]
    fn migrate_v1_helper_assigns_fallback() {
        let panels = vec![PanelInfo {
            id: "layers".into(),
            docked: true,
            visible: true,
            window_label: None,
            saved_bounds: None,
            dock_side: None,
        }];
        let loaded = migrate_v1_to_v2(panels, DockSide::Left);
        assert_eq!(loaded.left_order, vec!["layers"]);
        assert!(loaded.right_order.is_empty());
        assert_eq!(loaded.panels[0].dock_side, Some(DockSide::Left));
    }
}
