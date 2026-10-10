//! CMYK print-export Tauri commands (TIFF + ICC). Does not mark the document dirty.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use engine_color::{
    estimate_export, gamut_report, rgba8_to_cmyk8, scale_cmyk8_nearest, validate_config,
    ExportSummary, GamutReport, PrintExportConfig, PrintExportEstimate, PrintExportTransform,
    TiffCompression,
};
use engine_io::{write_cmyk_tiff_atomic, CmykTiffCompression};
use engine_project::serialize::build_processed_composite_rgba8;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::commands::AppState;
use crate::services::AppError;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PrintExportProgress {
    pub doc_id: u32,
    pub stage: String,
    pub fraction: f32,
}

fn proof_service(state: &AppState) -> Result<Arc<crate::services::ProofService>, String> {
    state
        .proof
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "soft-proof catalog not initialized".into())
}

fn load_rgba(
    state: &AppState,
    doc_id: u32,
) -> Result<(u32, u32, Vec<u8>, PrintExportConfig), AppError> {
    let session = state.session(doc_id).map_err(|_| {
        AppError::Generic(format!(
            "Document was closed (id {doc_id}); cannot export for print"
        ))
    })?;
    let snapshot = session.document_handle.snapshot();
    let width = snapshot.width;
    let height = snapshot.height;
    let soft = snapshot.soft_proof.clone();
    let doc = (*snapshot).clone();
    drop(snapshot);

    let rgba = build_processed_composite_rgba8(&state.tiles.tile_cache, &doc).map_err(|e| {
        AppError::Generic(format!("Print export composite error: {e}"))
    })?;

    // Defaults from soft-proof settings (profile/intent/BPC); export-only overrides come from req.
    let defaults = PrintExportConfig {
        profile_id: soft.profile_id,
        intent: soft.intent,
        bpc: soft.bpc,
        ..PrintExportConfig::default()
    };
    Ok((width, height, rgba, defaults))
}

#[tauri::command]
pub async fn print_export_estimate(
    doc_id: u32,
    config: PrintExportConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<PrintExportEstimate, String> {
    validate_config(&config).map_err(|e| e.to_string())?;
    let state = Arc::clone(state.inner());
    tauri::async_runtime::spawn_blocking(move || {
        let (w, h, rgba, _) = load_rgba(&state, doc_id).map_err(|e| e.to_string())?;
        estimate_export(w, h, &rgba, &config).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("estimate task failed: {e}"))?
}

#[tauri::command]
pub async fn print_export_gamut_report(
    doc_id: u32,
    config: PrintExportConfig,
    state: State<'_, Arc<AppState>>,
) -> Result<GamutReport, String> {
    validate_config(&config).map_err(|e| e.to_string())?;
    let state = Arc::clone(state.inner());
    let svc = proof_service(&state)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (_w, _h, rgba, _) = load_rgba(&state, doc_id).map_err(|e| e.to_string())?;
        let icc = svc
            .profile_icc_bytes(&config.profile_id)
            .ok_or_else(|| format!("Profile not found: {}", config.profile_id))?;
        let xform = PrintExportTransform::new(&icc, config.intent, config.bpc)
            .map_err(|e| e.to_string())?;
        gamut_report(&rgba, &xform, config.pure_black_k, None).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("gamut report task failed: {e}"))?
}

#[tauri::command]
pub async fn print_export_run(
    doc_id: u32,
    config: PrintExportConfig,
    path: String,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ExportSummary, String> {
    validate_config(&config).map_err(|e| e.to_string())?;
    let state = Arc::clone(state.inner());
    let state_clear = Arc::clone(&state);
    let svc = proof_service(&state)?;
    let cancel = Arc::new(AtomicBool::new(false));
    // Store cancel handle for a future cancel command (best-effort).
    {
        let mut slot = state
            .print_export_cancel
            .lock()
            .map_err(|e| e.to_string())?;
        *slot = Some(Arc::clone(&cancel));
    }

    let path_buf = PathBuf::from(&path);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let emit = |stage: &str, fraction: f32| {
            let _ = app_handle.emit(
                "print_export_progress",
                PrintExportProgress {
                    doc_id,
                    stage: stage.to_string(),
                    fraction,
                },
            );
        };

        emit("composite", 0.05);
        let session = state.session(doc_id).map_err(|_| {
            format!("Document was closed (id {doc_id}); cannot export for print")
        })?;
        let _io_guard = session.begin_io();
        let (w, h, rgba, _) = load_rgba(&state, doc_id).map_err(|e| e.to_string())?;

        let icc = svc
            .profile_icc_bytes(&config.profile_id)
            .ok_or_else(|| format!("Profile not found: {}", config.profile_id))?;

        emit("convert", 0.2);
        let xform = PrintExportTransform::new(&icc, config.intent, config.bpc)
            .map_err(|e| e.to_string())?;
        let (cmyk, used_palette, unique) =
            rgba8_to_cmyk8(&rgba, &xform, config.pure_black_k, Some(&cancel))
                .map_err(|e| e.to_string())?;

        emit("scale", 0.7);
        let cmyk = scale_cmyk8_nearest(&cmyk, w, h, config.scale).map_err(|e| e.to_string())?;
        let (ow, oh) =
            engine_color::scaled_dimensions(w, h, config.scale).map_err(|e| e.to_string())?;

        if cancel.load(Ordering::Relaxed) {
            return Err("export cancelled".to_string());
        }

        emit("write", 0.85);
        let compression = match config.compression {
            TiffCompression::None => CmykTiffCompression::None,
            TiffCompression::Lzw => CmykTiffCompression::Lzw,
        };
        write_cmyk_tiff_atomic(&path_buf, ow, oh, &cmyk, &icc, config.ppi, compression)
            .map_err(|e| e.to_string())?;

        emit("done", 1.0);
        Ok(ExportSummary {
            out_width: ow,
            out_height: oh,
            unique_colors: unique as u32,
            used_palette_path: used_palette,
            path: path_buf.to_string_lossy().into_owned(),
        })
    })
    .await
    .map_err(|e| format!("print export task failed: {e}"))?;

    if let Ok(mut slot) = state_clear.print_export_cancel.lock() {
        *slot = None;
    }

    result
}

#[tauri::command]
pub fn print_export_cancel(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    if let Some(flag) = state
        .print_export_cancel
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
    {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}
