//! Batch folder export via headless API (no AppState tile path).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use engine_project::{
    expand_output_name, flatten_bottom_to_top, run_headless_batch, FilterInstance,
    HeadlessJob, HeadlessJobResult, HeadlessOutputFormat, LayerRef,
};
use engine_color::palette::Palette;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::commands::AppState;
use crate::services::document_service::IMAGE_IMPORT_EXTENSIONS;

#[derive(Debug, Clone, Deserialize)]
pub struct BatchExportRequest {
    pub doc_id: u32,
    /// Layer whose filter stack is applied; default = first leaf with filters, else first leaf.
    #[serde(default)]
    pub layer_id: Option<u32>,
    pub input_dir: String,
    pub output_dir: String,
    /// e.g. `{name}_dithered.png` — `{name}` / `{ext}` expanded per file.
    #[serde(default = "default_name_template")]
    pub name_template: String,
    #[serde(default = "default_format")]
    pub format: HeadlessOutputFormat,
    #[serde(default = "default_true")]
    pub lock_pattern_phase: bool,
}

fn default_name_template() -> String {
    "{name}_dithered.png".into()
}

fn default_format() -> HeadlessOutputFormat {
    HeadlessOutputFormat::Png
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchExportProgress {
    pub done: usize,
    pub total: usize,
    pub current_input: String,
    pub last_error: Option<String>,
    pub stage: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct BatchExportSummary {
    pub succeeded: usize,
    pub failed: usize,
    pub cancelled: bool,
    pub results: Vec<HeadlessJobResultDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeadlessJobResultDto {
    pub input: String,
    pub output: Option<String>,
    pub error: Option<String>,
}

impl From<&HeadlessJobResult> for HeadlessJobResultDto {
    fn from(r: &HeadlessJobResult) -> Self {
        Self {
            input: r.input.to_string_lossy().into_owned(),
            output: r.output.as_ref().map(|p| p.to_string_lossy().into_owned()),
            error: r.error.clone(),
        }
    }
}

fn collect_input_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read input folder: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read input folder: {e}"))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if IMAGE_IMPORT_EXTENSIONS.contains(&ext.as_str()) {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(format!(
            "No supported images in {} (png, jpg, jpeg, webp)",
            dir.display()
        ));
    }
    Ok(files)
}

fn extract_preset(
    state: &AppState,
    doc_id: u32,
    layer_id: Option<u32>,
) -> Result<(Vec<FilterInstance>, Vec<Palette>), String> {
    let session = state
        .session(doc_id)
        .map_err(|_| format!("Document was closed (id {doc_id}); cannot batch export"))?;
    let snapshot = session.document_handle.snapshot();
    let palettes = snapshot.palettes.clone();

    let leaves: Vec<&engine_project::Layer> = flatten_bottom_to_top(&snapshot.root)
        .into_iter()
        .filter_map(|r| match r {
            LayerRef::Leaf(l) => Some(l),
            _ => None,
        })
        .collect();

    let layer = if let Some(id) = layer_id {
        leaves
            .iter()
            .find(|l| l.id.0 == id)
            .copied()
            .ok_or_else(|| format!("Layer {id} not found"))?
    } else {
        leaves
            .iter()
            .find(|l| !l.filters.is_empty())
            .or_else(|| leaves.first())
            .copied()
            .ok_or_else(|| "Document has no layers to take a filter stack from".to_string())?
    };

    Ok((layer.filters.clone(), palettes))
}

#[tauri::command]
pub async fn batch_export_run(
    req: BatchExportRequest,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<BatchExportSummary, String> {
    let state = Arc::clone(state.inner());
    let state_clear = Arc::clone(&state);

    let (filters, palettes) = extract_preset(&state, req.doc_id, req.layer_id)?;
    let input_dir = PathBuf::from(&req.input_dir);
    let output_dir = PathBuf::from(&req.output_dir);
    let inputs = collect_input_files(&input_dir)?;

    let jobs: Vec<HeadlessJob> = inputs
        .iter()
        .map(|input| {
            let name = expand_output_name(&req.name_template, input);
            HeadlessJob {
                input: input.clone(),
                output: output_dir.join(name),
                format: req.format,
                filters: filters.clone(),
                palettes: palettes.clone(),
                lock_pattern_phase: req.lock_pattern_phase,
                png8_palette_id: None,
            }
        })
        .collect();

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state
            .batch_export_cancel
            .lock()
            .map_err(|e| e.to_string())?;
        *slot = Some(Arc::clone(&cancel));
    }

    let result = tauri::async_runtime::spawn_blocking(move || {
        let results = run_headless_batch(&jobs, &cancel, |done, total, latest| {
            let _ = app_handle.emit(
                "batch_export_progress",
                BatchExportProgress {
                    done,
                    total,
                    current_input: latest.input.to_string_lossy().into_owned(),
                    last_error: latest.error.clone(),
                    stage: if done >= total {
                        "done".into()
                    } else {
                        "running".into()
                    },
                },
            );
        });

        let cancelled = cancel.load(Ordering::Relaxed)
            || results.iter().any(|r| {
                r.error
                    .as_deref()
                    .is_some_and(|e| e == "cancelled" || e.contains("cancelled"))
            });
        let succeeded = results.iter().filter(|r| r.error.is_none()).count();
        let failed = results.len().saturating_sub(succeeded);

        let _ = app_handle.emit(
            "batch_export_progress",
            BatchExportProgress {
                done: results.len(),
                total: results.len(),
                current_input: String::new(),
                last_error: None,
                stage: if cancelled {
                    "cancelled".into()
                } else {
                    "done".into()
                },
            },
        );

        BatchExportSummary {
            succeeded,
            failed,
            cancelled,
            results: results.iter().map(HeadlessJobResultDto::from).collect(),
        }
    })
    .await
    .map_err(|e| format!("batch export task failed: {e}"))?;

    if let Ok(mut slot) = state_clear.batch_export_cancel.lock() {
        *slot = None;
    }

    Ok(result)
}

#[tauri::command]
pub fn batch_export_cancel(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if let Some(flag) = state
        .batch_export_cancel
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
    {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}
