//! Single-thread delayed Composite requeue (H8 last resort).
//!
//! Avoids spawning one sleeper thread per tile. Entries are dropped when the
//! task generation no longer matches the live document gen, or when the
//! document is closed (`cancel_doc`).

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use engine_tiles::RecomputeTask;

use crate::commands::AppState;

struct Entry {
    due: Instant,
    seq: u64,
    task: RecomputeTask,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.seq == other.seq
    }
}

impl Eq for Entry {}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.due
            .cmp(&other.due)
            .then_with(|| self.seq.cmp(&other.seq))
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct QueueState {
    /// Min-heap by due time (`Reverse` flips `Ord`).
    heap: BinaryHeap<Reverse<Entry>>,
    cancelled_docs: std::collections::HashSet<u32>,
}

pub struct DelayedRequeue {
    inner: Mutex<QueueState>,
    cv: Condvar,
    seq: AtomicU64,
    started: AtomicBool,
}

impl DelayedRequeue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(QueueState {
                heap: BinaryHeap::new(),
                cancelled_docs: std::collections::HashSet::new(),
            }),
            cv: Condvar::new(),
            seq: AtomicU64::new(1),
            started: AtomicBool::new(false),
        }
    }

    /// Schedule `task` after `delay`. Starts the sleeper thread on first use.
    pub fn schedule(
        self: &Arc<Self>,
        state: Arc<AppState>,
        task: RecomputeTask,
        delay: Duration,
    ) {
        self.ensure_started(state);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let due = Instant::now() + delay;
        let mut g = self.inner.lock().unwrap();
        g.cancelled_docs.remove(&task.key.doc);
        g.heap.push(Reverse(Entry { due, seq, task }));
        self.cv.notify_one();
    }

    /// Drop all pending entries for `doc` (document close).
    pub fn cancel_doc(&self, doc: u32) {
        let mut g = self.inner.lock().unwrap();
        g.cancelled_docs.insert(doc);
        self.cv.notify_one();
    }

    fn ensure_started(self: &Arc<Self>, state: Arc<AppState>) {
        if self
            .started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let queue = Arc::clone(self);
        thread::Builder::new()
            .name("composite-ed-backoff".into())
            .spawn(move || queue.run_loop(state))
            .ok();
    }

    fn run_loop(self: Arc<Self>, state: Arc<AppState>) {
        loop {
            let mut g = self.inner.lock().unwrap();
            let wait = loop {
                while let Some(Reverse(peek)) = g.heap.peek() {
                    if g.cancelled_docs.contains(&peek.task.key.doc) {
                        g.heap.pop();
                        continue;
                    }
                    break;
                }
                let now = Instant::now();
                match g.heap.peek() {
                    None => {
                        g = self.cv.wait(g).unwrap();
                        continue;
                    }
                    Some(Reverse(peek)) if peek.due > now => {
                        break peek.due.saturating_duration_since(now);
                    }
                    Some(_) => break Duration::ZERO,
                }
            };
            if wait > Duration::ZERO {
                let (guard, _) = self.cv.wait_timeout(g, wait).unwrap();
                g = guard;
                continue;
            }
            let Some(Reverse(entry)) = g.heap.pop() else {
                continue;
            };
            if g.cancelled_docs.contains(&entry.task.key.doc) {
                continue;
            }
            drop(g);

            // Stale gen or closed doc → skip (no wake).
            let live_ok = state.session(entry.task.key.doc).ok().map(|s| {
                let snap = s.document_handle.snapshot();
                snap.generations
                    .document_gen
                    .load(Ordering::Acquire)
                    == entry.task.generation
            });
            if live_ok != Some(true) {
                continue;
            }
            state.tiles.scheduler.enqueue_or_bump(entry.task);
            state.worker_wake.notify_one();
        }
    }
}

impl Default for DelayedRequeue {
    fn default() -> Self {
        Self::new()
    }
}
