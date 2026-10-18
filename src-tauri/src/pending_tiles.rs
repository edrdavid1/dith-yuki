//! Registry of tiles that received a 202 (`PreviewServe::Pending`).
//!
//! When Composite (or Processed, if requested) becomes ready, emit `tile-ready`
//! for every matching registration regardless of the current viewport level
//! (split-dither-preview Step 1 / H1).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::Emitter;

use engine_tiles::{CacheStage, TileKey};

use crate::commands::AppState;
use crate::worker::TileReadyPayload;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct PendingKey {
    pub doc: u32,
    pub level: u8,
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Debug)]
struct PendingEntry {
    key: PendingKey,
    #[allow(dead_code)]
    registered_at: Instant,
    #[allow(dead_code)]
    doc_gen: u64,
}

fn registry() -> &'static Mutex<HashSet<PendingKey>> {
    static REG: OnceLock<Mutex<HashSet<PendingKey>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashSet::new()))
}

fn meta() -> &'static Mutex<Vec<PendingEntry>> {
    static META: OnceLock<Mutex<Vec<PendingEntry>>> = OnceLock::new();
    META.get_or_init(|| Mutex::new(Vec::new()))
}

/// Register a tile that was served as 202.
pub fn register(doc: u32, level: u8, x: u32, y: u32, doc_gen: u64) {
    let key = PendingKey { doc, level, x, y };
    if let Ok(mut g) = registry().lock() {
        g.insert(key);
    }
    if let Ok(mut m) = meta().lock() {
        m.retain(|e| e.key != key);
        m.push(PendingEntry {
            key,
            registered_at: Instant::now(),
            doc_gen,
        });
    }
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit(
            "pending_tile_registered",
            None,
            Some(doc_gen),
            None,
            Some(crate::tile_gen_trace::tile(level, x, y)),
            Some(doc_gen),
            serde_json::json!({ "doc": doc }),
        );
    }
}

pub fn clear_doc(doc: u32) {
    if let Ok(mut g) = registry().lock() {
        g.retain(|k| k.doc != doc);
    }
    if let Ok(mut m) = meta().lock() {
        m.retain(|e| e.key.doc != doc);
    }
}

/// Drop entries older than `ttl` (garbage).
#[allow(dead_code)]
pub fn gc(ttl: Duration) {
    let now = Instant::now();
    if let Ok(mut m) = meta().lock() {
        let before = m.len();
        m.retain(|e| now.duration_since(e.registered_at) < ttl);
        let drop_keys: HashSet<_> = {
            // rebuild registry from meta
            m.iter().map(|e| e.key).collect()
        };
        if let Ok(mut g) = registry().lock() {
            *g = drop_keys;
        }
        let _ = before;
    }
}

/// True if this composite key was requested via 202.
pub fn is_pending(doc: u32, level: u8, x: u32, y: u32) -> bool {
    let key = PendingKey { doc, level, x, y };
    registry()
        .lock()
        .map(|g| g.contains(&key))
        .unwrap_or(false)
}

/// Take (remove) a pending registration if present.
pub fn take(doc: u32, level: u8, x: u32, y: u32) -> bool {
    let key = PendingKey { doc, level, x, y };
    let removed = registry()
        .lock()
        .map(|mut g| g.remove(&key))
        .unwrap_or(false);
    if removed {
        if let Ok(mut m) = meta().lock() {
            m.retain(|e| e.key != key);
        }
    }
    removed
}

/// After a Composite insert, emit `tile-ready` if the tile was pending (any level).
pub fn notify_if_pending<R: tauri::Runtime>(
    state: &AppState,
    app: &tauri::AppHandle<R>,
    key: TileKey,
    doc_id: u32,
) {
    if key.stage != CacheStage::Composite {
        return;
    }
    if !take(key.doc, key.coord.level, key.coord.x, key.coord.y) {
        return;
    }
    let payload = TileReadyPayload {
        doc_id,
        layer_id: key.layer,
        stage: "composite".to_string(),
        level: key.coord.level,
        x: key.coord.x,
        y: key.coord.y,
    };
    if crate::tile_gen_trace::enabled() {
        crate::tile_gen_trace::emit(
            "tile_ready_emitted",
            Some(key.layer),
            None,
            None,
            Some(crate::tile_gen_trace::tile(
                key.coord.level,
                key.coord.x,
                key.coord.y,
            )),
            Some(key.doc as u64),
            serde_json::json!({
                "reason": "pending_registry",
                "viewport_agnostic": true,
            }),
        );
    }
    let _ = app.emit_to(tauri::EventTarget::Any, "tile-ready", payload);
    let _ = state; // reserved for future gen checks
}

#[derive(Clone, Debug, Serialize)]
pub struct PendingReasonInfo {
    pub pending_reason: &'static str,
    pub missing_dep: Option<MissingDep>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MissingDep {
    pub stage: String,
    pub level: u8,
    pub x: u32,
    pub y: u32,
    pub layer: u32,
}

/// Classify why a Composite tile is not ready (for 202 headers / trace).
pub fn classify_pending(state: &AppState, key: TileKey) -> PendingReasonInfo {
    if key.stage != CacheStage::Composite {
        return PendingReasonInfo {
            pending_reason: "queued",
            missing_dep: None,
        };
    }
    if key.coord.level > 0 {
        // Pyramid parent waiting on children.
        return PendingReasonInfo {
            pending_reason: "waiting_children",
            missing_dep: Some(MissingDep {
                stage: "Composite".into(),
                level: key.coord.level - 1,
                x: key.coord.x * 2,
                y: key.coord.y * 2,
                layer: key.layer,
            }),
        };
    }
    // L0: check ED prefix / Processed readiness.
    let Ok(session) = state.session(key.doc) else {
        return PendingReasonInfo {
            pending_reason: "queued",
            missing_dep: None,
        };
    };
    let snap = session.document_handle.snapshot();
    let deps = crate::tile_pipeline::composite_ed_park_deps(
        state,
        &snap.root,
        key.doc,
        key.coord,
    );
    if let Some(dep) = deps.first() {
        return PendingReasonInfo {
            pending_reason: "ed_prefix",
            missing_dep: Some(MissingDep {
                stage: format!("{:?}", dep.stage),
                level: dep.coord.level,
                x: dep.coord.x,
                y: dep.coord.y,
                layer: dep.layer,
            }),
        };
    }
    if state.tiles.scheduler.contains_key(&key) {
        return PendingReasonInfo {
            pending_reason: "queued",
            missing_dep: None,
        };
    }
    PendingReasonInfo {
        pending_reason: "computing",
        missing_dep: None,
    }
}
