use engine_gpu::{
    apply_blocked_fs_gpu_opts, apply_blue_noise_gpu_opts, BlockedFsGpuOpts, BlueNoiseGpuOpts,
};
use engine_project::algorithms::gpu_dither_cpu::{
    blocked_fs_cpu, blue_noise_cpu, blue_noise_ranks, with_pixel_size_blocks, PaletteRgb, Rgba8,
    CORE, PAD,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::case::{Algorithm, ParityCase};
use super::inputs::{build_input, from_bytes, to_bytes};
use super::matrix::{build_matrix, print_coverage_table, ParityMode};
use super::report::{collect_mismatches, write_failure_report};
use super::rng::SplitMix64;

/// Groups that must finish with skipped=0 for an overnight/full run to count as complete.
const REQUIRED_NO_SKIP_GROUPS: &[&str] = &["full_large", "full_strips"];

#[derive(Clone, Copy, Default)]
struct GroupStats {
    passed: usize,
    failed: usize,
    skipped: usize,
}

struct Stats {
    passed: usize,
    failed: usize,
    skipped: usize,
    by_group: BTreeMap<String, GroupStats>,
}

impl Stats {
    fn new() -> Self {
        Self {
            passed: 0,
            failed: 0,
            skipped: 0,
            by_group: BTreeMap::new(),
        }
    }

    fn bump_pass(&mut self, group: &str) {
        self.passed += 1;
        self.by_group.entry(group.to_string()).or_default().passed += 1;
    }

    fn bump_fail(&mut self, group: &str) {
        self.failed += 1;
        self.by_group.entry(group.to_string()).or_default().failed += 1;
    }

    fn bump_skip(&mut self, group: &str) {
        self.skipped += 1;
        self.by_group.entry(group.to_string()).or_default().skipped += 1;
    }

    fn processed(&self) -> usize {
        self.passed + self.failed + self.skipped
    }
}

fn progress_path() -> PathBuf {
    std::env::var("PARITY_PROGRESS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("parity_progress.txt")
        })
}

fn write_progress(
    path: &std::path::Path,
    mode: ParityMode,
    seed: u64,
    idx: usize,
    total: usize,
    stats: &Stats,
    case_id: &str,
) {
    let required_skips: Vec<String> = REQUIRED_NO_SKIP_GROUPS
        .iter()
        .filter_map(|g| {
            let s = stats.by_group.get(*g)?;
            if s.skipped > 0 {
                Some(format!("{g}:{}", s.skipped))
            } else {
                None
            }
        })
        .collect();
    let body = format!(
        "mode={mode:?}\nseed={seed:#x}\ncompleted={idx}\ntotal={total}\n\
         passed={}\nfailed={}\nskipped={}\nlast_case={case_id}\n\
         resume_hint=PARITY_SKIP={idx}\nrequired_group_skips={}\n",
        stats.passed,
        stats.failed,
        stats.skipped,
        if required_skips.is_empty() {
            "none".into()
        } else {
            required_skips.join(",")
        }
    );
    if let Ok(mut f) = fs::File::create(path) {
        let _ = f.write_all(body.as_bytes());
    }
}

fn palette_rgb(pal: &[[u8; 4]]) -> Vec<PaletteRgb> {
    pal.iter()
        .map(|c| PaletteRgb {
            r: c[0],
            g: c[1],
            b: c[2],
        })
        .collect()
}

fn run_cpu(case: &ParityCase, input: &[Rgba8]) -> Result<Vec<u8>, String> {
    let pal = palette_rgb(&case.palette);
    let ps = case.params.pixel_size.max(1);
    match case.algorithm {
        Algorithm::BlockedFs => {
            let pad = if case.params.pad == 0 {
                PAD
            } else {
                case.params.pad
            };
            // GPU warm-up is fixed at PAD=8; non-default pad is CPU-only.
            let out =
                with_pixel_size_blocks(input, case.width, case.height, ps, |small, sw, sh| {
                    blocked_fs_cpu(small, sw, sh, &pal, CORE, CORE, pad)
                })
                .map_err(|e| e.to_string())?;
            Ok(to_bytes(&out))
        }
        Algorithm::BlueNoise => {
            let out =
                with_pixel_size_blocks(input, case.width, case.height, ps, |small, sw, sh| {
                    blue_noise_cpu(small, sw, sh, &pal, blue_noise_ranks(), case.params.spread)
                })
                .map_err(|e| e.to_string())?;
            Ok(to_bytes(&out))
        }
    }
}

fn run_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    case: &ParityCase,
    input_bytes: &[u8],
    cancel: &AtomicBool,
    strip_cores: Option<u32>,
    strip_h: Option<u32>,
) -> Result<Vec<u8>, String> {
    let ps = case.params.pixel_size.max(1);
    let input = from_bytes(input_bytes);
    // Same ED-style block lattice as CPU / apply_gpu_dither_rgba.
    let (tw, th, lattice) = if ps <= 1 {
        (case.width, case.height, None)
    } else {
        let bw = case.width.div_ceil(ps);
        let bh = case.height.div_ceil(ps);
        let w = case.width as usize;
        let mut small = Vec::with_capacity((bw * bh) as usize);
        for by in 0..bh {
            for bx in 0..bw {
                small.push(input[(by * ps) as usize * w + (bx * ps) as usize]);
            }
        }
        (bw, bh, Some(small))
    };
    let src = lattice.as_deref().unwrap_or(&input);
    let in_b = to_bytes(src);
    let gpu = match case.algorithm {
        Algorithm::BlockedFs => {
            if case.params.pad != 8 && case.params.pad != 0 {
                return Err("skip:pad".into());
            }
            apply_blocked_fs_gpu_opts(
                device,
                queue,
                &in_b,
                tw,
                th,
                &case.palette,
                cancel,
                BlockedFsGpuOpts {
                    force_strip_cores: strip_cores,
                    force_max_bind: None,
                },
            )?
        }
        Algorithm::BlueNoise => apply_blue_noise_gpu_opts(
            device,
            queue,
            &in_b,
            tw,
            th,
            &case.palette,
            blue_noise_ranks(),
            case.params.spread,
            cancel,
            BlueNoiseGpuOpts {
                force_strip_h: strip_h,
                force_max_bind: None,
            },
        )?,
    };
    if ps <= 1 {
        return Ok(gpu.rgba8);
    }
    let dithered = from_bytes(&gpu.rgba8);
    let bw = tw as usize;
    let w = case.width as usize;
    let h = case.height as usize;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        let by = (y as u32 / ps) as usize;
        for x in 0..w {
            let bx = (x as u32 / ps) as usize;
            let p = dithered[by * bw + bx];
            out.push(Rgba8::new(p.r, p.g, p.b, input[y * w + x].a));
        }
    }
    Ok(to_bytes(&out))
}

fn check_properties(case: &ParityCase, input: &[Rgba8], out: &[u8]) -> Result<(), String> {
    let expected = (case.width as usize) * (case.height as usize) * 4;
    if out.len() != expected {
        return Err(format!("size {} != {expected}", out.len()));
    }
    let pal: std::collections::HashSet<[u8; 3]> =
        case.palette.iter().map(|c| [c[0], c[1], c[2]]).collect();
    let pixels = from_bytes(out);
    for (i, px) in pixels.iter().enumerate() {
        if !pal.contains(&[px.r, px.g, px.b]) {
            return Err(format!(
                "pixel {i} rgb=({},{},{}) not in palette",
                px.r, px.g, px.b
            ));
        }
        if px.a != input[i].a {
            return Err(format!(
                "alpha mismatch at {i}: out={} in={}",
                px.a, input[i].a
            ));
        }
    }
    // Block uniformity: every ps×ps cell (incl. ragged edges) is one RGB.
    let ps = case.params.pixel_size.max(1);
    if ps > 1 {
        let w = case.width as usize;
        let h = case.height as usize;
        let bw = case.width.div_ceil(ps) as usize;
        let bh = case.height.div_ceil(ps) as usize;
        for by in 0..bh {
            for bx in 0..bw {
                let x0 = bx * ps as usize;
                let y0 = by * ps as usize;
                let rep = pixels[y0 * w + x0];
                for y in y0..(y0 + ps as usize).min(h) {
                    for x in x0..(x0 + ps as usize).min(w) {
                        let p = pixels[y * w + x];
                        if p.r != rep.r || p.g != rep.g || p.b != rep.b {
                            return Err(format!(
                                "block ({bx},{by}) not uniform at ({x},{y}): rep=({},{},{}) got=({},{},{})",
                                rep.r, rep.g, rep.b, p.r, p.g, p.b
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// App FullDocument path (`apply_gpu_dither_rgba`) — used for `params_ps`.
fn run_app_path(case: &ParityCase, input: &[Rgba8], path: &str) -> Result<Vec<u8>, String> {
    use engine_project::algorithms::gpu_dither::apply_gpu_dither_rgba;
    use engine_project::filter::{DitherModeV2, DitherParamsV2};
    std::env::set_var("DITHER_GPU_PATH", path);
    let n = input.len();
    let mut rgba = Vec::with_capacity(n * 4);
    for p in input {
        rgba.push(engine_color::palette::srgb_to_linear(p.r));
        rgba.push(engine_color::palette::srgb_to_linear(p.g));
        rgba.push(engine_color::palette::srgb_to_linear(p.b));
        rgba.push(p.a as f32 / 255.0);
    }
    let mode = match case.algorithm {
        Algorithm::BlockedFs => DitherModeV2::BlockedFs,
        Algorithm::BlueNoise => DitherModeV2::BlueNoise,
    };
    let ps = case.params.pixel_size.max(1);
    if ps > 32 {
        return Err(format!(
            "app path rejects pixel_size={ps} (DitherParamsV2 max 32); use kernel path"
        ));
    }
    let params = DitherParamsV2 {
        mode,
        pixel_size: ps as u8,
        threshold_scale: case.params.spread as f32 / 256.0,
        ..DitherParamsV2::default()
    };
    let pal = palette_rgb(&case.palette);
    let cancel = AtomicBool::new(false);
    apply_gpu_dither_rgba(&mut rgba, case.width, case.height, &params, &pal, &cancel)
        .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(n * 4);
    for chunk in rgba.chunks_exact(4) {
        out.push(engine_color::palette::linear_to_srgb(chunk[0]));
        out.push(engine_color::palette::linear_to_srgb(chunk[1]));
        out.push(engine_color::palette::linear_to_srgb(chunk[2]));
        out.push((chunk[3].clamp(0.0, 1.0) * 255.0).round() as u8);
    }
    Ok(out)
}

fn goldens_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gpu_parity/cpu_goldens.json")
}

fn load_goldens() -> BTreeMap<String, String> {
    let path = goldens_path();
    if !path.exists() {
        return BTreeMap::new();
    }
    let s = fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&s).unwrap_or_default()
}

fn save_goldens(map: &BTreeMap<String, String>) {
    let path = goldens_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let s = serde_json::to_string_pretty(map).expect("goldens json");
    fs::write(path, s).expect("write goldens");
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn default_seed() -> u64 {
    std::env::var("PARITY_SEED")
        .ok()
        .and_then(|s| {
            if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16).ok()
            } else {
                s.parse().ok()
            }
        })
        .unwrap_or_else(|| {
            let mut rng = SplitMix64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0xDEAD_BEEF),
            );
            rng.next_u64()
        })
}

fn repeat_count(mode: ParityMode) -> usize {
    if let Ok(s) = std::env::var("PARITY_REPEAT") {
        if let Ok(n) = s.parse::<usize>() {
            return n.max(1);
        }
    }
    match mode {
        // CI budget: one GPU run per case (use PARITY_REPEAT=20 locally for stability).
        ParityMode::Quick => 1,
        _ => 3,
    }
}

fn parity_skip() -> usize {
    std::env::var("PARITY_SKIP")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn progress_interval() -> usize {
    std::env::var("PARITY_PROGRESS_EVERY")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(50)
}

/// Run strip-equality + cancel checks on a subset of cases.
fn extras_for(case: &ParityCase) -> bool {
    // Strip-equality extras are expensive; keep them off the CI `quick` matrix
    // (covered by `gpu_strip_bind_parity` + full/pre-release runs).
    matches!(
        case.group.as_str(),
        "full_small" | "full_cover" | "full_strips" | "params_spread" | "params_ps"
    ) && case.width * case.height <= 256 * 256
}

pub fn run_parity_matrix() {
    let mode = ParityMode::from_env();
    let seed = default_seed();
    eprintln!("=== gpu_parity_matrix mode={mode:?} seed={seed:#x} ===");

    let cases = build_matrix(mode, seed);
    let skip_n = parity_skip();
    eprintln!("cases: {} (PARITY_SKIP={skip_n})", cases.len());
    if skip_n > 0 {
        eprintln!("resuming after first {skip_n} cases — use same PARITY_SEED for reproducibility");
    }
    print_coverage_table(&cases);
    if std::env::var("PARITY_INVENTORY_ONLY").ok().as_deref() == Some("1") {
        eprintln!("PARITY_INVENTORY_ONLY=1 — skipping GPU");
        return;
    }

    // Use the process-cached device so `params_ps` app-path calls
    // (`apply_gpu_dither_rgba` → `cached_blocked_fs_device`) share the same
    // OnceLock pipeline. A second device would panic: BindGroupLayout from
    // device A used on device B.
    let Some((device, queue, backend)) = engine_gpu::cached_blocked_fs_device() else {
        eprintln!("skip: no suitable GPU adapter");
        return;
    };
    eprintln!("adapter backend={backend:?}");
    let repeats = repeat_count(mode);
    eprintln!("gpu repeats/case: {repeats}");
    let fail_fast = std::env::var("PARITY_FAIL_FAST").ok().as_deref() == Some("1");
    if fail_fast {
        eprintln!("PARITY_FAIL_FAST=1 (stop after first failure)");
    }
    let prog_path = progress_path();
    let prog_every = progress_interval();
    eprintln!(
        "progress file: {} (every {prog_every} cases)",
        prog_path.display()
    );

    let mut stats = Stats::new();
    let mut goldens = load_goldens();
    let record_goldens = std::env::var("PARITY_RECORD_GOLDENS").ok().as_deref() == Some("1");
    let mut golden_checked = 0usize;
    let mut golden_fail = 0usize;

    // Cancel test once per run on a mid-size case.
    let mut cancel_tested = false;
    let mut last_progress = Instant::now();

    for (idx, case) in cases.iter().enumerate() {
        if idx < skip_n {
            continue;
        }
        if fail_fast && stats.failed > 0 {
            break;
        }
        if idx % 200 == 0 {
            eprintln!("… {idx}/{} ({})", cases.len(), case.id);
        }

        // Pad ≠ 8 is not GPU-eligible — count as skipped (not a pass).
        if case.algorithm == Algorithm::BlockedFs && case.params.pad != 8 && case.params.pad != 0 {
            eprintln!("SKIP {} (pad={} not on GPU path)", case.id, case.params.pad);
            stats.bump_skip(&case.group);
            maybe_write_progress(
                &prog_path,
                mode,
                seed,
                idx + 1,
                cases.len(),
                &stats,
                &case.id,
                prog_every,
                &mut last_progress,
            );
            continue;
        }

        let input = build_input(&case.input, case.width, case.height);
        let input_bytes = to_bytes(&input);
        // App `DitherParamsV2.pixel_size` is capped at 32. Larger lattice
        // values (56/57/64 from the spec matrix) exercise the kernel wrap only.
        let via_app = case.group == "params_ps" && case.params.pixel_size <= 32;

        let cpu = match if via_app {
            run_app_path(case, &input, "cpu")
        } else {
            run_cpu(case, &input)
        } {
            Ok(v) => v,
            Err(e) => {
                eprintln!("FAIL {} cpu: {e}", case.id);
                write_failure_report(case, &input, &[], &[], &format!("cpu_error:{e}"));
                stats.bump_fail(&case.group);
                continue;
            }
        };

        if let Err(e) = check_properties(case, &input, &cpu) {
            eprintln!("FAIL {} cpu properties: {e}", case.id);
            write_failure_report(case, &input, &cpu, &[], &format!("cpu_props:{e}"));
            stats.bump_fail(&case.group);
            continue;
        }

        // Golden hashes for a sparse subset (one case per size class × algo).
        // Exact B/W palette only — not p2same / p2close / pog (also len==2).
        let is_bw_p2 = case.palette.len() == 2
            && case.palette[0] == [0, 0, 0, 255]
            && case.palette[1] == [255, 255, 255, 255];
        let is_golden_ps1 = case.width == case.height
            && matches!(case.width, 8 | 56 | 64 | 128)
            && case.params.spread == 256
            && case.params.pad == 8
            && case.params.pixel_size <= 1
            && matches!(case.input, super::case::InputKind::GradientH)
            && is_bw_p2;
        // Draft goldens: one ragged square per ps∈{2,4,7} (regenerate after geometry pick).
        let is_golden_ps_draft = case.group == "params_ps"
            && case.width == case.height
            && matches!(
                (case.params.pixel_size, case.width),
                (2, 15) | (4, 31) | (7, 55)
            )
            && matches!(case.input, super::case::InputKind::GradientH)
            && is_bw_p2
            && case.algorithm == Algorithm::BlockedFs;
        if is_golden_ps1 || is_golden_ps_draft {
            let algo = match case.algorithm {
                Algorithm::BlockedFs => "bfs",
                Algorithm::BlueNoise => "bn",
            };
            let key = if is_golden_ps1 {
                format!("{algo}_{}x{}_p2_grad_h_cpu", case.width, case.height)
            } else {
                format!(
                    "{algo}_{}x{}_ps{}_p2_grad_h_cpu_draft",
                    case.width, case.height, case.params.pixel_size
                )
            };
            let hash = sha256_hex(&cpu);
            if record_goldens {
                goldens.insert(key, hash);
            } else if let Some(expected) = goldens.get(&key) {
                golden_checked += 1;
                if expected != &hash {
                    golden_fail += 1;
                    eprintln!("FAIL {} golden hash mismatch", case.id);
                    write_failure_report(case, &input, &cpu, &[], "golden_mismatch");
                    stats.bump_fail(&case.group);
                    continue;
                }
            }
        }

        let cancel = AtomicBool::new(false);
        let mut gpu_results = Vec::with_capacity(repeats);
        let mut gpu_err: Option<String> = None;
        for r in 0..repeats {
            let once = if via_app {
                run_app_path(case, &input, "gpu")
            } else {
                run_gpu(&device, &queue, case, &input_bytes, &cancel, None, None)
            };
            match once {
                Ok(v) => gpu_results.push(v),
                Err(e) => {
                    gpu_err = Some(format!("repeat{r}: {e}"));
                    break;
                }
            }
        }
        if let Some(e) = gpu_err {
            // Binding cannot fit even one strip — skipped, not a pass.
            if e.contains("use CPU path") {
                eprintln!("SKIP {} ({e})", case.id);
                stats.bump_skip(&case.group);
                maybe_write_progress(
                    &prog_path,
                    mode,
                    seed,
                    idx + 1,
                    cases.len(),
                    &stats,
                    &case.id,
                    prog_every,
                    &mut last_progress,
                );
                continue;
            }
            eprintln!("FAIL {} gpu: {e}", case.id);
            write_failure_report(case, &input, &cpu, &[], &format!("gpu_error:{e}"));
            stats.bump_fail(&case.group);
            continue;
        }

        // Repeatability
        let gpu0 = &gpu_results[0];
        let mut rep_fail = false;
        for (ri, g) in gpu_results.iter().enumerate().skip(1) {
            if g != gpu0 {
                eprintln!("FAIL {} gpu repeat {ri} differs", case.id);
                write_failure_report(case, &input, gpu0, g, "gpu_repeat");
                rep_fail = true;
                break;
            }
        }
        if rep_fail {
            stats.bump_fail(&case.group);
            continue;
        }

        if let Err(e) = check_properties(case, &input, gpu0) {
            eprintln!("FAIL {} gpu properties: {e}", case.id);
            write_failure_report(case, &input, &cpu, gpu0, &format!("gpu_props:{e}"));
            stats.bump_fail(&case.group);
            continue;
        }

        // Byte identity
        if cpu != *gpu0 {
            let n = collect_mismatches(&cpu, gpu0, case.width).len();
            eprintln!("FAIL {} GPU≠CPU ({n} px)", case.id);
            write_failure_report(case, &input, &cpu, gpu0, "parity");
            stats.bump_fail(&case.group);
            continue;
        }

        // Strip equality
        if extras_for(case) {
            let strip_ok = match case.algorithm {
                Algorithm::BlockedFs => {
                    let mut ok = true;
                    for cores in [1u32, 2, 3] {
                        match run_gpu(
                            &device,
                            &queue,
                            case,
                            &input_bytes,
                            &cancel,
                            Some(cores),
                            None,
                        ) {
                            Ok(v) if v == cpu => {}
                            Ok(v) => {
                                eprintln!("FAIL {} strip_cores={cores}", case.id);
                                write_failure_report(
                                    case,
                                    &input,
                                    &cpu,
                                    &v,
                                    &format!("strip_cores_{cores}"),
                                );
                                ok = false;
                                break;
                            }
                            Err(e) => {
                                eprintln!("FAIL {} strip_cores={cores}: {e}", case.id);
                                ok = false;
                                break;
                            }
                        }
                    }
                    ok
                }
                Algorithm::BlueNoise => {
                    let mut ok = true;
                    for h in [1u32, 2, 8, 16] {
                        if h >= case.height {
                            continue;
                        }
                        match run_gpu(&device, &queue, case, &input_bytes, &cancel, None, Some(h)) {
                            Ok(v) if v == cpu => {}
                            Ok(v) => {
                                eprintln!("FAIL {} strip_h={h}", case.id);
                                write_failure_report(
                                    case,
                                    &input,
                                    &cpu,
                                    &v,
                                    &format!("strip_h_{h}"),
                                );
                                ok = false;
                                break;
                            }
                            Err(e) => {
                                eprintln!("FAIL {} strip_h={h}: {e}", case.id);
                                ok = false;
                                break;
                            }
                        }
                    }
                    ok
                }
            };
            if !strip_ok {
                stats.bump_fail(&case.group);
                continue;
            }
        }

        // Cancel between strips (once): flag set before dispatch → Cancelled, no output.
        if !cancel_tested && case.height >= CORE && case.width >= 32 {
            cancel_tested = true;
            let flag = AtomicBool::new(true);
            match run_gpu(&device, &queue, case, &input_bytes, &flag, Some(1), Some(8)) {
                Err(e) if e.to_ascii_lowercase().contains("cancel") => {}
                Ok(_) => {
                    eprintln!("FAIL {} cancel returned Ok", case.id);
                    stats.bump_fail(&case.group);
                    continue;
                }
                Err(e) => {
                    eprintln!("FAIL {} cancel unexpected err: {e}", case.id);
                    stats.bump_fail(&case.group);
                    continue;
                }
            }
        }

        stats.bump_pass(&case.group);
        maybe_write_progress(
            &prog_path,
            mode,
            seed,
            idx + 1,
            cases.len(),
            &stats,
            &case.id,
            prog_every,
            &mut last_progress,
        );
    }

    // Final progress snapshot.
    write_progress(
        &prog_path,
        mode,
        seed,
        skip_n + stats.processed(),
        cases.len(),
        &stats,
        "DONE",
    );

    if record_goldens {
        save_goldens(&goldens);
        eprintln!("recorded {} golden hashes", goldens.len());
    }

    eprintln!("\n=== summary ===");
    eprintln!(
        "passed={} failed={} skipped={} (skips are not success)",
        stats.passed, stats.failed, stats.skipped
    );
    eprintln!("goldens checked={golden_checked} fail={golden_fail}");
    eprintln!("group\tpassed\tfailed\tskipped");
    for (g, s) in &stats.by_group {
        eprintln!("{g}\t{}\t{}\t{}", s.passed, s.failed, s.skipped);
    }

    let mut required_skip_fail = false;
    let filter = super::matrix::filter_substr();
    for g in REQUIRED_NO_SKIP_GROUPS {
        // Filtered runs only enforce required groups that the filter selects.
        if let Some(ref f) = filter {
            if !super::matrix::filter_matches("", g, f) {
                continue;
            }
        }
        let sk = stats.by_group.get(*g).map(|s| s.skipped).unwrap_or(0);
        if sk > 0 {
            eprintln!("REQUIRED group {g}: skipped={sk} (must be 0)");
            required_skip_fail = true;
        } else if matches!(mode, ParityMode::Full | ParityMode::Nightly)
            && stats
                .by_group
                .get(*g)
                .map(|s| s.passed + s.failed)
                .unwrap_or(0)
                == 0
            && skip_n == 0
        {
            eprintln!("REQUIRED group {g}: no cases executed");
            required_skip_fail = true;
        }
    }

    assert!(
        stats.failed == 0 && golden_fail == 0 && !required_skip_fail,
        "parity incomplete: failed={} golden_fail={} required_skips={} (see gpu_parity_failures/ and {})",
        stats.failed,
        golden_fail,
        required_skip_fail,
        prog_path.display()
    );
}

fn maybe_write_progress(
    path: &std::path::Path,
    mode: ParityMode,
    seed: u64,
    next_idx: usize,
    total: usize,
    stats: &Stats,
    case_id: &str,
    every: usize,
    last: &mut Instant,
) {
    let due = stats.processed() % every == 0 || last.elapsed() >= Duration::from_secs(30);
    if due {
        write_progress(path, mode, seed, next_idx, total, stats, case_id);
        *last = Instant::now();
    }
}
