//! B1 ED strip ownership gate: ≤1 compute per `(doc, layer, level, ty)`.
//!
//! Waiters never block a worker permanently: they `try_claim`, briefly
//! `wait_timeout` for the owner to finish, then either observe `tile_fresh`
//! or return [`engine_project::error::EngineError::EdStripBusy`] so the
//! worker re-parks on the frontier.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Key for one ED horizontal strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EdStripKey {
    pub doc: u32,
    pub layer: u32,
    pub level: u8,
    pub ty: u32,
}

struct GateState {
    /// True while an owner holds [`EdStripGuard`].
    owned: bool,
}

/// Per-strip mutex + condvar.
pub struct EdStripGate {
    state: Mutex<GateState>,
    cv: Condvar,
}

impl EdStripGate {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(GateState { owned: false }),
            cv: Condvar::new(),
        }
    }

    /// Non-blocking claim. `None` ⇒ another owner is running the strip.
    pub fn try_claim(self: &Arc<Self>) -> Option<EdStripGuard> {
        let mut g = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if g.owned {
            return None;
        }
        g.owned = true;
        Some(EdStripGuard {
            gate: Arc::clone(self),
        })
    }

    /// Wait up to `timeout` for the owner to release. Returns `true` if the
    /// gate is free when this returns (caller should re-check `tile_fresh`
    /// then `try_claim` again).
    pub fn wait_free_timeout(&self, timeout: Duration) -> bool {
        let guard = match self.state.lock() {
            Ok(g) => g,
            Err(e) => {
                drop(e.into_inner());
                return true;
            }
        };
        match self.cv.wait_timeout_while(guard, timeout, |s| s.owned) {
            Ok((g, _)) => !g.owned,
            Err(_) => true,
        }
    }

    pub fn is_owned(&self) -> bool {
        self.state
            .lock()
            .map(|g| g.owned)
            .unwrap_or(false)
    }
}

impl Default for EdStripGate {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII owner handle — releases + notifies waiters on drop.
pub struct EdStripGuard {
    gate: Arc<EdStripGate>,
}

impl Drop for EdStripGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = self.gate.state.lock() {
            g.owned = false;
        }
        self.gate.cv.notify_all();
    }
}

/// Process-wide map of strip gates.
pub struct EdStripGateMap {
    map: Mutex<HashMap<EdStripKey, Arc<EdStripGate>>>,
}

impl EdStripGateMap {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
        }
    }

    pub fn gate(&self, key: EdStripKey) -> Arc<EdStripGate> {
        let mut map = self.map.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key)
            .or_insert_with(|| Arc::new(EdStripGate::new()))
            .clone()
    }

    pub fn clear(&self) {
        if let Ok(mut map) = self.map.lock() {
            map.clear();
        }
    }

    pub fn evict_document(&self, doc: u32) {
        if let Ok(mut map) = self.map.lock() {
            map.retain(|k, _| k.doc != doc);
        }
    }

    /// Test helper: whether any gate is currently owned.
    pub fn any_owned(&self) -> bool {
        let Ok(map) = self.map.lock() else {
            return false;
        };
        map.values().any(|g| g.is_owned())
    }
}

impl Default for EdStripGateMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Brief poll window before returning [`EngineError::EdStripBusy`].
pub const STRIP_WAIT_SLICE: Duration = Duration::from_millis(2);
/// Max slices a compute call spends waiting before re-park (~10 ms).
pub const STRIP_WAIT_SLICES: u32 = 5;
