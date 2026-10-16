//! Host sink for `.local-doc/time-testing.md` profiling.
//! Writes JSONL under `profiling/results/<date>/` when `DITHER_PROFILE=1`.

use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use engine_project::dither_profile::{self, ProfileMark};
use serde::Serialize;
use tauri::State;

use crate::commands::AppState;
use std::sync::Arc;

static OUT_PATH: OnceLock<PathBuf> = OnceLock::new();
static FILE_LOCK: Mutex<()> = Mutex::new(());
static RUN_META: Mutex<Option<RunMeta>> = Mutex::new(None);

#[derive(Clone, Debug, Serialize)]
struct RunMeta {
    scenario_id: String,
    mode: String,
    width: u32,
    height: u32,
    palette_colors: u32,
    pixel_size: u32,
    backend: String,
    cold: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileMarkRequest {
    pub run_id: u64,
    pub mark: String,
    pub epoch_ms: f64,
    #[serde(default)]
    pub generation: Option<u64>,
}

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBeginRequest {
    pub run_id: u64,
    #[serde(default)]
    pub generation: Option<u64>,
    #[serde(default)]
    pub scenario_id: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub palette_colors: Option<u32>,
    #[serde(default)]
    pub pixel_size: Option<u32>,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub cold: Option<bool>,
}

pub fn init() {
    dither_profile::init_from_env();
    if !dither_profile::enabled() {
        return;
    }
    let date = chrono_date();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join(&date);
    if let Err(e) = create_dir_all(&dir) {
        log::warn!(target: "dither_profile", "cannot create {dir:?}: {e}");
        return;
    }
    let path = dir.join("marks.jsonl");
    let _ = OUT_PATH.set(path.clone());
    dither_profile::set_sink(Box::new(move |mark| {
        append_mark(&mark);
    }));
    log::info!(target: "dither_profile", "writing marks to {}", path.display());
}

fn chrono_date() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // YYYY-MM-DD from UTC day; good enough for folder names.
    let days = secs / 86400;
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Howard Hinnant civil_from_days (UTC).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

fn append_mark(mark: &ProfileMark) {
    let Some(path) = OUT_PATH.get() else {
        return;
    };
    let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut line = match serde_json::to_value(mark) {
        Ok(mut v) => {
            if let Ok(meta) = RUN_META.lock() {
                if let Some(m) = meta.as_ref() {
                    if let Some(obj) = v.as_object_mut() {
                        obj.insert(
                            "scenario_id".into(),
                            serde_json::Value::String(m.scenario_id.clone()),
                        );
                        obj.insert("mode".into(), serde_json::Value::String(m.mode.clone()));
                        obj.insert("width".into(), m.width.into());
                        obj.insert("height".into(), m.height.into());
                        obj.insert("palette_colors".into(), m.palette_colors.into());
                        obj.insert("pixel_size".into(), m.pixel_size.into());
                        obj.insert(
                            "backend".into(),
                            serde_json::Value::String(m.backend.clone()),
                        );
                        obj.insert("cold".into(), m.cold.into());
                    }
                }
            }
            match serde_json::to_string(&v) {
                Ok(s) => s,
                Err(_) => return,
            }
        }
        Err(_) => return,
    };
    line.push('\n');
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

#[tauri::command]
pub fn dither_profile_enabled() -> bool {
    dither_profile::enabled()
}

#[tauri::command]
pub fn dither_profile_begin(req: ProfileBeginRequest) -> Result<(), String> {
    if !dither_profile::enabled() {
        return Ok(());
    }
    let gen = req.generation.unwrap_or(0);
    dither_profile::begin_run(req.run_id, gen);
    if let Ok(mut meta) = RUN_META.lock() {
        *meta = Some(RunMeta {
            scenario_id: req.scenario_id.unwrap_or_else(|| format!("run-{}", req.run_id)),
            mode: req.mode.unwrap_or_else(|| "unknown".into()),
            width: req.width.unwrap_or(0),
            height: req.height.unwrap_or(0),
            palette_colors: req.palette_colors.unwrap_or(0),
            pixel_size: req.pixel_size.unwrap_or(1),
            backend: req.backend.unwrap_or_else(|| {
                std::env::var("DITHER_GPU_PATH").unwrap_or_else(|_| "auto".into())
            }),
            cold: req.cold.unwrap_or(false),
        });
    }
    Ok(())
}

#[tauri::command]
pub fn dither_profile_mark(req: ProfileMarkRequest) -> Result<(), String> {
    if !dither_profile::enabled() {
        return Ok(());
    }
    dither_profile::mark_from_frontend(req.run_id, &req.mark, req.epoch_ms, req.generation);
    Ok(())
}

#[tauri::command]
pub fn dither_profile_active(
    _state: State<'_, Arc<AppState>>,
) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "enabled": dither_profile::enabled(),
        "run_id": dither_profile::active_run_id(),
        "generation": dither_profile::active_generation(),
        "out": OUT_PATH.get().map(|p| p.display().to_string()),
    }))
}
