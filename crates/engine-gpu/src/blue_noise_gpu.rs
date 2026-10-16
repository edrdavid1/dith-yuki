//! GPU threshold-shift blue noise.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::blocked_fs::GpuPhaseTiming;
use crate::strip_bind::{effective_max_bind, max_blue_noise_strip_h, row_bytes};
use crate::timestamp_diag::DispatchTimestampDiag;

/// WGSL source (self-test cache key + pipeline).
pub const BLUE_NOISE_SHADER_SRC: &str = include_str!("shaders/blue_noise.wgsl");
const SHADER: &str = BLUE_NOISE_SHADER_SRC;
const NOISE_SIZE: u32 = 128;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    img_w: u32,
    img_h: u32,
    palette_len: u32,
    spread: u32,
    noise_size: u32,
    strip_y0: u32,
    strip_h: u32,
    buf_y0: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BlueNoiseGpuOpts {
    /// Force strip height in pixels.
    pub force_strip_h: Option<u32>,
    /// Pretend `max_storage_buffer_binding_size` is at most this many bytes.
    pub force_max_bind: Option<u64>,
}

pub fn apply_blue_noise_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    input_rgba8: &[u8],
    width: u32,
    height: u32,
    palette_rgba8: &[[u8; 4]],
    ranks_u16: &[u16],
    spread: u32,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    apply_blue_noise_gpu_opts(
        device,
        queue,
        input_rgba8,
        width,
        height,
        palette_rgba8,
        ranks_u16,
        spread,
        cancel,
        BlueNoiseGpuOpts::default(),
    )
    .map(|r| r.rgba8)
}

pub fn apply_blue_noise_gpu_opts(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    input_rgba8: &[u8],
    width: u32,
    height: u32,
    palette_rgba8: &[[u8; 4]],
    ranks_u16: &[u16],
    spread: u32,
    cancel: &AtomicBool,
    opts: BlueNoiseGpuOpts,
) -> Result<crate::blocked_fs::GpuImageResult, String> {
    if palette_rgba8.is_empty() {
        return Err("InvalidPalette: empty".into());
    }
    if ranks_u16.len() != (NOISE_SIZE * NOISE_SIZE) as usize {
        return Err("noise must be 128×128 u16".into());
    }
    let n = (width as usize)
        .checked_mul(height as usize)
        .ok_or("size overflow")?;
    if input_rgba8.len() != n * 4 {
        return Err("input size mismatch".into());
    }

    let input_u32: Vec<u32> = input_rgba8
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let palette_u32: Vec<u32> = palette_rgba8
        .iter()
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    let mut noise_u32 = Vec::with_capacity(ranks_u16.len() / 2);
    for chunk in ranks_u16.as_chunks::<2>().0 {
        noise_u32.push((chunk[0] as u32) | ((chunk[1] as u32) << 16));
    }

    let t_prepare0 = Instant::now();
    let max_bind = effective_max_bind(device, opts.force_max_bind);
    let full_bytes = n as u64 * 4;
    let use_full = full_bytes <= max_bind && opts.force_strip_h.is_none();

    let mut strip_h = max_blue_noise_strip_h(width, max_bind).ok_or_else(|| {
        format!(
            "blue_noise: cannot fit one strip (width={width}, max_bind={max_bind}); use CPU path"
        )
    })?;
    strip_h = strip_h.min(height).max(1);
    if let Some(forced) = opts.force_strip_h {
        strip_h = forced.max(1).min(strip_h);
    }

    let (bgl, pipeline, pipeline_label) = cached_bn_pipeline(device);
    let prepare_ms = t_prepare0.elapsed().as_secs_f64() * 1000.0;

    let t_upload0 = Instant::now();
    let palette_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("bn-pal"),
        contents: bytemuck::cast_slice(&palette_u32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let noise_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("bn-noise"),
        contents: bytemuck::cast_slice(&noise_u32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("bn-u"),
        size: std::mem::size_of::<Params>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut out_u32 = vec![0u32; n];
    let mut strips = 0u32;
    let upload_ms;
    let mut dispatch_ms = 0.0;
    let mut readback_ms = 0.0;
    let mut ts_diag = DispatchTimestampDiag::begin(device, "blue_noise", width, height);

    if use_full {
        let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bn-in"),
            contents: bytemuck::cast_slice(&input_u32),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bn-out"),
            size: full_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let bind_group = make_bn_bg(
            device,
            bgl,
            &input_buf,
            &output_buf,
            &palette_buf,
            &noise_buf,
            &uniform_buf,
        );
        upload_ms = t_upload0.elapsed().as_secs_f64() * 1000.0;
        let t_d0 = Instant::now();
        let mut y0 = 0u32;
        while y0 < height {
            if cancel.load(Ordering::Acquire) {
                return Err("cancelled".into());
            }
            let h = strip_h.min(height - y0);
            write_bn_params(
                queue,
                &uniform_buf,
                width,
                height,
                palette_rgba8.len() as u32,
                spread,
                y0,
                h,
                0,
            );
            let groups_x = width.div_ceil(8);
            let groups_y = h.div_ceil(8);
            if let Some(ref mut diag) = ts_diag {
                diag.dispatch_submit(
                    device,
                    queue,
                    pipeline,
                    &bind_group,
                    groups_x,
                    groups_y,
                    "bn",
                );
            } else {
                dispatch_bn(device, queue, pipeline, &bind_group, width, h);
            }
            strips += 1;
            y0 += h;
        }
        device.poll(wgpu::Maintain::Wait);
        if let Some(ref mut diag) = ts_diag {
            diag.note_poll_end(false);
        }
        dispatch_ms = t_d0.elapsed().as_secs_f64() * 1000.0;
        let t_r0 = Instant::now();
        out_u32 = readback_u32(device, queue, &output_buf, full_bytes)?;
        readback_ms = t_r0.elapsed().as_secs_f64() * 1000.0;
    } else {
        upload_ms = t_upload0.elapsed().as_secs_f64() * 1000.0;
        let mut y0 = 0u32;
        while y0 < height {
            if cancel.load(Ordering::Acquire) {
                return Err("cancelled".into());
            }
            let h = strip_h.min(height - y0);
            let bind_bytes = h as u64 * row_bytes(width);
            if bind_bytes > max_bind {
                return Err(format!(
                    "blue_noise: strip binding {bind_bytes} exceeds max_bind {max_bind}; use CPU path"
                ));
            }
            let start = (y0 as usize) * (width as usize);
            let end = start + (h as usize) * (width as usize);
            let slice = &input_u32[start..end];
            let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bn-in-strip"),
                contents: bytemuck::cast_slice(slice),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bn-out-strip"),
                size: bind_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let bind_group = make_bn_bg(
                device,
                bgl,
                &input_buf,
                &output_buf,
                &palette_buf,
                &noise_buf,
                &uniform_buf,
            );
            write_bn_params(
                queue,
                &uniform_buf,
                width,
                height,
                palette_rgba8.len() as u32,
                spread,
                y0,
                h,
                y0, // buf_y0 == strip start
            );
            let t_d0 = Instant::now();
            let groups_x = width.div_ceil(8);
            let groups_y = h.div_ceil(8);
            if let Some(ref mut diag) = ts_diag {
                diag.dispatch_submit(
                    device,
                    queue,
                    pipeline,
                    &bind_group,
                    groups_x,
                    groups_y,
                    "bn-strip",
                );
            } else {
                dispatch_bn(device, queue, pipeline, &bind_group, width, h);
            }
            device.poll(wgpu::Maintain::Wait);
            if let Some(ref mut diag) = ts_diag {
                diag.note_poll_end(true);
            }
            dispatch_ms += t_d0.elapsed().as_secs_f64() * 1000.0;
            strips += 1;
            let t_r0 = Instant::now();
            let strip_out = readback_u32(device, queue, &output_buf, bind_bytes)?;
            readback_ms += t_r0.elapsed().as_secs_f64() * 1000.0;
            out_u32[start..end].copy_from_slice(&strip_out);
            y0 += h;
        }
    }

    let strips_out = strips.max(1);
    if let Some(diag) = ts_diag.take() {
        diag.finish(device, queue, strips_out, dispatch_ms);
    }

    let t_post0 = Instant::now();
    let mut out = Vec::with_capacity(n * 4);
    for &p in &out_u32 {
        out.extend_from_slice(&p.to_le_bytes());
    }
    let post_ms = t_post0.elapsed().as_secs_f64() * 1000.0;
    Ok(crate::blocked_fs::GpuImageResult {
        rgba8: out,
        strips: strips_out,
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

struct CachedBnPipe {
    bgl: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

fn cached_bn_pipeline(
    device: &wgpu::Device,
) -> (
    &'static wgpu::BindGroupLayout,
    &'static wgpu::ComputePipeline,
    &'static str,
) {
    static CELL: OnceLock<CachedBnPipe> = OnceLock::new();
    static CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let pipe = CELL.get_or_init(|| {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blue_noise"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bn_bgl"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, true),
                storage_entry(3, true),
                uniform_entry(4),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bn_pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("bn_pipe"),
            layout: Some(&pl),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        CachedBnPipe { bgl, pipeline }
    });
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    (
        &pipe.bgl,
        &pipe.pipeline,
        if n == 0 { "new" } else { "cached" },
    )
}

fn make_bn_bg<'a>(
    device: &wgpu::Device,
    bgl: &wgpu::BindGroupLayout,
    input_buf: &'a wgpu::Buffer,
    output_buf: &'a wgpu::Buffer,
    palette_buf: &'a wgpu::Buffer,
    noise_buf: &'a wgpu::Buffer,
    uniform_buf: &'a wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bn-bg"),
        layout: bgl,
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
                resource: noise_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: uniform_buf.as_entire_binding(),
            },
        ],
    })
}

fn write_bn_params(
    queue: &wgpu::Queue,
    uniform_buf: &wgpu::Buffer,
    width: u32,
    height: u32,
    palette_len: u32,
    spread: u32,
    strip_y0: u32,
    strip_h: u32,
    buf_y0: u32,
) {
    let params = Params {
        img_w: width,
        img_h: height,
        palette_len,
        spread,
        noise_size: NOISE_SIZE,
        strip_y0,
        strip_h,
        buf_y0,
    };
    queue.write_buffer(uniform_buf, 0, bytemuck::bytes_of(&params));
}

fn dispatch_bn(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    width: u32,
    h: u32,
) {
    let groups_x = width.div_ceil(8);
    let groups_y = h.div_ceil(8);
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("bn-strip"),
    });
    {
        let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bn"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(groups_x, groups_y, 1);
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
        label: Some("bn-rb"),
        size: byte_count,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("bn-rb-enc"),
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
