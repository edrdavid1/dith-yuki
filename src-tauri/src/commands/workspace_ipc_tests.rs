//! Workspace IPC smoke tests (catches State type mismatches).
//!
//! Production manages `Arc<AppState>` (see `main.rs`). Commands must take
//! `State<'_, Arc<AppState>>`. Using `State<'_, AppState>` compiles but fails
//! at invoke time — the bug that made Gamedev switching a no-op (`6c8490a`).
//!
//! These tests build a mock Tauri app with the same manage + handler wiring
//! and invoke the public commands.

use std::sync::Arc;

use serde_json::{json, Value};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::Manager;
use tauri::WebviewWindowBuilder;

use super::{make_test_app_state, AppState};
use crate::config::{Workspace, WorkspaceSwitchSource};

fn invoke_url() -> tauri::Url {
    if cfg!(any(windows, target_os = "android")) {
        "http://tauri.localhost".parse().unwrap()
    } else {
        "tauri://localhost".parse().unwrap()
    }
}

/// Same manage type as production: `Arc<AppState>`.
fn build_workspace_app() -> tauri::App<tauri::test::MockRuntime> {
    let state = make_test_app_state();
    // Shared test config dir can retain prior workspace; pin known defaults.
    state
        .config
        .set_workspace(Workspace::General, WorkspaceSwitchSource::Settings);
    state.config.set_tiling_advanced(false);

    mock_builder()
        .manage(state)
        // Paths must match `main.rs` so `__cmd__*` macros resolve at crate root.
        .invoke_handler(tauri::generate_handler![
            crate::commands::get_app_config,
            crate::commands::get_ui_visibility,
            crate::commands::set_workspace,
            crate::commands::set_tiling_advanced,
        ])
        .build(mock_context(noop_assets()))
        .expect("mock workspace app")
}

fn invoke_cmd(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    cmd: &str,
    body: Value,
) -> Result<Value, Value> {
    get_ipc_response(
        webview,
        InvokeRequest {
            cmd: cmd.into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: invoke_url(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|b| b.deserialize::<Value>().expect("deserialize ipc ok body"))
}

#[test]
fn workspace_commands_registered_and_resolvable() {
    let app = build_workspace_app();
    // Managed type must be Arc — mirrors main.rs `.manage(state.clone())`.
    let _ = app.state::<Arc<AppState>>();

    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    for cmd in [
        "get_app_config",
        "get_ui_visibility",
        "set_workspace",
        "set_tiling_advanced",
    ] {
        let body = match cmd {
            "set_workspace" => json!({ "req": { "workspace": "general", "source": "settings" } }),
            "set_tiling_advanced" => json!({ "req": { "enabled": false } }),
            _ => json!({}),
        };
        let res = invoke_cmd(&webview, cmd, body);
        assert!(
            res.is_ok(),
            "command `{cmd}` must be registered and injectable; got Err({:?}). \
             If you see a state/injection error, check State<'_, Arc<AppState>> matches .manage(Arc<AppState>).",
            res.err()
        );
    }

    // Unknown command must fail (proves we are actually going through IPC).
    let missing = invoke_cmd(&webview, "definitely_not_a_command", json!({}));
    assert!(missing.is_err(), "unknown command should fail");
}

#[test]
fn set_workspace_gamedev_updates_config_and_visibility() {
    let app = build_workspace_app();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    let before = invoke_cmd(&webview, "get_app_config", json!({})).expect("get_app_config");
    assert_eq!(
        before.get("workspace").and_then(|v| v.as_str()),
        Some("general"),
        "default workspace is General"
    );
    assert_eq!(
        before
            .pointer("/visibility/showTilingSection")
            .and_then(|v| v.as_bool()),
        Some(false)
    );

    let after = invoke_cmd(
        &webview,
        "set_workspace",
        json!({ "req": { "workspace": "gamedev", "source": "settings" } }),
    )
    .expect("set_workspace");

    assert_eq!(
        after.get("workspace").and_then(|v| v.as_str()),
        Some("gamedev"),
        "set_workspace must persist Gamedev immediately (FR-3, no restart)"
    );
    assert_eq!(
        after
            .pointer("/visibility/showTilingSection")
            .and_then(|v| v.as_bool()),
        Some(true),
        "UiVisibility must flip on switch"
    );
    assert_eq!(
        after
            .pointer("/visibility/showWrapPadding")
            .and_then(|v| v.as_bool()),
        Some(false),
        "wrap-padding stays hidden until advanced tiling is on"
    );

    // In-memory store matches DTO (same process, immediate).
    let store = app.state::<Arc<AppState>>();
    assert_eq!(store.config.snapshot().workspace, Workspace::Gamedev);

    let vis = invoke_cmd(&webview, "get_ui_visibility", json!({})).expect("get_ui_visibility");
    assert_eq!(vis.get("showTilingSection").and_then(|v| v.as_bool()), Some(true));
}

#[test]
fn set_tiling_advanced_enables_wrap_padding_visibility() {
    let app = build_workspace_app();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    invoke_cmd(
        &webview,
        "set_workspace",
        json!({ "req": { "workspace": "gamedev", "source": "settings" } }),
    )
    .expect("set_workspace");

    let dto = invoke_cmd(
        &webview,
        "set_tiling_advanced",
        json!({ "req": { "enabled": true } }),
    )
    .expect("set_tiling_advanced");

    assert_eq!(
        dto.pointer("/advanced/tiling_advanced")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        dto.pointer("/visibility/showWrapPadding")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
}
