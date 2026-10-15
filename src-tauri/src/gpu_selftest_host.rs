//! Host-side GPU family self-test: cache I/O, background run, IPC emit.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use engine_project::algorithms::gpu_selftest::{
    cache_file_for_current, parse_cache_bytes, run_or_use_cache, run_with_timeout, snapshot,
    CacheFile, CacheKey, FailReason, MismatchReport, SelftestSnapshot, SelftestState,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::commands::AppState;

const CACHE_NAME: &str = "gpu_selftest_cache.json";

#[derive(Clone, Debug, Serialize)]
pub struct GpuComputeCapabilityDto {
    pub state: String,
    pub reason: Option<String>,
    pub backend: Option<String>,
    pub adapter: Option<String>,
    pub cache_hit: bool,
    pub checked_at: Option<u64>,
}

impl From<SelftestSnapshot> for GpuComputeCapabilityDto {
    fn from(s: SelftestSnapshot) -> Self {
        Self {
            state: s.state.as_label().to_string(),
            reason: s.state.reason_str().map(|r| r.to_string()),
            backend: s.backend,
            adapter: s.adapter,
            cache_hit: s.cache_hit,
            checked_at: s.checked_at,
        }
    }
}

fn cache_path(app_data: &Path) -> PathBuf {
    app_data.join(CACHE_NAME)
}

fn failures_dir(app_data: &Path) -> PathBuf {
    app_data.join("gpu_selftest_failures")
}

fn read_cache(app_data: &Path) -> Option<CacheFile> {
    let bytes = fs::read(cache_path(app_data)).ok()?;
    parse_cache_bytes(&bytes)
}

fn write_cache(app_data: &Path, file: &CacheFile) {
    let path = cache_path(app_data);
    if let Ok(json) = serde_json::to_vec_pretty(file) {
        let _ = fs::write(path, json);
    }
}

fn write_failure_report(app_data: &Path, report: &MismatchReport) {
    let dir = failures_dir(app_data);
    let _ = fs::create_dir_all(&dir);
    let name = format!("{}.json", report.checked_at);
    if let Ok(json) = serde_json::to_vec_pretty(report) {
        let _ = fs::write(dir.join(name), json);
    }
}

fn emit_status(app: &AppHandle) {
    let dto = GpuComputeCapabilityDto::from(snapshot());
    let _ = app.emit_to(tauri::EventTarget::Any, "gpu-selftest-status", dto);
}

/// Start background self-test when experimental GPU families are enabled.
pub fn start_if_enabled(app: AppHandle, state: Arc<AppState>, app_data: PathBuf) {
    if !state.config.snapshot().experimental.gpu_dither_families {
        return;
    }

    engine_project::algorithms::gpu_selftest::mark_unknown();
    emit_status(&app);

    std::thread::Builder::new()
        .name("gpu-selftest-host".into())
        .spawn(move || {
            // Warm pipelines first so self-test is not charged cold compile alone.
            let _ = engine_gpu::warm_blocked_fs_pipelines();

            let cached = read_cache(&app_data);
            let (st, hit, report) = run_with_timeout(move || run_or_use_cache(cached));

            if let Some(rep) = report.as_ref() {
                write_failure_report(&app_data, rep);
            }

            if !hit {
                if let Some(fp) = engine_gpu::cached_adapter_fingerprint() {
                    let key = CacheKey::from_fingerprint(&fp);
                    if let Some(file) = cache_file_for_current(key) {
                        // Only persist Pass/Fail.
                        match &file.state {
                            SelftestState::Pass | SelftestState::Fail { .. } => {
                                write_cache(&app_data, &file);
                            }
                            _ => {}
                        }
                    }
                } else if matches!(
                    st,
                    SelftestState::Fail {
                        reason: FailReason::SoftwareAdapter
                    }
                ) {
                    // no fingerprint
                }
            }

            // Ensure snapshot reflects result (run_with_timeout already set state).
            let _ = (st, hit);
            emit_status(&app);
        })
        .ok();
}

/// Force a fresh self-test (debug / Preferences).
pub fn rerun(app: AppHandle, app_data: PathBuf) {
    engine_project::algorithms::gpu_selftest::mark_unknown();
    emit_status(&app);
    let _ = fs::remove_file(cache_path(&app_data));
    std::thread::spawn(move || {
        let _ = engine_gpu::warm_blocked_fs_pipelines();
        let (st, _, report) = run_with_timeout(|| run_or_use_cache(None));
        if let Some(rep) = report.as_ref() {
            write_failure_report(&app_data, rep);
        }
        if let Some(fp) = engine_gpu::cached_adapter_fingerprint() {
            let key = CacheKey::from_fingerprint(&fp);
            if let Some(file) = cache_file_for_current(key) {
                write_cache(&app_data, &file);
            }
        }
        let _ = st;
        emit_status(&app);
    });
}

#[tauri::command]
pub fn gpu_compute_capability() -> GpuComputeCapabilityDto {
    GpuComputeCapabilityDto::from(snapshot())
}

#[tauri::command]
pub fn rerun_gpu_selftest(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    // Keep state alive for flag check.
    if !state.config.snapshot().experimental.gpu_dither_families {
        return Err("experimental.gpu_dither_families is off".into());
    }
    rerun(app, dir);
    Ok(())
}
