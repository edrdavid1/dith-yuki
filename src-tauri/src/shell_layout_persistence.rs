//! W7: single `layout.json` envelope for left/center/right FlexLayout models.
//!
//! Envelope:
//! ```json
//! { "version": 1, "savedAt": "<ISO>", "left": {…}, "center": {…}, "right": {…} }
//! ```
//! Atomic write: temp + fsync + rename; previous file kept as `layout.json.bak`.
//! Recovery: layout.json → layout.json.bak → legacy side files → defaults.
//! Corrupt primary is renamed to `layout.json.corrupt-<ts>`.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const LAYOUT_FILENAME: &str = "layout.json";
pub const LAYOUT_BAK_FILENAME: &str = "layout.json.bak";
pub const LAYOUT_VERSION: u64 = 1;

const LEGACY_LEFT: &str = "flexlayout_left.json";
const LEGACY_RIGHT: &str = "flexlayout_right.json";
const LEGACY_CENTER: &str = "flexlayout_center.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutLoadNotice {
    Migrated,
    RecoveredBak,
    CorruptReset,
    Defaults,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutEnvelope {
    pub version: u64,
    #[serde(rename = "savedAt")]
    pub saved_at: String,
    pub left: Value,
    pub center: Value,
    pub right: Value,
    /// Present on load responses only (not persisted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<LayoutLoadNotice>,
}

#[derive(Debug, Clone)]
pub enum ShellLayoutError {
    Read(String),
    Write(String),
    InvalidJson(String),
    InvalidSchema(String),
    Directory(String),
}

impl std::fmt::Display for ShellLayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellLayoutError::Read(m) => write!(f, "read: {m}"),
            ShellLayoutError::Write(m) => write!(f, "write: {m}"),
            ShellLayoutError::InvalidJson(m) => write!(f, "invalid json: {m}"),
            ShellLayoutError::InvalidSchema(m) => write!(f, "invalid schema: {m}"),
            ShellLayoutError::Directory(m) => write!(f, "directory: {m}"),
        }
    }
}

impl std::error::Error for ShellLayoutError {}

/// Mutex-serialized shell layout persistence.
pub struct ShellLayoutPersistence {
    app_data_dir: PathBuf,
    write_lock: Mutex<()>,
}

impl ShellLayoutPersistence {
    pub fn new(app_data_dir: PathBuf) -> Self {
        Self {
            app_data_dir,
            write_lock: Mutex::new(()),
        }
    }

    pub fn layout_path(&self) -> PathBuf {
        self.app_data_dir.join(LAYOUT_FILENAME)
    }

    pub fn bak_path(&self) -> PathBuf {
        self.app_data_dir.join(LAYOUT_BAK_FILENAME)
    }

    /// Save envelope atomically (serialized). Always writes all three sides.
    pub fn save(&self, envelope: &LayoutEnvelope) -> Result<(), ShellLayoutError> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|e| ShellLayoutError::Write(format!("lock poisoned: {e}")))?;

        if envelope.version != LAYOUT_VERSION {
            return Err(ShellLayoutError::InvalidSchema(format!(
                "unsupported version {}",
                envelope.version
            )));
        }
        validate_side(&envelope.left, "left")?;
        validate_side(&envelope.center, "center")?;
        validate_side(&envelope.right, "right")?;

        fs::create_dir_all(&self.app_data_dir)
            .map_err(|e| ShellLayoutError::Directory(format!("{e}")))?;

        let mut to_write = envelope.clone();
        to_write.notice = None;
        if to_write.saved_at.is_empty() {
            to_write.saved_at = now_iso();
        }

        let bytes = serde_json::to_vec_pretty(&to_write)
            .map_err(|e| ShellLayoutError::InvalidJson(format!("{e}")))?;

        atomic_write_with_bak(&self.layout_path(), &self.bak_path(), &bytes)
    }

    /// Load with recovery + optional legacy migration.
    pub fn load(&self) -> Result<LayoutEnvelope, ShellLayoutError> {
        let _guard = self
            .write_lock
            .lock()
            .map_err(|e| ShellLayoutError::Read(format!("lock poisoned: {e}")))?;

        let primary = self.layout_path();
        match try_read_envelope(&primary) {
            Ok(mut env) => {
                env.notice = None;
                return Ok(env);
            }
            Err(ReadFail::Missing) => {}
            Err(ReadFail::Corrupt(reason)) => {
                eprintln!("[flex-dock] layout.json corrupt: {reason}");
                quarantine_corrupt(&primary);
                if let Ok(mut env) = try_read_envelope(&self.bak_path()) {
                    eprintln!("[flex-dock] recovered layout from layout.json.bak");
                    env.notice = Some(LayoutLoadNotice::RecoveredBak);
                    // Re-persist recovered bak as primary (best-effort).
                    let _ = self.save_unlocked(&env);
                    return Ok(env);
                }
                let mut env = default_envelope();
                env.notice = Some(LayoutLoadNotice::CorruptReset);
                return Ok(env);
            }
        }

        // Primary missing — try bak.
        if let Ok(mut env) = try_read_envelope(&self.bak_path()) {
            eprintln!("[flex-dock] layout.json missing; using layout.json.bak");
            env.notice = Some(LayoutLoadNotice::RecoveredBak);
            let _ = self.save_unlocked(&env);
            return Ok(env);
        }

        // Migrate legacy side files.
        if let Some(mut env) = self.try_migrate_legacy() {
            env.notice = Some(LayoutLoadNotice::Migrated);
            if let Err(err) = self.save_unlocked(&env) {
                eprintln!("[flex-dock] migration save failed: {err}");
            } else {
                self.rename_legacy_to_migrated();
            }
            return Ok(env);
        }

        let mut env = default_envelope();
        env.notice = Some(LayoutLoadNotice::Defaults);
        Ok(env)
    }

    fn save_unlocked(&self, envelope: &LayoutEnvelope) -> Result<(), ShellLayoutError> {
        // Caller holds write_lock.
        let mut to_write = envelope.clone();
        to_write.notice = None;
        if to_write.saved_at.is_empty() {
            to_write.saved_at = now_iso();
        }
        validate_side(&to_write.left, "left")?;
        validate_side(&to_write.center, "center")?;
        validate_side(&to_write.right, "right")?;
        fs::create_dir_all(&self.app_data_dir)
            .map_err(|e| ShellLayoutError::Directory(format!("{e}")))?;
        let bytes = serde_json::to_vec_pretty(&to_write)
            .map_err(|e| ShellLayoutError::InvalidJson(format!("{e}")))?;
        atomic_write_with_bak(&self.layout_path(), &self.bak_path(), &bytes)
    }

    fn try_migrate_legacy(&self) -> Option<LayoutEnvelope> {
        let left = read_legacy_side(&self.app_data_dir.join(LEGACY_LEFT));
        let right = read_legacy_side(&self.app_data_dir.join(LEGACY_RIGHT));
        let center = read_legacy_side(&self.app_data_dir.join(LEGACY_CENTER));
        if left.is_none() && right.is_none() && center.is_none() {
            return None;
        }
        eprintln!(
            "[flex-dock] migrating legacy flexlayout_{{left,right,center}}.json → layout.json"
        );
        Some(LayoutEnvelope {
            version: LAYOUT_VERSION,
            saved_at: now_iso(),
            left: left.unwrap_or_else(default_side_stub),
            center: center.unwrap_or_else(default_side_stub),
            right: right.unwrap_or_else(default_side_stub),
            notice: None,
        })
    }

    fn rename_legacy_to_migrated(&self) {
        for name in [LEGACY_LEFT, LEGACY_RIGHT, LEGACY_CENTER] {
            let src = self.app_data_dir.join(name);
            if !src.exists() {
                continue;
            }
            let dest = self.app_data_dir.join(format!("{name}.migrated"));
            match fs::rename(&src, &dest) {
                Ok(()) => eprintln!("[flex-dock] renamed {name} → {name}.migrated"),
                Err(err) => eprintln!("[flex-dock] rename {name} failed: {err}"),
            }
        }
    }
}

fn default_envelope() -> LayoutEnvelope {
    LayoutEnvelope {
        version: LAYOUT_VERSION,
        saved_at: now_iso(),
        left: default_side_stub(),
        center: default_side_stub(),
        right: default_side_stub(),
        notice: None,
    }
}

/// Minimal IJsonModel-shaped stub; FE replaces with real defaults when empty.
fn default_side_stub() -> Value {
    json!({
        "global": {},
        "borders": [],
        "layout": {
            "type": "row",
            "weight": 100,
            "children": [{
                "type": "tabset",
                "weight": 100,
                "children": []
            }]
        }
    })
}

fn validate_side(value: &Value, side: &str) -> Result<(), ShellLayoutError> {
    let layout = value
        .get("layout")
        .ok_or_else(|| ShellLayoutError::InvalidSchema(format!("{side}: missing layout")))?;
    if layout.get("type").and_then(|t| t.as_str()) != Some("row") {
        return Err(ShellLayoutError::InvalidSchema(format!(
            "{side}: layout.type must be row"
        )));
    }
    Ok(())
}

enum ReadFail {
    Missing,
    Corrupt(String),
}

fn try_read_envelope(path: &Path) -> Result<LayoutEnvelope, ReadFail> {
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(ReadFail::Missing),
        Err(e) => return Err(ReadFail::Corrupt(format!("io: {e}"))),
    };
    let value: Value = serde_json::from_str(&contents)
        .map_err(|e| ReadFail::Corrupt(format!("parse: {e}")))?;
    parse_envelope_value(value).map_err(ReadFail::Corrupt)
}

fn parse_envelope_value(value: Value) -> Result<LayoutEnvelope, String> {
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| "missing version".to_string())?;
    if version != LAYOUT_VERSION {
        return Err(format!("unsupported version {version}"));
    }
    let saved_at = value
        .get("savedAt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let left = value
        .get("left")
        .cloned()
        .ok_or_else(|| "missing left".to_string())?;
    let center = value
        .get("center")
        .cloned()
        .ok_or_else(|| "missing center".to_string())?;
    let right = value
        .get("right")
        .cloned()
        .ok_or_else(|| "missing right".to_string())?;
    validate_side(&left, "left").map_err(|e| e.to_string())?;
    validate_side(&center, "center").map_err(|e| e.to_string())?;
    validate_side(&right, "right").map_err(|e| e.to_string())?;
    Ok(LayoutEnvelope {
        version,
        saved_at,
        left,
        center,
        right,
        notice: None,
    })
}

fn read_legacy_side(path: &Path) -> Option<Value> {
    let contents = fs::read_to_string(path).ok()?;
    let value: Value = serde_json::from_str(&contents).ok()?;
    // Accept raw IJsonModel.
    if value.get("layout").is_some() {
        return Some(value);
    }
    None
}

fn quarantine_corrupt(path: &Path) {
    if !path.exists() {
        return;
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dest = path.with_file_name(format!("layout.json.corrupt-{ts}"));
    match fs::rename(path, &dest) {
        Ok(()) => eprintln!(
            "[flex-dock] quarantined corrupt layout → {}",
            dest.file_name().unwrap_or_default().to_string_lossy()
        ),
        Err(err) => eprintln!("[flex-dock] quarantine failed: {err}"),
    }
}

fn atomic_write_with_bak(final_path: &Path, bak_path: &Path, bytes: &[u8]) -> Result<(), ShellLayoutError> {
    let tmp_path = final_path.with_extension("json.tmp");

    {
        let mut file = File::create(&tmp_path)
            .map_err(|e| ShellLayoutError::Write(format!("create temp: {e}")))?;
        file
            .write_all(bytes)
            .map_err(|e| ShellLayoutError::Write(format!("write temp: {e}")))?;
        file
            .sync_all()
            .map_err(|e| ShellLayoutError::Write(format!("fsync temp: {e}")))?;
    }

    if final_path.exists() {
        // Keep previous good file as .bak (overwrite prior bak).
        fs::copy(final_path, bak_path)
            .map_err(|e| ShellLayoutError::Write(format!("backup: {e}")))?;
    }

    fs::rename(&tmp_path, final_path)
        .map_err(|e| ShellLayoutError::Write(format!("rename: {e}")))?;

    Ok(())
}

fn now_iso() -> String {
    // Prefer chrono-free ISO-ish UTC seconds stamp.
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_side(tag: &str) -> Value {
        json!({
            "global": {},
            "borders": [],
            "layout": {
                "type": "row",
                "weight": 100,
                "children": [{
                    "type": "tabset",
                    "weight": 100,
                    "children": [{
                        "type": "tab",
                        "name": tag,
                        "component": tag
                    }]
                }]
            }
        })
    }

    fn sample_envelope() -> LayoutEnvelope {
        LayoutEnvelope {
            version: LAYOUT_VERSION,
            saved_at: "test".into(),
            left: sample_side("layers"),
            center: sample_side("preview"),
            right: sample_side("effect"),
            notice: None,
        }
    }

    #[test]
    fn save_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let p = ShellLayoutPersistence::new(dir.path().to_path_buf());
        p.save(&sample_envelope()).unwrap();
        let loaded = p.load().unwrap();
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.left["layout"]["children"][0]["children"][0]["component"], "layers");
        assert!(loaded.notice.is_none());
        assert!(dir.path().join(LAYOUT_FILENAME).exists());
    }

    #[test]
    fn atomic_write_keeps_bak_and_no_tmp() {
        let dir = TempDir::new().unwrap();
        let p = ShellLayoutPersistence::new(dir.path().to_path_buf());
        p.save(&sample_envelope()).unwrap();

        let mut second = sample_envelope();
        second.left = sample_side("layers2");
        p.save(&second).unwrap();

        assert!(dir.path().join(LAYOUT_BAK_FILENAME).exists());
        assert!(!dir.path().join("layout.json.tmp").exists());
        let bak: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join(LAYOUT_BAK_FILENAME)).unwrap())
                .unwrap();
        assert_eq!(bak["left"]["layout"]["children"][0]["children"][0]["component"], "layers");
    }

    #[test]
    fn interrupted_temp_does_not_damage_primary() {
        let dir = TempDir::new().unwrap();
        let p = ShellLayoutPersistence::new(dir.path().to_path_buf());
        p.save(&sample_envelope()).unwrap();
        let before = fs::read_to_string(dir.path().join(LAYOUT_FILENAME)).unwrap();

        // Simulate leftover tmp from a crash mid-write — primary must stay intact.
        fs::write(dir.path().join("layout.json.tmp"), "{broken").unwrap();
        let loaded = p.load().unwrap();
        assert_eq!(loaded.left["layout"]["children"][0]["children"][0]["component"], "layers");
        let after = fs::read_to_string(dir.path().join(LAYOUT_FILENAME)).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn recover_from_bak_when_primary_corrupt() {
        let dir = TempDir::new().unwrap();
        let p = ShellLayoutPersistence::new(dir.path().to_path_buf());
        p.save(&sample_envelope()).unwrap();
        // Force bak = good, primary = corrupt
        fs::copy(
            dir.path().join(LAYOUT_FILENAME),
            dir.path().join(LAYOUT_BAK_FILENAME),
        )
        .unwrap();
        fs::write(dir.path().join(LAYOUT_FILENAME), "{not json").unwrap();

        let loaded = p.load().unwrap();
        assert_eq!(loaded.notice, Some(LayoutLoadNotice::RecoveredBak));
        assert_eq!(loaded.left["layout"]["children"][0]["children"][0]["component"], "layers");
        // Corrupt quarantined
        let corrupt: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("layout.json.corrupt-")
            })
            .collect();
        assert_eq!(corrupt.len(), 1);
    }

    #[test]
    fn migrate_legacy_side_files() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join(LEGACY_LEFT),
            serde_json::to_string(&sample_side("layers")).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.path().join(LEGACY_RIGHT),
            serde_json::to_string(&sample_side("effect")).unwrap(),
        )
        .unwrap();
        fs::write(
            dir.path().join(LEGACY_CENTER),
            serde_json::to_string(&sample_side("preview")).unwrap(),
        )
        .unwrap();

        let p = ShellLayoutPersistence::new(dir.path().to_path_buf());
        let loaded = p.load().unwrap();
        assert_eq!(loaded.notice, Some(LayoutLoadNotice::Migrated));
        assert!(dir.path().join(LAYOUT_FILENAME).exists());
        assert!(dir.path().join(format!("{LEGACY_LEFT}.migrated")).exists());
        assert!(!dir.path().join(LEGACY_LEFT).exists());
    }

    #[test]
    fn missing_everything_returns_defaults_notice() {
        let dir = TempDir::new().unwrap();
        let p = ShellLayoutPersistence::new(dir.path().to_path_buf());
        let loaded = p.load().unwrap();
        assert_eq!(loaded.notice, Some(LayoutLoadNotice::Defaults));
        assert_eq!(loaded.version, 1);
    }
}
