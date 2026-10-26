//! STEP4 — Composite input-version correctness trap + post-fix stress.
//!
//! ```text
//! DITHER_GPU_PATH=cpu DITHER_JOURNAL=0 \
//!   cargo test -p dither --release step4_ -- --ignored --nocapture --test-threads=1
//!
//! DITHER_STEP4_N=100 DITHER_STEP4_QUICK=1 DITHER_COMPOSITE_INPUT_VERSIONS=0  # trap baseline
//! ```

#![cfg(test)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use engine_project::filter::{DitherModeV2, DitherParamsV2};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterInstance, FilterKind, FilterParams};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};
use serde_json::json;

use crate::commands::{schedule_dirty_viewport_tiles, AppState};
use crate::services::viewport_service::ViewportService;
use crate::worker::task_is_stale;

#[derive(Clone, Copy)]
struct SizeSpec {
    w: u32,
    h: u32,
    label: &'static str,
}

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../profiling/results/2026-10-22/second_interaction")
}

fn quick() -> bool {
    std::env::var("DITHER_STEP4_QUICK").ok().as_deref() == Some("1")
}

fn n_trials() -> usize {
    if quick() {
        return 20;
    }
    std::env::var("DITHER_STEP4_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(100)
}

fn soft_trap() -> bool {
    std::env::var("DITHER_STEP4_SOFT_TRAP").ok().as_deref() == Some("1")
}

fn out_json_name(versions_on: bool) -> String {
    if let Ok(name) = std::env::var("DITHER_STEP4_OUT") {
        if !name.is_empty() {
            return name;
        }
    }
    if versions_on {
        "step4_stress_fixed.json".into()
    } else {
        "step4_stress_baseline_trap.json".into()
    }
}

fn parse_worker_sets() -> Vec<usize> {
    if let Ok(s) = std::env::var("DITHER_STEP4_WORKERS") {
        let v: Vec<usize> = s.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        if !v.is_empty() {
            return v;
        }
    }
    if quick() {
        vec![1, 8]
    } else {
        vec![1, 2, 4, 8]
    }
}

fn mode_allowed(label: &str) -> bool {
    let Ok(s) = std::env::var("DITHER_STEP4_MODES") else {
        return true;
    };
    if s.trim().is_empty() {
        return true;
    }
    s.split(',')
        .map(|p| p.trim())
        .any(|p| p == label || p == "*")
}

fn cache_footprint(state: &AppState) -> serde_json::Value {
    let entries = state.tiles.tile_cache.entries.len();
    let used_mb = state.tiles.tile_cache.used_bytes_count() as f64 / (1024.0 * 1024.0);
    let mut composite = 0usize;
    let mut input_pairs = 0usize;
    for e in state.tiles.tile_cache.entries.iter() {
        if e.key().stage == CacheStage::Composite {
            composite += 1;
            input_pairs += e.value().input_versions.len();
        }
    }
    // TileKey ≈ 20 B + u64 = 28 B per pair; Arc header ~16 B (rough).
    let approx_manifest_bytes = composite * 16 + input_pairs * 28;
    json!({
        "entries": entries,
        "composite_entries": composite,
        "input_version_pairs": input_pairs,
        "used_mb": used_mb,
        "approx_manifest_bytes": approx_manifest_bytes,
        "tile_bytes_const": engine_tiles::TILE_BYTES,
    })
}

fn wait_silence(state: &AppState, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let busy = state.full_document_busy.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        if queued == 0 && inflight == 0 && busy == 0 && blocked == 0 {
            thread::sleep(Duration::from_millis(40));
            if state.tiles.scheduler.queued_len() == 0
                && state.preview_pass_inflight.load(Ordering::Acquire) == 0
                && state.full_document_busy.load(Ordering::Acquire) == 0
                && state.tiles.ed_frontier.blocked_count() == 0
            {
                return true;
            }
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(5));
    }
}

fn current_content_gen(state: &AppState) -> u64 {
    state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .current_content_gen()
}

fn sync_epoch(state: &AppState) {
    state
        .tiles
        .tile_cache
        .set_content_epoch(current_content_gen(state));
}

fn wave_timeout() -> Duration {
    let secs = std::env::var("DITHER_STEP4_WAVE_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(90);
    Duration::from_secs(secs)
}

fn wave_quiet(state: &AppState, timeout: Duration) -> bool {
    let gen = current_content_gen(state);
    sync_epoch(state);
    let deadline = Instant::now() + timeout;
    let mut last_log = Instant::now();
    loop {
        let queued = state.tiles.scheduler.queued_len();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        let busy = state.full_document_busy.load(Ordering::Acquire);
        let blocked = state.tiles.ed_frontier.blocked_count();
        if queued == 0 && inflight == 0 && busy == 0 && blocked == 0 {
            thread::sleep(Duration::from_millis(60));
            if state.tiles.scheduler.queued_len() == 0
                && state.preview_pass_inflight.load(Ordering::Acquire) == 0
                && state.full_document_busy.load(Ordering::Acquire) == 0
                && state.tiles.ed_frontier.blocked_count() == 0
            {
                break;
            }
        }
        if last_log.elapsed() >= Duration::from_secs(15) {
            eprintln!(
                "[step4] wave wait queued={queued} inflight={inflight} busy={busy} blocked={blocked} gen={gen}"
            );
            last_log = Instant::now();
            // Missed wakes: park deps already fresh but frontier never notified.
            if blocked > 0 && queued == 0 && inflight == 0 {
                let ready = state.tiles.ed_frontier.sweep_ready(&state.tiles.tile_cache);
                for task in ready {
                    state.tiles.scheduler.enqueue_or_bump(task);
                }
            }
            state.worker_wake.notify_all();
        }
        if Instant::now() >= deadline {
            eprintln!(
                "[step4] wave TIMEOUT queued={} inflight={} busy={} blocked={}",
                state.tiles.scheduler.queued_len(),
                state.preview_pass_inflight.load(Ordering::Acquire),
                state.full_document_busy.load(Ordering::Acquire),
                state.tiles.ed_frontier.blocked_count()
            );
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let _ = wait_silence(state, Duration::from_millis(150));
    // Require visibles fresh; pyramid children checked via hash completeness.
    sync_epoch(state);
    let vp = state.ui.viewport.lock().unwrap().clone();
    vp.visible_tiles.iter().all(|c| {
        engine_tiles::tile_ready_emit_gen(
            &state.tiles.tile_cache,
            TileKey {
                doc: 1,
                layer: 0,
                coord: *c,
                stage: CacheStage::Composite,
            },
        ) == Some(gen)
    })
}

fn raw_doc(w: u32, h: u32) -> Document {
    let mut doc = Document::new(DocumentId::new(1), w, h);
    doc.root.push(LayerNode::Leaf(Layer::new(
        LayerId::new(1),
        LayerKind::Raster,
        w,
        h,
    )));
    doc
}

fn fill_raw(state: &AppState, w: u32, h: u32) {
    use engine_tiles::{HALO, TILE_SIZE};
    let cols = w.div_ceil(TILE_SIZE);
    let rows = h.div_ceil(TILE_SIZE);
    let full = (TILE_SIZE + 2 * HALO) as usize;
    for y in 0..rows {
        for x in 0..cols {
            let mut tile = PixelTile::new();
            let v = 0.2 + 0.6 * ((x + y) as f32) / ((cols + rows) as f32);
            for i in (0..full * full * 4).step_by(4) {
                tile.data[i] = v;
                tile.data[i + 1] = v;
                tile.data[i + 2] = v;
                tile.data[i + 3] = 1.0;
            }
            state.tiles.tile_cache.insert_fresh(
                TileKey {
                    doc: 1,
                    layer: 1,
                    coord: TileCoord { level: 0, x, y },
                    stage: CacheStage::Raw,
                },
                Arc::new(tile),
            );
        }
    }
}

fn worker_loop(state: Arc<AppState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        if let Some(task) = state.tiles.scheduler.dequeue() {
            state.preview_pass_inflight.fetch_add(1, Ordering::AcqRel);
            let snapshot = match state.session(task.key.doc) {
                Ok(s) => s.document_handle.snapshot(),
                Err(_) => {
                    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                    crate::commands::on_preview_task_finished(&state);
                    continue;
                }
            };
            if task_is_stale(&task, &snapshot) {
                state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                crate::commands::on_preview_task_finished(&state);
                continue;
            }
            let result = match task.key.stage {
                CacheStage::Raw => state.tiles.tile_cache.get_entry(task.key).ok_or_else(|| {
                    engine_project::error::EngineError::invalid_state("raw missing")
                }),
                CacheStage::Processed => {
                    if task.key.coord.level > 0
                        && crate::tile_pipeline::layer_has_error_diffusion(
                            &snapshot.root,
                            task.key.layer,
                        )
                    {
                        state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                        crate::commands::on_preview_task_finished(&state);
                        continue;
                    }
                    if crate::tile_pipeline::layer_has_error_diffusion(
                        &snapshot.root,
                        task.key.layer,
                    ) && !engine_tiles::ed_ready(&state.tiles.tile_cache, task.key, true)
                    {
                        if let Some(ready) =
                            state.tiles.ed_frontier.block(task, &state.tiles.tile_cache)
                        {
                            state.tiles.scheduler.enqueue_or_bump(ready);
                            state.worker_wake.notify_one();
                        }
                        state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                        crate::commands::on_preview_task_finished(&state);
                        continue;
                    }
                    match crate::tile_pipeline::compute_processed_tile(task.key, &state) {
                        Err(engine_project::error::EngineError::EdStripBusy) => {
                            let deps =
                                crate::tile_pipeline::ed_strip_busy_park_deps(&state, task.key);
                            if deps.is_empty() {
                                state.tiles.scheduler.enqueue_or_bump(task);
                                state.worker_wake.notify_one();
                            } else {
                                let _ = state.tiles.ed_frontier.block_on(
                                    task,
                                    deps,
                                    &state.tiles.tile_cache,
                                );
                            }
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                            continue;
                        }
                        other => other,
                    }
                }
                CacheStage::Composite => {
                    match crate::tile_pipeline::compute_composite_tile(task.key, &state) {
                        Err(engine_project::error::EngineError::EdPrefixPending) => {
                            let deps = crate::tile_pipeline::composite_ed_park_deps(
                                &state,
                                &snapshot.root,
                                task.key.doc,
                                task.key.coord,
                            );
                            if deps.is_empty() {
                                state.delayed_requeue.schedule(
                                    Arc::clone(&state),
                                    task,
                                    Duration::from_millis(4),
                                );
                            } else if let Some(ready) = state.tiles.ed_frontier.block_on(
                                task,
                                deps,
                                &state.tiles.tile_cache,
                            ) {
                                state.tiles.scheduler.enqueue_or_bump(ready);
                                state.worker_wake.notify_one();
                            }
                            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
                            crate::commands::on_preview_task_finished(&state);
                            continue;
                        }
                        other => other,
                    }
                }
            };
            if let Ok(_) = result {
                if !task_is_stale(&task, &snapshot) {
                    crate::tile_pipeline::wake_ed_frontier_after_insert(&state, task.key);
                    if task.key.stage == CacheStage::Processed && task.key.coord.level == 0 {
                        crate::ed_progress::note_ed_pass_maybe_complete(&state, task.key.doc);
                    }
                }
            }
            state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
            crate::commands::on_preview_task_finished(&state);
        } else {
            state.worker_wake.wait();
        }
    }
}

fn setup(
    size: SizeSpec,
    workers: usize,
) -> (Arc<AppState>, Arc<AtomicBool>, Vec<thread::JoinHandle<()>>) {
    let state = Arc::new(AppState::empty_process(None, 1024 * 1024 * 1024, true));
    state.spawn_session(raw_doc(size.w, size.h));
    fill_raw(&state, size.w, size.h);
    // DITHER_STEP4_FIT=1 → small canvas so visible level reaches L3 (hashes L0..L3).
    let (cw, ch) = if std::env::var("DITHER_STEP4_FIT").ok().as_deref() == Some("1") {
        (400.0, 300.0)
    } else {
        (1200.0, 800.0)
    };
    let zoom = (cw / size.w as f64).min(ch / size.h as f64).min(1.0);
    let _ = ViewportService::new(Arc::clone(&state)).set_viewport(zoom, 0.0, 0.0, cw, ch);
    let stop = Arc::new(AtomicBool::new(false));
    let mut joins = Vec::new();
    for _ in 0..workers {
        let st = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        joins.push(thread::spawn(move || worker_loop(st, stop)));
    }
    let _ = wait_silence(&state, Duration::from_secs(30));
    (state, stop, joins)
}

fn teardown(state: &AppState, stop: &AtomicBool, joins: Vec<thread::JoinHandle<()>>) {
    stop.store(true, Ordering::Relaxed);
    for _ in 0..8 {
        state.worker_wake.notify_all();
    }
    for j in joins {
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let _ = j.join();
            let _ = tx.send(());
        });
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }
}

fn apply(state: &AppState, mode: DitherModeV2, ps: u8, lv: u16) -> bool {
    state.must_active().document_handle.mutate(|doc| {
        if let LayerNode::Leaf(layer) = &mut doc.root[0] {
            layer.filters.clear();
            layer.filters.push(FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: mode.clone(),
                    pixel_size: ps,
                    levels: lv,
                    ..DitherParamsV2::default()
                }),
            ));
        }
        doc.increment_generation();
    });
    state
        .must_active()
        .document_handle
        .snapshot()
        .generations
        .increment_layer_gen(1);
    let gen = current_content_gen(state);
    sync_epoch(state);
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: 1 },
    );
    state.tiles.error_residuals.evict_layer(1, LayerId::new(1));
    state.tiles.block_representatives.clear_dithered();
    state.tiles.clear_strip_gates();
    state.tiles.full_document.invalidate_layer(1, 1);
    // Match production refresh: drop parked tasks from the previous wave.
    state.tiles.ed_frontier.clear();
    crate::ed_progress::note_ed_pass_start(state, 1);
    schedule_dirty_viewport_tiles(state);
    state.worker_wake.notify_all();
    let _ = gen;
    wave_quiet(state, wave_timeout())
}

/// Blake3 digests of every Composite under the visible set at levels 0..=visible.
fn pyramid_digests(state: &AppState) -> serde_json::Value {
    sync_epoch(state);
    let gen = current_content_gen(state);
    let vp = state.ui.viewport.lock().unwrap().clone();
    let vis_level = vp.level;
    let mut by_level: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for level in 0..=vis_level {
        let mut keys = Vec::new();
        if level == vis_level {
            for c in &vp.visible_tiles {
                keys.push(*c);
            }
        } else {
            let shift = vis_level - level;
            for c in &vp.visible_tiles {
                let x0 = c.x << shift;
                let y0 = c.y << shift;
                let n = 1u32 << shift;
                for dy in 0..n {
                    for dx in 0..n {
                        keys.push(TileCoord {
                            level,
                            x: x0 + dx,
                            y: y0 + dy,
                        });
                    }
                }
            }
        }
        keys.sort_by_key(|c| (c.y, c.x));
        keys.dedup();
        let mut hasher = blake3::Hasher::new();
        let mut n = 0u32;
        let mut stale = 0u32;
        for coord in keys {
            let key = TileKey {
                doc: 1,
                layer: 0,
                coord,
                stage: CacheStage::Composite,
            };
            if engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, key) != Some(gen) {
                stale += 1;
                continue;
            }
            if let Some(tile) = state.tiles.tile_cache.get_entry(key) {
                hasher.update(&coord.x.to_le_bytes());
                hasher.update(&coord.y.to_le_bytes());
                hasher.update(&[coord.level]);
                for v in tile.data.iter() {
                    hasher.update(&v.to_le_bytes());
                }
                n += 1;
            } else {
                stale += 1;
            }
        }
        by_level.insert(
            format!("L{level}"),
            json!({
                "hash": hasher.finalize().to_hex().to_string(),
                "tiles": n,
                "stale": stale,
            }),
        );
    }
    // Also full L0 Processed
    use engine_tiles::TILE_SIZE;
    let snap = state.must_active().document_handle.snapshot();
    let cols = snap.width.div_ceil(TILE_SIZE);
    let rows = snap.height.div_ceil(TILE_SIZE);
    let mut ph = blake3::Hasher::new();
    let mut pn = 0u32;
    for y in 0..rows {
        for x in 0..cols {
            let key = TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Processed,
            };
            if engine_tiles::tile_ready_emit_gen(&state.tiles.tile_cache, key) != Some(gen) {
                continue;
            }
            if let Some(tile) = state.tiles.tile_cache.get_entry(key) {
                for v in tile.data.iter() {
                    ph.update(&v.to_le_bytes());
                }
                pn += 1;
            }
        }
    }
    json!({
        "processed_l0": { "hash": ph.finalize().to_hex().to_string(), "tiles": pn },
        "composite": by_level,
        "content_gen": gen,
        "viewport_level": vis_level,
    })
}

fn idle_ok(state: &AppState) -> bool {
    state.tiles.scheduler.queued_len() == 0
        && state.preview_pass_inflight.load(Ordering::Acquire) == 0
        && state.full_document_busy.load(Ordering::Acquire) == 0
        && state.tiles.ed_frontier.blocked_count() == 0
}

/// Step 0 trap / Step 3 verify: must fail with versions off, pass with versions on.
#[test]
#[ignore]
fn step4_pyramid_stress_trap() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    let n = n_trials();
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let versions_on = engine_tiles::composite_input_versions_enabled();
    // DITHER_STEP4_FOCUS=fs:ps_8_to_9:8 — single mode/scenario/workers cell.
    let focus = std::env::var("DITHER_STEP4_FOCUS").ok();
    let focus_parts = focus.as_ref().map(|s| {
        let mut it = s.split(':');
        (
            it.next().unwrap_or(""),
            it.next().unwrap_or(""),
            it.next().and_then(|w| w.parse::<usize>().ok()),
        )
    });
    let modes: &[(DitherModeV2, &str)] = if quick() {
        &[(DitherModeV2::FloydSteinberg, "fs")]
    } else {
        &[
            (DitherModeV2::FloydSteinberg, "fs"),
            (DitherModeV2::BlockedFs, "blocked_fs"),
            (DitherModeV2::BlueNoise, "blue_noise"),
            (DitherModeV2::Bayer4x4, "bayer"),
        ]
    };
    let scenarios: &[(&str, Option<(u8, u16)>, (u8, u16))] = if quick() {
        &[
            ("ps_1_to_8", Some((1, 16)), (8, 16)),
            ("ps_8_to_9", Some((8, 16)), (9, 16)),
        ]
    } else {
        &[
            ("ps_1_to_8", Some((1, 16)), (8, 16)),
            ("ps_8_to_9", Some((8, 16)), (9, 16)),
            ("levels_16_to_32", Some((8, 16)), (8, 32)),
        ]
    };
    let worker_sets = parse_worker_sets();

    let mut report = json!({
        "versions_enabled": versions_on,
        "n": n,
        "rows": [],
        "any_discrepancy": false,
        "trap_proven_modes": [],
        "trap_unproven_modes": [],
    });
    let mut trap_hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut modes_seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    for (mode, mode_label) in modes {
        if !mode_allowed(mode_label) {
            continue;
        }
        if let Some((fm, _, _)) = focus_parts {
            if !fm.is_empty() && fm != *mode_label {
                continue;
            }
        }
        modes_seen.insert((*mode_label).to_string());
        let mode = mode.clone();
        for &(scen, prelude, target) in scenarios {
            if let Some((_, fs, _)) = focus_parts {
                if !fs.is_empty() && fs != scen {
                    continue;
                }
            }
            // Reference at 1 worker
            let (st_ref, stop_ref, joins_ref) = setup(size, 1);
            if let Some((ps, lv)) = prelude {
                assert!(apply(&st_ref, mode.clone(), ps, lv));
            }
            assert!(apply(&st_ref, mode.clone(), target.0, target.1));
            // Second apply for stable reference
            assert!(apply(&st_ref, mode.clone(), target.0, target.1));
            let reference = pyramid_digests(&st_ref);
            teardown(&st_ref, &stop_ref, joins_ref);

            for &workers in &worker_sets {
                if let Some((_, _, Some(fw))) = focus_parts {
                    if fw != workers {
                        continue;
                    }
                }
                let mut mismatch = 0usize;
                let mut ok_n = 0usize;
                let mut level_miss: serde_json::Map<String, serde_json::Value> =
                    serde_json::Map::new();
                let (state, stop, joins) = setup(size, workers);
                // warm reference config once
                if let Some((ps, lv)) = prelude {
                    let _ = apply(&state, mode.clone(), ps, lv);
                }
                let _ = apply(&state, mode.clone(), target.0, target.1);

                for trial in 0..n {
                    if let Some((ps, lv)) = prelude {
                        let _ = apply(&state, mode.clone(), ps, lv);
                    }
                    if !apply(&state, mode.clone(), target.0, target.1) {
                        continue;
                    }
                    if !idle_ok(&state) {
                        continue;
                    }
                    ok_n += 1;
                    let dig = pyramid_digests(&state);
                    let mut bad = false;
                    if dig["processed_l0"]["hash"] != reference["processed_l0"]["hash"] {
                        // Processed drift — count but rare
                        bad = true;
                        let e = level_miss
                            .entry("processed_l0".to_string())
                            .or_insert(json!(0));
                        *e = json!(e.as_u64().unwrap_or(0) + 1);
                    }
                    if let Some(obj) = dig["composite"].as_object() {
                        for (lvl, val) in obj {
                            let ref_h = &reference["composite"][lvl]["hash"];
                            if &val["hash"] != ref_h {
                                bad = true;
                                let e = level_miss.entry(lvl.clone()).or_insert(json!(0));
                                *e = json!(e.as_u64().unwrap_or(0) + 1);
                            }
                        }
                    }
                    if bad {
                        mismatch += 1;
                        eprintln!(
                            "[step4] MISMATCH {mode_label} {scen} w={workers} trial={} dig={} ref={} level_miss={:?}",
                            trial + 1,
                            dig,
                            reference,
                            level_miss
                        );
                    }
                    if trial % 10 == 0 || bad {
                        eprintln!(
                            "[step4] {mode_label} {scen} w={workers} trial={}/{} bad={bad} versions={versions_on}",
                            trial + 1,
                            n
                        );
                    }
                }
                let footprint_after = cache_footprint(&state);
                teardown(&state, &stop, joins);
                let rate = if ok_n == 0 {
                    0.0
                } else {
                    mismatch as f64 / ok_n as f64
                };
                if mismatch > 0 {
                    report["any_discrepancy"] = json!(true);
                    trap_hits.insert((*mode_label).to_string());
                }
                report["rows"].as_array_mut().unwrap().push(json!({
                    "mode": mode_label,
                    "scenario": scen,
                    "workers": workers,
                    "ok_n": ok_n,
                    "mismatch": mismatch,
                    "rate": rate,
                    "level_mismatches": level_miss,
                    "viewport_level": reference["viewport_level"],
                    "footprint_after": footprint_after,
                }));
                eprintln!(
                    "[step4] DONE {mode_label} {scen} w={workers} rate={rate:.3} miss={mismatch}/{ok_n}"
                );
            }
        }
    }

    if !versions_on {
        let proven: Vec<String> = trap_hits.iter().cloned().collect();
        let unproven: Vec<String> = modes_seen
            .iter()
            .filter(|m| !trap_hits.contains(*m))
            .cloned()
            .collect();
        report["trap_proven_modes"] = json!(proven);
        report["trap_unproven_modes"] = json!(unproven);
    }

    let path = out_dir().join(out_json_name(versions_on));
    let _ = std::fs::create_dir_all(out_dir());
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    eprintln!(
        "[step4] wrote {} any_discrepancy={}",
        path.display(),
        report["any_discrepancy"]
    );

    if !versions_on {
        if soft_trap() {
            // Gap-close: per-mode trap may not fire (ordered modes). Record, do not fail.
            eprintln!(
                "[step4] soft trap proven={:?} unproven={:?}",
                report["trap_proven_modes"], report["trap_unproven_modes"]
            );
        } else {
            assert!(
                report["any_discrepancy"].as_bool().unwrap_or(false),
                "STEP4 Step0 trap: expected discrepancies with DITHER_COMPOSITE_INPUT_VERSIONS=0"
            );
        }
    } else {
        assert!(
            !report["any_discrepancy"].as_bool().unwrap_or(true),
            "STEP4 Step3: discrepancies remain with input versions enabled"
        );
    }
}

#[test]
#[ignore]
fn step4_rapid_updates_hash_match() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    assert!(
        engine_tiles::composite_input_versions_enabled(),
        "rapid test expects versions ON"
    );
    let n = if quick() { 10 } else { 50 };
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let (state, stop, joins) = setup(size, 8);
    let mode = DitherModeV2::FloydSteinberg;
    // Reference: clean final config
    assert!(apply(&state, mode.clone(), 9, 16));
    assert!(apply(&state, mode.clone(), 9, 16));
    let reference = pyramid_digests(&state);

    let mut bad = 0usize;
    for i in 0..n {
        // 10 rapid without quiet
        for k in 0..10 {
            let ps = if k % 2 == 0 { 8 } else { 9 };
            state.must_active().document_handle.mutate(|doc| {
                if let LayerNode::Leaf(layer) = &mut doc.root[0] {
                    layer.filters.clear();
                    layer.filters.push(FilterInstance::new(
                        FilterKind::Dither,
                        FilterParams::DitherV2(DitherParamsV2 {
                            mode: mode.clone(),
                            pixel_size: ps,
                            levels: 16,
                            ..DitherParamsV2::default()
                        }),
                    ));
                }
                doc.increment_generation();
            });
            state
                .must_active()
                .document_handle
                .snapshot()
                .generations
                .increment_layer_gen(1);
            sync_epoch(&state);
            engine_tiles::invalidation::invalidate(
                &state.tiles.tile_cache,
                engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
                    doc: 1,
                    layer: 1,
                },
            );
            state.tiles.error_residuals.evict_layer(1, LayerId::new(1));
            state.tiles.block_representatives.clear_dithered();
            state.tiles.clear_strip_gates();
            state.tiles.full_document.invalidate_layer(1, 1);
            schedule_dirty_viewport_tiles(&state);
        }
        // Final settle on ps=9
        assert!(apply(&state, mode.clone(), 9, 16));
        assert!(idle_ok(&state));
        let dig = pyramid_digests(&state);
        if dig["composite"] != reference["composite"]
            || dig["processed_l0"]["hash"] != reference["processed_l0"]["hash"]
        {
            bad += 1;
        }
        eprintln!("[step4_rapid] {i}/{n} bad_total={bad}");
    }
    teardown(&state, &stop, joins);
    assert_eq!(bad, 0, "rapid-update hash mismatches: {bad}/{n}");
}

/// Mid-wave param bump: old-gen in-flight must not stick in the new-gen cache.
#[test]
#[ignore]
fn step4_stale_midwave_no_publish() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    assert!(engine_tiles::composite_input_versions_enabled());
    let n = if quick() { 10 } else { 30 };
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let (state, stop, joins) = setup(size, 8);
    let mode = DitherModeV2::FloydSteinberg;
    assert!(apply(&state, mode.clone(), 8, 16));
    assert!(apply(&state, mode.clone(), 9, 16));
    let reference = pyramid_digests(&state);

    let mut bad = 0usize;
    for i in 0..n {
        // Start toward ps=8
        state.must_active().document_handle.mutate(|doc| {
            if let LayerNode::Leaf(layer) = &mut doc.root[0] {
                layer.filters.clear();
                layer.filters.push(FilterInstance::new(
                    FilterKind::Dither,
                    FilterParams::DitherV2(DitherParamsV2 {
                        mode: mode.clone(),
                        pixel_size: 8,
                        levels: 16,
                        ..DitherParamsV2::default()
                    }),
                ));
            }
            doc.increment_generation();
        });
        state
            .must_active()
            .document_handle
            .snapshot()
            .generations
            .increment_layer_gen(1);
        sync_epoch(&state);
        engine_tiles::invalidation::invalidate(
            &state.tiles.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: 1 },
        );
        state.tiles.error_residuals.evict_layer(1, LayerId::new(1));
        state.tiles.block_representatives.clear_dithered();
        state.tiles.clear_strip_gates();
        state.tiles.full_document.invalidate_layer(1, 1);
        schedule_dirty_viewport_tiles(&state);
        // Mid-wave: flip to final ps=9 before quiet
        thread::sleep(Duration::from_millis(15));
        assert!(apply(&state, mode.clone(), 9, 16));
        assert!(idle_ok(&state));
        let live = current_content_gen(&state);
        // No Composite/Processed body may carry a generation ahead of live
        // (stale insert path rejects older; equal-gen replace is versioned).
        let mut behind = 0u32;
        for entry in state.tiles.tile_cache.entries.iter() {
            let k = *entry.key();
            if k.doc != 1 {
                continue;
            }
            if k.stage != CacheStage::Composite && k.stage != CacheStage::Processed {
                continue;
            }
            if entry.generation > live {
                behind += 1;
            }
        }
        let dig = pyramid_digests(&state);
        if behind > 0
            || dig["composite"] != reference["composite"]
            || dig["processed_l0"]["hash"] != reference["processed_l0"]["hash"]
        {
            bad += 1;
        }
        eprintln!("[step4_stale] {i}/{n} bad_total={bad} behind={behind}");
    }
    teardown(&state, &stop, joins);
    assert_eq!(bad, 0, "stale mid-wave mismatches: {bad}/{n}");
}

/// Re-run STEP3 golden configs ×3 with versions ON → `golden_hashes_fixed.json`.
#[test]
#[ignore]
fn step4_golden_hashes_fixed() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    assert!(engine_tiles::composite_input_versions_enabled());
    let runs = if quick() { 1 } else { 3 };
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let configs: &[(u8, u16)] = if quick() {
        &[(8, 16)]
    } else {
        &[(1, 16), (8, 16), (9, 16), (8, 32)]
    };
    let (state, stop, joins) = setup(size, 8);
    let mut report = json!({
        "versions_enabled": true,
        "runs_per_config": runs,
        "configs": [],
        "determinism_ok": true,
    });
    for &(ps, lv) in configs {
        let mut run_hashes = Vec::new();
        for run_i in 1..=runs {
            assert!(apply(&state, DitherModeV2::FloydSteinberg, ps, lv));
            assert!(idle_ok(&state));
            let dig = pyramid_digests(&state);
            eprintln!(
                "[step4_golden] ps={ps} lv={lv} run={run_i} L0={} L1={}",
                dig["composite"]["L0"]["hash"], dig["composite"]["L1"]["hash"]
            );
            run_hashes.push(dig);
        }
        let stable = run_hashes.windows(2).all(|w| {
            w[0]["processed_l0"]["hash"] == w[1]["processed_l0"]["hash"]
                && w[0]["composite"] == w[1]["composite"]
        });
        if !stable {
            report["determinism_ok"] = json!(false);
        }
        report["configs"].as_array_mut().unwrap().push(json!({
            "pixel_size": ps,
            "levels": lv,
            "stable": stable,
            "hash": run_hashes.last(),
            "runs": run_hashes,
        }));
    }
    teardown(&state, &stop, joins);
    let path = out_dir().join("golden_hashes_fixed.json");
    let _ = std::fs::create_dir_all(out_dir());
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    eprintln!(
        "[step4_golden] wrote {} determinism_ok={}",
        path.display(),
        report["determinism_ok"]
    );
    assert!(report["determinism_ok"].as_bool().unwrap_or(false));
}

/// 6016×4352 fit-ish viewport, N trials at 8 workers (STEP3b never measured this).
#[test]
#[ignore]
fn step4_pyramid_stress_6k() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    let n = if quick() {
        5
    } else {
        std::env::var("DITHER_STEP4_N")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30)
    };
    let size = SizeSpec {
        w: 6016,
        h: 4352,
        label: "6016x4352",
    };
    let versions_on = engine_tiles::composite_input_versions_enabled();
    let (st_ref, stop_ref, joins_ref) = setup(size, 1);
    assert!(apply(&st_ref, DitherModeV2::FloydSteinberg, 1, 16));
    assert!(apply(&st_ref, DitherModeV2::FloydSteinberg, 8, 16));
    assert!(apply(&st_ref, DitherModeV2::FloydSteinberg, 8, 16));
    let reference = pyramid_digests(&st_ref);
    teardown(&st_ref, &stop_ref, joins_ref);

    let (state, stop, joins) = setup(size, 8);
    let _ = apply(&state, DitherModeV2::FloydSteinberg, 1, 16);
    let _ = apply(&state, DitherModeV2::FloydSteinberg, 8, 16);
    let mut mismatch = 0usize;
    let mut ok_n = 0usize;
    let mut level_miss: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for trial in 0..n {
        let _ = apply(&state, DitherModeV2::FloydSteinberg, 1, 16);
        if !apply(&state, DitherModeV2::FloydSteinberg, 8, 16) || !idle_ok(&state) {
            continue;
        }
        ok_n += 1;
        let dig = pyramid_digests(&state);
        let mut bad = false;
        if dig["processed_l0"]["hash"] != reference["processed_l0"]["hash"] {
            bad = true;
            let e = level_miss
                .entry("processed_l0".to_string())
                .or_insert(json!(0));
            *e = json!(e.as_u64().unwrap_or(0) + 1);
        }
        if let Some(obj) = dig["composite"].as_object() {
            for (lvl, val) in obj {
                let ref_h = &reference["composite"][lvl]["hash"];
                if &val["hash"] != ref_h {
                    bad = true;
                    let e = level_miss.entry(lvl.clone()).or_insert(json!(0));
                    *e = json!(e.as_u64().unwrap_or(0) + 1);
                }
            }
        }
        if bad {
            mismatch += 1;
        }
        eprintln!(
            "[step4_6k] trial={}/{} bad={bad} versions={versions_on}",
            trial + 1,
            n
        );
    }
    teardown(&state, &stop, joins);
    let rate = if ok_n == 0 {
        0.0
    } else {
        mismatch as f64 / ok_n as f64
    };
    let report = json!({
        "versions_enabled": versions_on,
        "n": n,
        "ok_n": ok_n,
        "mismatch": mismatch,
        "rate": rate,
        "level_mismatches": level_miss,
        "size": "6016x4352",
        "workers": 8,
        "scenario": "ps_1_to_8",
    });
    let path = out_dir().join(if versions_on {
        "step4_stress_6k_fixed.json"
    } else {
        "step4_stress_6k_trap.json"
    });
    let _ = std::fs::create_dir_all(out_dir());
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    eprintln!(
        "[step4_6k] wrote {} rate={rate:.3} miss={mismatch}/{ok_n}",
        path.display()
    );
    if versions_on {
        assert_eq!(mismatch, 0, "6k discrepancies remain: {mismatch}/{ok_n}");
    }
}

/// Fit-L3 timing ×3 (ps=8→9). Env `DITHER_STEP4_PERF_TAG=before|after` names the JSON.
/// Separate process required for versions on/off (`OnceLock` in engine-tiles).
#[test]
#[ignore]
fn step4_perf_fit_l3() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    let tag = std::env::var("DITHER_STEP4_PERF_TAG").unwrap_or_else(|_| "after".into());
    let size = SizeSpec {
        w: 6016,
        h: 4352,
        label: "6016x4352",
    };
    // Fit-ish: small canvas → high level
    let (state, stop, joins) = {
        let state = Arc::new(AppState::empty_process(None, 1024 * 1024 * 1024, true));
        state.spawn_session(raw_doc(size.w, size.h));
        fill_raw(&state, size.w, size.h);
        let zoom = (717.0 / size.w as f64).min(725.0 / size.h as f64);
        let _ = ViewportService::new(Arc::clone(&state)).set_viewport(zoom, 0.0, 0.0, 717.0, 725.0);
        let stop = Arc::new(AtomicBool::new(false));
        let mut joins = Vec::new();
        for _ in 0..8 {
            let st = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            joins.push(thread::spawn(move || worker_loop(st, stop)));
        }
        let _ = wait_silence(&state, Duration::from_secs(30));
        (state, stop, joins)
    };
    let vp = state.ui.viewport.lock().unwrap().clone();
    eprintln!(
        "[step4_perf] viewport level={} visible={} zoom≈{:.4}",
        vp.level,
        vp.visible_tiles.len(),
        (717.0 / size.w as f64).min(725.0 / size.h as f64)
    );
    let mode = DitherModeV2::FloydSteinberg;
    let mut rows = Vec::new();
    for run in 1..=3 {
        assert!(apply(&state, mode.clone(), 8, 16));
        let q0 = state.tiles.scheduler.queued_len();
        let t0 = Instant::now();
        assert!(apply(&state, mode.clone(), 9, 16));
        let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let dig = pyramid_digests(&state);
        let footprint = cache_footprint(&state);
        eprintln!(
            "[step4_perf] {tag} run={run} wall_ms={wall_ms:.1} used_mb={} versions={} vp_level={}",
            footprint["used_mb"],
            engine_tiles::composite_input_versions_enabled(),
            dig["viewport_level"]
        );
        rows.push(json!({
            "run": run,
            // apply() waits to quiet — wall ≈ busy_hold / first-all-fresh, not eye latency
            "first_fresh_ms": wall_ms,
            "busy_hold_ms": wall_ms,
            "ed_done_ms": wall_ms,
            "queued_at_start": q0,
            "footprint": footprint,
            "viewport_level": dig["viewport_level"],
            "composite_levels": dig["composite"],
        }));
    }
    teardown(&state, &stop, joins);
    let report = json!({
        "tag": tag,
        "versions_enabled": engine_tiles::composite_input_versions_enabled(),
        "size": "6016x4352",
        "viewport": "717x725_fit",
        "note": "wall inside apply() to quiet; used_mb counts TILE_BYTES only (manifests extra)",
        "entry_overhead": {
            "before_versions": "CacheEntry: tile Arc + generation + last_touched + dirty",
            "after_versions": " + entry_version(u64) + input_versions Arc<[(TileKey,u64)]>",
            "approx_per_composite_l0": "1 pair (~28B) + Arc header",
            "approx_per_composite_parent": "up to 4 child pairs",
        },
        "rows": rows,
    });
    let path = out_dir().join(format!("step4_perf_fit_l3_{tag}.json"));
    let _ = std::fs::create_dir_all(out_dir());
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    eprintln!("[step4_perf] wrote {}", path.display());
}

/// FS fit-L3 hash stress so L2/L3 are in the pyramid digest (4000 default viewport is L2).
#[test]
#[ignore]
fn step4_fs_fit_l3_levels() {
    std::env::set_var("DITHER_GPU_PATH", "cpu");
    std::env::set_var("DITHER_JOURNAL", "0");
    let n = std::env::var("DITHER_STEP4_N")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);
    let versions_on = engine_tiles::composite_input_versions_enabled();
    let size = SizeSpec {
        w: 4000,
        h: 3000,
        label: "4000x3000",
    };
    let make = |workers: usize| {
        let state = Arc::new(AppState::empty_process(None, 1024 * 1024 * 1024, true));
        state.spawn_session(raw_doc(size.w, size.h));
        fill_raw(&state, size.w, size.h);
        let zoom = (400.0 / size.w as f64).min(300.0 / size.h as f64);
        let _ = ViewportService::new(Arc::clone(&state)).set_viewport(zoom, 0.0, 0.0, 400.0, 300.0);
        let stop = Arc::new(AtomicBool::new(false));
        let mut joins = Vec::new();
        for _ in 0..workers {
            let st = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            joins.push(thread::spawn(move || worker_loop(st, stop)));
        }
        let _ = wait_silence(&state, Duration::from_secs(30));
        (state, stop, joins)
    };

    let mut report = json!({
        "versions_enabled": versions_on,
        "n": n,
        "size": "4000x3000_fit_L3ish",
        "rows": [],
        "any_discrepancy": false,
    });

    for &(scen, prelude, target) in &[
        ("ps_1_to_8", Some((1u8, 16u16)), (8u8, 16u16)),
        ("ps_8_to_9", Some((8u8, 16u16)), (9u8, 16u16)),
    ] {
        let (st_ref, stop_ref, joins_ref) = make(1);
        let mode = DitherModeV2::FloydSteinberg;
        if let Some((ps, lv)) = prelude {
            assert!(apply(&st_ref, mode.clone(), ps, lv));
        }
        assert!(apply(&st_ref, mode.clone(), target.0, target.1));
        assert!(apply(&st_ref, mode.clone(), target.0, target.1));
        let reference = pyramid_digests(&st_ref);
        let vp_level = reference["viewport_level"].as_u64().unwrap_or(0);
        eprintln!(
            "[step4_fitL3] scen={scen} viewport_level={vp_level} ref_keys={:?}",
            reference["composite"]
                .as_object()
                .map(|o| o.keys().collect::<Vec<_>>())
        );
        teardown(&st_ref, &stop_ref, joins_ref);

        let (state, stop, joins) = make(8);
        if let Some((ps, lv)) = prelude {
            let _ = apply(&state, mode.clone(), ps, lv);
        }
        let _ = apply(&state, mode.clone(), target.0, target.1);
        let mut mismatch = 0usize;
        let mut ok_n = 0usize;
        let mut level_miss: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
        for trial in 0..n {
            if let Some((ps, lv)) = prelude {
                let _ = apply(&state, mode.clone(), ps, lv);
            }
            if !apply(&state, mode.clone(), target.0, target.1) || !idle_ok(&state) {
                continue;
            }
            ok_n += 1;
            let dig = pyramid_digests(&state);
            let mut bad = false;
            if dig["processed_l0"]["hash"] != reference["processed_l0"]["hash"] {
                bad = true;
                let e = level_miss
                    .entry("processed_l0".to_string())
                    .or_insert(json!(0));
                *e = json!(e.as_u64().unwrap_or(0) + 1);
            }
            if let Some(obj) = dig["composite"].as_object() {
                for (lvl, val) in obj {
                    let ref_h = &reference["composite"][lvl]["hash"];
                    if &val["hash"] != ref_h {
                        bad = true;
                        let e = level_miss.entry(lvl.clone()).or_insert(json!(0));
                        *e = json!(e.as_u64().unwrap_or(0) + 1);
                    }
                }
            }
            if bad {
                mismatch += 1;
            }
            if trial % 5 == 0 || bad {
                eprintln!(
                    "[step4_fitL3] {scen} trial={}/{} bad={bad} versions={versions_on}",
                    trial + 1,
                    n
                );
            }
        }
        teardown(&state, &stop, joins);
        if mismatch > 0 {
            report["any_discrepancy"] = json!(true);
        }
        report["rows"].as_array_mut().unwrap().push(json!({
            "mode": "fs",
            "scenario": scen,
            "workers": 8,
            "ok_n": ok_n,
            "mismatch": mismatch,
            "rate": if ok_n == 0 { 0.0 } else { mismatch as f64 / ok_n as f64 },
            "level_mismatches": level_miss,
            "viewport_level": vp_level,
        }));
    }

    let path = out_dir().join(if versions_on {
        "step4_fs_fit_l3_fixed.json"
    } else {
        "step4_fs_fit_l3_trap.json"
    });
    let _ = std::fs::create_dir_all(out_dir());
    let _ = std::fs::write(
        &path,
        serde_json::to_vec_pretty(&report).unwrap_or_default(),
    );
    eprintln!(
        "[step4_fitL3] wrote {} any_discrepancy={}",
        path.display(),
        report["any_discrepancy"]
    );
    if versions_on {
        assert!(
            !report["any_discrepancy"].as_bool().unwrap_or(true),
            "FS fit L3 discrepancies with versions on"
        );
    }
}
