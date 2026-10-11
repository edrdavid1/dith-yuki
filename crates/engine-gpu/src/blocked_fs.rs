//! GPU blocked Floyd–Steinberg (in-workgroup wavefront).
//!
//! Geometry: 56×56 core + 8 warm-up = 64×64 workgroup.
//! Large images use strip-sized STORAGE bindings derived from
//! `device.limits().max_storage_buffer_binding_size`.
//! Scheme: `.local-doc/Dither GPU-аналоги ED — схема реализации.md`

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::strip_bind::{
    blocked_fs_strip_rows, effective_max_bind, max_blocked_fs_strip_cores, row_bytes,
};

/// Workgroup / padded-tile side.
pub const WG_SIZE: u32 = 64;
/// Left/top warm-up.
pub const PAD: u32 = 8;
/// Central written region.
pub const CORE: u32 = WG_SIZE - PAD;

const SHADER: &str = include_str!("shaders/blocked_fs.wgsl");

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    img_w: u32,
    img_h: u32,
    core: u32,
    pad: u32,
    palette_len: u32,
    strip_y0: u32,
    buf_y0: u32,
    _pad1: u32,
}

/// Optional overrides for tests (strip height, binding budget, etc.).
#[derive(Clone, Copy, Debug, Default)]
pub struct BlockedFsGpuOpts {
    /// Force strip height in core-rows (1 = one tile row per dispatch).
    pub force_strip_cores: Option<u32>,
    /// Pretend `max_storage_buffer_binding_size` is at most this many bytes.
    pub force_max_bind: Option<u64>,
}

/// Phase timings for one GPU apply (milliseconds; includes GPU wait on readback).
#[derive(Clone, Debug, Default)]
pub struct GpuPhaseTiming {
    pub prepare_ms: f64,
    pub upload_ms: f64,
    pub dispatch_ms: f64,
    pub readback_ms: f64,
    pub post_ms: f64,
    /// `"cached"` when the compute pipeline was reused; `"new"` on first build.
    pub pipeline: &'static str,
}

/// GPU apply result with strip accounting for the preview indicator.
#[derive(Clone, Debug)]
pub struct GpuImageResult {
    pub rgba8: Vec<u8>,
    /// Number of strip dispatches submitted.
    pub strips: u32,
    pub timing: GpuPhaseTiming,
}

struct CachedBfsPipe {
    bgl: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

/// Session-cached pipeline. Returns `(pipe, "new"|"cached")`.
fn cached_bfs_pipeline(device: &wgpu::Device) -> (&'static CachedBfsPipe, &'static str) {
    static CELL: OnceLock<CachedBfsPipe> = OnceLock::new();
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let pipe = CELL.get_or_init(|| {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blocked_fs"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blocked_fs_bgl"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, true),
                uniform_entry(3),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blocked_fs_pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("blocked_fs_pipe"),
            layout: Some(&pl),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        CachedBfsPipe { bgl, pipeline }
    });
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    (pipe, if n == 0 { "new" } else { "cached" })
}

/// Packed RGBA8 image + packed RGBA8 palette → packed RGBA8 output.
///
/// `cancel` checked between strip dispatches. Strips are rows of cores.
pub fn apply_blocked_fs_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    input_rgba8: &[u8],
    width: u32,
    height: u32,
    palette_rgba8: &[[u8; 4]],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    apply_blocked_fs_gpu_opts(
        device,
        queue,
        input_rgba8,
        width,
        height,
        palette_rgba8,
        cancel,
        BlockedFsGpuOpts::default(),
    )
    .map(|r| r.rgba8)
}

/// Like [`apply_blocked_fs_gpu`] with test overrides; returns strip count.
pub fn apply_blocked_fs_gpu_opts(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    input_rgba8: &[u8],
    width: u32,
    height: u32,
    palette_rgba8: &[[u8; 4]],
    cancel: &AtomicBool,
    opts: BlockedFsGpuOpts,
) -> Result<GpuImageResult, String> {
    if palette_rgba8.is_empty() {
        return Err("InvalidPalette: empty".into());
    }
    let n = (width as usize)
        .checked_mul(height as usize)
        .ok_or("size overflow")?;
    if input_rgba8.len() != n * 4 {
        return Err("input size mismatch".into());
    }

    let input_u32: Vec<u32> = input_rgba8
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let palette_u32: Vec<u32> = palette_rgba8
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    let t_prepare0 = Instant::now();
    let max_bind = effective_max_bind(device, opts.force_max_bind);
    let full_bytes = n as u64 * 4;
    let use_full = full_bytes <= max_bind && opts.force_strip_cores.is_none();

    let mut strip_cores = max_blocked_fs_strip_cores(width, max_bind).ok_or_else(|| {
        format!(
            "blocked_fs: cannot fit one strip (width={width}, max_bind={max_bind}); use CPU path"
        )
    })?;
    strip_cores = strip_cores.min(height.div_ceil(CORE)).max(1);
    if let Some(forced) = opts.force_strip_cores {
        // Allow forcing smaller than auto; never exceed what fits in max_bind.
        let max_c = max_blocked_fs_strip_cores(width, max_bind).unwrap_or(1);
        strip_cores = forced.max(1).min(max_c);
    }

    // Preflight: smallest strip must fit.
    let (y0, y1) = blocked_fs_strip_rows(0, 1.min(strip_cores), height);
    let min_bind = (y1 - y0) as u64 * row_bytes(width);
    if min_bind > max_bind {
        return Err(format!(
            "blocked_fs: min strip binding {min_bind} exceeds max_bind {max_bind}; use CPU path"
        ));
    }

    let (pipe, pipeline_label) = cached_bfs_pipeline(device);
    let prepare_ms = t_prepare0.elapsed().as_secs_f64() * 1000.0;

    let t_upload0 = Instant::now();
    let palette_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("bfs-pal"),
        contents: bytemuck::cast_slice(&palette_u32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bfs-u"),
        size: std::mem::size_of::<Params>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let tiles_x = width.div_ceil(CORE);
    let tiles_y = height.div_ceil(CORE);
    let mut out_u32 = input_u32.clone();
    let mut strips = 0u32;
    let mut upload_ms;
    let mut dispatch_ms = 0.0;
    let mut readback_ms = 0.0;

    if use_full {
        // Single full-image binding; strip only for cancel checkpoints.
        let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bfs-in"),
            contents: bytemuck::cast_slice(&input_u32),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let output_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bfs-out"),
            contents: bytemuck::cast_slice(&input_u32),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bfs-bg"),
            layout: &pipe.bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: input_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: output_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: palette_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: uniform_buf.as_entire_binding(),
                },
            ],
        });
        upload_ms = t_upload0.elapsed().as_secs_f64() * 1000.0;

        let t_disp0 = Instant::now();
        let mut ty = 0u32;
        while ty < tiles_y {
            if cancel.load(Ordering::Acquire) {
                return Err("cancelled".into());
            }
            let rows = strip_cores.min(tiles_y - ty);
            write_params(
                queue,
                &uniform_buf,
                width,
                height,
                palette_rgba8.len() as u32,
                ty * CORE,
                0,
            );
            dispatch_strip(device, queue, &pipe.pipeline, &bind_group, tiles_x, rows);
            strips += 1;
            ty += rows;
        }
        // Wait for GPU work before attributing time solely to readback.
        device.poll(wgpu::Maintain::Wait);
        dispatch_ms = t_disp0.elapsed().as_secs_f64() * 1000.0;

        let t_rb0 = Instant::now();
        out_u32 = readback_u32(device, queue, &output_buf, full_bytes)?;
        readback_ms = t_rb0.elapsed().as_secs_f64() * 1000.0;
    } else {
        upload_ms = t_upload0.elapsed().as_secs_f64() * 1000.0;
        // Strip-sized STORAGE buffers.
        let mut ty = 0u32;
        while ty < tiles_y {
            if cancel.load(Ordering::Acquire) {
                return Err("cancelled".into());
            }
            let rows = strip_cores.min(tiles_y - ty);
            let strip_y0 = ty * CORE;
            let (buf_y0, buf_y1) = blocked_fs_strip_rows(strip_y0, rows, height);
            let bind_h = buf_y1 - buf_y0;
            let bind_bytes = bind_h as u64 * row_bytes(width);
            if bind_bytes > max_bind {
                return Err(format!(
                    "blocked_fs: strip binding {bind_bytes} exceeds max_bind {max_bind}; use CPU path"
                ));
            }
            let start = (buf_y0 as usize) * (width as usize);
            let end = (buf_y1 as usize) * (width as usize);
            let slice = &input_u32[start..end];

            let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bfs-in-strip"),
                contents: bytemuck::cast_slice(slice),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
            let output_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bfs-out-strip"),
                contents: bytemuck::cast_slice(slice),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            });
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bfs-bg-strip"),
                layout: &pipe.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: input_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: output_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: palette_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: uniform_buf.as_entire_binding(),
                    },
                ],
            });
            write_params(
                queue,
                &uniform_buf,
                width,
                height,
                palette_rgba8.len() as u32,
                strip_y0,
                buf_y0,
            );
            let t_d0 = Instant::now();
            dispatch_strip(device, queue, &pipe.pipeline, &bind_group, tiles_x, rows);
            device.poll(wgpu::Maintain::Wait);
            dispatch_ms += t_d0.elapsed().as_secs_f64() * 1000.0;
            strips += 1;
            let t_r0 = Instant::now();
            let strip_out = readback_u32(device, queue, &output_buf, bind_bytes)?;
            readback_ms += t_r0.elapsed().as_secs_f64() * 1000.0;
            // Copy written core rows (and any overlapping warm-up) back.
            // Only rows in [strip_y0, strip_y0+rows*CORE) are authoritative.
            let write_y0 = strip_y0;
            let write_y1 = (strip_y0 + rows * CORE).min(height);
            for y in write_y0..write_y1 {
                let src_off = ((y - buf_y0) as usize) * (width as usize);
                let dst_off = (y as usize) * (width as usize);
                out_u32[dst_off..dst_off + width as usize]
                    .copy_from_slice(&strip_out[src_off..src_off + width as usize]);
            }
            ty += rows;
        }
    }

    let t_post0 = Instant::now();
    let mut out = Vec::with_capacity(n * 4);
    for &p in &out_u32 {
        out.extend_from_slice(&p.to_le_bytes());
    }
    let post_ms = t_post0.elapsed().as_secs_f64() * 1000.0;
    Ok(GpuImageResult {
        rgba8: out,
        strips: strips.max(1),
        timing: GpuPhaseTiming {
            prepare_ms,
            upload_ms,
            dispatch_ms,
            readback_ms,
            post_ms,
            pipeline: pipeline_label,
        },
    })
}

fn write_params(
    queue: &wgpu::Queue,
    uniform_buf: &wgpu::Buffer,
    width: u32,
    height: u32,
    palette_len: u32,
    strip_y0: u32,
    buf_y0: u32,
) {
    let params = Params {
        img_w: width,
        img_h: height,
        core: CORE,
        pad: PAD,
        palette_len,
        strip_y0,
        buf_y0,
        _pad1: 0,
    };
    queue.write_buffer(uniform_buf, 0, bytemuck::bytes_of(&params));
}

fn dispatch_strip(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    tiles_x: u32,
    rows: u32,
) {
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("bfs-strip"),
    });
    {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bfs"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(tiles_x, rows, 1);
    }
    queue.submit(Some(enc.finish()));
}

fn readback_u32(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    src: &wgpu::Buffer,
    byte_count: u64,
) -> Result<Vec<u32>, String> {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bfs-rb"),
        size: byte_count,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("bfs-rb-enc"),
    });
    enc.copy_buffer_to_buffer(src, 0, &readback, 0, byte_count);
    queue.submit(Some(enc.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv()
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("map: {e}"))?;
    let data = slice.get_mapped_range();
    let out: Vec<u32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    readback.unmap();
    Ok(out)
}

fn storage_entry(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Cached [`request_blocked_fs_device`] for the process lifetime.
pub fn cached_blocked_fs_device() -> Option<&'static (wgpu::Device, wgpu::Queue, String)> {
    static CELL: std::sync::OnceLock<Option<(wgpu::Device, wgpu::Queue, String)>> =
        std::sync::OnceLock::new();
    CELL.get_or_init(request_blocked_fs_device).as_ref()
}

/// Request a device suitable for blocked_fs (32KB+ workgroup storage).
/// Uses the adapter's actual `max_storage_buffer_binding_size` / `max_buffer_size`.
pub fn request_blocked_fs_device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    let info = adapter.get_info();
    let al = adapter.limits();
    if al.max_compute_workgroup_storage_size < 32768
        || al.max_compute_invocations_per_workgroup < 64
        || al.max_compute_workgroup_size_x < 64
    {
        return None;
    }
    // Request adapter limits so binding size is the real device max, not 128 MiB default.
    let mut required = al.clone();
    // Keep WebGPU-safe floors where adapter reports 0 (shouldn't happen).
    required.max_storage_buffer_binding_size = al.max_storage_buffer_binding_size.max(1 << 27);
    required.max_buffer_size = al
        .max_buffer_size
        .max(u64::from(required.max_storage_buffer_binding_size));

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("blocked_fs"),
            required_features: wgpu::Features::empty(),
            required_limits: required,
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    ))
    .ok()?;

    // Log validation / GPU errors instead of panicking the overnight harness.
    device.on_uncaptured_error(Box::new(|err| {
        log::error!("wgpu uncaptured error: {err}");
        eprintln!("wgpu uncaptured error: {err}");
    }));

    let lim = device.limits();
    eprintln!(
        "blocked_fs device: {} ({:?}) max_storage_buffer_binding_size={} max_buffer_size={}",
        info.name,
        info.backend,
        lim.max_storage_buffer_binding_size,
        lim.max_buffer_size
    );

    Some((
        device,
        queue,
        format!("{} ({:?})", info.name, info.backend),
    ))
}
