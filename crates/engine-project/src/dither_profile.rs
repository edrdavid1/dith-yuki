//! End-to-end `blocked_fs` / `blue_noise` profiling (`.local-doc/time-testing.md`).
//!
//! Off unless `DITHER_PROFILE=1`. When off, every entry returns before clocks.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

static ENABLED: AtomicBool = AtomicBool::new(false);
static RUN_ID: AtomicU64 = AtomicU64::new(0);
static GENERATION: AtomicU64 = AtomicU64::new(0);
static PROCESS_START: OnceLock<Instant> = OnceLock::new();
static EPOCH_ORIGIN_MS: OnceLock<f64> = OnceLock::new();

type SinkFn = Box<dyn Fn(ProfileMark) + Send + Sync>;
static SINK: OnceLock<SinkFn> = OnceLock::new();
static FALLBACK_LOG: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Debug, Serialize)]
pub struct ProfileMark {
    pub run_id: u64,
    pub mark: String,
    pub mono_us: u64,
    pub epoch_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// Call once at process start (safe to call repeatedly).
pub fn init_from_env() {
    let on = std::env::var("DITHER_PROFILE")
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
        log::info!(target: "dither_profile", "enabled");
    }
}

pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Register a sink that receives every mark (app writes JSONL). Optional.
pub fn set_sink(sink: SinkFn) {
    let _ = SINK.set(sink);
    FALLBACK_LOG.store(false, Ordering::Relaxed);
}

pub fn begin_run(run_id: u64, generation: u64) {
    if !enabled() {
        return;
    }
    RUN_ID.store(run_id, Ordering::Relaxed);
    GENERATION.store(generation, Ordering::Relaxed);
}

pub fn set_generation(generation: u64) {
    if !enabled() {
        return;
    }
    GENERATION.store(generation, Ordering::Relaxed);
}

pub fn active_run_id() -> u64 {
    RUN_ID.load(Ordering::Relaxed)
}

pub fn active_generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

fn clocks() -> (u64, f64) {
    let start = PROCESS_START.get_or_init(Instant::now);
    let mono_us = start.elapsed().as_micros() as u64;
    let origin = *EPOCH_ORIGIN_MS.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    });
    let epoch_ms = origin + start.elapsed().as_secs_f64() * 1000.0;
    (mono_us, epoch_ms)
}

/// Record a named mark for the active run.
pub fn mark(name: &str) {
    mark_extra(name, None);
}

pub fn mark_extra(name: &str, extra: Option<serde_json::Value>) {
    if !enabled() {
        return;
    }
    let run_id = RUN_ID.load(Ordering::Relaxed);
    if run_id == 0 {
        return;
    }
    let (mono_us, epoch_ms) = clocks();
    let generation = GENERATION.load(Ordering::Relaxed);
    let event = ProfileMark {
        run_id,
        mark: name.to_string(),
        mono_us,
        epoch_ms,
        generation: if generation == 0 {
            None
        } else {
            Some(generation)
        },
        extra,
    };
    if let Some(sink) = SINK.get() {
        sink(event);
    } else if FALLBACK_LOG.load(Ordering::Relaxed) {
        if let Ok(line) = serde_json::to_string(&event) {
            log::info!(target: "dither_profile", "{line}");
        }
    }
}

/// Record a mark with an explicit frontend epoch ms (for cross-clock join).
pub fn mark_from_frontend(run_id: u64, name: &str, epoch_ms: f64, generation: Option<u64>) {
    if !enabled() {
        return;
    }
    let (mono_us, _) = clocks();
    let event = ProfileMark {
        run_id,
        mark: name.to_string(),
        mono_us,
        epoch_ms,
        generation,
        extra: Some(serde_json::json!({ "side": "frontend" })),
    };
    if let Some(sink) = SINK.get() {
        sink(event);
    } else if let Ok(line) = serde_json::to_string(&event) {
        log::info!(target: "dither_profile", "{line}");
    }
}

/// Test helper: force enable without env (does not change dither output).
#[cfg(test)]
pub fn force_enable_for_test() {
    ENABLED.store(true, Ordering::Relaxed);
    let _ = PROCESS_START.get_or_init(Instant::now);
    let _ = EPOCH_ORIGIN_MS.get_or_init(|| 0.0);
}

/// Collect marks in-memory for unit tests.
#[cfg(test)]
pub fn install_test_collector() -> std::sync::Arc<std::sync::Mutex<Vec<ProfileMark>>> {
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let buf2 = buf.clone();
    set_sink(Box::new(move |m| {
        if let Ok(mut g) = buf2.lock() {
            g.push(m);
        }
    }));
    buf
}
