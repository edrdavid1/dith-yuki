//! Startup GPU self-test for `blocked_fs` / `blue_noise` (spec Part B).
//!
//! Until [`SelftestState::Pass`], renders use the CPU reference (same bytes).
//! Cache key: adapter + driver + wgpu + shader hash + [`ALGO_VERSION`].

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::gpu_dither_cpu::{
    blocked_fs_cpu, blue_noise_cpu, blue_noise_ranks, with_pixel_size_blocks, PaletteRgb, Rgba8,
    CORE, DEFAULT_SPREAD, PAD,
};

/// Frozen until geometry pick / first external ship (Part A §3.8).
pub const ALGO_VERSION: u32 = 1;

/// Wall-clock budget for the whole suite on the self-test thread (spec §2.6).
pub const SELFTEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Mini-image side: ≥3×3 cores of current geometry + ragged edge (spec §2.2).
pub fn selftest_image_side() -> u32 {
    3 * CORE + (CORE / 4).max(7)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailReason {
    Mismatch,
    Timeout,
    ShaderCompile,
    Limits,
    SoftwareAdapter,
    Error,
}

impl FailReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Mismatch => "mismatch",
            Self::Timeout => "timeout",
            Self::ShaderCompile => "shader_compile",
            Self::Limits => "limits",
            Self::SoftwareAdapter => "software_adapter",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelftestState {
    Unknown,
    Pass,
    Fail { reason: FailReason },
    Unavailable,
    Lost,
}

impl SelftestState {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Pass => "pass",
            Self::Fail { .. } => "fail",
            Self::Unavailable => "unavailable",
            Self::Lost => "lost",
        }
    }

    pub fn reason_str(&self) -> Option<&'static str> {
        match self {
            Self::Fail { reason } => Some(reason.as_str()),
            _ => None,
        }
    }

    /// GPU path is allowed only after a successful self-test.
    pub fn allows_gpu(&self) -> bool {
        matches!(self, Self::Pass)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelftestSnapshot {
    pub state: SelftestState,
    pub backend: Option<String>,
    pub adapter: Option<String>,
    pub cache_hit: bool,
    pub checked_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheKey {
    pub adapter_name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub wgpu_version: String,
    pub shader_hash: String,
    pub algo_version: u32,
}

impl CacheKey {
    pub fn from_fingerprint(fp: &engine_gpu::AdapterFingerprint) -> Self {
        Self {
            adapter_name: fp.name.clone(),
            backend: fp.backend.clone(),
            device_type: fp.device_type.clone(),
            driver: fp.driver.clone(),
            driver_info: fp.driver_info.clone(),
            wgpu_version: engine_gpu::wgpu_version_label().to_string(),
            shader_hash: engine_gpu::gpu_family_shader_hash(),
            algo_version: ALGO_VERSION,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheFile {
    pub key: CacheKey,
    pub state: SelftestState,
    pub checked_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MismatchReport {
    pub adapter: Option<String>,
    pub driver: Option<String>,
    pub backend: Option<String>,
    pub case: String,
    pub first_diffs: Vec<MismatchPixel>,
    pub checked_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MismatchPixel {
    pub x: u32,
    pub y: u32,
    pub cpu: [u8; 4],
    pub gpu: [u8; 4],
}

struct Gate {
    state: Mutex<SelftestState>,
    checked_at: AtomicU64,
    cache_hit: AtomicBool,
    backend: Mutex<Option<String>>,
    adapter: Mutex<Option<String>>,
    /// Counts how many times the full suite actually ran (tests).
    run_count: AtomicU64,
    /// When true, flip one GPU output byte during compare (debug/tests).
    corrupt_inject: AtomicBool,
    /// Artificial sleep inside the suite (tests / timeout).
    artificial_delay_ms: AtomicU64,
    /// Optional forced fingerprint for tests (software / no adapter).
    forced_fp: Mutex<Option<ForcedAdapter>>,
}

#[derive(Clone, Debug)]
enum ForcedAdapter {
    None,
    Software,
}

fn gate() -> &'static Gate {
    static G: OnceLock<Gate> = OnceLock::new();
    G.get_or_init(|| Gate {
        state: Mutex::new(SelftestState::Unavailable),
        checked_at: AtomicU64::new(0),
        cache_hit: AtomicBool::new(false),
        backend: Mutex::new(None),
        adapter: Mutex::new(None),
        run_count: AtomicU64::new(0),
        corrupt_inject: AtomicBool::new(false),
        artificial_delay_ms: AtomicU64::new(0),
        forced_fp: Mutex::new(None),
    })
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Current gate snapshot (for IPC / footer).
pub fn snapshot() -> SelftestSnapshot {
    let g = gate();
    let state = g
        .state
        .lock()
        .map(|s| s.clone())
        .unwrap_or(SelftestState::Unavailable);
    let checked = g.checked_at.load(Ordering::Relaxed);
    SelftestSnapshot {
        state,
        backend: g.backend.lock().ok().and_then(|b| b.clone()),
        adapter: g.adapter.lock().ok().and_then(|a| a.clone()),
        cache_hit: g.cache_hit.load(Ordering::Relaxed),
        checked_at: if checked == 0 { None } else { Some(checked) },
    }
}

/// True iff GPU family path may run (Pass only).
pub fn gpu_path_allowed() -> bool {
    gate().state.lock().map(|s| s.allows_gpu()).unwrap_or(false)
}

pub fn set_state(state: SelftestState) {
    if let Ok(mut g) = gate().state.lock() {
        *g = state;
    }
}

pub fn mark_lost() {
    set_state(SelftestState::Lost);
}

pub fn mark_unknown() {
    set_state(SelftestState::Unknown);
    gate().cache_hit.store(false, Ordering::Relaxed);
}

pub fn run_count() -> u64 {
    gate().run_count.load(Ordering::Relaxed)
}

/// Debug/test: flip one GPU byte during the next suite compare.
fn suite_lock() -> std::sync::MutexGuard<'static, ()> {
    static SUITE: Mutex<()> = Mutex::new(());
    SUITE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn set_corrupt_inject(on: bool) {
    if cfg!(debug_assertions) || cfg!(test) {
        let _g = suite_lock();
        gate().corrupt_inject.store(on, Ordering::Relaxed);
    }
}

pub fn set_artificial_delay_ms(ms: u64) {
    if cfg!(debug_assertions) || cfg!(test) {
        let _g = suite_lock();
        gate().artificial_delay_ms.store(ms, Ordering::Relaxed);
    }
}

pub fn force_no_adapter_for_test() {
    if cfg!(debug_assertions) || cfg!(test) {
        let _g = suite_lock();
        if let Ok(mut g) = gate().forced_fp.lock() {
            *g = Some(ForcedAdapter::None);
        }
    }
}

pub fn force_software_adapter_for_test() {
    if cfg!(debug_assertions) || cfg!(test) {
        let _g = suite_lock();
        if let Ok(mut g) = gate().forced_fp.lock() {
            *g = Some(ForcedAdapter::Software);
        }
    }
}

pub fn clear_test_hooks() {
    if cfg!(debug_assertions) || cfg!(test) {
        let _g = suite_lock();
        gate().corrupt_inject.store(false, Ordering::Relaxed);
        gate().artificial_delay_ms.store(0, Ordering::Relaxed);
        if let Ok(mut g) = gate().forced_fp.lock() {
            *g = None;
        }
    }
}

/// Parse a cache file; corrupt/unreadable → `None` (silent re-check).
pub fn parse_cache_bytes(bytes: &[u8]) -> Option<CacheFile> {
    serde_json::from_slice(bytes).ok()
}

pub fn cache_matches(file: &CacheFile, key: &CacheKey) -> bool {
    file.key == *key
}

/// Run the suite (or accept a matching cache). Returns the new state and optional mismatch report.
pub fn run_or_use_cache(
    cached: Option<CacheFile>,
) -> (SelftestState, bool, Option<MismatchReport>) {
    // Serialize suite + test hooks (inject/delay/forced adapter) across threads.
    let _suite = suite_lock();

    let g = gate();
    let forced = g.forced_fp.lock().ok().and_then(|f| f.clone());

    match forced {
        Some(ForcedAdapter::None) => {
            set_meta(None, None);
            set_state(SelftestState::Unavailable);
            g.checked_at.store(now_unix(), Ordering::Relaxed);
            g.cache_hit.store(false, Ordering::Relaxed);
            return (SelftestState::Unavailable, false, None);
        }
        Some(ForcedAdapter::Software) => {
            let state = SelftestState::Fail {
                reason: FailReason::SoftwareAdapter,
            };
            set_meta(Some("Cpu".into()), Some("software-test".into()));
            set_state(state.clone());
            g.checked_at.store(now_unix(), Ordering::Relaxed);
            g.cache_hit.store(false, Ordering::Relaxed);
            return (state, false, None);
        }
        None => {}
    }

    // Probe first so limits-fail is Fail(limits), not Unavailable.
    match engine_gpu::probe_blocked_fs_adapter() {
        engine_gpu::BlockedFsAdapterProbe::NoAdapter => {
            set_meta(None, None);
            set_state(SelftestState::Unavailable);
            g.checked_at.store(now_unix(), Ordering::Relaxed);
            g.cache_hit.store(false, Ordering::Relaxed);
            return (SelftestState::Unavailable, false, None);
        }
        engine_gpu::BlockedFsAdapterProbe::Limits { fingerprint, .. } => {
            set_meta(
                Some(fingerprint.backend.clone()),
                Some(fingerprint.name.clone()),
            );
            let state = SelftestState::Fail {
                reason: FailReason::Limits,
            };
            let key = CacheKey::from_fingerprint(&fingerprint);
            if let Some(c) = cached.as_ref() {
                if cache_matches(c, &key) {
                    set_state(c.state.clone());
                    g.checked_at.store(c.checked_at, Ordering::Relaxed);
                    g.cache_hit.store(true, Ordering::Relaxed);
                    return (c.state.clone(), true, None);
                }
            }
            set_state(state.clone());
            g.checked_at.store(now_unix(), Ordering::Relaxed);
            g.cache_hit.store(false, Ordering::Relaxed);
            return (state, false, None);
        }
        engine_gpu::BlockedFsAdapterProbe::Ok { fingerprint, .. } => {
            set_meta(
                Some(fingerprint.backend.clone()),
                Some(fingerprint.name.clone()),
            );
            let key = CacheKey::from_fingerprint(&fingerprint);

            if fingerprint.is_software() && !engine_gpu::allow_software_adapter() {
                let state = SelftestState::Fail {
                    reason: FailReason::SoftwareAdapter,
                };
                set_state(state.clone());
                g.checked_at.store(now_unix(), Ordering::Relaxed);
                g.cache_hit.store(false, Ordering::Relaxed);
                return (state, false, None);
            }

            if let Some(c) = cached {
                if cache_matches(&c, &key) {
                    set_state(c.state.clone());
                    g.checked_at.store(c.checked_at, Ordering::Relaxed);
                    g.cache_hit.store(true, Ordering::Relaxed);
                    return (c.state, true, None);
                }
            }

            g.cache_hit.store(false, Ordering::Relaxed);
            g.run_count.fetch_add(1, Ordering::Relaxed);

            let delay = g.artificial_delay_ms.load(Ordering::Relaxed);
            if delay > 0 {
                std::thread::sleep(Duration::from_millis(delay));
            }

            // Ensure device cache is warm before the suite.
            if engine_gpu::cached_blocked_fs_device().is_none() {
                let state = SelftestState::Fail {
                    reason: FailReason::Error,
                };
                set_state(state.clone());
                g.checked_at.store(now_unix(), Ordering::Relaxed);
                return (state, false, None);
            }

            match run_suite_inner(g.corrupt_inject.load(Ordering::Relaxed)) {
                Ok(()) => {
                    let state = SelftestState::Pass;
                    set_state(state.clone());
                    g.checked_at.store(now_unix(), Ordering::Relaxed);
                    (state, false, None)
                }
                Err(report) => {
                    let state = SelftestState::Fail {
                        reason: FailReason::Mismatch,
                    };
                    set_state(state.clone());
                    g.checked_at.store(now_unix(), Ordering::Relaxed);
                    (state, false, Some(*report))
                }
            }
        }
    }
}

fn set_meta(backend: Option<String>, adapter: Option<String>) {
    let g = gate();
    if let Ok(mut b) = g.backend.lock() {
        *b = backend;
    }
    if let Ok(mut a) = g.adapter.lock() {
        *a = adapter;
    }
}

fn run_suite_inner(corrupt: bool) -> Result<(), Box<MismatchReport>> {
    let Some((device, queue, label)) = engine_gpu::cached_blocked_fs_device() else {
        return Err(Box::new(MismatchReport {
            adapter: None,
            driver: None,
            backend: None,
            case: "no_device".into(),
            first_diffs: vec![],
            checked_at: now_unix(),
        }));
    };
    let side = selftest_image_side();
    let cancel = AtomicBool::new(false);
    let fp = engine_gpu::cached_adapter_fingerprint();

    for case in mini_cases(side) {
        let cpu = run_cpu_case(&case)?;
        let mut gpu = run_gpu_case(device, queue, &case, &cancel).map_err(|e| {
            Box::new(MismatchReport {
                adapter: fp.as_ref().map(|f| f.name.clone()),
                driver: fp.as_ref().map(|f| f.driver.clone()),
                backend: Some(label.clone()),
                case: format!("{}:{e}", case.name),
                first_diffs: vec![],
                checked_at: now_unix(),
            })
        })?;
        if corrupt && !gpu.is_empty() {
            gpu[0] ^= 0xff;
        }
        if let Some(report) = compare_bytes(&case, &cpu, &gpu, label, fp.as_ref()) {
            return Err(Box::new(report));
        }
    }
    Ok(())
}

struct MiniCase {
    name: String,
    width: u32,
    height: u32,
    pixels: Vec<Rgba8>,
    palette: Vec<PaletteRgb>,
    algo: Algo,
    pixel_size: u32,
}

#[derive(Clone, Copy)]
enum Algo {
    BlockedFs,
    BlueNoise,
}

fn mini_cases(side: u32) -> Vec<MiniCase> {
    let mut out = Vec::new();
    // Spec §2.2: 2 / 16 / 257 colors + ties; gradient / noise / gray / OOG / alpha.
    let palettes: Vec<(&str, Vec<PaletteRgb>)> = vec![
        ("pal2", bilevel()),
        ("pal16", grayscale_n(16)),
        ("pal257", grayscale_n(257)),
        ("ties", tie_palette()),
    ];

    let inputs: Vec<(&str, Vec<Rgba8>)> = vec![
        ("grad", gradient(side, side)),
        ("noise", noise(side, side, 0xC0FFEE)),
        ("gray", solid(side, side, 128, 128, 128, 255)),
        ("oog", solid(side, side, 40, 200, 90, 255)),
        ("alpha", alpha_ramp(side, side)),
    ];

    for algo in [Algo::BlockedFs, Algo::BlueNoise] {
        let aname = match algo {
            Algo::BlockedFs => "blocked_fs",
            Algo::BlueNoise => "blue_noise",
        };
        for (pname, pal) in &palettes {
            for (iname, px) in &inputs {
                for ps in [1u32, 5u32] {
                    out.push(MiniCase {
                        name: format!("{aname}/{pname}/{iname}/ps{ps}"),
                        width: side,
                        height: side,
                        pixels: px.clone(),
                        palette: pal.clone(),
                        algo,
                        pixel_size: ps,
                    });
                }
            }
        }
    }
    out
}

fn bilevel() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ]
}

fn grayscale_n(n: u16) -> Vec<PaletteRgb> {
    let n = n.max(2) as u32;
    let denom = (n - 1).max(1);
    (0..n)
        .map(|i| {
            let v = ((i * 255) / denom) as u8;
            PaletteRgb { r: v, g: v, b: v }
        })
        .collect()
}

fn tie_palette() -> Vec<PaletteRgb> {
    // Two identical entries force distance ties (nearest picks first).
    vec![
        PaletteRgb {
            r: 10,
            g: 20,
            b: 30,
        },
        PaletteRgb {
            r: 10,
            g: 20,
            b: 30,
        },
        PaletteRgb {
            r: 200,
            g: 200,
            b: 200,
        },
    ]
}

fn gradient(w: u32, h: u32) -> Vec<Rgba8> {
    let mut v = Vec::with_capacity((w * h) as usize);
    let denom = w.max(2) - 1;
    for y in 0..h {
        for x in 0..w {
            let g = ((x * 255) / denom) as u8;
            let a = if y % 3 == 0 { 255 } else { 200 };
            v.push(Rgba8::new(g, g, g, a));
        }
    }
    v
}

fn solid(w: u32, h: u32, r: u8, g: u8, b: u8, a: u8) -> Vec<Rgba8> {
    vec![Rgba8::new(r, g, b, a); (w * h) as usize]
}

fn noise(w: u32, h: u32, seed: u64) -> Vec<Rgba8> {
    let mut s = seed;
    let mut v = Vec::with_capacity((w * h) as usize);
    for _ in 0..(w * h) {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1);
        let x = (s >> 33) as u8;
        v.push(Rgba8::new(x, x.wrapping_mul(3), x.wrapping_mul(7), 255));
    }
    v
}

fn alpha_ramp(w: u32, h: u32) -> Vec<Rgba8> {
    let mut v = Vec::with_capacity((w * h) as usize);
    let denom = (w * h).max(2) - 1;
    for i in 0..(w * h) {
        let a = ((i * 255) / denom) as u8;
        v.push(Rgba8::new(180, 90, 40, a));
    }
    v
}

fn run_cpu_case(case: &MiniCase) -> Result<Vec<u8>, Box<MismatchReport>> {
    let out = with_pixel_size_blocks(
        &case.pixels,
        case.width,
        case.height,
        case.pixel_size,
        |small, sw, sh| match case.algo {
            Algo::BlockedFs => blocked_fs_cpu(small, sw, sh, &case.palette, CORE, CORE, PAD),
            Algo::BlueNoise => blue_noise_cpu(
                small,
                sw,
                sh,
                &case.palette,
                blue_noise_ranks(),
                DEFAULT_SPREAD,
            ),
        },
    )
    .map_err(|e| {
        Box::new(MismatchReport {
            adapter: None,
            driver: None,
            backend: None,
            case: format!("{}:cpu:{e}", case.name),
            first_diffs: vec![],
            checked_at: now_unix(),
        })
    })?;
    Ok(rgba8_to_bytes(&out))
}

fn run_gpu_case(
    device: &engine_gpu::wgpu::Device,
    queue: &engine_gpu::wgpu::Queue,
    case: &MiniCase,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    let ps = case.pixel_size.max(1);
    let (tw, th, lattice) = if ps <= 1 {
        (case.width, case.height, None)
    } else {
        let bw = case.width.div_ceil(ps);
        let bh = case.height.div_ceil(ps);
        let w = case.width as usize;
        let mut small = vec![Rgba8::new(0, 0, 0, 0); (bw * bh) as usize];
        for by in 0..bh {
            for bx in 0..bw {
                small[(by * bw + bx) as usize] =
                    case.pixels[(by * ps) as usize * w + (bx * ps) as usize];
            }
        }
        (bw, bh, Some(small))
    };
    let src = lattice.as_deref().unwrap_or(&case.pixels);
    let in_b = rgba8_to_bytes(src);
    let pal_b: Vec<[u8; 4]> = case.palette.iter().map(|p| [p.r, p.g, p.b, 255]).collect();

    let gpu = match case.algo {
        Algo::BlockedFs => engine_gpu::apply_blocked_fs_gpu_opts(
            device,
            queue,
            &in_b,
            tw,
            th,
            &pal_b,
            cancel,
            engine_gpu::BlockedFsGpuOpts::default(),
        ),
        Algo::BlueNoise => engine_gpu::apply_blue_noise_gpu_opts(
            device,
            queue,
            &in_b,
            tw,
            th,
            &pal_b,
            blue_noise_ranks(),
            DEFAULT_SPREAD,
            cancel,
            engine_gpu::BlueNoiseGpuOpts::default(),
        ),
    }
    .map_err(|e| e.to_string())?;

    if ps <= 1 {
        Ok(gpu.rgba8)
    } else {
        let dithered = bytes_to_rgba8(&gpu.rgba8);
        let bw = tw as usize;
        let w = case.width as usize;
        let h = case.height as usize;
        let mut out = vec![Rgba8::new(0, 0, 0, 0); w * h];
        for y in 0..h {
            for x in 0..w {
                let by = y / ps as usize;
                let bx = x / ps as usize;
                let p = dithered[by * bw + bx];
                out[y * w + x] = Rgba8::new(p.r, p.g, p.b, case.pixels[y * w + x].a);
            }
        }
        Ok(rgba8_to_bytes(&out))
    }
}

fn compare_bytes(
    case: &MiniCase,
    cpu: &[u8],
    gpu: &[u8],
    label: &str,
    fp: Option<&engine_gpu::AdapterFingerprint>,
) -> Option<MismatchReport> {
    if cpu.len() != gpu.len() {
        return Some(MismatchReport {
            adapter: fp.map(|f| f.name.clone()),
            driver: fp.map(|f| f.driver.clone()),
            backend: Some(label.to_string()),
            case: format!("{}:len {} vs {}", case.name, cpu.len(), gpu.len()),
            first_diffs: vec![],
            checked_at: now_unix(),
        });
    }
    let w = case.width as usize;
    let mut diffs = Vec::new();
    for (i, (c, g)) in cpu
        .as_chunks::<4>()
        .0
        .iter()
        .zip(gpu.as_chunks::<4>().0.iter())
        .enumerate()
    {
        if c != g {
            let x = (i % w) as u32;
            let y = (i / w) as u32;
            diffs.push(MismatchPixel {
                x,
                y,
                cpu: [c[0], c[1], c[2], c[3]],
                gpu: [g[0], g[1], g[2], g[3]],
            });
            if diffs.len() >= 16 {
                break;
            }
        }
    }
    if diffs.is_empty() {
        None
    } else {
        Some(MismatchReport {
            adapter: fp.map(|f| f.name.clone()),
            driver: fp.map(|f| f.driver.clone()),
            backend: Some(label.to_string()),
            case: case.name.clone(),
            first_diffs: diffs,
            checked_at: now_unix(),
        })
    }
}

fn rgba8_to_bytes(px: &[Rgba8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(px.len() * 4);
    for p in px {
        v.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    v
}

fn bytes_to_rgba8(bytes: &[u8]) -> Vec<Rgba8> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| Rgba8::new(c[0], c[1], c[2], c[3]))
        .collect()
}

/// Build a [`CacheFile`] for the current gate state (caller writes to disk).
pub fn cache_file_for_current(key: CacheKey) -> Option<CacheFile> {
    let snap = snapshot();
    match &snap.state {
        SelftestState::Pass | SelftestState::Fail { .. } => Some(CacheFile {
            key,
            state: snap.state,
            checked_at: snap.checked_at.unwrap_or_else(now_unix),
        }),
        _ => None,
    }
}

/// Timed wrapper: if `run` exceeds [`SELFTEST_TIMEOUT`], set Fail(timeout).
pub fn run_with_timeout<F>(f: F) -> (SelftestState, bool, Option<MismatchReport>)
where
    F: FnOnce() -> (SelftestState, bool, Option<MismatchReport>) + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("gpu-selftest".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .expect("spawn gpu-selftest");

    match rx.recv_timeout(SELFTEST_TIMEOUT) {
        Ok(r) => r,
        Err(_) => {
            let state = SelftestState::Fail {
                reason: FailReason::Timeout,
            };
            set_state(state.clone());
            gate().checked_at.store(now_unix(), Ordering::Relaxed);
            gate().cache_hit.store(false, Ordering::Relaxed);
            // Leave the worker thread running (spec §2.6).
            (state, false, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_side_covers_three_cores_plus_ragged() {
        let s = selftest_image_side();
        assert!(s >= 3 * CORE);
        assert_ne!(s % 5, 0, "side must not be multiple of ps=5");
    }

    #[test]
    fn corrupt_cache_parses_as_none() {
        assert!(parse_cache_bytes(b"not-json").is_none());
        assert!(parse_cache_bytes(b"{}").is_none());
    }

    #[test]
    fn selftest_no_adapter() {
        clear_test_hooks();
        force_no_adapter_for_test();
        let (st, hit, _) = run_or_use_cache(None);
        assert_eq!(st, SelftestState::Unavailable);
        assert!(!hit);
        assert!(!gpu_path_allowed());
        clear_test_hooks();
    }

    #[test]
    fn selftest_software_adapter() {
        clear_test_hooks();
        force_software_adapter_for_test();
        let (st, _, _) = run_or_use_cache(None);
        assert_eq!(
            st,
            SelftestState::Fail {
                reason: FailReason::SoftwareAdapter
            }
        );
        assert!(!gpu_path_allowed());
        clear_test_hooks();
    }

    #[test]
    fn selftest_cache_corrupt() {
        assert!(parse_cache_bytes(b"{not valid").is_none());
    }

    #[test]
    #[ignore = "needs GPU adapter: cargo test -p engine-project gpu_selftest -- --ignored"]
    fn selftest_pass_normal() {
        clear_test_hooks();
        mark_unknown();
        let before = run_count();
        let (st, hit, report) = run_or_use_cache(None);
        assert!(report.is_none(), "{report:?}");
        assert_eq!(st, SelftestState::Pass, "hit={hit}");
        assert!(gpu_path_allowed());
        assert!(run_count() > before);

        let fp = engine_gpu::cached_adapter_fingerprint().expect("adapter");
        let key = CacheKey::from_fingerprint(&fp);
        let file = CacheFile {
            key: key.clone(),
            state: SelftestState::Pass,
            checked_at: 1,
        };
        let before2 = run_count();
        let (st2, hit2, _) = run_or_use_cache(Some(file));
        assert_eq!(st2, SelftestState::Pass);
        assert!(hit2);
        assert_eq!(run_count(), before2, "cache hit must not re-run suite");

        // Driver string change → miss.
        let mut key2 = key;
        key2.driver_info = "mutated-for-test".into();
        let miss = CacheFile {
            key: key2,
            state: SelftestState::Pass,
            checked_at: 1,
        };
        let before3 = run_count();
        let _ = run_or_use_cache(Some(miss));
        assert!(run_count() > before3);
        clear_test_hooks();
    }

    #[test]
    #[ignore = "needs GPU adapter"]
    fn selftest_detects_corrupt_byte() {
        clear_test_hooks();
        set_corrupt_inject(true);
        let (st, _, report) = run_or_use_cache(None);
        assert_eq!(
            st,
            SelftestState::Fail {
                reason: FailReason::Mismatch
            }
        );
        assert!(report.is_some());
        assert!(!gpu_path_allowed());
        clear_test_hooks();
    }

    #[test]
    fn routing_fail_never_allows_gpu() {
        set_state(SelftestState::Fail {
            reason: FailReason::Mismatch,
        });
        assert!(!gpu_path_allowed());
        set_state(SelftestState::Lost);
        assert!(!gpu_path_allowed());
        set_state(SelftestState::Unknown);
        assert!(!gpu_path_allowed());
    }

    #[test]
    fn routing_unknown_does_not_allow_gpu() {
        mark_unknown();
        assert!(!gpu_path_allowed());
        set_state(SelftestState::Pass);
        assert!(gpu_path_allowed());
        set_state(SelftestState::Unavailable);
    }

    #[test]
    fn selftest_timeout() {
        clear_test_hooks();
        set_artificial_delay_ms(6_000);
        // Force a path that actually sleeps inside run_or_use_cache after
        // adapter checks: use software force which returns before delay.
        // Instead wrap a sleeping closure.
        let (st, _, _) = run_with_timeout(|| {
            std::thread::sleep(Duration::from_secs(6));
            (SelftestState::Pass, false, None)
        });
        assert_eq!(
            st,
            SelftestState::Fail {
                reason: FailReason::Timeout
            }
        );
        clear_test_hooks();
    }
}
