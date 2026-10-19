//! Process-wide journal write counters (Step 1 baseline; always-on atomics).
//!
//! Dump with [`snapshot`] from harnesses or under `DITHER_TRACE=1` logs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;

/// Who invoked [`super::write::write_journal_for_doc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalWriteSource {
    Debounce,
    Heartbeat,
    SoftDiscard,
    Signals,
    /// Direct / unit-test / harness call.
    Direct,
}

#[derive(Debug, Default, Serialize)]
pub struct JournalStatsSnapshot {
    pub debounce: u64,
    pub heartbeat: u64,
    pub soft_discard: u64,
    pub signals: u64,
    pub direct: u64,
    pub incomplete_raw_skips: u64,
    pub kill_switch_noops: u64,
    pub concurrent_peak: u64,
    pub in_flight: u64,
    pub total_write_ms: u64,
    pub write_samples: u64,
    pub skipped: u64,
}

static DEBOUNCE: AtomicU64 = AtomicU64::new(0);
static HEARTBEAT: AtomicU64 = AtomicU64::new(0);
static SOFT_DISCARD: AtomicU64 = AtomicU64::new(0);
static SIGNALS: AtomicU64 = AtomicU64::new(0);
static DIRECT: AtomicU64 = AtomicU64::new(0);
static INCOMPLETE_RAW: AtomicU64 = AtomicU64::new(0);
static KILL_SWITCH: AtomicU64 = AtomicU64::new(0);
static CONCURRENT_PEAK: AtomicU64 = AtomicU64::new(0);
static IN_FLIGHT: AtomicU64 = AtomicU64::new(0);
static TOTAL_WRITE_MS: AtomicU64 = AtomicU64::new(0);
static WRITE_SAMPLES: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);

pub fn reset() {
    for a in [
        &DEBOUNCE,
        &HEARTBEAT,
        &SOFT_DISCARD,
        &SIGNALS,
        &DIRECT,
        &INCOMPLETE_RAW,
        &KILL_SWITCH,
        &CONCURRENT_PEAK,
        &IN_FLIGHT,
        &TOTAL_WRITE_MS,
        &WRITE_SAMPLES,
        &SKIPPED,
    ] {
        a.store(0, Ordering::Relaxed);
    }
}

pub fn note_skipped() {
    SKIPPED.fetch_add(1, Ordering::Relaxed);
}

pub fn note_source(source: JournalWriteSource) {
    let counter = match source {
        JournalWriteSource::Debounce => &DEBOUNCE,
        JournalWriteSource::Heartbeat => &HEARTBEAT,
        JournalWriteSource::SoftDiscard => &SOFT_DISCARD,
        JournalWriteSource::Signals => &SIGNALS,
        JournalWriteSource::Direct => &DIRECT,
    };
    counter.fetch_add(1, Ordering::Relaxed);
}

pub fn note_incomplete_raw_skip() {
    INCOMPLETE_RAW.fetch_add(1, Ordering::Relaxed);
}

pub fn note_kill_switch_noop() {
    KILL_SWITCH.fetch_add(1, Ordering::Relaxed);
}

/// RAII guard that tracks concurrent in-flight journal writes.
pub struct InFlightGuard;

impl InFlightGuard {
    pub fn enter() -> Self {
        let n = IN_FLIGHT.fetch_add(1, Ordering::AcqRel) + 1;
        CONCURRENT_PEAK.fetch_max(n, Ordering::Relaxed);
        Self
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        IN_FLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

pub fn note_write_duration(d: Duration) {
    let ms = d.as_millis().min(u64::MAX as u128) as u64;
    TOTAL_WRITE_MS.fetch_add(ms, Ordering::Relaxed);
    WRITE_SAMPLES.fetch_add(1, Ordering::Relaxed);
}

pub fn snapshot() -> JournalStatsSnapshot {
    JournalStatsSnapshot {
        debounce: DEBOUNCE.load(Ordering::Relaxed),
        heartbeat: HEARTBEAT.load(Ordering::Relaxed),
        soft_discard: SOFT_DISCARD.load(Ordering::Relaxed),
        signals: SIGNALS.load(Ordering::Relaxed),
        direct: DIRECT.load(Ordering::Relaxed),
        incomplete_raw_skips: INCOMPLETE_RAW.load(Ordering::Relaxed),
        kill_switch_noops: KILL_SWITCH.load(Ordering::Relaxed),
        concurrent_peak: CONCURRENT_PEAK.load(Ordering::Relaxed),
        in_flight: IN_FLIGHT.load(Ordering::Relaxed),
        total_write_ms: TOTAL_WRITE_MS.load(Ordering::Relaxed),
        write_samples: WRITE_SAMPLES.load(Ordering::Relaxed),
        skipped: SKIPPED.load(Ordering::Relaxed),
    }
}
