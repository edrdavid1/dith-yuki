//! Debounce + heartbeat + single-flight journal scheduling (H9 v2).
//!
//! `schedule_dirty` only marks flags and wakes a dedicated low-priority
//! background thread — no tokio disk I/O. Timing: 1 s quiet, ≤5 s max between
//! writes while dirty, 30 s heartbeat.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::commands::AppState;
use crate::undo::is_dirty_doc;

use super::stats::JournalWriteSource;
use super::write::{delete_journal_for_session, write_journal_for_doc_from};

/// Quiet period after the last edit before a write (spec D2 / §4.3).
pub const QUIET: Duration = Duration::from_secs(1);
/// Maximum time between writes while continuously dirty.
pub const MAX_INTERVAL: Duration = Duration::from_secs(5);
/// Heartbeat / hard cap while dirty.
pub const HEARTBEAT: Duration = Duration::from_secs(30);

/// Back-compat alias used by older call sites / docs.
pub const DEBOUNCE: Duration = QUIET;

#[derive(Debug, Default)]
pub struct JournalRuntime {
    pub recovery_dir: Option<PathBuf>,
    /// Per-doc schedule generation — stale quiet waiters no-op.
    generations: HashMap<u32, u64>,
    last_flush: HashMap<u32, Instant>,
    last_flush_revision: HashMap<u32, u64>,
    last_state_hash: HashMap<u32, u64>,
    /// Last edit / schedule time (quiet clock).
    last_edit: HashMap<u32, Instant>,
    /// When the doc first became dirty since last successful flush.
    dirty_since: HashMap<u32, Instant>,
    /// Docs that need a write.
    pending: HashSet<u32>,
    /// Single-flight: doc currently writing.
    writing: HashSet<u32>,
    /// Coalesce: one deferred rewrite after current flight.
    deferred: HashSet<u32>,
    /// recovery_id → (layer_id, raw_rev) → sha256
    blob_digests: HashMap<Uuid, HashMap<(u32, u64), String>>,
    heartbeat_started: bool,
    worker_started: bool,
}

impl JournalRuntime {
    pub fn disabled() -> Self {
        Self::default()
    }

    pub fn note_flushed(&mut self, doc_id: u32, revision: u64) {
        self.last_flush.insert(doc_id, Instant::now());
        self.last_flush_revision.insert(doc_id, revision);
        self.dirty_since.remove(&doc_id);
        self.pending.remove(&doc_id);
    }

    pub fn note_state_hash(&mut self, doc_id: u32, hash: u64) {
        self.last_state_hash.insert(doc_id, hash);
    }

    pub fn state_hash_matches(&self, doc_id: u32, hash: u64) -> bool {
        self.last_state_hash.get(&doc_id).copied() == Some(hash)
    }

    pub fn flush_matches_revision(&self, doc_id: u32, revision: u64) -> bool {
        self.last_flush_revision
            .get(&doc_id)
            .copied()
            .is_some_and(|r| r == revision)
    }

    pub fn remember_blob(&mut self, recovery_id: Uuid, layer_id: u32, raw_rev: u64, sha: String) {
        self.blob_digests
            .entry(recovery_id)
            .or_default()
            .insert((layer_id, raw_rev), sha);
    }

    pub fn blob_sha(&self, recovery_id: Uuid, layer_id: u32, raw_rev: u64) -> Option<String> {
        self.blob_digests
            .get(&recovery_id)
            .and_then(|m| m.get(&(layer_id, raw_rev)).cloned())
    }

    fn bump_generation(&mut self, doc_id: u32) -> u64 {
        let g = self.generations.entry(doc_id).or_insert(0);
        *g = g.wrapping_add(1);
        *g
    }

    fn current_generation(&self, doc_id: u32) -> u64 {
        self.generations.get(&doc_id).copied().unwrap_or(0)
    }

    /// Try to begin a write flight. Returns false if already writing (sets deferred).
    pub fn try_begin_write(&mut self, doc_id: u32) -> bool {
        if self.writing.contains(&doc_id) {
            self.deferred.insert(doc_id);
            if crate::tile_gen_trace::enabled()
                || std::env::var("DITHER_TRACE").ok().as_deref() == Some("1")
            {
                log::info!(target: "journal", "journal_coalesced doc={doc_id}");
            }
            return false;
        }
        self.writing.insert(doc_id);
        true
    }

    pub fn end_write(&mut self, doc_id: u32) -> bool {
        self.writing.remove(&doc_id);
        self.deferred.remove(&doc_id)
    }
}

static WORKER_TX: OnceLock<Sender<WorkerMsg>> = OnceLock::new();

enum WorkerMsg {
    Wake,
    /// Synchronous flush request (soft discard / signals / tests).
    Flush {
        doc_id: u32,
        source: JournalWriteSource,
        reply: Sender<Result<(), String>>,
    },
}

pub fn set_recovery_dir(state: &AppState, dir: PathBuf) {
    if !super::enabled() {
        log::info!(target: "journal", "DITHER_JOURNAL=0 — recovery dir not set");
        return;
    }
    if let Ok(mut guard) = state.journal.lock() {
        let _ = super::fsutil::ensure_dir_private(&dir);
        super::cleanup::startup_cleanup(&dir);
        guard.recovery_dir = Some(dir);
    }
}

/// Called when a document becomes dirty (or stays dirty after an edit).
pub fn schedule_dirty(app: Option<&AppHandle>, state: &AppState, doc_id: u32) {
    if !super::enabled() {
        return;
    }
    let Some(app) = app else {
        return;
    };
    if !is_dirty_doc(state, doc_id) {
        if let Ok(session) = state.require_session(doc_id) {
            delete_journal_for_session(state, &session);
        }
        return;
    }

    {
        let Ok(mut guard) = state.journal.lock() else {
            return;
        };
        if guard.recovery_dir.is_none() {
            return;
        }
        let now = Instant::now();
        guard.bump_generation(doc_id);
        guard.last_edit.insert(doc_id, now);
        guard.dirty_since.entry(doc_id).or_insert(now);
        guard.pending.insert(doc_id);
        ensure_worker_locked(&mut guard, app);
    }

    if crate::tile_gen_trace::enabled()
        || std::env::var("DITHER_TRACE").ok().as_deref() == Some("1")
    {
        log::info!(target: "journal", "journal_enqueue doc={doc_id}");
    }
    wake_worker();
}

fn ensure_worker_locked(guard: &mut JournalRuntime, app: &AppHandle) {
    if guard.worker_started {
        return;
    }
    guard.worker_started = true;
    start_worker_thread(app.clone());
}

fn start_worker_thread(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<WorkerMsg>();
    let _ = WORKER_TX.set(tx);
    let _ = thread::Builder::new()
        .name("journal-writer".into())
        .spawn(move || {
            set_low_priority();
            worker_loop(app, rx);
        });
}

fn wake_worker() {
    if let Some(tx) = WORKER_TX.get() {
        let _ = tx.send(WorkerMsg::Wake);
    }
}

/// Sync flush on the journal worker (or inline if worker not up).
pub fn flush_doc_sync(
    state: &AppState,
    doc_id: u32,
    source: JournalWriteSource,
) -> Result<(), String> {
    if let Some(tx) = WORKER_TX.get() {
        let (rtx, rrx) = mpsc::channel();
        tx.send(WorkerMsg::Flush {
            doc_id,
            source,
            reply: rtx,
        })
        .map_err(|e| e.to_string())?;
        return rrx.recv().map_err(|e| e.to_string())?;
    }
    // Worker not started (tests): write inline with single-flight.
    run_single_flight_write(state, doc_id, source)
}

fn run_single_flight_write(
    state: &AppState,
    doc_id: u32,
    source: JournalWriteSource,
) -> Result<(), String> {
    loop {
        let began = {
            let Ok(mut guard) = state.journal.lock() else {
                return Err("journal lock poisoned".into());
            };
            guard.try_begin_write(doc_id)
        };
        if !began {
            // Wait briefly for the other flight (tests / rare race).
            thread::sleep(Duration::from_millis(5));
            continue;
        }
        let result = write_journal_for_doc_from(state, doc_id, source);
        let again = state
            .journal
            .lock()
            .ok()
            .map(|mut g| g.end_write(doc_id))
            .unwrap_or(false);
        if again {
            // One coalesced rewrite under a fresh flight.
            let began = state
                .journal
                .lock()
                .ok()
                .map(|mut g| g.try_begin_write(doc_id))
                .unwrap_or(false);
            if began {
                let _ = write_journal_for_doc_from(state, doc_id, source);
                let _ = state.journal.lock().map(|mut g| {
                    g.deferred.remove(&doc_id);
                    g.end_write(doc_id)
                });
            }
        }
        return result;
    }
}

fn worker_loop(app: AppHandle, rx: Receiver<WorkerMsg>) {
    loop {
        // Wait for wake, flush, or timeout for quiet/max deadlines.
        let msg = rx.recv_timeout(Duration::from_millis(200));
        match msg {
            Ok(WorkerMsg::Flush {
                doc_id,
                source,
                reply,
            }) => {
                let state = app.state::<Arc<AppState>>();
                let r = run_single_flight_write(state.inner(), doc_id, source);
                let _ = reply.send(r);
            }
            Ok(WorkerMsg::Wake) | Err(mpsc::RecvTimeoutError::Timeout) => {
                let state = app.state::<Arc<AppState>>();
                drain_due(&app, state.inner());
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

fn drain_due(app: &AppHandle, state: &AppState) {
    let now = Instant::now();
    let due: Vec<u32> = {
        let Ok(guard) = state.journal.lock() else {
            return;
        };
        guard
            .pending
            .iter()
            .copied()
            .filter(|id| {
                if !is_dirty_doc(state, *id) {
                    return false;
                }
                let quiet_ok = guard
                    .last_edit
                    .get(id)
                    .map(|t| now.duration_since(*t) >= QUIET)
                    .unwrap_or(true);
                let max_ok = guard
                    .dirty_since
                    .get(id)
                    .map(|t| now.duration_since(*t) >= MAX_INTERVAL)
                    .unwrap_or(false);
                let hb_ok = match guard.last_flush.get(id) {
                    Some(t) => now.duration_since(*t) >= HEARTBEAT,
                    None => guard.dirty_since.contains_key(id),
                };
                quiet_ok || max_ok || hb_ok
            })
            .collect()
    };

    for doc_id in due {
        if !is_dirty_doc(state, doc_id) {
            if let Ok(mut g) = state.journal.lock() {
                g.pending.remove(&doc_id);
            }
            continue;
        }
        let source = {
            let Ok(guard) = state.journal.lock() else {
                continue;
            };
            match guard.last_flush.get(&doc_id) {
                Some(t) if now.duration_since(*t) >= HEARTBEAT => JournalWriteSource::Heartbeat,
                _ => JournalWriteSource::Debounce,
            }
        };
        let _ = run_single_flight_write(state, doc_id, source);
        let _ = app; // keep app alive for state
    }
}

/// Start heartbeat + worker (process-wide).
pub fn start_heartbeat(app: AppHandle) {
    if !super::enabled() {
        return;
    }
    {
        let state = app.state::<Arc<AppState>>();
        let Ok(mut guard) = state.journal.lock() else {
            return;
        };
        if guard.heartbeat_started || guard.recovery_dir.is_none() {
            return;
        }
        guard.heartbeat_started = true;
        ensure_worker_locked(&mut guard, &app);
    }

    // Lightweight tick: wake worker so HEARTBEAT / MAX_INTERVAL fire without edits.
    let app2 = app.clone();
    thread::Builder::new()
        .name("journal-heartbeat".into())
        .spawn(move || {
            set_low_priority();
            loop {
                thread::sleep(Duration::from_secs(1));
                // Mark still-dirty docs pending so drain_due can apply HEARTBEAT.
                let state = app2.state::<Arc<AppState>>();
                if let Ok(mut guard) = state.journal.lock() {
                    let ids: Vec<u32> = state
                        .sessions
                        .lock()
                        .ok()
                        .map(|m| m.keys().copied().collect())
                        .unwrap_or_default();
                    for id in ids {
                        if is_dirty_doc(state.inner(), id) {
                            guard.pending.insert(id);
                            guard.dirty_since.entry(id).or_insert_with(Instant::now);
                        }
                    }
                }
                wake_worker();
            }
        })
        .ok();
}

fn set_low_priority() {
    #[cfg(target_os = "macos")]
    {
        // Best-effort background QoS (utility).
        unsafe {
            const QOS_CLASS_UTILITY: u32 = 0x11;
            extern "C" {
                fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
            }
            let _ = pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0);
        }
    }
    // Other platforms: dedicated thread is enough; niceness optional.
}

#[allow(dead_code)]
pub type JournalLock = Mutex<JournalRuntime>;
