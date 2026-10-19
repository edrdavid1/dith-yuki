//! Step A counters for CPU Floyd–Steinberg / ED strip profiling.
//!
//! Active when `DITHER_TRACE=1` **or** `DITHER_FS_PROF=1`. Zero-cost when off
//! (single atomic load before any timing).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static ENABLED: AtomicBool = AtomicBool::new(false);
static INIT: OnceLock<()> = OnceLock::new();

static STRIP_RUNS: AtomicU64 = AtomicU64::new(0);
static STRIP_PIXELS: AtomicU64 = AtomicU64::new(0);
static TILE_TASKS: AtomicU64 = AtomicU64::new(0);
static STRIP_RECALC_EXTRA: AtomicU64 = AtomicU64::new(0);
static NS_RAW: AtomicU64 = AtomicU64::new(0);
static NS_RESIDUAL_LOAD: AtomicU64 = AtomicU64::new(0);
static NS_ED_CYCLE: AtomicU64 = AtomicU64::new(0);
static NS_RESIDUAL_STORE: AtomicU64 = AtomicU64::new(0);
static NS_CACHE_WRITE: AtomicU64 = AtomicU64::new(0);
static NS_WAKE: AtomicU64 = AtomicU64::new(0);
static NS_PREV_ROW: AtomicU64 = AtomicU64::new(0);

/// Per-ty strip compute counts (index = ty, capped).
const TY_SLOTS: usize = 512;
static TY_STRIP_COUNT: [AtomicU64; TY_SLOTS] = {
    #[allow(clippy::declare_interior_mutable_const)]
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; TY_SLOTS]
};

pub fn init_from_env() {
    INIT.get_or_init(|| {
        let on = std::env::var("DITHER_FS_PROF")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            || std::env::var("DITHER_TRACE")
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
        ENABLED.store(on, Ordering::Relaxed);
    });
}

/// Test/harness override (bypasses OnceLock freeze after first `init_from_env`).
pub fn force_enable(on: bool) {
    let _ = INIT.get_or_init(|| ());
    ENABLED.store(on, Ordering::Relaxed);
}

pub fn enabled() -> bool {
    init_from_env();
    ENABLED.load(Ordering::Relaxed)
}

pub fn reset() {
    init_from_env();
    STRIP_RUNS.store(0, Ordering::Relaxed);
    STRIP_PIXELS.store(0, Ordering::Relaxed);
    TILE_TASKS.store(0, Ordering::Relaxed);
    STRIP_RECALC_EXTRA.store(0, Ordering::Relaxed);
    NS_RAW.store(0, Ordering::Relaxed);
    NS_RESIDUAL_LOAD.store(0, Ordering::Relaxed);
    NS_ED_CYCLE.store(0, Ordering::Relaxed);
    NS_RESIDUAL_STORE.store(0, Ordering::Relaxed);
    NS_CACHE_WRITE.store(0, Ordering::Relaxed);
    NS_WAKE.store(0, Ordering::Relaxed);
    NS_PREV_ROW.store(0, Ordering::Relaxed);
    for a in TY_STRIP_COUNT.iter() {
        a.store(0, Ordering::Relaxed);
    }
}

#[inline]
pub fn note_tile_task() {
    if !enabled() {
        return;
    }
    TILE_TASKS.fetch_add(1, Ordering::Relaxed);
}

#[inline]
pub fn note_strip_run(ty: u32, pixels: u64) {
    if !enabled() {
        return;
    }
    STRIP_RUNS.fetch_add(1, Ordering::Relaxed);
    STRIP_PIXELS.fetch_add(pixels, Ordering::Relaxed);
    let idx = ty as usize;
    if idx < TY_SLOTS {
        let prev = TY_STRIP_COUNT[idx].fetch_add(1, Ordering::Relaxed);
        if prev > 0 {
            STRIP_RECALC_EXTRA.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[inline]
pub fn add_ns_raw(ns: u64) {
    if enabled() {
        NS_RAW.fetch_add(ns, Ordering::Relaxed);
    }
}
#[inline]
pub fn add_ns_residual_load(ns: u64) {
    if enabled() {
        NS_RESIDUAL_LOAD.fetch_add(ns, Ordering::Relaxed);
    }
}
#[inline]
pub fn add_ns_ed_cycle(ns: u64) {
    if enabled() {
        NS_ED_CYCLE.fetch_add(ns, Ordering::Relaxed);
    }
}
#[inline]
pub fn add_ns_residual_store(ns: u64) {
    if enabled() {
        NS_RESIDUAL_STORE.fetch_add(ns, Ordering::Relaxed);
    }
}
#[inline]
pub fn add_ns_cache_write(ns: u64) {
    if enabled() {
        NS_CACHE_WRITE.fetch_add(ns, Ordering::Relaxed);
    }
}
#[inline]
pub fn add_ns_wake(ns: u64) {
    if enabled() {
        NS_WAKE.fetch_add(ns, Ordering::Relaxed);
    }
}
#[inline]
pub fn add_ns_prev_row(ns: u64) {
    if enabled() {
        NS_PREV_ROW.fetch_add(ns, Ordering::Relaxed);
    }
}

pub struct Scope {
    start: Instant,
    add: fn(u64),
}

impl Scope {
    #[inline]
    pub fn new(add: fn(u64)) -> Option<Self> {
        if enabled() {
            Some(Self {
                start: Instant::now(),
                add,
            })
        } else {
            None
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        let ns = self.start.elapsed().as_nanos() as u64;
        (self.add)(ns);
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub strip_runs: u64,
    pub strip_pixels: u64,
    pub tile_tasks: u64,
    pub strip_recalc_extra: u64,
    pub unique_ty_strips: u64,
    pub ns_raw: u64,
    pub ns_residual_load: u64,
    pub ns_ed_cycle: u64,
    pub ns_residual_store: u64,
    pub ns_cache_write: u64,
    pub ns_wake: u64,
    pub ns_prev_row: u64,
}

impl Snapshot {
    pub fn capture() -> Self {
        let mut unique = 0u64;
        for a in TY_STRIP_COUNT.iter() {
            if a.load(Ordering::Relaxed) > 0 {
                unique += 1;
            }
        }
        Self {
            strip_runs: STRIP_RUNS.load(Ordering::Relaxed),
            strip_pixels: STRIP_PIXELS.load(Ordering::Relaxed),
            tile_tasks: TILE_TASKS.load(Ordering::Relaxed),
            strip_recalc_extra: STRIP_RECALC_EXTRA.load(Ordering::Relaxed),
            unique_ty_strips: unique,
            ns_raw: NS_RAW.load(Ordering::Relaxed),
            ns_residual_load: NS_RESIDUAL_LOAD.load(Ordering::Relaxed),
            ns_ed_cycle: NS_ED_CYCLE.load(Ordering::Relaxed),
            ns_residual_store: NS_RESIDUAL_STORE.load(Ordering::Relaxed),
            ns_cache_write: NS_CACHE_WRITE.load(Ordering::Relaxed),
            ns_wake: NS_WAKE.load(Ordering::Relaxed),
            ns_prev_row: NS_PREV_ROW.load(Ordering::Relaxed),
        }
    }

    pub fn timed_ns_total(&self) -> u64 {
        self.ns_raw
            + self.ns_residual_load
            + self.ns_ed_cycle
            + self.ns_residual_store
            + self.ns_cache_write
            + self.ns_wake
            + self.ns_prev_row
    }

    pub fn report_lines(&self) -> String {
        let total = self.timed_ns_total().max(1) as f64;
        let pct = |ns: u64| 100.0 * ns as f64 / total;
        let ms = |ns: u64| ns as f64 / 1e6;
        format!(
            "fs_ed_prof:\n\
             strip_runs={strip_runs} unique_ty={unique_ty} recalc_extra={recalc} \
             tile_tasks={tasks} tasks/unique_ty={tp:.2}\n\
             strip_pixels={pix}  (≈{mpix:.3} MPix of strip work)\n\
             phases ms/% of timed:\n\
               prev_row_ensure  {prev_ms:8.1}  {prev_pct:5.1}%\n\
               raw_gather       {raw_ms:8.1}  {raw_pct:5.1}%\n\
               residual_load    {rl_ms:8.1}  {rl_pct:5.1}%\n\
               ed_cycle         {ed_ms:8.1}  {ed_pct:5.1}%\n\
               residual_store   {rs_ms:8.1}  {rs_pct:5.1}%\n\
               cache_write      {cw_ms:8.1}  {cw_pct:5.1}%\n\
               wake             {wk_ms:8.1}  {wk_pct:5.1}%\n\
               timed_total      {tot_ms:8.1}\n",
            strip_runs = self.strip_runs,
            unique_ty = self.unique_ty_strips,
            recalc = self.strip_recalc_extra,
            tasks = self.tile_tasks,
            tp = if self.unique_ty_strips > 0 {
                self.tile_tasks as f64 / self.unique_ty_strips as f64
            } else {
                0.0
            },
            pix = self.strip_pixels,
            mpix = self.strip_pixels as f64 / 1e6,
            prev_ms = ms(self.ns_prev_row),
            prev_pct = pct(self.ns_prev_row),
            raw_ms = ms(self.ns_raw),
            raw_pct = pct(self.ns_raw),
            rl_ms = ms(self.ns_residual_load),
            rl_pct = pct(self.ns_residual_load),
            ed_ms = ms(self.ns_ed_cycle),
            ed_pct = pct(self.ns_ed_cycle),
            rs_ms = ms(self.ns_residual_store),
            rs_pct = pct(self.ns_residual_store),
            cw_ms = ms(self.ns_cache_write),
            cw_pct = pct(self.ns_cache_write),
            wk_ms = ms(self.ns_wake),
            wk_pct = pct(self.ns_wake),
            tot_ms = ms(self.timed_ns_total()),
        )
    }
}
