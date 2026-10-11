use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use engine_gpu::{apply_blocked_fs_gpu_opts, apply_blue_noise_gpu_opts, BlockedFsGpuOpts, BlueNoiseGpuOpts};
use engine_project::algorithms::gpu_dither_cpu::{
    blue_noise_cpu, blue_noise_ranks, blocked_fs_cpu, PaletteRgb, Rgba8, CORE, PAD,
};
use sha2::{Digest, Sha256};

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
    match case.algorithm {
        Algorithm::BlockedFs => {
            let pad = if case.params.pad == 0 {
                PAD
            } else {
                case.params.pad
            };
            // GPU warm-up is fixed at PAD=8; non-default pad is CPU-only.
            let out = blocked_fs_cpu(
                input,
                case.width,
                case.height,
                &pal,
                CORE,
                CORE,
                pad,
            )
            .map_err(|e| e.to_string())?;
            Ok(to_bytes(&out))
        }
        Algorithm::BlueNoise => {
            let out = blue_noise_cpu(
                input,
                case.width,
                case.height,
                &pal,
                blue_noise_ranks(),
                case.params.spread,
            )
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
    match case.algorithm {
        Algorithm::BlockedFs => {
            if case.params.pad != 8 && case.params.pad != 0 {
                return Err("skip:pad".into());
            }
            apply_blocked_fs_gpu_opts(
                device,
                queue,
                input_bytes,
                case.width,
                case.height,
                &case.palette,
                cancel,
                BlockedFsGpuOpts {
                    force_strip_cores: strip_cores,
                    force_max_bind: None,
                },
            )
            .map(|r| r.rgba8)
        }
        Algorithm::BlueNoise => apply_blue_noise_gpu_opts(
            device,
            queue,
            input_bytes,
            case.width,
            case.height,
            &case.palette,
            blue_noise_ranks(),
            case.params.spread,
            cancel,
            BlueNoiseGpuOpts {
                force_strip_h: strip_h,
                force_max_bind: None,
            },
        )
        .map(|r| r.rgba8),
    }
}

fn check_properties(case: &ParityCase, input: &[Rgba8], out: &[u8]) -> Result<(), String> {
    let expected = (case.width as usize) * (case.height as usize) * 4;
    if out.len() != expected {
        return Err(format!("size {} != {expected}", out.len()));
    }
    let pal: std::collections::HashSet<[u8; 3]> = case
        .palette
        .iter()
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    for (i, px) in from_bytes(out).iter().enumerate() {
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
    Ok(())
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

    let Some((device, queue, backend)) = engine_gpu::request_blocked_fs_device() else {
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

        let cpu = match run_cpu(case, &input) {
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
        let is_golden = case.width == case.height
            && matches!(case.width, 8 | 56 | 64 | 128)
            && case.params.spread == 256
            && case.params.pad == 8
            && matches!(case.input, super::case::InputKind::GradientH)
            && is_bw_p2;
        if is_golden {
            let algo = match case.algorithm {
                Algorithm::BlockedFs => "bfs",
                Algorithm::BlueNoise => "bn",
            };
            let key = format!("{algo}_{}x{}_p2_grad_h_cpu", case.width, case.height);
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
            match run_gpu(&device, &queue, case, &input_bytes, &cancel, None, None) {
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
                        match run_gpu(
                            &device,
                            &queue,
                            case,
                            &input_bytes,
                            &cancel,
                            None,
                            Some(h),
                        ) {
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
            match run_gpu(
                &device,
                &queue,
                case,
                &input_bytes,
                &flag,
                Some(1),
                Some(8),
            ) {
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
    for g in REQUIRED_NO_SKIP_GROUPS {
        let sk = stats.by_group.get(*g).map(|s| s.skipped).unwrap_or(0);
        if sk > 0 {
            eprintln!("REQUIRED group {g}: skipped={sk} (must be 0)");
            required_skip_fail = true;
        } else if matches!(mode, ParityMode::Full | ParityMode::Nightly)
            && stats.by_group.get(*g).map(|s| s.passed + s.failed).unwrap_or(0) == 0
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
