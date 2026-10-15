//! Step 0 counters for one zoomed-out `blocked_fs` rebuild.
//!
//! Not part of `cargo test` by default. The slot state machine is untouched.
//! Each run bumps `document_gen`, drops the full-document slot, marks cached
//! tiles dirty, then calls `set_viewport` at fit zoom on a 4096² document.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use engine_project::filter::{DitherModeV2, DitherParamsV2};
use engine_project::layer::{Layer, LayerNode};
use engine_project::types::{DocumentId, LayerId, LayerKind};
use engine_project::{Document, FilterInstance, FilterKind, FilterParams};
use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};

use crate::commands::AppState;
use crate::services::viewport_service::ViewportService;
use crate::worker::task_is_stale;

const SIDE: u32 = 4096;
const VIEW_W: f64 = 1920.0;
const VIEW_H: f64 = 1080.0;
const RUNS: usize = 10;

#[test]
#[ignore]
fn step0_blocked_fs_fit_ten_runs() {
    let _gate = crate::preview_diag::test_gate();
    crate::preview_diag::clear_test_state();

    let gpu = engine_gpu::request_blocked_fs_device();
    let gpu_label = gpu
        .as_ref()
        .map(|(_, _, label)| label.clone())
        .unwrap_or_else(|| "none".to_string());
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let budget = crate::memory_budget::resolve_ram_budget();
    eprintln!(
        "step0 setup gpu={gpu_label} cores={cores} cache_budget_mb={}",
        budget.bytes / (1024 * 1024)
    );

    let _ = engine_gpu::warm_blocked_fs_pipelines();
    let state = Arc::new(AppState::empty_process(None, budget.bytes, true));
    // Config load resets the diagnostic atomic. Turn it back on after that.
    crate::preview_diag::sync_from_config(true, 1);
    eprintln!("step0 diag_level={}", crate::preview_diag::level());
    let doc = blocked_fs_doc();
    state.spawn_session(doc);
    fill_raw_tiles(&state);

    let stop = Arc::new(AtomicBool::new(false));
    let mut joins = Vec::new();
    for _ in 0..cores {
        let state = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        joins.push(thread::spawn(move || worker_loop(state, stop)));
    }

    let zoom = (VIEW_W / SIDE as f64).min(VIEW_H / SIDE as f64);
    let mut walls = Vec::with_capacity(RUNS);
    let mut apply_rows: Vec<ApplyStages> = Vec::with_capacity(RUNS);
    let mut fd_rows: Vec<FdStages> = Vec::with_capacity(RUNS);
    let mut render_ms_rows: Vec<f64> = Vec::with_capacity(RUNS);
    let mut outside_rows: Vec<OutsideStages> = Vec::with_capacity(RUNS);

    for run in 0..RUNS {
        let gen = bump_and_invalidate(&state);
        let t0 = Instant::now();
        let vp = ViewportService::new(Arc::clone(&state))
            .set_viewport(zoom, 0.0, 0.0, VIEW_W, VIEW_H)
            .expect("set_viewport");
        eprintln!(
            "step0 enqueued run={run} gen={gen} level={} tiles={} pending={}",
            vp.level,
            vp.tile_count,
            state.tiles.scheduler.pending_count()
        );
        let line = wait_s0(&state, gen, Duration::from_secs(180));
        let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
        walls.push(wall_ms);
        outside_rows.push(OutsideStages::from_line(&line, wall_ms));

        let gpu = engine_project::algorithms::gpu_dither::last_gpu_dither_status();
        let fd = engine_project::filters::last_fd_render_stages();
        if let Some(s) = gpu.as_ref() {
            let row = ApplyStages::from_status(s);
            eprintln!(
                "step0 apply run={run} path={} backend={} pipeline={} gpu_total={:.1} stages_sum={:.1} delta={:.1}",
                s.path,
                s.backend,
                s.pipeline,
                row.gpu_total,
                row.stages_sum(),
                row.gpu_total - row.stages_sum()
            );
            apply_rows.push(row);
        } else {
            eprintln!("step0 apply run={run} MISSING");
        }
        if let Some(f) = fd {
            let row = FdStages::from_fd(f);
            eprintln!(
                "step0 fd run={run} render_total={:.1} stages_sum={:.1} delta={:.1} apply={:.1}",
                row.total,
                row.stages_sum(),
                row.total - row.stages_sum(),
                row.apply
            );
            fd_rows.push(row);
        } else {
            eprintln!("step0 fd run={run} MISSING");
        }
        if let Some(render) = parse_render_ms_sum(&line) {
            render_ms_rows.push(render);
        }
        eprintln!("step0 run {run} gen={gen} wall_ms={wall_ms:.1} {line}");
    }

    stop.store(true, Ordering::Release);
    for join in joins {
        let _ = join.join();
    }

    let mut sorted = walls.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    eprintln!(
        "step0 gesture_wall_ms p50={:.1} p95={:.1} min={:.1} max={:.1}",
        percentile(&sorted, 0.50),
        percentile(&sorted, 0.95),
        sorted[0],
        sorted[sorted.len() - 1]
    );

    eprintln!("=== box1 apply_gpu_dither_rgba stages (ms) ===");
    print_stage_table(&[
        (
            "in_f32_to_u8",
            apply_rows.iter().map(|r| r.in_f32_to_u8).collect(),
        ),
        (
            "setup_hash",
            apply_rows.iter().map(|r| r.setup_hash).collect(),
        ),
        (
            "lattice_ps",
            apply_rows.iter().map(|r| r.lattice_ps).collect(),
        ),
        (
            "pack_upload_bytes",
            apply_rows.iter().map(|r| r.pack_upload).collect(),
        ),
        ("prepare", apply_rows.iter().map(|r| r.prepare).collect()),
        ("upload", apply_rows.iter().map(|r| r.upload).collect()),
        ("dispatch", apply_rows.iter().map(|r| r.dispatch).collect()),
        ("readback", apply_rows.iter().map(|r| r.readback).collect()),
        ("post", apply_rows.iter().map(|r| r.post).collect()),
        (
            "expand_ps",
            apply_rows.iter().map(|r| r.expand_ps).collect(),
        ),
        (
            "out_bytes_to_rgba8",
            apply_rows.iter().map(|r| r.out_bytes).collect(),
        ),
        (
            "out_u8_to_f32",
            apply_rows.iter().map(|r| r.out_linear).collect(),
        ),
        ("apply_misc", apply_rows.iter().map(|r| r.misc).collect()),
        (
            "stages_sum",
            apply_rows.iter().map(|r| r.stages_sum()).collect(),
        ),
        (
            "gpu_total",
            apply_rows.iter().map(|r| r.gpu_total).collect(),
        ),
        (
            "|sum-total|",
            apply_rows
                .iter()
                .map(|r| (r.gpu_total - r.stages_sum()).abs())
                .collect(),
        ),
    ]);

    eprintln!("=== box2 outside apply inside render (ms) ===");
    print_stage_table(&[
        ("alloc_zero", fd_rows.iter().map(|r| r.alloc_zero).collect()),
        (
            "assemble_blit",
            fd_rows.iter().map(|r| r.assemble_blit).collect(),
        ),
        (
            "promote_f32",
            fd_rows.iter().map(|r| r.promote_f32).collect(),
        ),
        ("palette", fd_rows.iter().map(|r| r.palette).collect()),
        ("apply", fd_rows.iter().map(|r| r.apply).collect()),
        ("arc", fd_rows.iter().map(|r| r.arc).collect()),
        ("misc", fd_rows.iter().map(|r| r.misc).collect()),
        (
            "stages_sum",
            fd_rows.iter().map(|r| r.stages_sum()).collect(),
        ),
        ("fd_total", fd_rows.iter().map(|r| r.total).collect()),
        ("render_ms_sum", render_ms_rows.clone()),
        (
            "|sum-fd_total|",
            fd_rows
                .iter()
                .map(|r| (r.total - r.stages_sum()).abs())
                .collect(),
        ),
        (
            "|fd_total-render_ms|",
            fd_rows
                .iter()
                .zip(render_ms_rows.iter())
                .map(|(f, r)| (f.total - *r).abs())
                .collect(),
        ),
    ]);

    eprintln!("=== box3 gesture outside render (ms, worker-sum unless noted) ===");
    print_stage_table(&[
        ("wall", outside_rows.iter().map(|r| r.wall).collect()),
        (
            "render_ms_sum",
            outside_rows.iter().map(|r| r.render).collect(),
        ),
        (
            "wall-render",
            outside_rows.iter().map(|r| r.wall - r.render).collect(),
        ),
        (
            "schedule_ms_sum",
            outside_rows.iter().map(|r| r.schedule).collect(),
        ),
        ("wait_ms_sum", outside_rows.iter().map(|r| r.wait).collect()),
        (
            "publish_ms_sum",
            outside_rows.iter().map(|r| r.publish).collect(),
        ),
        (
            "composite_ms_sum",
            outside_rows.iter().map(|r| r.composite).collect(),
        ),
        ("emit_ms_sum", outside_rows.iter().map(|r| r.emit).collect()),
        ("tile_ms_sum", outside_rows.iter().map(|r| r.tile).collect()),
        (
            "accounted_sum",
            outside_rows
                .iter()
                .map(|r| r.schedule + r.wait + r.publish + r.composite + r.emit)
                .collect(),
        ),
    ]);
    eprintln!(
        "note: wait/publish/tile/composite/emit are worker-time sums (can exceed wall); \
schedule is wall-clock on the UI thread. Frontend tile:// encode is not in this path \
(tile-ready only notifies; encode is on fetch)."
    );

    if let Some(i) = walls
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
    {
        eprintln!("=== outlier run index={i} wall_ms={:.1} ===", walls[i]);
        if let Some(a) = apply_rows.get(i) {
            eprintln!(
                "apply: total={:.1} in_f32_to_u8={:.1} setup={:.1} lattice={:.1} pack={:.1} \
prepare={:.1} upload={:.1} dispatch={:.1} readback={:.1} post={:.1} expand={:.1} \
out_u8={:.1} out_lin={:.1} misc={:.1}",
                a.gpu_total,
                a.in_f32_to_u8,
                a.setup_hash,
                a.lattice_ps,
                a.pack_upload,
                a.prepare,
                a.upload,
                a.dispatch,
                a.readback,
                a.post,
                a.expand_ps,
                a.out_bytes,
                a.out_linear,
                a.misc
            );
        }
        if let Some(f) = fd_rows.get(i) {
            eprintln!(
                "fd: total={:.1} alloc={:.1} blit={:.1} promote={:.1} palette={:.1} \
apply={:.1} arc={:.1} misc={:.1}",
                f.total,
                f.alloc_zero,
                f.assemble_blit,
                f.promote_f32,
                f.palette,
                f.apply,
                f.arc,
                f.misc
            );
        }
    }

    crate::preview_diag::clear_test_state();
}

struct OutsideStages {
    wall: f64,
    render: f64,
    wait: f64,
    publish: f64,
    tile: f64,
    schedule: f64,
    composite: f64,
    emit: f64,
}

impl OutsideStages {
    fn from_line(line: &str, wall: f64) -> Self {
        Self {
            wall,
            render: parse_f64_field(line, "render_ms_sum").unwrap_or(0.0),
            wait: parse_f64_field(line, "wait_ms_sum").unwrap_or(0.0),
            publish: parse_f64_field(line, "publish_ms_sum").unwrap_or(0.0),
            tile: parse_f64_field(line, "tile_ms_sum").unwrap_or(0.0),
            schedule: parse_f64_field(line, "schedule_ms_sum").unwrap_or(0.0),
            composite: parse_f64_field(line, "composite_ms_sum").unwrap_or(0.0),
            emit: parse_f64_field(line, "emit_ms_sum").unwrap_or(0.0),
        }
    }
}

/// Historical Mass/A/B harness — superseded; Mass/Slice env removed from release.
/// Kept ignored for log archaeology; see `.local-doc/publish_ab_experiment.md`.
#[test]
#[ignore = "Mass/Slice branches removed; see publish_ab_experiment.md"]
fn publish_ab_ten_runs_each() {
    let _gate = crate::preview_diag::test_gate();
    crate::preview_diag::clear_test_state();

    let _ = engine_gpu::warm_blocked_fs_pipelines();
    let budget = crate::memory_budget::resolve_ram_budget();
    let state = Arc::new(AppState::empty_process(None, budget.bytes, true));
    crate::preview_diag::sync_from_config(true, 1);
    let doc = blocked_fs_doc();
    state.spawn_session(doc);
    fill_raw_tiles(&state);

    let stop = Arc::new(AtomicBool::new(false));
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let mut joins = Vec::new();
    for _ in 0..cores {
        let state = Arc::clone(&state);
        let stop = Arc::clone(&stop);
        joins.push(thread::spawn(move || worker_loop(state, stop)));
    }

    let zoom = (VIEW_W / SIDE as f64).min(VIEW_H / SIDE as f64);
    let modes = [("once", "A"), ("slice", "B")];
    let mut gold_hash: Option<String> = None;

    eprintln!("=== publish A vs B (10 runs each) ===");
    eprintln!(
        "{:<6} {:>10} {:>10} {:>10} {:>12} {:>10} {:>8}",
        "mode", "wall_p50", "wall_p95", "render_p50", "publish_runs", "comp_miss", "T-BYTES"
    );

    for &(env_val, label) in &modes {
        std::env::set_var("DITHER_FD_PUBLISH", env_val);
        crate::tile_pipeline::reset_fd_publish_claims();

        let mut walls = Vec::with_capacity(RUNS);
        let mut renders = Vec::with_capacity(RUNS);
        let mut pubs = Vec::with_capacity(RUNS);
        let mut misses = Vec::with_capacity(RUNS);
        let mut bytes_ok = true;

        for run in 0..RUNS {
            crate::preview_diag::clear_test_state();
            crate::preview_diag::sync_from_config(true, 1);
            let gen = bump_and_invalidate(&state);
            let t0 = Instant::now();
            let _ = ViewportService::new(Arc::clone(&state))
                .set_viewport(zoom, 0.0, 0.0, VIEW_W, VIEW_H)
                .expect("set_viewport");
            let line = wait_s0(&state, gen, Duration::from_secs(180));
            walls.push(t0.elapsed().as_secs_f64() * 1000.0);
            renders.push(parse_f64_field(&line, "render_ms_sum").unwrap_or(0.0));
            pubs.push(parse_f64_field(&line, "publish_runs").unwrap_or(0.0));
            misses.push(parse_f64_field(&line, "composer_tile_misses").unwrap_or(0.0));

            let hash = hash_processed_l0(&state);
            match &gold_hash {
                None => gold_hash = Some(hash),
                Some(g) if g != &hash => {
                    bytes_ok = false;
                    eprintln!("publish_{label} run={run} T-BYTES mismatch");
                }
                Some(_) => {}
            }
        }

        let mut sw = walls.clone();
        sw.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut sr = renders.clone();
        sr.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let pub_med = {
            let mut s = pubs.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            percentile(&s, 0.50)
        };
        let miss_med = {
            let mut s = misses.clone();
            s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            percentile(&s, 0.50)
        };
        eprintln!(
            "{:<6} {:>10.1} {:>10.1} {:>10.1} {:>12.1} {:>10.1} {:>8}",
            label,
            percentile(&sw, 0.50),
            percentile(&sw, 0.95),
            percentile(&sr, 0.50),
            pub_med,
            miss_med,
            if bytes_ok { "PASS" } else { "FAIL" }
        );
        // Per-run detail for the report.
        for i in 0..RUNS {
            eprintln!(
                "  {label} run={i} wall={:.1} render={:.1} publish_runs={:.0} misses={:.0}",
                walls[i], renders[i], pubs[i], misses[i]
            );
        }
    }

    stop.store(true, Ordering::Release);
    for join in joins {
        let _ = join.join();
    }
    std::env::remove_var("DITHER_FD_PUBLISH");
    crate::tile_pipeline::reset_fd_publish_claims();
    crate::preview_diag::clear_test_state();
}

fn parse_f64_field(line: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{key}\":");
    let i = line.find(&needle)?;
    let rest = &line[i + needle.len()..];
    let end = rest
        .find(|c: char| c == ',' || c == '}')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn hash_processed_l0(state: &AppState) -> String {
    hash_processed_l0_side(state, SIDE)
}

fn hash_processed_l0_side(state: &AppState, side: u32) -> String {
    use sha2::{Digest, Sha256};
    let cols = side.div_ceil(engine_tiles::TILE_SIZE);
    let mut hasher = Sha256::new();
    for y in 0..cols {
        for x in 0..cols {
            let key = TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Processed,
            };
            if let Some(tile) = state.tiles.tile_cache.get_entry(key) {
                for v in tile.data.iter() {
                    hasher.update(v.to_le_bytes());
                }
            } else {
                hasher.update([0xff]);
            }
        }
    }
    format!("{:x}", hasher.finalize())
}

/// Historical Mass/A/B matrix — superseded after Once selection.
#[test]
#[ignore = "Mass/Slice branches removed; see publish_ab_experiment.md"]
fn publish_mab_thirty_scenarios() {
    let _gate = crate::preview_diag::test_gate();
    let _ = engine_gpu::warm_blocked_fs_pipelines();
    let modes = ["mass", "once", "slice"];
    fn mode_label(m: &str) -> &'static str {
        match m {
            "mass" => "Mass",
            "once" => "A",
            "slice" => "B",
            _ => "?",
        }
    }

    eprintln!("=== Mass/A/B 30-run alternating × scenarios ===");
    eprintln!(
        "{:<14} {:<6} {:>9} {:>9} {:>10} {:>8} {:>8} {:>8} {:>7}",
        "scenario",
        "mode",
        "wall_p50",
        "wall_p95",
        "render_p50",
        "pub_med",
        "miss_med",
        "fall_med",
        "T-BYTES"
    );

    let scenarios: &[(&str, u32, usize, ScenarioKind)] = &[
        ("fit_l1", 4096, 256 * 1024 * 1024, ScenarioKind::Fit),
        ("zoom_100", 4096, 256 * 1024 * 1024, ScenarioKind::Zoom100),
        ("pan", 4096, 256 * 1024 * 1024, ScenarioKind::Pan),
        ("low_budget", 4096, 48 * 1024 * 1024, ScenarioKind::Fit),
        ("side_8192", 8192, 512 * 1024 * 1024, ScenarioKind::Fit),
    ];

    let mut decision_rows: Vec<(String, f64, f64, f64, f64, f64, f64, bool)> = Vec::new();

    for &(name, side, budget, kind) in scenarios {
        let state = Arc::new(AppState::empty_process(None, budget, true));
        crate::preview_diag::sync_from_config(true, 1);
        state.spawn_session(blocked_fs_doc_side(side));
        fill_raw_tiles_side(&state, side);

        let stop = Arc::new(AtomicBool::new(false));
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let mut joins = Vec::new();
        for _ in 0..cores {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            joins.push(thread::spawn(move || worker_loop(state, stop)));
        }

        let fit_zoom = (VIEW_W / side as f64).min(VIEW_H / side as f64);
        let mut by_mode: [ModeAgg; 3] = std::array::from_fn(|_| ModeAgg::default());
        let mut gold: Option<String> = None;

        const N: usize = 30;
        for run in 0..N {
            let mode = modes[run % 3];
            let mi = run % 3;
            std::env::set_var("DITHER_FD_PUBLISH", mode);
            crate::tile_pipeline::reset_fd_publish_claims();
            crate::preview_diag::clear_test_state();
            crate::preview_diag::sync_from_config(true, 1);

            let gen = bump_and_invalidate(&state);
            let t0 = Instant::now();
            let vp = ViewportService::new(Arc::clone(&state));
            match kind {
                ScenarioKind::Fit => {
                    let _ = vp
                        .set_viewport(fit_zoom, 0.0, 0.0, VIEW_W, VIEW_H)
                        .expect("fit");
                }
                ScenarioKind::Zoom100 => {
                    let _ = vp
                        .set_viewport(fit_zoom, 0.0, 0.0, VIEW_W, VIEW_H)
                        .expect("fit");
                    let _ = wait_s0(&state, gen, Duration::from_secs(180));
                    // Second gesture at 100% — new gen for fair wall measure.
                    let gen2 = bump_and_invalidate(&state);
                    let t1 = Instant::now();
                    let _ = vp
                        .set_viewport(1.0, 0.0, 0.0, VIEW_W, VIEW_H)
                        .expect("100%");
                    let line = wait_s0(&state, gen2, Duration::from_secs(180));
                    let wall = t1.elapsed().as_secs_f64() * 1000.0;
                    record_mab_line(&mut by_mode[mi], &mut gold, &state, side, wall, &line);
                    continue;
                }
                ScenarioKind::Pan => {
                    let _ = vp
                        .set_viewport(1.0, 512.0, 384.0, VIEW_W, VIEW_H)
                        .expect("pan");
                }
            }
            let line = wait_s0(&state, gen, Duration::from_secs(240));
            let wall = t0.elapsed().as_secs_f64() * 1000.0;
            record_mab_line(&mut by_mode[mi], &mut gold, &state, side, wall, &line);
        }

        stop.store(true, Ordering::Release);
        for join in joins {
            let _ = join.join();
        }

        for (mi, mode) in modes.iter().enumerate() {
            let a = &by_mode[mi];
            let (wp50, wp95) = pct_pair(&a.walls);
            let (rp50, _) = pct_pair(&a.renders);
            let (pp50, _) = pct_pair(&a.pubs);
            let (mp50, _) = pct_pair(&a.misses);
            let (fp50, _) = pct_pair(&a.fallbacks);
            eprintln!(
                "{:<14} {:<6} {:>9.1} {:>9.1} {:>10.1} {:>8.1} {:>8.1} {:>8.1} {:>7}",
                name,
                mode_label(mode),
                wp50,
                wp95,
                rp50,
                pp50,
                mp50,
                fp50,
                if a.bytes_ok { "PASS" } else { "FAIL" }
            );
            decision_rows.push((
                format!("{name}/{}", mode_label(mode)),
                wp50,
                wp95,
                rp50,
                pp50,
                mp50,
                fp50,
                a.bytes_ok,
            ));
        }

        // Per-scenario B vs Mass rule.
        let mass = &by_mode[0];
        let b = &by_mode[2];
        let (m_p50, m_p95) = pct_pair(&mass.walls);
        let (b_p50, b_p95) = pct_pair(&b.walls);
        let (m_miss, _) = pct_pair(&mass.misses);
        let (b_miss, _) = pct_pair(&b.misses);
        let ok_p50 = b_p50 <= m_p50 + 0.05; // numerical slack
        let ok_p95 = b_p95 <= m_p95 * 1.05 + 0.05;
        let ok_miss = b_miss <= m_miss + 0.05;
        let ok_bytes = b.bytes_ok && mass.bytes_ok;
        eprintln!(
            "  rule[{name}]: B_p50={b_p50:.1} vs Mass={m_p50:.1} ({}) | B_p95={b_p95:.1} vs Mass*1.05={:.1} ({}) | miss B={b_miss:.1} Mass={m_miss:.1} ({}) | T-BYTES={}",
            if ok_p50 { "ok" } else { "FAIL" },
            m_p95 * 1.05,
            if ok_p95 { "ok" } else { "FAIL" },
            if ok_miss { "ok" } else { "FAIL" },
            if ok_bytes { "PASS" } else { "FAIL" }
        );
        eprintln!(
            "  SELECT_B[{name}]={}",
            ok_p50 && ok_p95 && ok_miss && ok_bytes
        );
    }

    std::env::remove_var("DITHER_FD_PUBLISH");
    crate::tile_pipeline::reset_fd_publish_claims();
    crate::preview_diag::clear_test_state();
}

#[derive(Clone, Copy)]
enum ScenarioKind {
    Fit,
    Zoom100,
    Pan,
}

struct ModeAgg {
    walls: Vec<f64>,
    renders: Vec<f64>,
    pubs: Vec<f64>,
    misses: Vec<f64>,
    fallbacks: Vec<f64>,
    bytes_ok: bool,
}

impl Default for ModeAgg {
    fn default() -> Self {
        Self {
            walls: Vec::new(),
            renders: Vec::new(),
            pubs: Vec::new(),
            misses: Vec::new(),
            fallbacks: Vec::new(),
            bytes_ok: true,
        }
    }
}

fn pct_pair(vals: &[f64]) -> (f64, f64) {
    if vals.is_empty() {
        return (0.0, 0.0);
    }
    let mut s = vals.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    (percentile(&s, 0.50), percentile(&s, 0.95))
}

fn record_mab_line(
    agg: &mut ModeAgg,
    gold: &mut Option<String>,
    state: &AppState,
    side: u32,
    wall: f64,
    line: &str,
) {
    agg.walls.push(wall);
    agg.renders
        .push(parse_f64_field(line, "render_ms_sum").unwrap_or(0.0));
    agg.pubs
        .push(parse_f64_field(line, "publish_runs").unwrap_or(0.0));
    agg.misses
        .push(parse_f64_field(line, "composer_tile_misses").unwrap_or(0.0));
    agg.fallbacks
        .push(parse_f64_field(line, "tile_fallback_inserts").unwrap_or(0.0));
    let hash = hash_processed_l0_side(state, side);
    match gold {
        None => *gold = Some(hash),
        Some(g) if g != &hash => agg.bytes_ok = false,
        Some(_) => {}
    }
}

#[derive(Clone, Copy)]
struct ApplyStages {
    in_f32_to_u8: f64,
    setup_hash: f64,
    lattice_ps: f64,
    pack_upload: f64,
    prepare: f64,
    upload: f64,
    dispatch: f64,
    readback: f64,
    post: f64,
    expand_ps: f64,
    out_bytes: f64,
    out_linear: f64,
    misc: f64,
    gpu_total: f64,
}

impl ApplyStages {
    fn from_status(s: &engine_project::algorithms::gpu_dither::GpuDitherStatus) -> Self {
        Self {
            in_f32_to_u8: s.in_f32_to_u8_ms,
            setup_hash: s.setup_hash_ms,
            lattice_ps: s.lattice_ps_ms,
            pack_upload: s.pack_upload_bytes_ms,
            prepare: s.prepare_ms,
            upload: s.upload_ms,
            dispatch: s.dispatch_ms,
            readback: s.readback_ms,
            post: s.post_ms,
            expand_ps: s.expand_ps_ms,
            out_bytes: s.out_bytes_to_rgba8_ms,
            out_linear: s.out_u8_to_f32_ms,
            misc: s.apply_misc_ms,
            gpu_total: f64::from(s.elapsed_ms),
        }
    }

    fn stages_sum(self) -> f64 {
        self.in_f32_to_u8
            + self.setup_hash
            + self.lattice_ps
            + self.pack_upload
            + self.prepare
            + self.upload
            + self.dispatch
            + self.readback
            + self.post
            + self.expand_ps
            + self.out_bytes
            + self.out_linear
            + self.misc
    }
}

#[derive(Clone, Copy)]
struct FdStages {
    alloc_zero: f64,
    assemble_blit: f64,
    promote_f32: f64,
    palette: f64,
    apply: f64,
    arc: f64,
    misc: f64,
    total: f64,
}

impl FdStages {
    fn from_fd(f: engine_project::filters::FdRenderStages) -> Self {
        Self {
            alloc_zero: f.alloc_zero_ms,
            assemble_blit: f.assemble_blit_ms,
            promote_f32: f.promote_f32_ms,
            palette: f.palette_ms,
            apply: f.apply_ms,
            arc: f.arc_ms,
            misc: f.misc_ms,
            total: f.total_ms,
        }
    }

    fn stages_sum(self) -> f64 {
        self.alloc_zero
            + self.assemble_blit
            + self.promote_f32
            + self.palette
            + self.apply
            + self.arc
            + self.misc
    }
}

fn print_stage_table(rows: &[(&str, Vec<f64>)]) {
    eprintln!("stage                         median     max");
    for (name, vals) in rows {
        if vals.is_empty() {
            continue;
        }
        let mut sorted = vals.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let med = percentile(&sorted, 0.50);
        let max = sorted[sorted.len() - 1];
        eprintln!("{name:<28} {med:>8.1} {max:>8.1}");
    }
}

fn parse_render_ms_sum(line: &str) -> Option<f64> {
    let key = "\"render_ms_sum\":";
    let i = line.find(key)?;
    let rest = &line[i + key.len()..];
    let end = rest
        .find(|c: char| c == ',' || c == '}')
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.len() == 1 {
        return sorted[0];
    }
    let rank = p * (sorted.len() - 1) as f64;
    let lo = rank.floor() as usize;
    let hi = rank.ceil() as usize;
    let w = rank - lo as f64;
    sorted[lo] * (1.0 - w) + sorted[hi] * w
}

fn blocked_fs_doc() -> Document {
    blocked_fs_doc_side_ps(SIDE, 5)
}

fn blocked_fs_doc_side(side: u32) -> Document {
    blocked_fs_doc_side_ps(side, 5)
}

fn blocked_fs_doc_side_ps(side: u32, pixel_size: u8) -> Document {
    let mut doc = Document::new(DocumentId::new(1), side, side);
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, side, side);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::BlockedFs,
            pixel_size,
            levels: 2,
            ..DitherParamsV2::default()
        }),
    ));
    doc.root.push(LayerNode::Leaf(layer));
    doc
}

fn fill_raw_tiles(state: &AppState) {
    fill_raw_tiles_side(state, SIDE);
}

fn fill_raw_tiles_side(state: &AppState, side: u32) {
    let cols = side.div_ceil(engine_tiles::TILE_SIZE);
    let mut tile = PixelTile::new();
    tile.data.fill(0.45);
    let arc = Arc::new(tile);
    for y in 0..cols {
        for x in 0..cols {
            let key = TileKey {
                doc: 1,
                layer: 1,
                coord: TileCoord { level: 0, x, y },
                stage: CacheStage::Raw,
            };
            state
                .tiles
                .tile_cache
                .insert_fresh_gen(key, Arc::clone(&arc), 0);
        }
    }
}

fn bump_and_invalidate(state: &AppState) -> u64 {
    crate::tile_pipeline::reset_fd_publish_claims();
    state.must_active().document_handle.mutate(|doc| {
        doc.increment_generation();
    });
    state.tiles.full_document.invalidate_layer(1, 1);
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: 1 },
    );
    state
        .active_session()
        .expect("session")
        .document_handle
        .snapshot()
        .generations
        .document_gen
        .load(Ordering::Acquire)
}

fn wait_s0(state: &AppState, gen: u64, limit: Duration) -> String {
    let start = Instant::now();
    let mut next_log = Duration::from_secs(15);
    loop {
        let pending = state.tiles.scheduler.pending_count();
        let inflight = state.preview_pass_inflight.load(Ordering::Acquire);
        // Background fd-publish must finish before S0 so publish_ms is recorded.
        if pending == 0 && inflight == 0 {
            crate::tile_pipeline::join_fd_publishes();
            crate::preview_diag::maybe_flush(1, gen, 0, 0);
        }
        if start.elapsed() >= next_log {
            eprintln!(
                "step0 wait gen={gen} t={:.0}s pending={pending} inflight={inflight} lines={}",
                start.elapsed().as_secs_f64(),
                crate::preview_diag::test_lines().len()
            );
            next_log += Duration::from_secs(15);
        }
        if let Some(line) = crate::preview_diag::test_lines()
            .into_iter()
            .find(|l| l.contains("[diag/S0]") && l.contains(&format!("\"gen\":{gen},")))
        {
            crate::tile_pipeline::join_fd_publishes();
            return line;
        }
        if start.elapsed() > limit {
            let lines = crate::preview_diag::test_lines();
            for line in &lines {
                eprintln!("step0 held {line}");
            }
            crate::tile_pipeline::join_fd_publishes();
            crate::preview_diag::maybe_flush(1, gen, 0, 0);
            if let Some(line) = crate::preview_diag::test_lines()
                .into_iter()
                .find(|l| l.contains("[diag/S0]") && l.contains(&format!("\"gen\":{gen},")))
            {
                eprintln!("step0 flushed late {line}");
                return line;
            }
            panic!(
                "run gen={gen} timed out pending={} inflight={} lines={}",
                state.tiles.scheduler.pending_count(),
                state.preview_pass_inflight.load(Ordering::Acquire),
                lines.len()
            );
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn worker_loop(state: Arc<AppState>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        let Some(task) = state.tiles.scheduler.dequeue() else {
            thread::sleep(Duration::from_millis(2));
            continue;
        };
        run_task(&state, task);
    }
}

fn run_task(state: &AppState, task: engine_tiles::RecomputeTask) {
    let diag = crate::preview_diag::level();
    state.preview_pass_inflight.fetch_add(1, Ordering::AcqRel);

    let snapshot = match state.session(task.key.doc) {
        Ok(s) => s.document_handle.snapshot(),
        Err(_) => {
            finish(state, &task, diag);
            return;
        }
    };
    if task_is_stale(&task, &snapshot) {
        finish(state, &task, diag);
        return;
    }
    if diag >= 1 {
        crate::preview_diag::reset_task_phases();
    }
    let started = if diag >= 1
        && task.key.coord.level == 0
        && matches!(
            task.key.stage,
            CacheStage::Processed | CacheStage::Composite
        ) {
        Some(Instant::now())
    } else {
        None
    };

    let result = match task.key.stage {
        CacheStage::Raw => state
            .tiles
            .tile_cache
            .get_entry(task.key)
            .ok_or_else(|| engine_project::error::EngineError::invalid_state("raw missing")),
        CacheStage::Processed => crate::tile_pipeline::compute_processed_tile(task.key, state),
        CacheStage::Composite => crate::tile_pipeline::compute_composite_tile(task.key, state),
    };

    if let Ok(_tile) = result {
        let snapshot = match state.session(task.key.doc) {
            Ok(s) => s.document_handle.snapshot(),
            Err(_) => {
                finish(state, &task, diag);
                return;
            }
        };
        if !task_is_stale(&task, &snapshot) {
            if let Some(t0) = started {
                let total = t0.elapsed();
                crate::preview_diag::record_l0_compute(task.key.doc, task.generation, total);
                let stage = match task.key.stage {
                    CacheStage::Raw => 0,
                    CacheStage::Processed => 1,
                    CacheStage::Composite => 2,
                };
                crate::preview_diag::commit_task_phases(
                    task.key.doc,
                    task.generation,
                    total,
                    stage,
                    task.key.coord.x,
                    task.key.coord.y,
                );
            }
            let viewport_level = state.ui.viewport.lock().unwrap().level;
            if task.key.stage == CacheStage::Composite
                && task.key.coord.level == viewport_level
                && state.tiles.tile_cache.get_entry(task.key).is_some()
            {
                crate::preview_diag::record_display_ready(task.key.doc, task.generation);
            }
        }
    }
    finish(state, &task, diag);
}

fn finish(state: &AppState, task: &engine_tiles::RecomputeTask, diag: u8) {
    state.preview_pass_inflight.fetch_sub(1, Ordering::AcqRel);
    if diag >= 1 {
        crate::preview_diag::note_cache(
            task.key.doc,
            task.generation,
            state.tiles.tile_cache.used_bytes_count() as u64,
            state.tiles.tile_cache.budget_bytes_count() as u64,
        );
        crate::preview_diag::maybe_flush(
            task.key.doc,
            task.generation,
            state.tiles.scheduler.pending_count(),
            state.preview_pass_inflight.load(Ordering::Acquire),
        );
    }
}

fn rss_bytes() -> u64 {
    use sysinfo::{get_current_pid, ProcessesToUpdate, System};
    let Ok(pid) = get_current_pid() else {
        return 0;
    };
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory()).unwrap_or(0)
}

/// Option A only: 30 release runs at 8192² and 16384² with stage / RSS / eviction table.
#[test]
#[ignore]
fn scale_a_thirty_large() {
    let _gate = crate::preview_diag::test_gate();
    let _ = engine_gpu::warm_blocked_fs_pipelines();

    eprintln!("=== scale A 30-run large docs ===");
    eprintln!(
        "{:<10} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>8} {:>8} {:>8} {:>10}",
        "side",
        "wall_p50",
        "wall_p95",
        "render",
        "apply",
        "publish",
        "tile",
        "wait",
        "evict",
        "ensure",
        "miss",
        "rss_peak_mb"
    );

    // 16384² baseline is in `.local-doc/scale_a_large.md` (pre-parallel publish).
    // Post-change measure focuses on 4K/8K for turnaround.
    for &side in &[4096u32, 8192] {
        // Cap budget high enough for the full-doc f32 buffer + tiles.
        let budget = match side {
            4096 => 2 * 1024 * 1024 * 1024,
            8192 => 4 * 1024 * 1024 * 1024,
            _ => 8 * 1024 * 1024 * 1024usize,
        };
        let state = Arc::new(AppState::empty_process(None, budget, true));
        crate::preview_diag::sync_from_config(true, 1);
        state.spawn_session(blocked_fs_doc_side(side));
        fill_raw_tiles_side(&state, side);

        let stop = Arc::new(AtomicBool::new(false));
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let mut joins = Vec::new();
        for _ in 0..cores {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            joins.push(thread::spawn(move || worker_loop(state, stop)));
        }

        let fit = (VIEW_W / side as f64).min(VIEW_H / side as f64);
        let mut walls = Vec::with_capacity(30);
        let mut renders = Vec::with_capacity(30);
        let mut applys = Vec::with_capacity(30);
        let mut pubs = Vec::with_capacity(30);
        let mut tiles = Vec::with_capacity(30);
        let mut waits = Vec::with_capacity(30);
        let mut evicts = Vec::with_capacity(30);
        let mut ensures = Vec::with_capacity(30);
        let mut misses = Vec::with_capacity(30);
        let mut rss_peak = rss_bytes();

        const N: usize = 30;
        for run in 0..N {
            crate::preview_diag::clear_test_state();
            crate::preview_diag::sync_from_config(true, 1);
            crate::tile_pipeline::reset_fd_publish_claims();
            let gen = bump_and_invalidate(&state);
            let t0 = Instant::now();
            let _ = ViewportService::new(Arc::clone(&state))
                .set_viewport(fit, 0.0, 0.0, VIEW_W, VIEW_H)
                .expect("set_viewport");
            let line = wait_s0(&state, gen, Duration::from_secs(600));
            walls.push(t0.elapsed().as_secs_f64() * 1000.0);
            renders.push(parse_f64_field(&line, "render_ms_sum").unwrap_or(0.0));
            pubs.push(parse_f64_field(&line, "publish_ms_sum").unwrap_or(0.0));
            tiles.push(parse_f64_field(&line, "tile_ms_sum").unwrap_or(0.0));
            waits.push(parse_f64_field(&line, "wait_ms_sum").unwrap_or(0.0));
            evicts.push(parse_f64_field(&line, "evicted_tiles").unwrap_or(0.0));
            ensures.push(parse_f64_field(&line, "ensure_calls").unwrap_or(0.0));
            misses.push(parse_f64_field(&line, "composer_tile_misses").unwrap_or(0.0));
            if let Some(s) = engine_project::algorithms::gpu_dither::last_gpu_dither_status() {
                applys.push(s.elapsed_ms as f64);
            } else {
                applys.push(0.0);
            }
            rss_peak = rss_peak.max(rss_bytes());
            eprintln!(
                "  scale side={side} run={run} wall={:.1} render={:.1} apply={:.1} pub={:.1} tile={:.1} wait={:.1} evict={:.0} ensure={:.0} miss={:.0} rss_mb={:.0}",
                walls[run],
                renders[run],
                applys[run],
                pubs[run],
                tiles[run],
                waits[run],
                evicts[run],
                ensures[run],
                misses[run],
                rss_bytes() as f64 / (1024.0 * 1024.0)
            );
        }

        stop.store(true, Ordering::Release);
        for join in joins {
            let _ = join.join();
        }

        let (wp50, wp95) = pct_pair(&walls);
        let (rp50, _) = pct_pair(&renders);
        let (ap50, _) = pct_pair(&applys);
        let (pp50, _) = pct_pair(&pubs);
        let (tp50, _) = pct_pair(&tiles);
        let (wtp50, _) = pct_pair(&waits);
        let (ep50, _) = pct_pair(&evicts);
        let (enp50, _) = pct_pair(&ensures);
        let (mp50, _) = pct_pair(&misses);
        eprintln!(
            "{:<10} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>8.1} {:>8.1} {:>8.1} {:>10.0}",
            side,
            wp50,
            wp95,
            rp50,
            ap50,
            pp50,
            tp50,
            wtp50,
            ep50,
            enp50,
            mp50,
            rss_peak as f64 / (1024.0 * 1024.0)
        );

        // Apply GPU stage medians from last status is coarse; print box1 from last run's hold.
        if let Some(s) = engine_project::algorithms::gpu_dither::last_gpu_dither_status() {
            eprintln!(
                "  apply stages side={side}: in_f32={:.1} setup={:.1} lattice={:.1} pack={:.1} \
prepare={:.1} upload={:.1} dispatch={:.1} readback={:.1} post={:.1} expand={:.1} \
out_u8={:.1} out_u8_to_f32={:.1} total={:.1}",
                s.in_f32_to_u8_ms,
                s.setup_hash_ms,
                s.lattice_ps_ms,
                s.pack_upload_bytes_ms,
                s.prepare_ms,
                s.upload_ms,
                s.dispatch_ms,
                s.readback_ms,
                s.post_ms,
                s.expand_ps_ms,
                s.out_bytes_to_rgba8_ms,
                s.out_u8_to_f32_ms,
                s.elapsed_ms
            );
        }
    }

    // Nonlinearity note vs 4096 baseline.
    eprintln!(
        "nonlinearity: area ratios 4096→8192 = 4×, 4096→16384 = 16×. \
If wall scaled with area alone, 8192 ≈ 4×4096 and 16384 ≈ 16×4096. \
Deviations come from publish∝tiles, FD assemble∝pixels, GPU strips, cache pressure."
    );

    crate::tile_pipeline::reset_fd_publish_claims();
    crate::preview_diag::clear_test_state();
}

/// Color diagnostic STEP 2: stage table for 4096² ps=1/5 (30 runs) and 8192² ps=5 (10 runs).
/// Measurement / counters only — no algorithm or golden changes.
#[test]
#[ignore = "color-diag: cargo test -p dither --release color_diag_stage_matrix -- --ignored --nocapture --test-threads=1"]
fn color_diag_stage_matrix() {
    let _gate = crate::preview_diag::test_gate();
    let _ = engine_gpu::warm_blocked_fs_pipelines();
    engine_project::algorithms::gpu_selftest::set_state(
        engine_project::algorithms::gpu_selftest::SelftestState::Pass,
    );

    let cases: &[(u32, u8, usize)] = &[(4096, 1, 30), (4096, 5, 30), (8192, 5, 10)];
    eprintln!("=== color_diag_stage_matrix ===");

    for &(side, ps, n) in cases {
        let budget = match side {
            4096 => 2 * 1024 * 1024 * 1024,
            _ => 4 * 1024 * 1024 * 1024usize,
        };
        let state = Arc::new(AppState::empty_process(None, budget, true));
        crate::preview_diag::sync_from_config(true, 1);
        state.spawn_session(blocked_fs_doc_side_ps(side, ps));
        fill_raw_tiles_side(&state, side);

        let stop = Arc::new(AtomicBool::new(false));
        let cores = std::thread::available_parallelism()
            .map(|c| c.get())
            .unwrap_or(1);
        let mut joins = Vec::new();
        for _ in 0..cores {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            joins.push(thread::spawn(move || worker_loop(state, stop)));
        }

        let fit = (VIEW_W / side as f64).min(VIEW_H / side as f64);
        let mut walls = Vec::with_capacity(n);
        let mut apply_totals = Vec::with_capacity(n);
        let mut in_f32 = Vec::with_capacity(n);
        let mut expand = Vec::with_capacity(n);
        let mut out_lin = Vec::with_capacity(n);
        let mut pack_up = Vec::with_capacity(n);
        let mut upload = Vec::with_capacity(n);
        let mut dispatch = Vec::with_capacity(n);
        let mut readback = Vec::with_capacity(n);
        let mut assemble = Vec::with_capacity(n);
        let mut promote = Vec::with_capacity(n);
        let mut rss_peak = rss_bytes();

        for run in 0..n {
            crate::preview_diag::clear_test_state();
            crate::preview_diag::sync_from_config(true, 1);
            crate::tile_pipeline::reset_fd_publish_claims();
            let gen = bump_and_invalidate(&state);
            let t0 = Instant::now();
            let _ = ViewportService::new(Arc::clone(&state))
                .set_viewport(fit, 0.0, 0.0, VIEW_W, VIEW_H)
                .expect("set_viewport");
            let _line = wait_s0(&state, gen, Duration::from_secs(600));
            walls.push(t0.elapsed().as_secs_f64() * 1000.0);
            if let Some(s) = engine_project::algorithms::gpu_dither::last_gpu_dither_status() {
                apply_totals.push(s.elapsed_ms as f64);
                in_f32.push(s.in_f32_to_u8_ms);
                expand.push(s.expand_ps_ms);
                out_lin.push(s.out_u8_to_f32_ms);
                pack_up.push(s.pack_upload_bytes_ms);
                upload.push(s.upload_ms);
                dispatch.push(s.dispatch_ms);
                readback.push(s.readback_ms);
            }
            if let Some(f) = engine_project::filters::last_fd_render_stages() {
                assemble.push(f.assemble_blit_ms);
                promote.push(f.promote_f32_ms);
            }
            rss_peak = rss_peak.max(rss_bytes());
            eprintln!(
                "  side={side} ps={ps} run={run}/{n} wall={:.1} apply={:.1} in_f32={:.1} expand={:.1} out_lin={:.1} pack={:.1} upload={:.1} dispatch={:.1} readback={:.1} assemble={:.1} promote={:.1}",
                walls[run],
                apply_totals.last().copied().unwrap_or(0.0),
                in_f32.last().copied().unwrap_or(0.0),
                expand.last().copied().unwrap_or(0.0),
                out_lin.last().copied().unwrap_or(0.0),
                pack_up.last().copied().unwrap_or(0.0),
                upload.last().copied().unwrap_or(0.0),
                dispatch.last().copied().unwrap_or(0.0),
                readback.last().copied().unwrap_or(0.0),
                assemble.last().copied().unwrap_or(0.0),
                promote.last().copied().unwrap_or(0.0),
            );
        }

        stop.store(true, Ordering::Release);
        for join in joins {
            let _ = join.join();
        }

        let pair = |v: &[f64]| {
            if v.is_empty() {
                (0.0, 0.0)
            } else {
                pct_pair(v)
            }
        };
        let (w50, w95) = pair(&walls);
        let (a50, a95) = pair(&apply_totals);
        let (i50, i95) = pair(&in_f32);
        let (e50, e95) = pair(&expand);
        let (o50, o95) = pair(&out_lin);
        let (pk50, pk95) = pair(&pack_up);
        let (u50, u95) = pair(&upload);
        let (d50, d95) = pair(&dispatch);
        let (r50, r95) = pair(&readback);
        let (as50, as95) = pair(&assemble);
        let (pr50, pr95) = pair(&promote);
        eprintln!(
            "SUMMARY side={side} ps={ps} n={n} | wall p50={w50:.1} p95={w95:.1} | apply p50={a50:.1} p95={a95:.1}"
        );
        eprintln!(
            "  in_f32_to_u8 p50={i50:.1} p95={i95:.1} | expand_ps p50={e50:.1} p95={e95:.1} | out_u8_to_f32 p50={o50:.1} p95={o95:.1}"
        );
        eprintln!(
            "  pack_upload p50={pk50:.1} p95={pk95:.1} | upload p50={u50:.1} p95={u95:.1} | dispatch p50={d50:.1} p95={d95:.1} | readback p50={r50:.1} p95={r95:.1}"
        );
        eprintln!(
            "  assemble_blit p50={as50:.1} p95={as95:.1} | promote_f32 p50={pr50:.1} p95={pr95:.1} | rss_peak_MiB={:.0}",
            rss_peak as f64 / (1024.0 * 1024.0)
        );
    }

    crate::tile_pipeline::reset_fd_publish_claims();
    crate::preview_diag::clear_test_state();
}
