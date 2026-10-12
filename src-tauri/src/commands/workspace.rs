//! Tauri commands for Workspace / AppConfig (FR-3, FR-8, FR-9).

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Deserialize;
use tauri::State;

use crate::commands::AppState;
use crate::config::{
    filename_suggests_tiles, AppConfigDto, ConfigStore, UiVisibility, Workspace,
    WorkspaceSwitchSource, WORKSPACE_METRICS,
};

fn store(state: &AppState) -> Arc<ConfigStore> {
    Arc::clone(&state.config)
}

/// Full config + visibility snapshot for the UI.
#[tauri::command]
pub fn get_app_config(state: State<'_, Arc<AppState>>) -> AppConfigDto {
    AppConfigDto::from_store(&store(&state))
}

/// Pure visibility for the current config (FR-4).
#[tauri::command]
pub fn get_ui_visibility(state: State<'_, Arc<AppState>>) -> UiVisibility {
    store(&state).visibility()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetWorkspaceRequest {
    pub workspace: Workspace,
    #[serde(default = "default_settings_source")]
    pub source: WorkspaceSwitchSource,
}

fn default_settings_source() -> WorkspaceSwitchSource {
    WorkspaceSwitchSource::Settings
}

#[tauri::command]
pub fn set_workspace(state: State<'_, Arc<AppState>>, req: SetWorkspaceRequest) -> AppConfigDto {
    let s = store(&state);
    s.set_workspace(req.workspace, req.source);
    AppConfigDto::from_store(&s)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetTilingAdvancedRequest {
    pub enabled: bool,
}

#[tauri::command]
pub fn set_tiling_advanced(
    state: State<'_, Arc<AppState>>,
    req: SetTilingAdvancedRequest,
) -> AppConfigDto {
    let s = store(&state);
    s.set_tiling_advanced(req.enabled);
    AppConfigDto::from_store(&s)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolveWorkspacePromptRequest {
    pub choice: Workspace,
    pub remember: bool,
}

#[tauri::command]
pub fn resolve_workspace_prompt(
    state: State<'_, Arc<AppState>>,
    req: ResolveWorkspacePromptRequest,
) -> AppConfigDto {
    let s = store(&state);
    s.resolve_prompt(req.choice, req.remember);
    AppConfigDto::from_store(&s)
}

/// Increment prompt-shown metric; call when modal is displayed (NFR-4).
#[tauri::command]
pub fn note_workspace_prompt_shown(state: State<'_, Arc<AppState>>) {
    store(&state).note_prompt_shown();
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceHintTriggerRequest {
    /// `tile_export` | `filename` | `ed_square` | `batch_tile`
    pub trigger: String,
    #[serde(default)]
    pub filename: Option<String>,
    #[serde(default)]
    pub doc_width: Option<u32>,
    #[serde(default)]
    pub doc_height: Option<u32>,
    #[serde(default)]
    pub ed_enabled: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceHintDecision {
    pub show: bool,
    pub trigger: String,
}

/// Evaluate whether the General→Gamedev hint banner should appear (FR-9).
#[tauri::command]
pub fn evaluate_workspace_hint(
    state: State<'_, Arc<AppState>>,
    req: WorkspaceHintTriggerRequest,
) -> WorkspaceHintDecision {
    let cfg = store(&state).snapshot();
    if cfg.workspace != Workspace::General {
        return WorkspaceHintDecision {
            show: false,
            trigger: req.trigger,
        };
    }
    let show = match req.trigger.as_str() {
        "batch_tile" | "tile_export" => true,
        "filename" => req
            .filename
            .as_deref()
            .map(filename_suggests_tiles)
            .unwrap_or(false),
        "ed_square" => {
            let w = req.doc_width.unwrap_or(0);
            let h = req.doc_height.unwrap_or(0);
            req.ed_enabled.unwrap_or(false) && w == h && w >= 32 && cfg.has_exported_tiles
        }
        _ => false,
    };
    if show {
        WORKSPACE_METRICS.hint_shown.fetch_add(1, Ordering::Relaxed);
    }
    WorkspaceHintDecision {
        show,
        trigger: req.trigger,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceHintActionRequest {
    /// `switch` | `dismiss` | `timeout`
    pub action: String,
}

#[tauri::command]
pub fn resolve_workspace_hint(
    state: State<'_, Arc<AppState>>,
    req: WorkspaceHintActionRequest,
) -> AppConfigDto {
    WORKSPACE_METRICS
        .hint_action
        .fetch_add(1, Ordering::Relaxed);
    let s = store(&state);
    if req.action == "switch" {
        s.set_workspace(Workspace::Gamedev, WorkspaceSwitchSource::Hint);
    }
    AppConfigDto::from_store(&s)
}

#[tauri::command]
pub fn note_tile_export(state: State<'_, Arc<AppState>>) {
    store(&state).mark_exported_tiles();
}
