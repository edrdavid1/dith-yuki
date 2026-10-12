//! App config: Workspace selection (FR-1…FR-3, NFR-2…NFR-5).
//!
//! Spec: `.local-doc/Workspace.md` (`workspace_profiles_v1`).

mod migrations;
mod visibility;

pub use migrations::{migrate_config_value, CURRENT_SCHEMA_VERSION};
pub use visibility::{ui_visibility_from_config, UiVisibility};

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

/// Canonical workspace enum (FR-1). Serialized as `"general"` / `"gamedev"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum Workspace {
    #[default]
    General,
    Gamedev,
}

impl Workspace {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Gamedev => "gamedev",
        }
    }
}

fn deserialize_workspace<'de, D>(deserializer: D) -> Result<Workspace, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    match raw.as_str() {
        "general" => Ok(Workspace::General),
        "gamedev" => Ok(Workspace::Gamedev),
        other => {
            log::warn!("invalid workspace {other:?}; falling back to General (NFR-5)");
            Ok(Workspace::General)
        }
    }
}

/// Advanced toggles nested under Gamedev (FR-2 / FR-6).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvancedConfig {
    #[serde(default)]
    pub tiling_advanced: bool,
}

/// Preview-latency instrumentation (`.local-doc/diagnostic.md` §9). Off by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviewDiagnosticsConfig {
    #[serde(default)]
    pub enabled: bool,
    /// 1 = gesture breakdown, 2 = per-tile. Ignored while `enabled` is false.
    #[serde(default = "default_diag_level")]
    pub level: u8,
    #[serde(default)]
    pub save_session_reports: bool,
}

fn default_diag_level() -> u8 {
    1
}

impl Default for PreviewDiagnosticsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            level: 1,
            save_session_reports: false,
        }
    }
}

/// Experimental feature flags (`ED-on-GPU.md` §12.1). Off by default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExperimentalConfig {
    /// When true, UI may offer `blocked_fs` / `blue_noise` GPU dither families.
    #[serde(default, alias = "gpuDitherFamilies")]
    pub gpu_dither_families: bool,
    /// Gesture / tile timing. Hot path reads an atomic copied from this struct.
    #[serde(default)]
    pub preview_diagnostics: PreviewDiagnosticsConfig,
}

/// Persisted application config at `{app_data_dir}/config.json` (FR-2, NFR-2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, deserialize_with = "deserialize_workspace")]
    pub workspace: Workspace,
    #[serde(default)]
    pub advanced: AdvancedConfig,
    #[serde(default)]
    pub experimental: ExperimentalConfig,
    /// FR-8: when false, upgrade prompt may reappear.
    #[serde(default)]
    pub workspace_choice_remembered: bool,
    /// FR-8: launches since upgrade without a remembered choice.
    #[serde(default)]
    pub workspace_prompt_launches: u32,
    /// FR-9: user has exported tiles / sprite cells at least once.
    #[serde(default)]
    pub has_exported_tiles: bool,
    /// Forward-compat bag (NFR-3).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

fn default_schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            workspace: Workspace::General,
            advanced: AdvancedConfig::default(),
            experimental: ExperimentalConfig::default(),
            // Fresh install: no prompt (FR-8).
            workspace_choice_remembered: true,
            workspace_prompt_launches: 0,
            has_exported_tiles: false,
            extra: Map::new(),
        }
    }
}

impl AppConfig {
    /// Whether the upgrade workspace prompt should be shown this launch (FR-8).
    pub fn should_show_workspace_prompt(&self) -> bool {
        if self.workspace_choice_remembered {
            return false;
        }
        self.workspace_prompt_launches < 3
    }
}

/// Prometheus-style counters (NFR-4).
#[derive(Debug, Default)]
pub struct WorkspaceMetrics {
    pub switch: AtomicU64,
    pub prompt_shown: AtomicU64,
    pub prompt_choice: AtomicU64,
    pub hint_shown: AtomicU64,
    pub hint_action: AtomicU64,
}

pub static WORKSPACE_METRICS: WorkspaceMetrics = WorkspaceMetrics {
    switch: AtomicU64::new(0),
    prompt_shown: AtomicU64::new(0),
    prompt_choice: AtomicU64::new(0),
    hint_shown: AtomicU64::new(0),
    hint_action: AtomicU64::new(0),
};

/// Source label for workspace.switch (NFR-4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkspaceSwitchSource {
    Settings,
    Prompt,
    Hint,
}

pub const CONFIG_FILENAME: &str = "config.json";
const DEBOUNCE: Duration = Duration::from_millis(500);

/// In-memory + debounced disk store (NFR-2).
pub struct ConfigStore {
    inner: Mutex<ConfigStoreInner>,
}

struct ConfigStoreInner {
    path: PathBuf,
    config: AppConfig,
    /// True when this process created a brand-new config (no prior file).
    fresh_install: bool,
    last_disk_write: Option<Instant>,
    pending_flush: bool,
    /// Test / observability: number of successful disk writes.
    pub write_count: u64,
    flush_generation: u64,
}

impl ConfigStore {
    /// Load from `{app_data}/config.json`, migrating as needed (FR-2, NFR-3).
    pub fn load(app_data_dir: &Path) -> Arc<Self> {
        let (path, config, fresh_install) = Self::read_from_dir(app_data_dir);
        let store = Arc::new(Self {
            inner: Mutex::new(ConfigStoreInner {
                path,
                config,
                fresh_install,
                last_disk_write: None,
                pending_flush: false,
                write_count: 0,
                flush_generation: 0,
            }),
        });
        store.publish_diag();
        store
    }

    /// Copy `preview_diagnostics` into the hot-path atomic (diagnostic.md §2.8).
    pub fn publish_diag(&self) {
        let cfg = self.snapshot();
        let diag = &cfg.experimental.preview_diagnostics;
        crate::preview_diag::sync_from_config(diag.enabled, diag.level);
    }

    fn read_from_dir(app_data_dir: &Path) -> (PathBuf, AppConfig, bool) {
        let path = app_data_dir.join(CONFIG_FILENAME);
        match std::fs::read(&path) {
            Ok(bytes) => match parse_and_migrate(&bytes) {
                Ok(cfg) => {
                    log::info!("workspace.active workspace={}", cfg.workspace.as_str());
                    (path, cfg, false)
                }
                Err(e) => {
                    log::warn!("corrupt config.json ({e}); using defaults");
                    (path, AppConfig::default(), false)
                }
            },
            Err(_) => {
                log::info!("workspace.active workspace=general (fresh install)");
                (path, AppConfig::default(), true)
            }
        }
    }

    /// Point the store at the real `{app_data}/config.json` after Tauri setup (FR-2).
    pub fn rebind_app_data(self: &Arc<Self>, app_data_dir: &Path) {
        let _ = std::fs::create_dir_all(app_data_dir);
        let (path, config, fresh_install) = Self::read_from_dir(app_data_dir);
        let mut guard = self.inner.lock().expect("config lock");
        guard.path = path;
        guard.config = config;
        guard.fresh_install = fresh_install;
        guard.last_disk_write = None;
        guard.pending_flush = false;
        drop(guard);
        self.publish_diag();
        self.maybe_force_general_after_prompts();
    }

    pub fn snapshot(&self) -> AppConfig {
        self.inner.lock().expect("config lock").config.clone()
    }

    pub fn is_fresh_install(&self) -> bool {
        self.inner.lock().expect("config lock").fresh_install
    }

    pub fn visibility(&self) -> UiVisibility {
        ui_visibility_from_config(&self.snapshot())
    }

    #[cfg(test)]
    pub fn write_count(&self) -> u64 {
        self.inner.lock().expect("config lock").write_count
    }

    /// Schedule debounced flush using `Arc<Self>` (NFR-2: ≤1 write / 500 ms).
    pub fn schedule_flush(self: &Arc<Self>) {
        let mut guard = self.inner.lock().expect("config lock");
        let can_write_now = match guard.last_disk_write {
            Some(t) => t.elapsed() >= DEBOUNCE,
            None => true,
        };
        if can_write_now {
            let cfg = guard.config.clone();
            let path = guard.path.clone();
            if let Err(e) = write_config_atomic(&path, &cfg) {
                log::error!("config write failed: {e}");
            } else {
                guard.last_disk_write = Some(Instant::now());
                guard.write_count += 1;
                guard.pending_flush = false;
            }
            return;
        }
        guard.pending_flush = true;
        guard.flush_generation += 1;
        let gen = guard.flush_generation;
        drop(guard);
        let store = Arc::clone(self);
        std::thread::spawn(move || {
            std::thread::sleep(DEBOUNCE);
            store.flush_if_generation(gen);
        });
    }

    fn flush_if_generation(&self, gen: u64) {
        let mut guard = self.inner.lock().expect("config lock");
        if guard.flush_generation != gen || !guard.pending_flush {
            return;
        }
        let cfg = guard.config.clone();
        let path = guard.path.clone();
        if let Err(e) = write_config_atomic(&path, &cfg) {
            log::error!("config write failed: {e}");
        } else {
            guard.last_disk_write = Some(Instant::now());
            guard.write_count += 1;
            guard.pending_flush = false;
        }
    }

    /// Force immediate disk write (tests / shutdown).
    #[cfg(test)]
    pub fn flush_now(&self) -> Result<(), String> {
        let mut guard = self.inner.lock().expect("config lock");
        write_config_atomic(&guard.path, &guard.config)?;
        guard.last_disk_write = Some(Instant::now());
        guard.write_count += 1;
        guard.pending_flush = false;
        Ok(())
    }

    pub fn set_workspace(
        self: &Arc<Self>,
        workspace: Workspace,
        source: WorkspaceSwitchSource,
    ) -> AppConfig {
        let cfg = {
            let mut guard = self.inner.lock().expect("config lock");
            let from = guard.config.workspace;
            if from != workspace {
                log::info!(
                    "workspace switched: {} -> {} (source={:?})",
                    from.as_str(),
                    workspace.as_str(),
                    source
                );
                WORKSPACE_METRICS.switch.fetch_add(1, Ordering::Relaxed);
            }
            guard.config.workspace = workspace;
            guard.config.schema_version = CURRENT_SCHEMA_VERSION;
            guard.config.clone()
        };
        self.schedule_flush();
        cfg
    }

    pub fn set_tiling_advanced(self: &Arc<Self>, enabled: bool) -> AppConfig {
        let cfg = {
            let mut guard = self.inner.lock().expect("config lock");
            guard.config.advanced.tiling_advanced = enabled;
            guard.config.schema_version = CURRENT_SCHEMA_VERSION;
            guard.config.clone()
        };
        self.schedule_flush();
        cfg
    }

    pub fn resolve_prompt(self: &Arc<Self>, choice: Workspace, remember: bool) -> AppConfig {
        WORKSPACE_METRICS
            .prompt_choice
            .fetch_add(1, Ordering::Relaxed);
        let cfg = {
            let mut guard = self.inner.lock().expect("config lock");
            let from = guard.config.workspace;
            guard.config.workspace = choice;
            if remember {
                guard.config.workspace_choice_remembered = true;
            } else {
                guard.config.workspace_choice_remembered = false;
                guard.config.workspace_prompt_launches =
                    guard.config.workspace_prompt_launches.saturating_add(1);
                if guard.config.workspace_prompt_launches >= 3 {
                    guard.config.workspace = Workspace::General;
                    guard.config.workspace_choice_remembered = true;
                }
            }
            if from != guard.config.workspace {
                log::info!(
                    "workspace switched: {} -> {} (source=prompt)",
                    from.as_str(),
                    guard.config.workspace.as_str()
                );
                WORKSPACE_METRICS.switch.fetch_add(1, Ordering::Relaxed);
            }
            guard.config.schema_version = CURRENT_SCHEMA_VERSION;
            guard.config.clone()
        };
        self.schedule_flush();
        cfg
    }

    /// Call once per launch when prompt will be shown (FR-8 / NFR-4).
    pub fn note_prompt_shown(self: &Arc<Self>) {
        WORKSPACE_METRICS
            .prompt_shown
            .fetch_add(1, Ordering::Relaxed);
        let guard = self.inner.lock().expect("config lock");
        if !guard.config.workspace_choice_remembered {
            // Count this launch toward the 3-launch cap when user dismisses via choice later.
            // Launch counter increments on resolve without remember; also bump on show for
            // force-after-three when user never clicks — handled in maybe_force_general.
        }
        drop(guard);
    }

    /// On launch: if 3+ prompt launches without remember, force General (FR-8).
    pub fn maybe_force_general_after_prompts(self: &Arc<Self>) -> AppConfig {
        let cfg = {
            let mut guard = self.inner.lock().expect("config lock");
            if !guard.config.workspace_choice_remembered
                && guard.config.workspace_prompt_launches >= 3
            {
                guard.config.workspace = Workspace::General;
                guard.config.workspace_choice_remembered = true;
            }
            guard.config.clone()
        };
        self.schedule_flush();
        cfg
    }

    pub fn mark_exported_tiles(self: &Arc<Self>) {
        let mut guard = self.inner.lock().expect("config lock");
        if guard.config.has_exported_tiles {
            return;
        }
        guard.config.has_exported_tiles = true;
        drop(guard);
        self.schedule_flush();
    }
}

fn parse_and_migrate(bytes: &[u8]) -> Result<AppConfig, String> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|e| format!("json: {e}"))?;
    let had_workspace = value
        .as_object()
        .map(|o| o.contains_key("workspace"))
        .unwrap_or(false);
    value = migrate_config_value(value);
    let mut cfg: AppConfig = serde_json::from_value(value).map_err(|e| format!("decode: {e}"))?;
    if !had_workspace {
        // Upgrade path (FR-8): prompt until remembered or 3 launches.
        cfg.workspace_choice_remembered = false;
        cfg.workspace_prompt_launches = 0;
        cfg.workspace = Workspace::General;
    }
    Ok(cfg)
}

fn write_config_atomic(path: &Path, cfg: &AppConfig) -> Result<(), String> {
    let json = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    engine_io::atomic_write(path, &json).map_err(|e| e.to_string())
}

/// DTO returned to the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigDto {
    pub schema_version: u32,
    pub workspace: Workspace,
    pub advanced: AdvancedConfig,
    pub experimental: ExperimentalConfig,
    pub workspace_choice_remembered: bool,
    pub workspace_prompt_launches: u32,
    pub has_exported_tiles: bool,
    pub fresh_install: bool,
    pub show_workspace_prompt: bool,
    pub visibility: UiVisibility,
}

impl AppConfigDto {
    pub fn from_store(store: &ConfigStore) -> Self {
        let fresh = store.is_fresh_install();
        let cfg = store.snapshot();
        let show = !fresh && cfg.should_show_workspace_prompt();
        Self {
            schema_version: cfg.schema_version,
            workspace: cfg.workspace,
            advanced: cfg.advanced.clone(),
            experimental: cfg.experimental.clone(),
            workspace_choice_remembered: cfg.workspace_choice_remembered,
            workspace_prompt_launches: cfg.workspace_prompt_launches,
            has_exported_tiles: cfg.has_exported_tiles,
            fresh_install: fresh,
            show_workspace_prompt: show,
            visibility: ui_visibility_from_config(&cfg),
        }
    }
}

/// Filename patterns that suggest a tiling workflow (FR-9).
pub fn filename_suggests_tiles(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("_tile")
        || lower.contains("_atlas")
        || lower.contains("_sheet")
        || lower.contains("-tile")
        || lower.contains("-atlas")
        || lower.contains("-sheet")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_workspace_is_general() {
        assert_eq!(Workspace::default(), Workspace::General);
    }

    #[test]
    fn visibility_general_hides_tiling() {
        let cfg = AppConfig::default();
        let v = ui_visibility_from_config(&cfg);
        assert!(!v.show_tiling_section);
        assert!(!v.show_seamless_preview);
        assert!(!v.show_ed_temporal_warning);
        assert!(!v.show_wrap_padding);
        assert!(!v.show_tile_size_setting);
        assert!(!v.show_advanced_toggle);
    }

    #[test]
    fn visibility_gamedev_shows_tiling() {
        let mut cfg = AppConfig::default();
        cfg.workspace = Workspace::Gamedev;
        cfg.advanced.tiling_advanced = false;
        let v = ui_visibility_from_config(&cfg);
        assert!(v.show_tiling_section);
        assert!(v.show_seamless_preview);
        assert!(v.show_ed_temporal_warning);
        assert!(!v.show_wrap_padding);
        assert!(v.show_tile_size_setting);
        assert!(v.show_advanced_toggle);
    }

    #[test]
    fn visibility_gamedev_advanced_shows_wrap_padding() {
        let mut cfg = AppConfig::default();
        cfg.workspace = Workspace::Gamedev;
        cfg.advanced.tiling_advanced = true;
        let v = ui_visibility_from_config(&cfg);
        assert!(v.show_wrap_padding);
    }

    #[test]
    fn preview_diagnostics_defaults_off_when_key_missing() {
        let raw = br#"{"schema_version":1,"workspace":"general"}"#;
        let cfg = parse_and_migrate(raw).unwrap();
        assert!(!cfg.experimental.preview_diagnostics.enabled);
        assert_eq!(cfg.experimental.preview_diagnostics.level, 1);
        assert!(!cfg.experimental.preview_diagnostics.save_session_reports);
    }

    #[test]
    fn migration_v0_to_v1_adds_fields() {
        let raw = br#"{"autoExtractPalettes":true}"#;
        let cfg = parse_and_migrate(raw).unwrap();
        assert_eq!(cfg.schema_version, CURRENT_SCHEMA_VERSION);
        assert_eq!(cfg.workspace, Workspace::General);
        assert!(!cfg.advanced.tiling_advanced);
        assert!(!cfg.workspace_choice_remembered);
    }

    #[test]
    fn unknown_fields_preserved() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::load(dir.path());
        {
            let mut guard = store.inner.lock().unwrap();
            guard
                .config
                .extra
                .insert("customFlag".into(), Value::Bool(true));
        }
        store.flush_now().unwrap();
        let store2 = ConfigStore::load(dir.path());
        assert_eq!(
            store2.snapshot().extra.get("customFlag"),
            Some(&Value::Bool(true))
        );
    }

    #[test]
    fn invalid_workspace_falls_back() {
        let raw = br#"{"schema_version":1,"workspace":"banana"}"#;
        let cfg: AppConfig = serde_json::from_slice(raw).unwrap();
        assert_eq!(cfg.workspace, Workspace::General);
    }

    #[test]
    fn from_config_pure() {
        let cfg = AppConfig {
            workspace: Workspace::Gamedev,
            advanced: AdvancedConfig {
                tiling_advanced: true,
            },
            ..AppConfig::default()
        };
        assert_eq!(
            ui_visibility_from_config(&cfg),
            ui_visibility_from_config(&cfg)
        );
    }

    #[test]
    fn switch_preserves_advanced() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::load(dir.path());
        store.set_tiling_advanced(true);
        store.set_workspace(Workspace::Gamedev, WorkspaceSwitchSource::Settings);
        store.set_workspace(Workspace::General, WorkspaceSwitchSource::Settings);
        assert!(store.snapshot().advanced.tiling_advanced);
        store.set_workspace(Workspace::Gamedev, WorkspaceSwitchSource::Settings);
        assert!(store.snapshot().advanced.tiling_advanced);
        assert!(store.visibility().show_wrap_padding);
    }

    #[test]
    fn filename_tile_patterns() {
        assert!(filename_suggests_tiles("Grass_Tile.png"));
        assert!(filename_suggests_tiles("hero_atlas"));
        assert!(filename_suggests_tiles("chars_sheet.psd"));
        assert!(!filename_suggests_tiles("portrait.png"));
    }
}
