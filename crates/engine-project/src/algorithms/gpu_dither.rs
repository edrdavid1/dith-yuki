//! `blocked_fs` and `blue_noise` — approximate GPU dither families (`ED-on-GPU.md`).
//!
//! CPU reference is canonical. GPU path is `GpuEligibility::Conditional` and
//! falls back to this CPU reference when the probe / flag / limits fail.
//!
//! Dev path override: `DITHER_GPU_PATH=auto|gpu|cpu` (default `auto`).

use std::sync::Mutex;
use std::time::Instant;

use engine_color::palette::linear_to_srgb;
use engine_registry::{
    AlgorithmId, EffectCategory, ExecutionScope, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField, TemporalStability,
};
use engine_tiles::{PixelTile, HALO, TILE_SIZE};
use serde::Serialize;

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::context::FilterContext;

use super::gpu_dither_cpu::{
    blue_noise_cpu, blue_noise_ranks, blocked_fs_cpu, PaletteRgb, Rgba8, CORE, DEFAULT_SPREAD, PAD,
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
}

static LAST_STATUS: Mutex<Option<GpuDitherStatus>> = Mutex::new(None);

/// Take the status from the most recent [`apply_gpu_dither_rgba`] call.
pub fn take_last_gpu_dither_status() -> Option<GpuDitherStatus> {
    LAST_STATUS.lock().ok().and_then(|mut g| g.take())
}

/// Peek without clearing (tests / debug).
pub fn last_gpu_dither_status() -> Option<GpuDitherStatus> {
    LAST_STATUS.lock().ok().and_then(|g| g.clone())
}

fn record_status(status: GpuDitherStatus) {
    log::info!(
        target: "gpu_dither",
        "{} · {} ({}) · {} ms · {} strips",
        status.algorithm,
        status.path,
        status.backend,
        status.elapsed_ms,
        status.strips
    );
    if let Ok(mut g) = LAST_STATUS.lock() {
        *g = Some(status);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathChoice {
    Auto,
    ForceGpu,
    ForceCpu,
}

fn path_choice() -> PathChoice {
    match std::env::var("DITHER_GPU_PATH")
        .unwrap_or_else(|_| "auto".into())
        .to_ascii_lowercase()
        .as_str()
    {
        "gpu" => PathChoice::ForceGpu,
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

fn rgba8_to_bytes(px: &[Rgba8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(px.len() * 4);
    for p in px {
        v.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    v
}

fn bytes_to_rgba8(bytes: &[u8]) -> Vec<Rgba8> {
    bytes
        .chunks_exact(4)
        .map(|c| Rgba8::new(c[0], c[1], c[2], c[3]))
        .collect()
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

fn bilevel_palette() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ]
}

fn palette_from_ctx(ctx: &FilterContext<'_>, params: &DitherParamsV2) -> Result<Vec<PaletteRgb>, FilterError> {
    let doc = ctx.document;
    // Resolve optional document palette by id; fall back to bilevel.
    let Some(pal) = params.palette_id.and_then(|id| doc.get_palette(id)) else {
        return Ok(bilevel_palette());
    };
    if pal.colors.is_empty() {
        return Err(FilterError::from_engine(
            crate::error::EngineError::invalid_filter_params("InvalidPalette: empty"),
        ));
    }
    Ok(pal
        .colors
        .iter()
        .map(|c| PaletteRgb {
            r: linear_to_srgb(c.r),
            g: linear_to_srgb(c.g),
            b: linear_to_srgb(c.b),
        })
        .collect())
}

fn spread_from_params(params: &DitherParamsV2) -> u32 {
    // Map threshold_scale (default 1.0) onto u8 spread units.
    let s = (params.threshold_scale * DEFAULT_SPREAD as f32).round();
    s.clamp(0.0, 1024.0) as u32
}

/// Apply blocked FS or blue noise to a full RGBA f32 buffer (FullDocument path).
///
/// Path selection via `DITHER_GPU_PATH` (`auto` / `gpu` / `cpu`). On GPU failure
/// or missing adapter, falls back to the CPU reference (never panics).
pub fn apply_gpu_dither_rgba(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    palette: &[PaletteRgb],
    should_cancel: &std::sync::atomic::AtomicBool,
) -> Result<(), FilterError> {
    let n = (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| FilterError::from_engine("gpu_dither: size overflow"))?;
    if rgba.len() < n * 4 {
        return Err(FilterError::from_engine("gpu_dither: buffer too small"));
    }
    if should_cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err(FilterError::from_engine("gpu_dither: cancelled"));
    }
    let mut input = Vec::with_capacity(n);
    for i in 0..n {
        let o = i * 4;
        input.push(Rgba8::new(
            linear_to_srgb(rgba[o]),
            linear_to_srgb(rgba[o + 1]),
            linear_to_srgb(rgba[o + 2]),
            (rgba[o + 3].clamp(0.0, 1.0) * 255.0).round() as u8,
        ));
    }

    let algo = algo_name(&params.mode);
    let choice = path_choice();
    let t0 = Instant::now();
    let _ = params.pixel_size; // warm-up fixed at PAD=8 for GPU parity (§1.1)

    let (out, path_label, backend, strips) = match choice {
        PathChoice::ForceCpu => {
            let out = run_cpu(&input, width, height, params, palette)?;
            (out, "CPU", "cpu".to_string(), 1u32)
        }
        PathChoice::Auto | PathChoice::ForceGpu => {
            match try_gpu(&input, width, height, params, palette, should_cancel) {
                Ok((bytes, backend, strips)) => {
                    (bytes_to_rgba8(&bytes), "GPU", backend, strips)
                }
                Err(reason) => {
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
                    let out = run_cpu(&input, width, height, params, palette)?;
                    (out, "CPU", "cpu".to_string(), 1u32)
                }
            }
        }
    };

    if should_cancel.load(std::sync::atomic::Ordering::Acquire) {
        return Err(FilterError::from_engine("gpu_dither: cancelled"));
    }
    for (i, p) in out.iter().enumerate() {
        let o = i * 4;
        rgba[o] = engine_color::palette::srgb_to_linear(p.r);
        rgba[o + 1] = engine_color::palette::srgb_to_linear(p.g);
        rgba[o + 2] = engine_color::palette::srgb_to_linear(p.b);
        rgba[o + 3] = p.a as f32 / 255.0;
    }

    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;
    record_status(GpuDitherStatus {
        algorithm: algo.to_string(),
        path: path_label.to_string(),
        backend,
        elapsed_ms: elapsed_ms.round().clamp(0.0, u32::MAX as f64) as u32,
        strips,
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
    match params.mode {
        DitherModeV2::BlockedFs => blocked_fs_cpu(input, width, height, palette, CORE, CORE, PAD)
            .map_err(FilterError::from_engine),
        DitherModeV2::BlueNoise => blue_noise_cpu(
            input,
            width,
            height,
            palette,
            blue_noise_ranks(),
            spread_from_params(params),
        )
        .map_err(FilterError::from_engine),
        _ => Err(FilterError::from_engine("gpu_dither: unexpected mode")),
    }
}

fn try_gpu(
    input: &[Rgba8],
    width: u32,
    height: u32,
    params: &DitherParamsV2,
    palette: &[PaletteRgb],
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(Vec<u8>, String, u32), String> {
    let Some((device, queue, label)) = engine_gpu::cached_blocked_fs_device() else {
        return Err("no adapter".into());
    };
    let in_b = rgba8_to_bytes(input);
    let pal_b = palette_rgba(palette);
    let backend = backend_short(label);
    let result = match params.mode {
        DitherModeV2::BlockedFs => {
            engine_gpu::apply_blocked_fs_gpu_opts(
                device,
                queue,
                &in_b,
                width,
                height,
                &pal_b,
                cancel,
                engine_gpu::BlockedFsGpuOpts::default(),
            )
        }
        DitherModeV2::BlueNoise => engine_gpu::apply_blue_noise_gpu_opts(
            device,
            queue,
            &in_b,
            width,
            height,
            &pal_b,
            blue_noise_ranks(),
            spread_from_params(params),
            cancel,
            engine_gpu::BlueNoiseGpuOpts::default(),
        ),
        _ => return Err("unexpected mode".into()),
    }?;
    Ok((result.rgba8, backend, result.strips))
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
    let out = match mode {
        DitherModeV2::BlockedFs => {
            blocked_fs_cpu(
                &input,
                TILE_SIZE,
                TILE_SIZE,
                &palette,
                CORE,
                CORE,
                PAD,
            )
            .map_err(FilterError::from_engine)?
        }
        DitherModeV2::BlueNoise => blue_noise_cpu(
            &input,
            TILE_SIZE,
            TILE_SIZE,
            &palette,
            blue_noise_ranks(),
            spread_from_params(&params),
        )
        .map_err(FilterError::from_engine)?,
        _ => unreachable!(),
    };
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
        // TODO(ED-on-GPU.md §4): Conditional — GPU when probe passes.
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
        // Stateless; FullDocument keeps strip/GPU path aligned with CPU reference.
        ExecutionScope::FullDocument
    }

    fn temporal_stability(&self) -> TemporalStability {
        TemporalStability::Stable
    }
}
