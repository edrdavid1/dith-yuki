//! Custom application icon (macOS only).
//!
//! Session Dock icon via `NSApplication.applicationIconImage`, persistent Finder
//! icon via `NSWorkspace.setIcon(_:forFile:options:)`. See
//! `.local-doc/SPEC_dither_custom_app_icon.md`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const DEFAULT_ICON_ID: &str = "default";

/// Stable variant ids. Labels are UI-facing and may change; ids must not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIconVariant {
    pub id: &'static str,
    pub label: &'static str,
    /// Relative URL under the frontend public root for the Preferences thumbnail.
    pub preview_src: &'static str,
    /// `true` for temporary / non-final artwork.
    pub todo_design: bool,
}

pub const VARIANTS: &[AppIconVariant] = &[
    AppIconVariant {
        id: DEFAULT_ICON_ID,
        label: "Standard",
        preview_src: "/app-icons/default.png",
        todo_design: false,
    },
    // TODO(design): replace alt2 artwork with final branded variant.
    AppIconVariant {
        id: "alt2",
        label: "Alt 2",
        preview_src: "/app-icons/alt2.png",
        todo_design: true,
    },
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIconState {
    pub id: String,
    pub persistent_ok: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIconApplyResult {
    pub id: String,
    pub persistent_ok: bool,
    /// Shown in Preferences when Persistent write failed (session-only Dock).
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredAppIcon {
    id: String,
}

fn store_path(app_data: &Path) -> PathBuf {
    app_data.join("app_icon.json")
}

/// Map a stored / requested id to a known variant. Unknown → `default` + warn.
pub fn resolve_variant_id(id: &str) -> &'static str {
    if VARIANTS.iter().any(|v| v.id == id) {
        for v in VARIANTS {
            if v.id == id {
                return v.id;
            }
        }
    }
    if id != DEFAULT_ICON_ID {
        log::warn!("unknown app icon id {id:?}; falling back to {DEFAULT_ICON_ID}");
    }
    DEFAULT_ICON_ID
}

pub fn load_stored_id(app_data: &Path) -> String {
    let path = store_path(app_data);
    let Ok(bytes) = std::fs::read(&path) else {
        return DEFAULT_ICON_ID.to_string();
    };
    match serde_json::from_slice::<StoredAppIcon>(&bytes) {
        Ok(stored) => resolve_variant_id(&stored.id).to_string(),
        Err(e) => {
            log::warn!(
                "corrupt app_icon.json at {}: {e}; using {DEFAULT_ICON_ID}",
                path.display()
            );
            DEFAULT_ICON_ID.to_string()
        }
    }
}

fn save_stored_id(app_data: &Path, id: &str) -> Result<(), String> {
    std::fs::create_dir_all(app_data).map_err(|e| format!("create app_data: {e}"))?;
    let payload = StoredAppIcon { id: id.to_string() };
    let json = serde_json::to_vec_pretty(&payload).map_err(|e| e.to_string())?;
    let path = store_path(app_data);
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

pub fn resolve_icns_path(resource_dir: &Path, id: &str) -> Option<PathBuf> {
    if id == DEFAULT_ICON_ID {
        return None;
    }
    let file = format!("{id}.icns");
    let candidates = [
        resource_dir.join("icons").join("alt").join(&file),
        resource_dir.join(&file),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join("alt")
            .join(&file),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

/// Bundled default app icon (Tauri copies `app-icon.icns` → Resources as `icon.icns`).
fn resolve_default_icns_path(resource_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        resource_dir.join("icon.icns"),
        resource_dir.join("app-icon.icns"),
        resource_dir.join("icons").join("app-icon.icns"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("icons")
            .join("app-icon.icns"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage, NSWorkspace, NSWorkspaceIconCreationOptions};
    use objc2_foundation::{NSBundle, NSString};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    const SESSION_ONLY_WARNING: &str = "Icon will only change while the app is running — no permission to modify the application file.";

    fn load_nsimage(path: &Path) -> Option<objc2::rc::Retained<NSImage>> {
        let path_str = path.to_str()?;
        let ns_path = NSString::from_str(path_str);
        NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path)
    }

    fn bundle_path_string() -> Option<String> {
        let bundle = NSBundle::mainBundle();
        Some(bundle.bundlePath().to_string())
    }

    /// True only for a real `.app` package. Raw `tauri dev` binaries resolve to
    /// `target/debug` (or similar) — writing a Finder icon there is useless and
    /// can disturb watchers.
    fn is_packaged_app_bundle(path: &str) -> bool {
        Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("app"))
    }

    fn apply_icon_inner(resource_dir: &Path, id: &str) -> Result<bool, String> {
        let id = resolve_variant_id(id);
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| "app icon must be applied on the main thread".to_string())?;
        let app = NSApplication::sharedApplication(mtm);
        let workspace = NSWorkspace::sharedWorkspace();

        let bundle_path = bundle_path_string()
            .ok_or_else(|| "could not resolve NSBundle.mainBundle.bundlePath".to_string())?;
        let ns_bundle = NSString::from_str(&bundle_path);
        let can_persist = is_packaged_app_bundle(&bundle_path);

        if id == DEFAULT_ICON_ID {
            // Persistent clear MUST happen before reading the bundle icon for Dock.
            // iconForFile() returns the custom Finder icon while it is still set, so
            // restoring via iconForFile-before-clear left the Dock on alt2 until relaunch.
            let persistent_ok = if can_persist {
                let ok = workspace.setIcon_forFile_options(
                    None,
                    &ns_bundle,
                    NSWorkspaceIconCreationOptions(0),
                );
                if ok {
                    workspace.noteFileSystemChanged_(&ns_bundle);
                }
                ok
            } else {
                log::info!(
                    "app icon: skipping NSWorkspace.setIcon clear (not a .app bundle: {bundle_path})"
                );
                true
            };

            // Documented Dock restore is nil. After clearing the Finder custom
            // icon, also set an explicit image so the tile refreshes immediately
            // (nil alone can leave a prior setApplicationIconImage in place).
            if let Some(default_icns) = resolve_default_icns_path(resource_dir) {
                if let Some(image) = load_nsimage(&default_icns) {
                    unsafe {
                        app.setApplicationIconImage(Some(&image));
                    }
                } else {
                    unsafe {
                        app.setApplicationIconImage(None);
                    }
                }
            } else {
                unsafe {
                    app.setApplicationIconImage(None);
                }
                let restored = workspace.iconForFile(&ns_bundle);
                unsafe {
                    app.setApplicationIconImage(Some(&restored));
                }
            }

            return Ok(persistent_ok);
        }

        let icns = resolve_icns_path(resource_dir, id).ok_or_else(|| {
            format!(
                "app icon resource missing for id {id:?} under {}",
                resource_dir.display()
            )
        })?;
        let image = load_nsimage(&icns)
            .ok_or_else(|| format!("failed to load NSImage from {}", icns.display()))?;

        // Session first so Dock updates even if Persistent write fails.
        unsafe {
            app.setApplicationIconImage(Some(&image));
        }

        if !can_persist {
            log::info!(
                "app icon: skipping NSWorkspace.setIcon (not a .app bundle: {bundle_path})"
            );
            return Ok(true);
        }

        let persistent_ok = workspace.setIcon_forFile_options(
            Some(&image),
            &ns_bundle,
            NSWorkspaceIconCreationOptions(0),
        );
        if persistent_ok {
            workspace.noteFileSystemChanged_(&ns_bundle);
        }

        Ok(persistent_ok)
    }

    /// Apply Persistent (packaged `.app` only) + Session (Dock).
    /// Returns whether Persistent succeeded. Never panics across the FFI boundary.
    pub fn apply_icon(resource_dir: &Path, id: &str) -> Result<bool, String> {
        match catch_unwind(AssertUnwindSafe(|| apply_icon_inner(resource_dir, id))) {
            Ok(result) => result,
            Err(_) => Err("app icon apply panicked (caught)".into()),
        }
    }

    pub fn warning_for_persistent(ok: bool) -> Option<String> {
        if ok {
            None
        } else {
            Some(SESSION_ONLY_WARNING.to_string())
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::*;

    pub fn apply_icon(_resource_dir: &Path, _id: &str) -> Result<bool, String> {
        Err("App icon selection is only available on macOS".into())
    }

    pub fn warning_for_persistent(_ok: bool) -> Option<String> {
        None
    }
}

fn resource_dir_for(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .resource_dir()
        .map_err(|e| format!("resource_dir: {e}"))
}

fn app_data_for(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))
}

/// Re-apply stored custom icon before the main window is shown.
pub fn reapply_on_startup(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let Ok(app_data) = app_data_for(app) else {
            return;
        };
        let id = load_stored_id(&app_data);
        if id == DEFAULT_ICON_ID {
            return;
        }
        let Ok(resource_dir) = resource_dir_for(app) else {
            log::warn!("app icon: resource_dir unavailable at startup");
            return;
        };
        match native::apply_icon(&resource_dir, &id) {
            Ok(persistent_ok) => {
                if !persistent_ok {
                    log::warn!(
                        "app icon: persistent setIcon failed at startup for {id}; session Dock applied"
                    );
                }
            }
            Err(e) => log::warn!("app icon: startup reapply failed: {e}"),
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = app;
    }
}

#[tauri::command]
pub fn list_app_icons() -> Vec<AppIconVariant> {
    #[cfg(target_os = "macos")]
    {
        VARIANTS.to_vec()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

#[tauri::command]
pub fn get_app_icon(app: AppHandle) -> Result<AppIconState, String> {
    let app_data = app_data_for(&app)?;
    let id = load_stored_id(&app_data);
    Ok(AppIconState {
        id,
        // Unknown until a set attempt; Preferences treats missing warning as ok.
        persistent_ok: true,
    })
}

#[tauri::command]
pub fn set_app_icon(app: AppHandle, id: String) -> Result<AppIconApplyResult, String> {
    let resolved = resolve_variant_id(&id).to_string();
    let resource_dir = resource_dir_for(&app)?;
    let app_data = app_data_for(&app)?;

    let persistent_ok = native::apply_icon(&resource_dir, &resolved)?;
    // Persist after at least Session apply succeeded (apply_icon Err = total failure).
    save_stored_id(&app_data, &resolved)?;

    Ok(AppIconApplyResult {
        id: resolved,
        persistent_ok,
        warning: native::warning_for_persistent(persistent_ok),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_id_falls_back_to_default() {
        assert_eq!(resolve_variant_id("nope"), DEFAULT_ICON_ID);
        assert_eq!(resolve_variant_id(""), DEFAULT_ICON_ID);
        assert_eq!(resolve_variant_id("alt2"), "alt2");
        assert_eq!(resolve_variant_id(DEFAULT_ICON_ID), DEFAULT_ICON_ID);
    }

    #[test]
    fn load_missing_store_is_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_stored_id(dir.path()), DEFAULT_ICON_ID);
    }

    #[test]
    fn load_corrupt_store_is_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(store_path(dir.path()), b"{not json").unwrap();
        assert_eq!(load_stored_id(dir.path()), DEFAULT_ICON_ID);
    }

    #[test]
    fn load_unknown_stored_id_is_default() {
        let dir = tempfile::tempdir().unwrap();
        save_stored_id(dir.path(), "ghost").unwrap();
        assert_eq!(load_stored_id(dir.path()), DEFAULT_ICON_ID);
    }

    #[test]
    fn roundtrip_known_id() {
        let dir = tempfile::tempdir().unwrap();
        save_stored_id(dir.path(), "alt2").unwrap();
        assert_eq!(load_stored_id(dir.path()), "alt2");
    }
}
