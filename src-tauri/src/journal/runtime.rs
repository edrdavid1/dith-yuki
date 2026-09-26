//! Debounce + heartbeat scheduling for dirty-document journals.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::commands::AppState;
use crate::undo::is_dirty_doc;

use super::write::{delete_journal_for_session, write_journal_for_doc};

pub const DEBOUNCE: Duration = Duration::from_secs(3);
pub const HEARTBEAT: Duration = Duration::from_secs(30);

#[derive(Debug, Default)]
pub struct JournalRuntime {
    pub recovery_dir: Option<PathBuf>,
    /// Per-doc schedule generation — stale debounce tasks no-op.
    generations: HashMap<u32, u64>,
    last_flush: HashMap<u32, Instant>,
    heartbeat_started: bool,
}

impl JournalRuntime {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn note_flushed(&mut self, doc_id: u32) {
        self.last_flush.insert(doc_id, Instant::now());
    }

    fn bump_generation(&mut self, doc_id: u32) -> u64 {
        let g = self.generations.entry(doc_id).or_insert(0);
        *g = g.wrapping_add(1);
        *g
    }

    fn current_generation(&self, doc_id: u32) -> u64 {
        self.generations.get(&doc_id).copied().unwrap_or(0)
    }
}

pub fn set_recovery_dir(state: &AppState, dir: PathBuf) {
    if let Ok(mut guard) = state.journal.lock() {
        let _ = std::fs::create_dir_all(&dir);
        guard.recovery_dir = Some(dir);
    }
}

/// Called when a document becomes dirty (or stays dirty after an edit).
pub fn schedule_dirty(app: Option<&AppHandle>, state: &AppState, doc_id: u32) {
    let Some(app) = app else {
        return;
    };
    if !is_dirty_doc(state, doc_id) {
        if let Ok(session) = state.require_session(doc_id) {
            delete_journal_for_session(state, &session);
        }
        return;
    }

    let gen = {
        let Ok(mut guard) = state.journal.lock() else {
            return;
        };
        if guard.recovery_dir.is_none() {
            return;
        }
        guard.bump_generation(doc_id)
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(DEBOUNCE).await;
        let state = app.state::<Arc<AppState>>();
        let still_current = state
            .journal
            .lock()
            .ok()
            .map(|g| g.current_generation(doc_id) == gen)
            .unwrap_or(false);
        if !still_current {
            return;
        }
        if !is_dirty_doc(state.inner(), doc_id) {
            return;
        }
        if let Err(e) = write_journal_for_doc(state.inner(), doc_id) {
            log::warn!("journal debounce write failed for doc {doc_id}: {e}");
        }
    });
}

/// Start a process-wide heartbeat that flushes stale dirty journals.
pub fn start_heartbeat(app: AppHandle) {
    {
        let state = app.state::<Arc<AppState>>();
        let Ok(mut guard) = state.journal.lock() else {
            return;
        };
        if guard.heartbeat_started || guard.recovery_dir.is_none() {
            return;
        }
        guard.heartbeat_started = true;
    }

    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(HEARTBEAT);
        loop {
            tick.tick().await;
            flush_stale(&app);
        }
    });
}

fn flush_stale(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>();
    let dirty_ids: Vec<u32> = {
        let Ok(map) = state.sessions.lock() else {
            return;
        };
        map.keys().copied().collect()
    };

    for doc_id in dirty_ids {
        if !is_dirty_doc(state.inner(), doc_id) {
            continue;
        }
        let needs = state
            .journal
            .lock()
            .ok()
            .map(|g| match g.last_flush.get(&doc_id) {
                Some(t) => t.elapsed() >= HEARTBEAT,
                None => true,
            })
            .unwrap_or(false);
        if !needs {
            continue;
        }
        if let Err(e) = write_journal_for_doc(state.inner(), doc_id) {
            log::warn!("journal heartbeat write failed for doc {doc_id}: {e}");
        }
    }
}

#[allow(dead_code)]
pub type JournalLock = Mutex<JournalRuntime>;
