//! Per-tile generation trace (`.local-doc/Спека трасса поколений тайлов…`).
//!
//! Off unless `DITHER_TRACE=1`. When off, every entry returns before clocks or locks.

#![allow(dead_code)] // harness entry points; clippy --bins excludes cfg(test) call sites
#![allow(clippy::type_complexity)]

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

static ENABLED: AtomicBool = AtomicBool::new(false);
static RUN_ID: AtomicU64 = AtomicU64::new(0);
static SEQ: AtomicU64 = AtomicU64::new(0);
static PROCESS_START: OnceLock<Instant> = OnceLock::new();
static EPOCH_ORIGIN_MS: OnceLock<f64> = OnceLock::new();
/// Per (doc, level, x, y, doc_gen) Composite requeue attempts (H8 / step 2.5).
static REQUEUE_ATTEMPTS: OnceLock<Mutex<HashMap<(u32, u8, u32, u32, u64), u32>>> = OnceLock::new();
/// Dequeues since last successful compute for the same tile/gen (WORKER_SPIN).
static DEQUEUE_WITHOUT_COMPUTE: OnceLock<Mutex<HashMap<(u32, u8, u32, u32, u64), u32>>> =
    OnceLock::new();
/// Checker threshold: more than this many dequeues without compute → WORKER_SPIN.
pub const WORKER_SPIN_THRESHOLD: u32 = 20;

type SinkFn = Box<dyn Fn(TraceEvent) + Send + Sync>;
static SINK: OnceLock<SinkFn> = OnceLock::new();
static FALLBACK_LOG: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Debug, Serialize)]
pub struct TraceTile {
    pub level: u8,
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceEvent {
    pub ts_ms: f64,
    pub seq: u64,
    pub run_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc_gen: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer_gen: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tile: Option<TraceTile>,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gen: Option<u64>,
    pub details: Value,
}

/// Call once at process start (safe to call repeatedly).
pub fn init_from_env() {
    let on = std::env::var("DITHER_TRACE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        let _ = PROCESS_START.get_or_init(Instant::now);
        let _ = EPOCH_ORIGIN_MS.get_or_init(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        });
        log::info!(target: "tile_gen_trace", "enabled");
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Force-enable for harness / tests (does not require env).
pub fn force_enable(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if on {
        let _ = PROCESS_START.get_or_init(Instant::now);
        let _ = EPOCH_ORIGIN_MS.get_or_init(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs_f64() * 1000.0)
                .unwrap_or(0.0)
        });
    }
}

pub fn set_sink(sink: SinkFn) {
    let _ = SINK.set(sink);
    FALLBACK_LOG.store(false, Ordering::Relaxed);
}

pub fn begin_run(run_id: u64) {
    if !enabled() {
        return;
    }
    RUN_ID.store(run_id, Ordering::Relaxed);
    SEQ.store(0, Ordering::Relaxed);
    // Fresh series must not inherit WORKER_SPIN offenders.
    clear_spin_counters();
}

fn clear_spin_counters() {
    if let Ok(mut g) = dequeue_map().lock() {
        g.clear();
    }
    if let Ok(mut g) = requeue_map().lock() {
        g.clear();
    }
}

pub fn set_run_id(run_id: u64) {
    if !enabled() {
        return;
    }
    RUN_ID.store(run_id, Ordering::Relaxed);
}

pub fn active_run_id() -> u64 {
    RUN_ID.load(Ordering::Relaxed)
}

fn clocks() -> f64 {
    let start = PROCESS_START.get_or_init(Instant::now);
    let origin = *EPOCH_ORIGIN_MS.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    });
    origin + start.elapsed().as_secs_f64() * 1000.0
}

/// Emit a named event. No-op when tracing is off.
pub fn emit(
    event: &str,
    layer_id: Option<u32>,
    doc_gen: Option<u64>,
    layer_gen: Option<u64>,
    tile: Option<TraceTile>,
    gen: Option<u64>,
    details: Value,
) {
    emit_at(
        event, None, layer_id, doc_gen, layer_gen, tile, gen, details,
    );
}

/// Like [`emit`], but uses `ts_ms` when provided (frontend wall clock).
pub fn emit_at(
    event: &str,
    ts_ms: Option<f64>,
    layer_id: Option<u32>,
    doc_gen: Option<u64>,
    layer_gen: Option<u64>,
    tile: Option<TraceTile>,
    gen: Option<u64>,
    details: Value,
) {
    if !enabled() {
        return;
    }
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let ev = TraceEvent {
        ts_ms: ts_ms.unwrap_or_else(clocks),
        seq,
        run_id: RUN_ID.load(Ordering::Relaxed),
        layer_id,
        doc_gen,
        layer_gen,
        tile,
        event: event.to_string(),
        gen,
        details,
    };
    #[cfg(test)]
    if let Ok(mut g) = TEST_EVENTS.lock() {
        g.push(ev.clone());
    }
    if let Some(sink) = SINK.get() {
        sink(ev);
    } else if FALLBACK_LOG.load(Ordering::Relaxed) {
        if let Ok(line) = serde_json::to_string(&ev) {
            log::info!(target: "tile_gen_trace", "{line}");
        }
    }
}

pub fn emit_simple(event: &str, details: Value) {
    emit(event, None, None, None, None, None, details);
}

/// Cache insert observation (Processed / Composite / Raw).
pub fn note_cache_write(
    layer_id: u32,
    level: u8,
    x: u32,
    y: u32,
    write_gen: u64,
    live_gen: u64,
    stage: &str,
    inserted: bool,
) {
    if !enabled() {
        return;
    }
    let stale_write = write_gen < live_gen && inserted;
    emit(
        "cache_write",
        Some(layer_id),
        Some(live_gen),
        None,
        Some(tile(level, x, y)),
        Some(write_gen),
        json!({
            "stage": stage,
            "inserted": inserted,
            "stale_write": stale_write,
            "write_gen": write_gen,
            "live_gen": live_gen,
        }),
    );
}

pub fn note_cache_evict(removed: u32, reason: &str, live_gen: Option<u64>) {
    if !enabled() || removed == 0 {
        return;
    }
    emit(
        "cache_evict",
        None,
        live_gen,
        None,
        None,
        live_gen,
        json!({
            "removed": removed,
            "reason": reason,
            // Per-tile generations not exposed by EvictStats — aggregate only.
            "tile_gens": "not_measured",
        }),
    );
}

pub fn tile(level: u8, x: u32, y: u32) -> TraceTile {
    TraceTile { level, x, y }
}

fn requeue_map() -> &'static Mutex<HashMap<(u32, u8, u32, u32, u64), u32>> {
    REQUEUE_ATTEMPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn dequeue_map() -> &'static Mutex<HashMap<(u32, u8, u32, u32, u64), u32>> {
    DEQUEUE_WITHOUT_COMPUTE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Increment and return the Composite requeue attempt for this tile/gen (1-based).
pub fn bump_requeue_attempt(doc: u32, level: u8, x: u32, y: u32, doc_gen: u64) -> u32 {
    let key = (doc, level, x, y, doc_gen);
    let Ok(mut g) = requeue_map().lock() else {
        return 1;
    };
    let e = g.entry(key).or_insert(0);
    *e = e.saturating_add(1);
    *e
}

/// Clear attempt counter after a successful compute / gen advance.
pub fn clear_requeue_attempt(doc: u32, level: u8, x: u32, y: u32, doc_gen: u64) {
    let key = (doc, level, x, y, doc_gen);
    if let Ok(mut g) = requeue_map().lock() {
        g.remove(&key);
    }
    note_compute_done(doc, level, x, y, doc_gen);
}

/// Count a spin-like Composite miss (empty-deps / inject-spin). Not for healthy park.
pub fn note_spin_requeue(doc: u32, level: u8, x: u32, y: u32, doc_gen: u64) {
    if !enabled() {
        return;
    }
    let key = (doc, level, x, y, doc_gen);
    if let Ok(mut g) = dequeue_map().lock() {
        let e = g.entry(key).or_insert(0);
        *e = e.saturating_add(1);
    }
}

/// Deprecated alias — prefer [`note_spin_requeue`].
pub fn note_worker_dequeue(doc: u32, level: u8, x: u32, y: u32, doc_gen: u64) {
    let _ = (doc, level, x, y, doc_gen);
}

/// Reset dequeue-without-compute after a successful tile compute.
pub fn note_compute_done(doc: u32, level: u8, x: u32, y: u32, doc_gen: u64) {
    let key = (doc, level, x, y, doc_gen);
    if let Ok(mut g) = dequeue_map().lock() {
        g.remove(&key);
    }
}

/// Drop spin / requeue counters for a document (close / gen rollover).
pub fn clear_spin_for_doc(doc: u32) {
    if let Ok(mut g) = dequeue_map().lock() {
        g.retain(|(d, ..), _| *d != doc);
    }
    if let Ok(mut g) = requeue_map().lock() {
        g.retain(|(d, ..), _| *d != doc);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SpinOffender {
    pub doc: u32,
    pub level: u8,
    pub x: u32,
    pub y: u32,
    pub doc_gen: u64,
    pub dequeues_without_compute: u32,
    pub requeue_attempt: u32,
}

/// Tiles that exceed [`WORKER_SPIN_THRESHOLD`] empty-deps / inject-spin requeues.
pub fn spin_offenders() -> Vec<SpinOffender> {
    let mut out = Vec::new();
    let Ok(dequeues) = dequeue_map().lock() else {
        return out;
    };
    for (key, &dq) in dequeues.iter() {
        if dq > WORKER_SPIN_THRESHOLD {
            out.push(SpinOffender {
                doc: key.0,
                level: key.1,
                x: key.2,
                y: key.3,
                doc_gen: key.4,
                dequeues_without_compute: dq,
                requeue_attempt: dq,
            });
        }
    }
    out.sort_by_key(|o| std::cmp::Reverse(o.dequeues_without_compute));
    out
}

/// `DITHER_TRACE_INJECT_SPIN=1`: force the pre-H8 empty-deps immediate requeue (red path).
pub fn inject_spin_enabled() -> bool {
    std::env::var("DITHER_TRACE_INJECT_SPIN")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Stable hash of JSON params for before/after comparison (not a security hash).
pub fn params_hash(value: &Value) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    value.to_string().hash(&mut h);
    h.finish()
}

/// FNV-1a 64-bit of `(level, x0, y0, x1, y1)` — must match FE `vpHash.ts`.
pub fn vp_hash(level: u8, x0: u32, y0: u32, x1: u32, y1: u32) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in [level as u64, x0 as u64, y0 as u64, x1 as u64, y1 as u64]
        .into_iter()
        .flat_map(|v| v.to_le_bytes())
    {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

/// Inclusive tile AABB at `level` (empty → zeros). Shared by schedule/viewport logs.
pub fn vp_hash_from_tiles(level: u8, tiles: &[(u8, u32, u32)]) -> (String, u32, u32, u32, u32) {
    if tiles.is_empty() {
        return (vp_hash(level, 0, 0, 0, 0), 0, 0, 0, 0);
    }
    let mut x0 = u32::MAX;
    let mut y0 = u32::MAX;
    let mut x1 = 0u32;
    let mut y1 = 0u32;
    for &(_, x, y) in tiles {
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (vp_hash(level, x0, y0, x1, y1), x0, y0, x1, y1)
}

pub fn vp_hash_from_coords(
    level: u8,
    tiles: &[engine_tiles::TileCoord],
) -> (String, u32, u32, u32, u32) {
    let flat: Vec<(u8, u32, u32)> = tiles.iter().map(|c| (c.level, c.x, c.y)).collect();
    vp_hash_from_tiles(level, &flat)
}

#[cfg(test)]
static TEST_EVENTS: std::sync::Mutex<Vec<TraceEvent>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
pub fn test_events() -> Vec<TraceEvent> {
    TEST_EVENTS.lock().map(|g| g.clone()).unwrap_or_default()
}

#[cfg(test)]
pub fn clear_test_events_only() {
    if let Ok(mut e) = TEST_EVENTS.lock() {
        e.clear();
    }
}

#[cfg(test)]
pub fn clear_test_state() {
    force_enable(false);
    RUN_ID.store(0, Ordering::Relaxed);
    SEQ.store(0, Ordering::Relaxed);
    clear_test_events_only();
    if let Ok(mut g) = dequeue_map().lock() {
        g.clear();
    }
    if let Ok(mut g) = requeue_map().lock() {
        g.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_is_noop() {
        clear_test_state();
        force_enable(false);
        emit_simple("test", json!({}));
        assert!(test_events().is_empty());
    }

    #[test]
    fn enabled_records_seq() {
        clear_test_state();
        force_enable(true);
        begin_run(7);
        emit_simple("a", json!({"n": 1}));
        emit_simple("b", json!({"n": 2}));
        let ev = test_events();
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[0].run_id, 7);
        assert_eq!(ev[0].event, "a");
        assert_eq!(ev[1].seq, 1);
        clear_test_state();
    }

    #[test]
    fn vp_hash_stable_known() {
        // Must stay in lockstep with frontend/src/diagnostics/vpHash.ts
        assert_eq!(vp_hash(1, 0, 0, 3, 2), "513f4888a1847625");
        assert_ne!(vp_hash(1, 0, 0, 3, 2), vp_hash(0, 0, 0, 3, 2));
    }

    #[test]
    fn worker_spin_fires_after_threshold_dequeues() {
        clear_test_state();
        force_enable(true);
        for _ in 0..=WORKER_SPIN_THRESHOLD {
            note_spin_requeue(1, 2, 3, 4, 9);
        }
        let offenders = spin_offenders();
        assert_eq!(offenders.len(), 1);
        assert_eq!(
            offenders[0].dequeues_without_compute,
            WORKER_SPIN_THRESHOLD + 1
        );
        note_compute_done(1, 2, 3, 4, 9);
        assert!(spin_offenders().is_empty());
        clear_test_state();
    }
}
