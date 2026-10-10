//! `UiVisibility` — sole authority for workspace-gated UI (FR-4 / FR-5).
//!
//! Spec: `.local-doc/Workspace.md`.

use serde::{Deserialize, Serialize};

use super::{AppConfig, Workspace};

/// Maps workspace + advanced toggles to UI booleans (FR-4).
///
/// Pure: no I/O, no globals. Safe to call every frame (NFR-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiVisibility {
    pub show_tiling_section: bool,
    pub show_seamless_preview: bool,
    pub show_ed_temporal_warning: bool,
    pub show_wrap_padding: bool,
    pub show_tile_size_setting: bool,
    pub show_advanced_toggle: bool,
}

/// Canonical visibility matrix (FR-5).
pub fn ui_visibility_from_config(cfg: &AppConfig) -> UiVisibility {
    match cfg.workspace {
        Workspace::General => UiVisibility {
            show_tiling_section: false,
            show_seamless_preview: false,
            show_ed_temporal_warning: false,
            show_wrap_padding: false,
            show_tile_size_setting: false,
            show_advanced_toggle: false,
        },
        Workspace::Gamedev => UiVisibility {
            show_tiling_section: true,
            show_seamless_preview: true,
            show_ed_temporal_warning: true,
            show_wrap_padding: cfg.advanced.tiling_advanced,
            show_tile_size_setting: true,
            show_advanced_toggle: true,
        },
    }
}

/// Alias matching the spec name `UiVisibility::from_config`.
impl UiVisibility {
    #[allow(dead_code)] // public API alias for FR-4
    pub fn from_config(cfg: &AppConfig) -> Self {
        ui_visibility_from_config(cfg)
    }
}
