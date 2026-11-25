//! Process-wide print/batch export mutual exclusion (one job at a time).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportJobKind {
    Print,
    Batch,
}

impl ExportJobKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Print => "print",
            Self::Batch => "batch",
        }
    }
}

/// Stable IPC error prefix — frontend parses `job_busy:print` / `job_busy:batch`.
pub fn job_busy_error(running: ExportJobKind) -> String {
    format!("job_busy:{}", running.as_str())
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportJobChangedPayload {
    pub kind: String,
    pub active: bool,
    /// Which job holds the process slot, if any.
    pub busy_kind: Option<String>,
}

fn emit_changed(app: Option<&AppHandle>, kind: ExportJobKind, active: bool, busy: Option<ExportJobKind>) {
    let Some(app) = app else {
        return;
    };
    let _ = app.emit(
        "export-job-changed",
        ExportJobChangedPayload {
            kind: kind.as_str().to_string(),
            active,
            busy_kind: busy.map(|k| k.as_str().to_string()),
        },
    );
}

/// Claim the process-wide export slot. Locks print then batch (fixed order).
pub fn try_begin(
    state: &AppState,
    app: Option<&AppHandle>,
    kind: ExportJobKind,
    cancel: Arc<AtomicBool>,
) -> Result<(), String> {
    let mut print = state
        .print_export_cancel
        .lock()
        .map_err(|e| e.to_string())?;
    let mut batch = state
        .batch_export_cancel
        .lock()
        .map_err(|e| e.to_string())?;

    if print.is_some() {
        return Err(job_busy_error(ExportJobKind::Print));
    }
    if batch.is_some() {
        return Err(job_busy_error(ExportJobKind::Batch));
    }

    match kind {
        ExportJobKind::Print => *print = Some(cancel),
        ExportJobKind::Batch => *batch = Some(cancel),
    }
    emit_changed(app, kind, true, Some(kind));
    Ok(())
}

pub fn end(state: &AppState, app: Option<&AppHandle>, kind: ExportJobKind) {
    match kind {
        ExportJobKind::Print => {
            if let Ok(mut slot) = state.print_export_cancel.lock() {
                *slot = None;
            }
        }
        ExportJobKind::Batch => {
            if let Ok(mut slot) = state.batch_export_cancel.lock() {
                *slot = None;
            }
        }
    }
    emit_changed(app, kind, false, None);
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_tiles::TILE_BYTES;

    #[test]
    fn second_job_returns_job_busy() {
        let state = AppState::empty_process(None, TILE_BYTES * 4, true);
        let c1 = Arc::new(AtomicBool::new(false));
        try_begin(&state, None, ExportJobKind::Print, Arc::clone(&c1)).unwrap();
        let err = try_begin(
            &state,
            None,
            ExportJobKind::Batch,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap_err();
        assert_eq!(err, "job_busy:print");
        end(&state, None, ExportJobKind::Print);
        try_begin(
            &state,
            None,
            ExportJobKind::Batch,
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
    }
}
