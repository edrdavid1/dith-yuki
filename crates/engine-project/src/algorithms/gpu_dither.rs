//! `blocked_fs` and `blue_noise` — approximate GPU dither families (`ED-on-GPU.md`).
//!
//! CPU reference is canonical. GPU path is `GpuEligibility::Conditional` and
//! falls back to this CPU reference when the probe / flag / limits fail.
//!
//! Dev path override: `DITHER_GPU_PATH=auto|gpu|cpu` (default `auto`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use engine_color::palette::linear_to_srgb;
use engine_registry::{
    AlgorithmId, EffectCategory, ExecutionScope, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField, TemporalStability,
};
use engine_tiles::{PixelTile, HALO, TILE_SIZE};
use rayon::prelude::*;
use serde::Serialize;

use crate::document::Document;
use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::context::FilterContext;

use super::gpu_dither_cpu::{
    blocked_fs_cpu, blue_noise_cpu, blue_noise_ranks, levels_palette, with_pixel_size_blocks,
    PaletteRgb, Rgba8, CORE, DEFAULT_SPREAD, PAD,
};
use super::scratch::scratch_src;

/// Last full-document apply status (preview footer / logs).
#[derive(Clone, Debug, Serialize)]
pub struct GpuDitherStatus {
    pub algorithm: String,
    /// `"GPU"` or `"CPU"`.
    pub path: String,
    /// Backend label, e.g. `"Metal"`, `"Vulkan"`, or `"cpu"`.
    pub backend: String,
    pub elapsed_ms: u32,
    pub strips: u32,
    pub width: u32,
    pub height: u32,
    pub pixel_size: u32,
    /// `"cached"` / `"new"` on GPU; `"n/a"` on CPU.
    pub pipeline: String,
    pub prepare_ms: f64,
    pub upload_ms: f64,
    pub dispatch_ms: f64,
    pub readback_ms: f64,
    pub post_ms: f64,
    /// Host stages inside [`apply_gpu_dither_rgba`] (ms). Sum ≈ `elapsed_ms`.
    pub in_f32_to_u8_ms: f64,
    pub setup_hash_ms: f64,
    pub lattice_ps_ms: f64,
    pub pack_upload_bytes_ms: f64,
    pub expand_ps_ms: f64,
    pub out_bytes_to_rgba8_ms: f64,
    pub out_srgb_to_linear_ms: f64,
    pub apply_misc_ms: f64,
}

static LAST_STATUS: Mutex<Option<GpuDitherStatus>> = Mutex::new(None);
/// Survives [`take_last_gpu_dither_status`] so diagnostics can still peek after emit.
static LAST_STATUS_HOLD: Mutex<Option<GpuDitherStatus>> = Mutex::new(None);
static REQ_SEQ: AtomicU64 = AtomicU64::new(1);

/// Log-only fingerprint (never persisted / not a cache key). Blake3 is fast and
/// internally parallel; safe to change because nothing on disk stores this value.
fn hash_input_fingerprint(input: &[Rgba8]) -> u64 {
    // Log-only. `Rgba8` is `repr(C)` — hash the packed bytes in place (no copy).
    let bytes = unsafe { std::slice::from_raw_parts(input.as_ptr() as *const u8, input.len() * 4) };
    let digest = blake3::hash(bytes);
    let mut out = [0u8; 8];
    out.copy_from_slice(&digest.as_bytes()[..8]);
    u64::from_le_bytes(out)
}

/// Take the status from the most recent [`apply_gpu_dither_rgba`] call.
pub fn take_last_gpu_dither_status() -> Option<GpuDitherStatus> {
    LAST_STATUS.lock().ok().and_then(|mut g| g.take())
}

/// Peek without clearing (tests / debug). Prefers the take-able slot, else hold.
pub fn last_gpu_dither_status() -> Option<GpuDitherStatus> {
    if let Ok(g) = LAST_STATUS.lock() {
        if g.is_some() {
            return g.clone();
        }
    }
    LAST_STATUS_HOLD.lock().ok().and_then(|g| g.clone())
}

fn record_status(status: GpuDitherStatus) {
    if let Ok(mut g) = LAST_STATUS.lock() {
        *g = Some(status.clone());
    }
    if let Ok(mut g) = LAST_STATUS_HOLD.lock() {
        *g = Some(status);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathChoice {
    Auto,
    ForceGpu,
    ForceCpu,
}

/// `DITHER_GPU_PATH=gpu` may bypass the self-test gate only in debug builds and
/// `cfg!(test)` unit-test binaries. Release app / release lib used by integration
/// tests must not skip Fail/Unknown — set Pass (or use `auto`) instead.
fn allow_force_gpu_bypass() -> bool {
    cfg!(debug_assertions) || cfg!(test)
}

fn path_choice() -> PathChoice {
    match std::env::var("DITHER_GPU_PATH")
        .unwrap_or_else(|_| "auto".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "gpu" if allow_force_gpu_bypass() => PathChoice::ForceGpu,
        // Release: honor the env as a preference only when the gate already allows GPU.
        "gpu" if crate::algorithms::gpu_selftest::gpu_path_allowed() => PathChoice::ForceGpu,
        "gpu" => PathChoice::Auto,
        "cpu" => PathChoice::ForceCpu,
        _ => PathChoice::Auto,
    }
}

fn backend_short(label: &str) -> String {
    // label looks like "Apple M3 (Metal)" — prefer the parenthetical.
    if let Some(start) = label.rfind('(') {
        if let Some(end) = label.rfind(')') {
            if end > start {
                return label[start + 1..end].to_string();
            }
        }
    }
    label.to_string()
}

fn rgba8_to_bytes_par(px: &[Rgba8]) -> Vec<u8> {
    let mut v = vec![0u8; px.len() * 4];
    crate::pixel_pool::install(|| {
        v.par_chunks_exact_mut(4)
            .zip(px.par_iter())
            .for_each(|(slot, p)| {
                slot[0] = p.r;
                slot[1] = p.g;
                slot[2] = p.b;
                slot[3] = p.a;
            });
    });
    v
}

fn bytes_to_rgba8(bytes: &[u8]) -> Vec<Rgba8> {
    let n = bytes.len() / 4;
    let mut out = vec![Rgba8::new(0, 0, 0, 0); n];
    crate::pixel_pool::install(|| {
        out.par_iter_mut()
            .zip(bytes.par_chunks_exact(4))
            .for_each(|(px, c)| {
                *px = Rgba8::new(c[0], c[1], c[2], c[3]);
            });
    });
    out
}

fn palette_rgba(palette: &[PaletteRgb]) -> Vec<[u8; 4]> {
    palette.iter().map(|p| [p.r, p.g, p.b, 255]).collect()
}

fn algo_name(mode: &DitherModeV2) -> &'static str {
    match mode {
        DitherModeV2::BlockedFs => "blocked_fs",
        DitherModeV2::BlueNoise => "blue_noise",
        _ => "gpu_dither",
    }
}

const BLOCKED_FS_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "pixel_size",
        label: "Pixel Size",
        min: 1.0,
        max: 32.0,
        default: 1.0,
        step: Some(1.0),
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[("rgb", "RGB"), ("grayscale", "Grayscale")],
        default: "rgb",
    },
];

const BLUE_NOISE_SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "threshold_scale",
        label: "Spread",
        min: 0.0,
        max: 4.0,
        default: 1.0,
        step: None,
    },
    ParamField::Slider {
        key: "pixel_size",
        label: "Pixel Size",
        min: 1.0,
        max: 32.0,
        default: 1.0,
        step: Some(1.0),
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[("rgb", "RGB"), ("grayscale", "Grayscale")],
        default: "rgb",
    },
];

fn tile_to_rgba8(src: &PixelTile) -> Vec<Rgba8> {
    let w = TILE_SIZE as usize;
    let h = TILE_SIZE as usize;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let sx = HALO as usize + x;
            let sy = HALO as usize + y;
            let r = linear_to_srgb(src.at(sx as u32, sy as u32, 0));
            let g = linear_to_srgb(src.at(sx as u32, sy as u32, 1));
            let b = linear_to_srgb(src.at(sx as u32, sy as u32, 2));
            let a = (src.at(sx as u32, sy as u32, 3).clamp(0.0, 1.0) * 255.0).round() as u8;
            out.push(Rgba8::new(r, g, b, a));
        }
    }
    out
}

fn write_rgba8_to_tile(dst: &mut PixelTile, pixels: &[Rgba8]) {
    let w = TILE_SIZE as usize;
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let p = pixels[(y as usize) * w + x as usize];
            let dx = HALO + x;
            let dy = HALO + y;
            dst.set(dx, dy, 0, engine_color::palette::srgb_to_linear(p.r));
            dst.set(dx, dy, 1, engine_color::palette::srgb_to_linear(p.g));
            dst.set(dx, dy, 2, engine_color::palette::srgb_to_linear(p.b));
            dst.set(dx, dy, 3, p.a as f32 / 255.0);
        }
    }
}

/// Resolve the palette for GPU ED families: document swatch if bound, otherwise
/// a synthetic levels palette (same idea as CPU ED’s uniform quantizer — not
/// hard black/white unless levels=2 grayscale).
pub fn resolve_gpu_dither_palette(
    params: &DitherParamsV2,
    doc: &Document,
) -> Result<Vec<PaletteRgb>, crate::error::EngineError> {
    if let Some(palette_id) = params.palette_id {
        let Some(pal) = doc.get_palette(palette_id) else {
            return Err(crate::error::EngineError::palette_not_found(palette_id));
        };
        if pal.colors.is_empty() {
            return Err(crate::error::EngineError::invalid_filter_params(
                "InvalidPalette: empty",
            ));
        }
        return Ok(pal
            .colors
            .iter()
            .map(|c| PaletteRgb {
                r: linear_to_srgb(c.r),
                g: linear_to_srgb(c.g),
                b: linear_to_srgb(c.b),
            })
            .collect());
    }
    Ok(levels_palette(params.levels, params.color_mode))
}

fn palette_from_ctx(
    ctx: &FilterContext<'_>,
    params: &DitherParamsV2,
) -> Result<Vec<PaletteRgb>, FilterError> {
    resolve_gpu_dither_palette(params, ctx.document).map_err(FilterError::from_engine)
}

fn spread_from_params(params: &DitherParamsV2) -> u32 {
    let s = (params.threshold_scale * DEFAULT_SPREAD as f32).round();
    s.clamp(0.0, 1024.0) as u32
}

/// Apply blocked FS or blue noise to a full RGBA f32 buffer (FullDocument path).
///
/// Path selection via `DITHER_GPU_PATH` (`auto` / `gpu` / `cpu`). On GPU failure
/// or missing adapter, falls back to the CPU reference (never panics).
fn ms_since(t0: Instant) -> f64 {
    t0.elapsed().as_secs_f64() * 1000.0
}

pub fn apply_gpu_dither_rgba(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    palette: &[PaletteRgb],
    should_cancel: &std::sync::atomic::AtomicBool,
) -> Result<(), FilterError> {
    let t_all = Instant::now();
    let n = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| FilterError::from_engine("gpu_dither: size overflow"))?;
    if rgba.len() < n * 4 {
        return Err(FilterError::from_engine("gpu_dither: buffer too small"));
    }
    if should_cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err(FilterError::from_engine("gpu_dither: cancelled"));
    }

    let t = Instant::now();
    let mut input = vec![Rgba8::new(0, 0, 0, 0); n];
    crate::pixel_pool::install(|| {
        input.par_iter_mut().enumerate().for_each(|(i, px)| {
            let o = i * 4;
            *px = Rgba8::new(
                linear_to_srgb(rgba[o]),
                linear_to_srgb(rgba[o + 1]),
                linear_to_srgb(rgba[o + 2]),
                (rgba[o + 3].clamp(0.0, 1.0) * 255.0).round() as u8,
            );
        });
    });
    let in_f32_to_u8_ms = ms_since(t);

    let t = Instant::now();
    let algo = algo_name(&params.mode);
    let choice = path_choice();
    let ps = params.pixel_size.max(1) as u32;
    let req = REQ_SEQ.fetch_add(1, Ordering::Relaxed);
    // `params_hash` / `input_hash` are log-only (not cache keys, not on disk).
    // Skip at Info and above — only compute when Debug/Trace is enabled for
    // `gpu_dither` (normal app logging stays at Info and must stay free).
    let log_detail = log::log_enabled!(target: "gpu_dither", log::Level::Debug);
    let (params_hash, input_hash, trigger, threads) = if log_detail {
        let params_hash = {
            let mut h = DefaultHasher::new();
            algo.hash(&mut h);
            ps.hash(&mut h);
            params.threshold_scale.to_bits().hash(&mut h);
            palette.len().hash(&mut h);
            h.finish()
        };
        let input_hash = hash_input_fingerprint(&input);
        let trigger = std::env::var("DITHER_GPU_TRIGGER").unwrap_or_else(|_| "other".into());
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        (params_hash, input_hash, trigger, threads)
    } else {
        (0u64, 0u64, String::new(), 0usize)
    };
    let setup_hash_ms = ms_since(t);

    let mut lattice_ps_ms = 0.0;
    let mut pack_upload_bytes_ms = 0.0;
    let mut expand_ps_ms = 0.0;
    let mut out_bytes_to_rgba8_ms = 0.0;

    let (out, path_label, backend, strips, phase_log, phases) = match choice {
        PathChoice::ForceCpu => {
            let t_cpu = Instant::now();
            let out = run_cpu(&input, width, height, params, palette)?;
            let total = ms_since(t_cpu);
            let log = format!(
                "{algo} | path=cpu | {width}x{height} | ps={ps} | strips=1 | pipeline=n/a | total={total:.0}ms\n  prepare=0 upload=0 dispatch=0 readback=0 post=0\n  req={req} params_hash={params_hash:x} input_hash={input_hash:x} trigger={trigger} cancelled=no threads={threads}"
            );
            (
                out,
                "CPU",
                "cpu".to_string(),
                1u32,
                log,
                engine_gpu::GpuPhaseTiming {
                    pipeline: "n/a",
                    ..Default::default()
                },
            )
        }
        PathChoice::Auto if !crate::algorithms::gpu_selftest::gpu_path_allowed() => {
            let reason = match crate::algorithms::gpu_selftest::snapshot().state {
                crate::algorithms::gpu_selftest::SelftestState::Unknown => "selftest_pending",
                crate::algorithms::gpu_selftest::SelftestState::Fail { .. } => "selftest_fail",
                crate::algorithms::gpu_selftest::SelftestState::Lost => "device_lost",
                crate::algorithms::gpu_selftest::SelftestState::Unavailable => {
                    "selftest_unavailable"
                }
                crate::algorithms::gpu_selftest::SelftestState::Pass => "selftest_gate",
            };
            let t_cpu = Instant::now();
            let out = run_cpu(&input, width, height, params, palette)?;
            let total = ms_since(t_cpu);
            let log = format!(
                "{algo} | path=cpu | {width}x{height} | ps={ps} | strips=1 | pipeline=n/a | total={total:.0}ms\n  prepare=0 upload=0 dispatch=0 readback=0 post=0\n  req={req} params_hash={params_hash:x} input_hash={input_hash:x} trigger={trigger} cancelled=no fallback={reason}"
            );
            (
                out,
                "CPU",
                "cpu".to_string(),
                1u32,
                log,
                engine_gpu::GpuPhaseTiming {
                    pipeline: "n/a",
                    ..Default::default()
                },
            )
        }
        PathChoice::Auto | PathChoice::ForceGpu => {
            match try_gpu_timed(
                &input,
                width,
                height,
                params,
                palette,
                should_cancel,
                &mut lattice_ps_ms,
                &mut pack_upload_bytes_ms,
                &mut expand_ps_ms,
            ) {
                Ok((bytes, backend, strips, timing)) => {
                    let t = Instant::now();
                    let out = bytes_to_rgba8(&bytes);
                    out_bytes_to_rgba8_ms = ms_since(t);
                    let total = ms_since(t_all);
                    let log = format!(
                        "{algo} | {backend} | {width}x{height} | ps={ps} | strips={strips} | pipeline={} | total={total:.0}ms\n  prepare={:.0} upload={:.0} dispatch={:.0} readback={:.0} post={:.0}\n  req={req} params_hash={params_hash:x} input_hash={input_hash:x} trigger={trigger} cancelled=no",
                        timing.pipeline,
                        timing.prepare_ms,
                        timing.upload_ms,
                        timing.dispatch_ms,
                        timing.readback_ms,
                        timing.post_ms,
                    );
                    (out, "GPU", backend, strips, log, timing)
                }
                Err(reason) => {
                    let cancelled = reason.contains("cancel");
                    if choice == PathChoice::ForceGpu {
                        log::warn!(
                            target: "gpu_dither",
                            "{algo}: DITHER_GPU_PATH=gpu failed ({reason}); falling back to CPU"
                        );
                    } else {
                        log::info!(
                            target: "gpu_dither",
                            "{algo}: GPU unavailable ({reason}); using CPU"
                        );
                    }
                    let t_cpu = Instant::now();
                    let out = run_cpu(&input, width, height, params, palette)?;
                    let total = ms_since(t_cpu);
                    let cflag = if cancelled { "yes" } else { "no" };
                    let log = format!(
                        "{algo} | path=cpu | {width}x{height} | ps={ps} | strips=1 | pipeline=n/a | total={total:.0}ms\n  prepare=0 upload=0 dispatch=0 readback=0 post=0\n  req={req} params_hash={params_hash:x} input_hash={input_hash:x} trigger={trigger} cancelled={cflag} fallback={reason}"
                    );
                    (
                        out,
                        "CPU",
                        "cpu".to_string(),
                        1u32,
                        log,
                        engine_gpu::GpuPhaseTiming {
                            pipeline: "n/a",
                            ..Default::default()
                        },
                    )
                }
            }
        }
    };

    if should_cancel.load(Ordering::Acquire) {
        log::info!(
            target: "gpu_dither",
            "{algo} | req={req} params_hash={params_hash:x} input_hash={input_hash:x} trigger={trigger} cancelled=yes"
        );
        return Err(FilterError::from_engine("gpu_dither: cancelled"));
    }

    let t = Instant::now();
    crate::pixel_pool::install(|| {
        rgba.par_chunks_exact_mut(4)
            .zip(out.par_iter())
            .for_each(|(slot, p)| {
                slot[0] = engine_color::palette::srgb_to_linear(p.r);
                slot[1] = engine_color::palette::srgb_to_linear(p.g);
                slot[2] = engine_color::palette::srgb_to_linear(p.b);
                slot[3] = p.a as f32 / 255.0;
            });
    });
    let out_srgb_to_linear_ms = ms_since(t);

    let elapsed_ms = ms_since(t_all);
    let accounted = in_f32_to_u8_ms
        + setup_hash_ms
        + lattice_ps_ms
        + pack_upload_bytes_ms
        + phases.prepare_ms
        + phases.upload_ms
        + phases.dispatch_ms
        + phases.readback_ms
        + phases.post_ms
        + expand_ps_ms
        + out_bytes_to_rgba8_ms
        + out_srgb_to_linear_ms;
    let apply_misc_ms = (elapsed_ms - accounted).max(0.0);

    log::info!(target: "gpu_dither", "{phase_log}");
    record_status(GpuDitherStatus {
        algorithm: algo.to_string(),
        path: path_label.to_string(),
        backend,
        elapsed_ms: elapsed_ms.round().clamp(0.0, u32::MAX as f64) as u32,
        strips,
        width,
        height,
        pixel_size: ps,
        pipeline: phases.pipeline.to_string(),
        prepare_ms: phases.prepare_ms,
        upload_ms: phases.upload_ms,
        dispatch_ms: phases.dispatch_ms,
        readback_ms: phases.readback_ms,
        post_ms: phases.post_ms,
        in_f32_to_u8_ms,
        setup_hash_ms,
        lattice_ps_ms,
        pack_upload_bytes_ms,
        expand_ps_ms,
        out_bytes_to_rgba8_ms,
        out_srgb_to_linear_ms,
        apply_misc_ms,
    });
    Ok(())
}

fn run_cpu(
    input: &[Rgba8],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    palette: &[PaletteRgb],
) -> Result<Vec<Rgba8>, FilterError> {
    let ps = params.pixel_size.max(1) as u32;
    with_pixel_size_blocks(input, width, height, ps, |small, sw, sh| {
        match params.mode {
            DitherModeV2::BlockedFs => blocked_fs_cpu(small, sw, sh, palette, CORE, CORE, PAD),
            DitherModeV2::BlueNoise => blue_noise_cpu(
                small,
                sw,
                sh,
                palette,
                blue_noise_ranks(),
                spread_from_params(params),
            ),
            _ => Err(crate::error::EngineError::invalid_filter_params(
                "gpu_dither: unexpected mode",
            )),
        }
    })
    .map_err(FilterError::from_engine)
}

fn try_gpu_timed(
    input: &[Rgba8],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    palette: &[PaletteRgb],
    cancel: &std::sync::atomic::AtomicBool,
    lattice_ps_ms: &mut f64,
    pack_upload_bytes_ms: &mut f64,
    expand_ps_ms: &mut f64,
) -> Result<(Vec<u8>, String, u32, engine_gpu::GpuPhaseTiming), String> {
    let Some((device, queue, label)) = engine_gpu::cached_blocked_fs_device() else {
        return Err("no adapter".into());
    };
    let backend = backend_short(label);
    let ps = params.pixel_size.max(1) as u32;
    let pal_b = palette_rgba(palette);

    // Build block-rep lattice (identity when ps==1), run GPU once, expand.
    let t = Instant::now();
    let (tw, th, lattice) = if ps <= 1 {
        (width, height, None)
    } else {
        let bw = width.div_ceil(ps);
        let bh = height.div_ceil(ps);
        let w = width as usize;
        let n_small = (bw * bh) as usize;
        let mut small = vec![Rgba8::new(0, 0, 0, 0); n_small];
        crate::pixel_pool::install(|| {
            small.par_iter_mut().enumerate().for_each(|(i, px)| {
                let by = (i as u32) / bw;
                let bx = (i as u32) % bw;
                *px = input[(by * ps) as usize * w + (bx * ps) as usize];
            });
        });
        (bw, bh, Some(small))
    };
    *lattice_ps_ms = ms_since(t);

    let src: &[Rgba8] = lattice.as_deref().unwrap_or(input);
    let t = Instant::now();
    let in_b = rgba8_to_bytes_par(src);
    *pack_upload_bytes_ms = ms_since(t);

    let gpu = match params.mode {
        DitherModeV2::BlockedFs => engine_gpu::apply_blocked_fs_gpu_opts(
            device,
            queue,
            &in_b,
            tw,
            th,
            &pal_b,
            cancel,
            engine_gpu::BlockedFsGpuOpts::default(),
        ),
        DitherModeV2::BlueNoise => engine_gpu::apply_blue_noise_gpu_opts(
            device,
            queue,
            &in_b,
            tw,
            th,
            &pal_b,
            blue_noise_ranks(),
            spread_from_params(params),
            cancel,
            engine_gpu::BlueNoiseGpuOpts::default(),
        ),
        _ => return Err("unexpected mode".into()),
    }?;

    let t = Instant::now();
    let out_bytes = if ps <= 1 {
        gpu.rgba8
    } else {
        let dithered = bytes_to_rgba8(&gpu.rgba8);
        let bw = tw as usize;
        let w = width as usize;
        let h = height as usize;
        let mut out = vec![Rgba8::new(0, 0, 0, 0); w * h];
        crate::pixel_pool::install(|| {
            out.par_iter_mut().enumerate().for_each(|(i, px)| {
                let y = i / w;
                let x = i % w;
                let by = (y as u32 / ps) as usize;
                let bx = (x as u32 / ps) as usize;
                let p = dithered[by * bw + bx];
                *px = Rgba8::new(p.r, p.g, p.b, input[y * w + x].a);
            });
        });
        rgba8_to_bytes_par(&out)
    };
    *expand_ps_ms = ms_since(t);

    Ok((out_bytes, backend, gpu.strips, gpu.timing))
}

fn apply_tile(
    tile: &mut PixelTile,
    mut params: DitherParamsV2,
    mode: DitherModeV2,
    ctx: &dyn FilterCtx,
) -> Result<(), FilterError> {
    params.mode = mode.clone();
    let fctx = FilterContext::from_ctx(ctx);
    let palette = palette_from_ctx(fctx, &params)?;
    let src = scratch_src(tile);
    let input = tile_to_rgba8(&src);
    let out = run_cpu(&input, TILE_SIZE, TILE_SIZE, &params, &palette)?;
    write_rgba8_to_tile(tile, &out);
    Ok(())
}

/// Fast approximate error diffusion (`blocked_fs`).
pub struct BlockedFs;

impl FilterAlgorithm for BlockedFs {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("blocked_fs")
    }

    fn display_name(&self) -> &'static str {
        "Fast error diffusion (approximate)"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        apply_tile(tile, params, DitherModeV2::BlockedFs, ctx)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Conditional
    }

    fn param_schema(&self) -> &'static [ParamField] {
        BLOCKED_FS_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }

    fn execution_scope(&self) -> ExecutionScope {
        ExecutionScope::FullDocument
    }

    fn temporal_stability(&self) -> TemporalStability {
        TemporalStability::Unstable
    }
}

/// Threshold-shift blue noise (`blue_noise`).
pub struct BlueNoise;

impl FilterAlgorithm for BlueNoise {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("blue_noise")
    }

    fn display_name(&self) -> &'static str {
        "Blue noise"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        #[cfg(debug_assertions)]
        {
            use std::sync::Once;
            static VERIFY: Once = Once::new();
            VERIFY.call_once(crate::algorithms::blue_noise_texture::verify_texture_hash);
        }
        let params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        apply_tile(tile, params, DitherModeV2::BlueNoise, ctx)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Conditional
    }

    fn param_schema(&self) -> &'static [ParamField] {
        BLUE_NOISE_SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }

    fn execution_scope(&self) -> ExecutionScope {
        ExecutionScope::FullDocument
    }

    fn temporal_stability(&self) -> TemporalStability {
        TemporalStability::Unstable
    }
}
