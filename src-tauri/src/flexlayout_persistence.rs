//! Disk persistence for FlexLayout state (v3 schema).
//!
//! Saves and loads FlexLayout Model JSON to a file in the app data directory.
//! FlexLayout uses an internal JSON representation to serialize the complete
//! layout (tabs, tabsets, splitters, floating windows, etc.) to and from disk.
//!
//! This module provides:
//! - `FlexLayoutPersistence`: main struct for save/load operations
//! - `default_layout_json()`: v3 default layout (Layers docked left, no floating)
//! - Graceful degradation: missing/corrupt files → default layout with log
//! - Atomic writes: temp file → rename pattern for data integrity
//! - v2 migration detection: if old `panel_state.json` exists, use default v3 (no translation)

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

/// Errors during FlexLayout persistence operations.
#[derive(Debug, Clone)]
pub enum LayoutPersistenceError {
    /// Failed to read the layout file from disk.
    ReadError(String),
    /// Failed to parse JSON from disk.
    InvalidJson(String),
    /// Layout JSON is missing required fields.
    InvalidSchema(String),
    /// Failed to write the layout file to disk.
    WriteError(String),
    /// Failed to create app data directory.
    DirectoryError(String),
}

impl std::fmt::Display for LayoutPersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutPersistenceError::ReadError(msg) => write!(f, "read error: {}", msg),
            LayoutPersistenceError::InvalidJson(msg) => write!(f, "invalid json: {}", msg),
            LayoutPersistenceError::InvalidSchema(msg) => write!(f, "invalid schema: {}", msg),
            LayoutPersistenceError::WriteError(msg) => write!(f, "write error: {}", msg),
            LayoutPersistenceError::DirectoryError(msg) => write!(f, "directory error: {}", msg),
        }
    }
}

impl std::error::Error for LayoutPersistenceError {}

/// FlexLayout persistence manager.
///
/// Handles save/load of FlexLayout Model JSON with graceful degradation and v2 detection.
pub struct FlexLayoutPersistence {
    app_data_dir: PathBuf,
    /// Filename (without directory) for the layout JSON file.
    /// Defaults to `"flexlayout_state.json"` for backwards compatibility.
    filename: String,
}

impl FlexLayoutPersistence {
    /// Create a new persistence manager with the given app data directory.
    /// Uses the default filename `flexlayout_state.json`.
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            app_data_dir,
            filename: "flexlayout_state.json".to_string(),
        }
    }

    /// Create a persistence manager with a custom filename.
    ///
    /// # Arguments
    /// * `app_data_dir` - Directory where the layout file is stored.
    /// * `filename`     - Filename to use (e.g. `"flexlayout_left.json"`).
    pub fn with_filename(app_data_dir: PathBuf, filename: impl Into<String>) -> Self {
        Self {
            app_data_dir,
            filename: filename.into(),
        }
    }

    /// Save layout JSON to disk with atomic write (temp → rename).
    ///
    /// Creates the app data directory if it does not exist.
    /// Uses atomic write pattern: write to temp file, then rename.
    ///
    /// # Arguments
    /// * `layout_json` - Serialized FlexLayout Model JSON string.
    ///
    /// # Returns
    /// - `Ok(())` on success
    /// - `Err(LayoutPersistenceError)` on any failure (read/write/dir creation)
    pub fn save(&self, layout_json: &str) -> Result<(), LayoutPersistenceError> {
        // Validate JSON before writing
        serde_json::from_str::<Value>(layout_json)
            .map_err(|e| LayoutPersistenceError::InvalidJson(format!("invalid layout json: {}", e)))?;

        // Ensure app data directory exists
        fs::create_dir_all(&self.app_data_dir)
            .map_err(|e| LayoutPersistenceError::DirectoryError(format!("{}", e)))?;

        let layout_path = self.layout_file_path();
        let temp_path = layout_path.with_extension("json.tmp");

        // Write to temp file first
        fs::write(&temp_path, layout_json)
            .map_err(|e| LayoutPersistenceError::WriteError(format!(
                "failed to write temp file: {}",
                e
            )))?;

        // Atomic rename: temp → final location
        fs::rename(&temp_path, &layout_path)
            .map_err(|e| LayoutPersistenceError::WriteError(format!(
                "failed to rename temp file: {}",
                e
            )))?;

        Ok(())
    }

    /// Load layout JSON from disk, with v2 detection and graceful degradation.
    ///
    /// Returns:
    /// - `Ok(layout_json)` if file exists and is valid v3 JSON
    /// - `Ok(default_layout_json())` if:
    ///   - File does not exist (first run)
    ///   - File is corrupt/invalid JSON (graceful fallback)
    ///   - v2 layout file (`panel_state.json`) is detected (migration: use v3 default)
    /// - `Err(...)` only on disk I/O errors that prevent recovery
    ///
    /// All error cases are logged (except missing file on first run).
    pub fn load(&self) -> Result<String, LayoutPersistenceError> {
        // Check for v2 layout file (old system) — if present, use default v3
        if self.detect_v2_migration() {
            log::info!("Detected v2 layout file; using default v3 layout for migration");
            return Ok(Self::default_layout_json());
        }

        let layout_path = self.layout_file_path();

        // Read v3 layout file
        let contents = match fs::read_to_string(&layout_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // First run: no layout file yet
                log::info!("No layout file found; using default v3 layout");
                return Ok(Self::default_layout_json());
            }
            Err(e) => {
                return Err(LayoutPersistenceError::ReadError(format!(
                    "failed to read layout file: {}",
                    e
                )));
            }
        };

        // Validate and parse JSON
        match serde_json::from_str::<Value>(&contents) {
            Ok(_) => Ok(contents),
            Err(e) => {
                log::warn!(
                    "Failed to parse layout JSON: {}; using default layout",
                    e
                );
                Ok(Self::default_layout_json())
            }
        }
    }

    /// Reset layout to default v3 (called on explicit "reset layout" action).
    pub fn reset_to_default(&self) -> Result<String, LayoutPersistenceError> {
        Ok(Self::default_layout_json())
    }

    /// Default v3 layout JSON: Layers panel docked on left, no floating windows.
    ///
    /// This is the canonical default layout for B3 (Layers on FlexLayout).
    /// Schema:
    /// - One BorderNode (container)
    /// - One TabSetNode (left, for Layers)
    /// - TabNode for Layers panel
    pub fn default_layout_json() -> String {
        // Default v3 layout: Layers docked left, no floating windows
        json!({
            "version": 3,
            "root": {
                "type": "border",
                "id": "root",
                "children": [
                    {
                        "type": "tabset",
                        "id": "tabset-left",
                        "weight": 20,
                        "selected": 0,
                        "position": "left",
                        "children": [
                            {
                                "type": "tab",
                                "id": "tab-layers",
                                "name": "Layers",
                                "component": "layers",
                                "icon": "icon-layers"
                            }
                        ]
                    }
                ],
                "sizes": [20, 80]
            }
        })
        .to_string()
    }

    /// Detect if v2 layout file exists (old PanelManager system).
    ///
    /// Returns `true` if `panel_state.json` exists in app data dir.
    /// This signals that the app is migrating from v2 to v3 layout system.
    fn detect_v2_migration(&self) -> bool {
        let v2_path = self.app_data_dir.join("panel_state.json");
        v2_path.exists()
    }

    /// Get the full path to the layout file.
    fn layout_file_path(&self) -> PathBuf {
        self.app_data_dir.join(&self.filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn save_and_load_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let layout_json = r#"{"version": 3, "root": {"type": "border", "id": "root", "children": []}}"#;

        // Save
        persistence.save(layout_json).unwrap();

        // Load
        let loaded = persistence.load().unwrap();
        assert_eq!(loaded, layout_json);
    }

    #[test]
    fn save_and_load_with_complex_layout() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let complex_layout = r#"{"version": 3, "root": {"type": "border", "id": "root", "children": [{"type": "tabset", "id": "ts1", "selected": 0, "children": [{"type": "tab", "id": "tab1", "name": "Layers", "component": "layers"}]}]}}"#;

        persistence.save(complex_layout).unwrap();
        let loaded = persistence.load().unwrap();
        
        // Verify structure is preserved exactly
        let original_value: Value = serde_json::from_str(complex_layout).unwrap();
        let loaded_value: Value = serde_json::from_str(&loaded).unwrap();
        assert_eq!(original_value, loaded_value);
    }

    #[test]
    fn missing_file_returns_default() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        // Load without saving anything
        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("\"version\": 3"));
        assert!(loaded.contains("Layers"));
        
        // Verify it's valid JSON
        let value: Value = serde_json::from_str(&loaded).unwrap();
        assert_eq!(value["version"], 3);
        assert_eq!(value["root"]["type"], "border");
    }

    #[test]
    fn corrupt_json_falls_back_to_default() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        // Write corrupt JSON directly
        let layout_path = temp_dir.path().join("flexlayout_state.json");
        fs::write(&layout_path, "{ this is not valid json }").unwrap();

        // Load should fall back to default (logs warning, does NOT crash)
        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("\"version\": 3"));
        assert!(loaded.contains("Layers"));
        
        // Verify returned JSON is valid
        let value: Value = serde_json::from_str(&loaded).unwrap();
        assert_eq!(value["version"], 3);
    }

    #[test]
    fn empty_json_falls_back_to_default() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let layout_path = temp_dir.path().join("flexlayout_state.json");
        fs::write(&layout_path, "{}").unwrap();

        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("\"version\": 3"));
    }

    #[test]
    fn v2_file_detection_returns_default_v3() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        // Create v2 layout file (old system)
        let v2_path = temp_dir.path().join("panel_state.json");
        fs::write(&v2_path, r#"{"version": 2, "panels": [], "left_order": [], "right_order": []}"#).unwrap();

        // Load should detect v2 and return default v3 (no translation)
        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("\"version\": 3"));
        assert!(!loaded.contains("\"version\": 2"));

        // v2 file should still exist (not deleted, migration is non-destructive)
        assert!(v2_path.exists());
    }

    #[test]
    fn v2_file_exists_with_v3_file_prefers_v3() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        // Create both v2 and v3 files
        let v2_path = temp_dir.path().join("panel_state.json");
        fs::write(&v2_path, r#"{"version": 2, "panels": []}"#).unwrap();

        let v3_layout = r#"{"version": 3, "root": {"type": "border", "id": "root-custom"}}"#;
        let v3_path = temp_dir.path().join("flexlayout_state.json");
        fs::write(&v3_path, v3_layout).unwrap();

        // Load should prefer v3 (which exists) over v2
        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("root-custom"));
        assert_eq!(loaded, v3_layout);
    }

    #[test]
    fn v2_detection_explicit_test_v2_fallback_no_translation() {
        // This is the explicit test for "v2 file exists → v2 detection → default v3 returned, no translation attempted"
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        // Create v2 file with specific panel configuration
        let v2_content = r#"{
            "version": 2,
            "panels": [
                {"id": "layers", "docked": true, "visible": true, "window_label": null, "saved_bounds": null, "dock_side": "left"},
                {"id": "effect", "docked": true, "visible": true, "window_label": null, "saved_bounds": null, "dock_side": "right"}
            ],
            "left_order": ["layers"],
            "right_order": ["effect"]
        }"#;
        
        let v2_path = temp_dir.path().join("panel_state.json");
        fs::write(&v2_path, v2_content).unwrap();

        // Load should detect v2 and return default v3, NOT attempt to translate v2 structure
        let loaded = persistence.load().unwrap();
        let value: Value = serde_json::from_str(&loaded).unwrap();

        // Verify it's v3 default structure, not translated v2
        assert_eq!(value["version"], 3);
        assert_eq!(value["root"]["type"], "border");
        assert!(value["root"].get("children").is_some());
        
        // Verify v2 concepts are NOT in the result (no panels array, no left_order/right_order at root)
        assert!(value.get("panels").is_none());
        assert!(value.get("left_order").is_none());
        assert!(value.get("right_order").is_none());
    }

    #[test]
    fn save_invalid_json_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let result = persistence.save("{ invalid json }");
        assert!(result.is_err());
        
        match result {
            Err(LayoutPersistenceError::InvalidJson(_)) => {} // expected
            _ => panic!("Expected InvalidJson error"),
        }
    }

    #[test]
    fn save_empty_string_returns_error() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let result = persistence.save("");
        assert!(result.is_err());
    }

    #[test]
    fn atomic_write_temp_cleanup_on_success() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let layout_json = r#"{"version": 3, "root": {"type": "border"}}"#;
        persistence.save(layout_json).unwrap();

        // Verify temp file is gone (atomic rename succeeded)
        let temp_path = temp_dir.path().join("flexlayout_state.json.tmp");
        assert!(!temp_path.exists());

        // Final file should exist
        let final_path = temp_dir.path().join("flexlayout_state.json");
        assert!(final_path.exists());
    }

    #[test]
    fn multiple_saves_are_atomic() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let layout1 = r#"{"version": 3, "root": {"id": "first"}}"#;
        let layout2 = r#"{"version": 3, "root": {"id": "second"}}"#;

        persistence.save(layout1).unwrap();
        let loaded1 = persistence.load().unwrap();
        assert!(loaded1.contains("first"));

        persistence.save(layout2).unwrap();
        let loaded2 = persistence.load().unwrap();
        assert!(loaded2.contains("second"));

        // Verify no temp files remain
        let temp_path = temp_dir.path().join("flexlayout_state.json.tmp");
        assert!(!temp_path.exists());
    }

    #[test]
    fn default_layout_json_is_valid_v3() {
        let json = FlexLayoutPersistence::default_layout_json();
        let value: Value = serde_json::from_str(&json).expect("default layout must be valid json");
        assert_eq!(value["version"], 3);
        assert_eq!(value["root"]["type"], "border");
        assert!(value["root"].get("children").is_some());
    }

    #[test]
    fn default_layout_contains_layers_panel() {
        let json = FlexLayoutPersistence::default_layout_json();
        assert!(json.contains("Layers"));
        assert!(json.contains("layers"));
        assert!(json.contains("tab-layers"));
    }

    #[test]
    fn reset_to_default_returns_v3() {
        let temp_dir = TempDir::new().unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().to_path_buf());

        let result = persistence.reset_to_default().unwrap();
        assert!(result.contains("\"version\": 3"));
        
        let value: Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["version"], 3);
    }

    #[test]
    fn save_creates_app_data_directory_if_missing() {
        let temp_dir = TempDir::new().unwrap();
        let nested_dir = temp_dir.path().join("app").join("data");
        
        // Verify nested directory doesn't exist
        assert!(!nested_dir.exists());

        let persistence = FlexLayoutPersistence::new(nested_dir.clone());
        let layout_json = r#"{"version": 3, "root": {"type": "border"}}"#;
        
        // Save should create the directory
        persistence.save(layout_json).unwrap();
        
        // Verify directory was created
        assert!(nested_dir.exists());
        
        // Verify file was written
        let file_path = nested_dir.join("flexlayout_state.json");
        assert!(file_path.exists());
    }

    #[test]
    fn graceful_degradation_coverage() {
        // Comprehensive test verifying all graceful degradation paths
        let temp_dir = TempDir::new().unwrap();

        // Test 1: Completely missing directory and file
        let persistence = FlexLayoutPersistence::new(temp_dir.path().join("scenario1").to_path_buf());
        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("\"version\": 3"));

        // Test 2: Directory exists but file missing
        fs::create_dir_all(temp_dir.path().join("scenario2")).unwrap();
        let persistence = FlexLayoutPersistence::new(temp_dir.path().join("scenario2").to_path_buf());
        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("\"version\": 3"));

        // Test 3: V2 migration scenario
        let scenario3 = temp_dir.path().join("scenario3");
        fs::create_dir_all(&scenario3).unwrap();
        fs::write(scenario3.join("panel_state.json"), r#"{"version": 2}"#).unwrap();
        let persistence = FlexLayoutPersistence::new(scenario3);
        let loaded = persistence.load().unwrap();
        assert!(loaded.contains("\"version\": 3"));
        assert!(!loaded.contains("\"version\": 2"));
    }
}
