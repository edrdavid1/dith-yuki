//! Preview-latency Level 1 on the worker side (`.local-doc/diagnostic.md` §5.1).
//!
//! The hot path loads [`LEVEL`]. `Instant::now` and the summary mutex run only
//! when that value is non-zero. Compute itself is not wrapped in a lock.

use std::cell::Cell;
use std::collections::HashSet;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 0 = off, 1 = gesture summary, 2 = per-tile (reserved). Relaxed load on the hot path.
pub static LEVEL: AtomicU8 = AtomicU8::new(0);

const CAP: Duration = Duration::from_secs(5);

pub fn level() -> u8 {
    LEVEL.load(Ordering::Relaxed)
}

/// `enabled: false` stores 0. An enabled level is clamped to 1 or 2.
pub fn sync_from_config(enabled: bool, level: u8) {
    let stored = if enabled { level.clamp(1, 2) } else { 0 };
    LEVEL.store(stored, Ordering::Relaxed);
}

struct Bucket {
    doc: u32,
    gen: u64,
    l0_tasks: u32,
    sum: Duration,
    max: Duration,
    display_ready: u32,
    started: Instant,
    emitted: bool,
    s0_emitted: bool,
    ensure_calls: u32,
    hits: u32,
    waits: u32,
    renders: u32,
    publish_runs: u32,
    publish_sum: Duration,
    publish_max: Duration,
    /// Processed-stage looks that found no cache entry while composing.
    composer_tile_misses: u32,
    /// Single-tile insert from a full-document result because the Processed
    /// entry was missing (slice fallback; common under Option B / eviction).
    tile_fallback_inserts: u32,
    wait_sum: Duration,
    render_sum: Duration,
    tile_sum: Duration,
    /// set_viewport enqueue of visible+prefetch Composite tasks.
    schedule_sum: Duration,
    /// Time inside `composite_tile` blend (excludes ensure/publish).
    composite_sum: Duration,
    /// `tile-ready` IPC emit.
    emit_sum: Duration,
    evict_passes: u32,
    evicted_tiles: u32,
    cache_used: u64,
    cache_budget: u64,
    unique_keys: HashSet<(u8, u32, u32)>,
    unique_coords: HashSet<(u32, u32)>,
    last_display: Option<Duration>,
    gpu_path: Option<String>,
    gpu_backend: Option<String>,
    gpu_pipeline: Option<String>,
    gpu_strips: u32,
    gpu_total_ms: f64,
    gpu_prepare_ms: f64,
    gpu_upload_ms: f64,
    gpu_dispatch_ms: f64,
    gpu_readback_ms: f64,
    gpu_post_ms: f64,
}

#[derive(Clone, Copy)]
struct TaskPhase {
    wait: Duration,
    render: Duration,
    publish: Duration,
}

thread_local! {
    static TASK_PHASE: Cell<TaskPhase> = const { Cell::new(TaskPhase {
        wait: Duration::ZERO,
        render: Duration::ZERO,
        publish: Duration::ZERO,
    }) };
}

static BUCKETS: Mutex<Vec<Bucket>> = Mutex::new(Vec::new());

#[cfg(test)]
static TEST_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[cfg(test)]
static TEST_GATE: Mutex<()> = Mutex::new(());

fn emit_line(line: String) {
    log::info!(target: "preview_diag", "{line}");
    #[cfg(test)]
    if let Ok(mut lines) = TEST_LINES.lock() {
        lines.push(line);
    }
}

/// Clear per-task phase accumulators. Call at the start of a worker task.
pub fn reset_task_phases() {
    TASK_PHASE.with(|p| {
        p.set(TaskPhase {
            wait: Duration::ZERO,
            render: Duration::ZERO,
            publish: Duration::ZERO,
        });
    });
}

/// Add wait/render/publish spent inside the current worker task.
pub fn add_task_phase(wait: Duration, render: Duration, publish: Duration) {
    TASK_PHASE.with(|p| {
        let mut cur = p.get();
        cur.wait += wait;
        cur.render += render;
        cur.publish += publish;
        p.set(cur);
    });
}

pub fn record_ensure(doc: u32, gen: u64) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.ensure_calls += 1;
}

pub fn record_ensure_class(doc: u32, gen: u64, hit: bool, waited: bool, rendered: bool) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    if hit {
        b.hits += 1;
    }
    if waited {
        b.waits += 1;
    }
    if rendered {
        b.renders += 1;
    }
}

/// Snapshot the last full-document GPU apply for this generation (call before
/// `take_last_gpu_dither_status` / emit clears it). Only stores when a fresh
/// render happened.
pub fn record_gpu_apply(
    doc: u32,
    gen: u64,
    s: &engine_project::algorithms::gpu_dither::GpuDitherStatus,
) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.gpu_path = Some(s.path.clone());
    b.gpu_backend = Some(s.backend.clone());
    b.gpu_pipeline = Some(s.pipeline.clone());
    b.gpu_strips = s.strips;
    b.gpu_total_ms = f64::from(s.elapsed_ms);
    b.gpu_prepare_ms = s.prepare_ms;
    b.gpu_upload_ms = s.upload_ms;
    b.gpu_dispatch_ms = s.dispatch_ms;
    b.gpu_readback_ms = s.readback_ms;
    b.gpu_post_ms = s.post_ms;
}

pub fn record_publish(doc: u32, gen: u64, elapsed: Duration) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.publish_runs += 1;
    b.publish_sum += elapsed;
    if elapsed > b.publish_max {
        b.publish_max = elapsed;
    }
}

/// Processed-stage look that found no cache entry while the composer gathers tiles.
pub fn record_composer_tile_miss(doc: u32, gen: u64) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.composer_tile_misses += 1;
}

/// Slice-insert of one Processed tile that was not already cached.
pub fn record_tile_fallback_insert(doc: u32, gen: u64) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.tile_fallback_inserts += 1;
}

pub fn record_schedule(doc: u32, gen: u64, elapsed: Duration) {
    if level() == 0 || elapsed.is_zero() {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.schedule_sum += elapsed;
}

pub fn record_composite(doc: u32, gen: u64, elapsed: Duration) {
    if level() == 0 || elapsed.is_zero() {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.composite_sum += elapsed;
}

pub fn record_emit(doc: u32, gen: u64, elapsed: Duration) {
    if level() == 0 || elapsed.is_zero() {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.emit_sum += elapsed;
}

pub fn record_evict(doc: u32, gen: u64, passes: u32, removed: u32) {
    if level() == 0 || (passes == 0 && removed == 0) {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.evict_passes = b.evict_passes.saturating_add(passes);
    b.evicted_tiles = b.evicted_tiles.saturating_add(removed);
}

pub fn note_cache(doc: u32, gen: u64, used: u64, budget: u64) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.cache_used = used;
    b.cache_budget = budget;
}

/// Fold this task's phases into the gesture and count its tile identity.
pub fn commit_task_phases(doc: u32, gen: u64, total: Duration, stage: u8, x: u32, y: u32) {
    if level() == 0 {
        return;
    }
    let phase = TASK_PHASE.with(|p| {
        p.replace(TaskPhase {
            wait: Duration::ZERO,
            render: Duration::ZERO,
            publish: Duration::ZERO,
        })
    });
    let accounted = phase
        .wait
        .saturating_add(phase.render)
        .saturating_add(phase.publish);
    let tile = total.saturating_sub(accounted);
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.wait_sum += phase.wait;
    b.render_sum += phase.render;
    b.tile_sum += tile;
    b.unique_keys.insert((stage, x, y));
    b.unique_coords.insert((x, y));
}

/// Wall time of one level-0 processed or composite task that actually ran.
pub fn record_l0_compute(doc: u32, gen: u64, elapsed: Duration) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.l0_tasks += 1;
    b.sum += elapsed;
    if elapsed > b.max {
        b.max = elapsed;
    }
}

pub fn record_display_ready(doc: u32, gen: u64) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let b = touch(&mut buckets, doc, gen);
    if b.s0_emitted {
        return;
    }
    b.display_ready += 1;
    b.last_display = Some(b.started.elapsed());
}

/// Emit the generation summary once the scheduler and in-flight set are empty,
/// or once the 5s cap is hit. Later tasks for an emitted generation are ignored.
pub fn maybe_flush(doc: u32, gen: u64, pending: usize, inflight: usize) {
    if level() == 0 {
        return;
    }
    let Ok(mut buckets) = BUCKETS.lock() else {
        return;
    };
    let Some(b) = buckets.iter_mut().find(|b| b.doc == doc && b.gen == gen) else {
        return;
    };
    if b.s0_emitted {
        return;
    }
    let capped = b.started.elapsed() >= CAP;
    let drained = pending == 0 && inflight == 0;
    if !b.emitted && (capped || drained) {
        let closed_by = if drained { "drained" } else { "cap_5s" };
        let line = format!(
            "[diag/L1] {{\"diag\":\"L1\",\"side\":\"rust\",\"doc\":{doc},\"gen\":{gen},\
\"l0_tasks\":{},\"l0_compute_ms_sum\":{:.1},\"l0_compute_ms_max\":{:.1},\
\"display_ready\":{},\"closed_by\":\"{closed_by}\"}}",
            b.l0_tasks,
            ms(b.sum),
            ms(b.max),
            b.display_ready,
        );
        b.emitted = true;
        drop(buckets);
        emit_line(line);
        if !drained {
            return;
        }
        let Ok(mut buckets) = BUCKETS.lock() else {
            return;
        };
        let Some(b) = buckets.iter_mut().find(|b| b.doc == doc && b.gen == gen) else {
            return;
        };
        emit_s0(b);
        return;
    }
    if drained {
        emit_s0(b);
    }
}

fn emit_s0(b: &mut Bucket) {
    if b.s0_emitted {
        return;
    }
    let (to_last, last_src) = match b.last_display {
        Some(d) => (d, "display"),
        None => (b.started.elapsed(), "drain"),
    };
    let gpu_path = b.gpu_path.as_deref().unwrap_or("none");
    let gpu_backend = b.gpu_backend.as_deref().unwrap_or("none");
    let gpu_pipeline = b.gpu_pipeline.as_deref().unwrap_or("none");
    let line = format!(
        "[diag/S0] {{\"diag\":\"S0\",\"doc\":{},\"gen\":{},\
\"ensure_calls\":{},\"hits\":{},\"waits\":{},\"renders\":{},\
\"publish_runs\":{},\"publish_ms_sum\":{:.1},\"publish_ms_max\":{:.1},\
\"composer_tile_misses\":{},\"tile_fallback_inserts\":{},\
\"wait_ms_sum\":{:.1},\"render_ms_sum\":{:.1},\"tile_ms_sum\":{:.1},\
\"schedule_ms_sum\":{:.1},\"composite_ms_sum\":{:.1},\"emit_ms_sum\":{:.1},\
\"evict_passes\":{},\"evicted_tiles\":{},\
\"cache_used\":{},\"cache_budget\":{},\
\"unique_tiles\":{},\"unique_coords\":{},\
\"l0_tasks\":{},\"l0_compute_ms_sum\":{:.1},\
\"to_last_tile_ms\":{:.1},\"last_tile_src\":\"{last_src}\",\
\"gpu_path\":\"{gpu_path}\",\"gpu_backend\":\"{gpu_backend}\",\"gpu_pipeline\":\"{gpu_pipeline}\",\
\"gpu_strips\":{},\"gpu_total_ms\":{:.1},\
\"gpu_prepare_ms\":{:.1},\"gpu_upload_ms\":{:.1},\"gpu_dispatch_ms\":{:.1},\
\"gpu_readback_ms\":{:.1},\"gpu_post_ms\":{:.1}}}",
        b.doc,
        b.gen,
        b.ensure_calls,
        b.hits,
        b.waits,
        b.renders,
        b.publish_runs,
        ms(b.publish_sum),
        ms(b.publish_max),
        b.composer_tile_misses,
        b.tile_fallback_inserts,
        ms(b.wait_sum),
        ms(b.render_sum),
        ms(b.tile_sum),
        ms(b.schedule_sum),
        ms(b.composite_sum),
        ms(b.emit_sum),
        b.evict_passes,
        b.evicted_tiles,
        b.cache_used,
        b.cache_budget,
        b.unique_keys.len(),
        b.unique_coords.len(),
        b.l0_tasks,
        ms(b.sum),
        ms(to_last),
        b.gpu_strips,
        b.gpu_total_ms,
        b.gpu_prepare_ms,
        b.gpu_upload_ms,
        b.gpu_dispatch_ms,
        b.gpu_readback_ms,
        b.gpu_post_ms,
    );
    b.s0_emitted = true;
    b.emitted = true;
    emit_line(line);
}

fn touch(buckets: &mut Vec<Bucket>, doc: u32, gen: u64) -> &mut Bucket {
    if let Some(i) = buckets.iter().position(|b| b.doc == doc && b.gen == gen) {
        return &mut buckets[i];
    }
    buckets.push(Bucket {
        doc,
        gen,
        l0_tasks: 0,
        sum: Duration::ZERO,
        max: Duration::ZERO,
        display_ready: 0,
        started: Instant::now(),
        emitted: false,
        s0_emitted: false,
        ensure_calls: 0,
        hits: 0,
        waits: 0,
        renders: 0,
        publish_runs: 0,
        publish_sum: Duration::ZERO,
        publish_max: Duration::ZERO,
        composer_tile_misses: 0,
        tile_fallback_inserts: 0,
        wait_sum: Duration::ZERO,
        render_sum: Duration::ZERO,
        tile_sum: Duration::ZERO,
        schedule_sum: Duration::ZERO,
        composite_sum: Duration::ZERO,
        emit_sum: Duration::ZERO,
        evict_passes: 0,
        evicted_tiles: 0,
        cache_used: 0,
        cache_budget: 0,
        unique_keys: HashSet::new(),
        unique_coords: HashSet::new(),
        last_display: None,
        gpu_path: None,
        gpu_backend: None,
        gpu_pipeline: None,
        gpu_strips: 0,
        gpu_total_ms: 0.0,
        gpu_prepare_ms: 0.0,
        gpu_upload_ms: 0.0,
        gpu_dispatch_ms: 0.0,
        gpu_readback_ms: 0.0,
        gpu_post_ms: 0.0,
    });
    buckets.last_mut().unwrap()
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

#[cfg(test)]
pub fn test_lines() -> Vec<String> {
    TEST_LINES.lock().map(|g| g.clone()).unwrap_or_default()
}

#[cfg(test)]
pub fn test_gate() -> std::sync::MutexGuard<'static, ()> {
    TEST_GATE.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
pub fn clear_test_state() {
    LEVEL.store(0, Ordering::Relaxed);
    if let Ok(mut b) = BUCKETS.lock() {
        b.clear();
    }
    if let Ok(mut lines) = TEST_LINES.lock() {
        lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_does_not_emit() {
        let _gate = TEST_GATE.lock().unwrap();
        clear_test_state();
        sync_from_config(false, 1);
        record_l0_compute(1, 1, Duration::from_millis(4));
        record_display_ready(1, 1);
        maybe_flush(1, 1, 0, 0);
        assert!(test_lines().is_empty());
        assert_eq!(level(), 0);
    }

    #[test]
    fn enabled_flushes_when_drained() {
        let _gate = TEST_GATE.lock().unwrap();
        clear_test_state();
        sync_from_config(true, 1);
        record_l0_compute(7, 3, Duration::from_millis(8));
        record_l0_compute(7, 3, Duration::from_millis(12));
        record_display_ready(7, 3);
        maybe_flush(7, 3, 0, 0);
        let lines: Vec<_> = test_lines()
            .into_iter()
            .filter(|l| l.contains("[diag/L1]"))
            .collect();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("[diag/L1] "));
        assert!(lines[0].contains("\"l0_tasks\":2"));
        assert!(lines[0].contains("\"display_ready\":1"));
        assert!(lines[0].contains("\"closed_by\":\"drained\""));
        assert!(lines[0].contains("\"l0_compute_ms_sum\":20.0"));
        maybe_flush(7, 3, 0, 0);
        let l1_again = test_lines()
            .into_iter()
            .filter(|l| l.contains("[diag/L1]"))
            .count();
        assert_eq!(l1_again, 1);
        clear_test_state();
    }
}
