//! Headless micro-bench + apply-path marks for `.local-doc/time-testing.md` §7.
//!
//! ```text
//! DITHER_PROFILE=1 DITHER_GPU_PATH=gpu cargo test -p dither --release \
//!   dither_profile_harness -- --ignored --nocapture --test-threads=1
//! ```

#![cfg(test)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use engine_project::algorithms::gpu_dither::{
    apply_gpu_dither_rgba, resolve_gpu_dither_palette, take_last_gpu_dither_status, GpuDitherStatus,
};
use engine_project::dither_profile;
use engine_project::filter::{DitherModeV2, DitherParamsV2};
use engine_project::types::DocumentId;
use engine_project::Document;

use crate::dither_profile_host;

fn out_dir() -> PathBuf {
    let date = {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = secs / 86400;
        // reuse host civil date via folder name from env or today-ish
        format!("harness-{}", days)
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("profiling")
        .join("results")
        .join(date);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn make_gradient(w: u32, h: u32) -> Vec<f32> {
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let t = x as f32 / w.max(1) as f32;
            let u = y as f32 / h.max(1) as f32;
            rgba[i] = t;
            rgba[i + 1] = u;
            rgba[i + 2] = 0.5;
            rgba[i + 3] = 1.0;
        }
    }
    rgba
}

fn params(mode: DitherModeV2, levels: u16, pixel_size: u8) -> DitherParamsV2 {
    DitherParamsV2 {
        mode,
        levels,
        pixel_size,
        palette_id: None,
        ..DitherParamsV2::default()
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

struct ApplySample {
    status: GpuDitherStatus,
    wall_ms: f64,
}

fn run_apply(
    mode: DitherModeV2,
    w: u32,
    h: u32,
    levels: u16,
    pixel_size: u8,
    run_id: u64,
) -> ApplySample {
    let p = params(mode, levels, pixel_size);
    let doc = Document::new(DocumentId::new(1), w, h);
    let palette = resolve_gpu_dither_palette(&p, &doc).expect("palette");
    let mut rgba = make_gradient(w, h);
    let cancel = AtomicBool::new(false);
    dither_profile::begin_run(run_id, run_id);
    let t0 = Instant::now();
    apply_gpu_dither_rgba(&mut rgba, w, h, &p, &palette, &cancel).expect("apply");
    let wall_ms = t0.elapsed().as_secs_f64() * 1000.0;
    let status = take_last_gpu_dither_status().expect("status");
    ApplySample { status, wall_ms }
}

#[test]
#[ignore]
fn dither_profile_harness_main_grid() {
    dither_profile_host::init();
    assert!(
        dither_profile::enabled(),
        "set DITHER_PROFILE=1 to run this harness"
    );

    // Warm device + pipelines so first measured run is not `pipeline=new`.
    let device_ok = engine_gpu::cached_blocked_fs_device().is_some();
    let warmed = engine_gpu::warm_blocked_fs_pipelines();
    eprintln!(
        "gpu device={} warm_pipelines={} selftest_allowed={}",
        device_ok,
        warmed,
        engine_project::algorithms::gpu_selftest::gpu_path_allowed()
    );
    // Force Pass in-process for harness when device exists (cfg(test) ForceGpu
    // already bypasses; this helps `auto` too).
    if device_ok {
        engine_project::algorithms::gpu_selftest::set_state(
            engine_project::algorithms::gpu_selftest::SelftestState::Pass,
        );
    }

    let modes = [DitherModeV2::BlockedFs, DitherModeV2::BlueNoise];
    // Main grid per spec; set DITHER_PROFILE_QUICK=1 to skip 4k and use 5 samples.
    let quick = std::env::var("DITHER_PROFILE_QUICK").ok().as_deref() == Some("1");
    let sizes: &[(u32, u32)] = if quick {
        &[(748, 1024), (1920, 1080)]
    } else {
        &[(748, 1024), (1920, 1080), (4000, 3000)]
    };
    let palettes = [16u16, 256u16];
    let samples = if quick { 5usize } else { 15 };
    let backend = std::env::var("DITHER_GPU_PATH").unwrap_or_else(|_| "auto".into());

    let mut summary = String::from("# dither_profile harness summary\n\n");
    summary.push_str(&format!(
        "backend env: `{backend}`  device_ok={device_ok} warmed={warmed} quick={quick}\n\n"
    ));

    let mut run_id = 1u64;
    for mode in &modes {
        let mode = mode.clone();
        let mode_name = match mode {
            DitherModeV2::BlockedFs => "blocked_fs",
            DitherModeV2::BlueNoise => "blue_noise",
            _ => "other",
        };
        for &(w, h) in sizes {
            for &levels in &palettes {
                // Warm-up 3
                for _ in 0..3 {
                    let _ = run_apply(mode.clone(), w, h, levels, 1, run_id);
                    run_id += 1;
                }
                let mut walls = Vec::new();
                let mut dispatch = Vec::new();
                let mut prepare = Vec::new();
                let mut upload = Vec::new();
                let mut readback = Vec::new();
                let mut f32u8 = Vec::new();
                let mut u8f32 = Vec::new();
                let mut paths = Vec::new();
                for _ in 0..samples {
                    let s = run_apply(mode.clone(), w, h, levels, 1, run_id);
                    run_id += 1;
                    walls.push(s.wall_ms);
                    dispatch.push(s.status.dispatch_ms);
                    prepare.push(s.status.prepare_ms);
                    upload.push(s.status.upload_ms);
                    readback.push(s.status.readback_ms);
                    f32u8.push(s.status.in_f32_to_u8_ms);
                    u8f32.push(s.status.out_u8_to_f32_ms);
                    paths.push(s.status.path.clone());
                }
                walls.sort_by(|a, b| a.partial_cmp(b).unwrap());
                dispatch.sort_by(|a, b| a.partial_cmp(b).unwrap());
                prepare.sort_by(|a, b| a.partial_cmp(b).unwrap());
                upload.sort_by(|a, b| a.partial_cmp(b).unwrap());
                readback.sort_by(|a, b| a.partial_cmp(b).unwrap());
                f32u8.sort_by(|a, b| a.partial_cmp(b).unwrap());
                u8f32.sort_by(|a, b| a.partial_cmp(b).unwrap());

                let path_note = paths.last().cloned().unwrap_or_default();
                summary.push_str(&format!(
                    "## {mode_name} {w}x{h} levels={levels} path={path_note}\n\n"
                ));
                summary.push_str(&format!(
                    "| stage | median_ms | p95_ms |\n|---|---:|---:|\n"
                ));
                summary.push_str(&format!(
                    "| wall_apply | {:.2} | {:.2} |\n",
                    percentile(&walls, 0.5),
                    percentile(&walls, 0.95)
                ));
                summary.push_str(&format!(
                    "| prepare | {:.2} | {:.2} |\n",
                    percentile(&prepare, 0.5),
                    percentile(&prepare, 0.95)
                ));
                summary.push_str(&format!(
                    "| upload | {:.2} | {:.2} |\n",
                    percentile(&upload, 0.5),
                    percentile(&upload, 0.95)
                ));
                summary.push_str(&format!(
                    "| dispatch_wall | {:.2} | {:.2} |\n",
                    percentile(&dispatch, 0.5),
                    percentile(&dispatch, 0.95)
                ));
                summary.push_str(&format!(
                    "| readback | {:.2} | {:.2} |\n",
                    percentile(&readback, 0.5),
                    percentile(&readback, 0.95)
                ));
                summary.push_str(&format!(
                    "| host_f32_to_u8 | {:.2} | {:.2} |\n",
                    percentile(&f32u8, 0.5),
                    percentile(&f32u8, 0.95)
                ));
                summary.push_str(&format!(
                    "| host_u8_to_f32 | {:.2} | {:.2} |\n\n",
                    percentile(&u8f32, 0.5),
                    percentile(&u8f32, 0.95)
                ));

                eprintln!(
                    "{mode_name} {w}x{h} L{levels}: wall_med={:.1}ms dispatch_med={:.1}ms path={path_note}",
                    percentile(&walls, 0.5),
                    percentile(&dispatch, 0.5)
                );
            }
        }
    }

    // §7.1 palette 16 vs 256 — reuse main-grid medians when quick.
    summary.push_str("## Micro: blocked_fs palette/levels 16 vs 256 @ 1920x1080\n\n");
    if quick {
        summary.push_str("- see main-grid rows above (quick mode skips extra micro loops)\n");
    } else {
        for levels in [16u16, 256u16] {
            for _ in 0..3 {
                let _ = run_apply(DitherModeV2::BlockedFs, 1920, 1080, levels, 1, run_id);
                run_id += 1;
            }
            let mut walls = Vec::new();
            for _ in 0..15 {
                walls.push(run_apply(DitherModeV2::BlockedFs, 1920, 1080, levels, 1, run_id).wall_ms);
                run_id += 1;
            }
            walls.sort_by(|a, b| a.partial_cmp(b).unwrap());
            summary.push_str(&format!(
                "- levels={levels}: median={:.2}ms p95={:.2}ms\n",
                percentile(&walls, 0.5),
                percentile(&walls, 0.95)
            ));
        }
    }

    // Host conversion micro @ ~12MP (GPU path only in quick — CPU is multi-minute).
    summary.push_str("\n## Micro: host f32↔u8 @ 4000x3000 (inside apply status)\n\n");
    if quick && backend == "cpu" {
        summary.push_str("- skipped in quick+cpu (too slow); use GPU quick or full grid\n");
    } else {
        for _ in 0..3 {
            let _ = run_apply(DitherModeV2::BlockedFs, 4000, 3000, 16, 1, run_id);
            run_id += 1;
        }
        let s = run_apply(DitherModeV2::BlockedFs, 4000, 3000, 16, 1, run_id);
        summary.push_str(&format!(
            "- in_f32_to_u8={:.2}ms out_u8_to_f32={:.2}ms path={}\n",
            s.status.in_f32_to_u8_ms, s.status.out_u8_to_f32_ms, s.status.path
        ));
    }

    summary.push_str("\n## Notes\n\n");
    summary.push_str(
        "- `gpu_dispatch_done` GPU timestamp-query: **not measured** (device created without `TIMESTAMP_QUERY`).\n",
    );
    summary.push_str(
        "- `dispatch_wall` includes GPU wait intertwined with submit/poll (wall clock).\n",
    );
    summary.push_str(
        "- Full T0–T23 UI chain requires release app with `DITHER_PROFILE=1` and slider gestures; marks land in `profiling/results/<date>/marks.jsonl`.\n",
    );

    let path = out_dir().join("summary_harness.md");
    std::fs::write(&path, &summary).expect("write summary");
    eprintln!("wrote {}", path.display());
}
