//! GPU Knuth dot diffusion (8×8 class matrix, 64 ordered dispatches).
//!
//! The error buffer is the full image (3×Q8.8 per pixel). Class order is not a
//! scanline, so this path does not strip: if `err` exceeds the device storage
//! binding limit the call returns an error and the host uses the CPU reference.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::blocked_fs::{GpuImageResult, GpuPhaseTiming};
use crate::strip_bind::effective_max_bind;

/// WGSL source (self-test cache key + pipeline).
pub const DOT_DIFFUSION_SHADER_SRC: &str = include_str!("shaders/dot_diffusion.wgsl");
const SHADER: &str = DOT_DIFFUSION_SHADER_SRC;

/// Knuth 8×8 class matrix, row-major. Must match `@class-matrix` in the shader.
pub const DOT_DIFFUSION_CLASS: [u32; 64] = [
    34, 48, 40, 32, 29, 15, 23, 31, 42, 58, 56, 53, 21, 5, 7, 10, 50, 62, 61, 45, 13, 1, 2, 18, 38,
    46, 54, 37, 25, 17, 9, 26, 28, 14, 22, 30, 35, 49, 41, 33, 20, 4, 6, 11, 43, 59, 57, 52, 12, 0,
    3, 19, 51, 63, 60, 44, 24, 16, 8, 27, 39, 47, 55, 36,
];

/// `(x, y)` of each class inside the 8×8 matrix. Index is the class id.
pub fn dot_diffusion_class_positions() -> [(u32, u32); 64] {
    let mut pos = [(0u32, 0u32); 64];
    for y in 0..8u32 {
        for x in 0..8u32 {
            let class = DOT_DIFFUSION_CLASS[(y * 8 + x) as usize] as usize;
            pos[class] = (x, y);
        }
    }
    pos
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    img_w: u32,
    img_h: u32,
    palette_len: u32,
    cls: u32,
    cls_x: u32,
    cls_y: u32,
    _p0: u32,
    _p1: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DotDiffusionGpuOpts {
    /// Pretend `max_storage_buffer_binding_size` is at most this many bytes.
    pub force_max_bind: Option<u64>,
}

pub fn apply_dot_diffusion_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    input_rgba8: &[u8],
    width: u32,
    height: u32,
    palette_rgba8: &[[u8; 4]],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    apply_dot_diffusion_gpu_opts(
        device,
        queue,
        input_rgba8,
        width,
        height,
        palette_rgba8,
        cancel,
        DotDiffusionGpuOpts::default(),
    )
    .map(|r| r.rgba8)
}

pub fn apply_dot_diffusion_gpu_opts(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    input_rgba8: &[u8],
    width: u32,
    height: u32,
    palette_rgba8: &[[u8; 4]],
    cancel: &AtomicBool,
    opts: DotDiffusionGpuOpts,
) -> Result<GpuImageResult, String> {
    if palette_rgba8.is_empty() {
        return Err("InvalidPalette: empty".into());
    }
    let n = (width as usize)
        .checked_mul(height as usize)
        .ok_or("size overflow")?;
    if width == 0 || height == 0 || input_rgba8.len() != n * 4 {
        return Err("input size mismatch".into());
    }

    let max_bind = effective_max_bind(device, opts.force_max_bind);
    let err_bytes = (n as u64)
        .checked_mul(12)
        .ok_or("dot_diffusion: error buffer overflow")?;
    let px_bytes = (n as u64)
        .checked_mul(4)
        .ok_or("dot_diffusion: image buffer overflow")?;
    if err_bytes > max_bind || px_bytes > max_bind {
        return Err(format!(
            "dot_diffusion: error buffer {err_bytes} exceeds max_bind {max_bind}; use CPU path"
        ));
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

    let t_prepare0 = Instant::now();
    let (bgl, pipeline, pipeline_label) = cached_dot_pipeline(device);
    let prepare_ms = t_prepare0.elapsed().as_secs_f64() * 1000.0;

    let t_upload0 = Instant::now();
    let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("dot-in"),
        contents: bytemuck::cast_slice(&input_u32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dot-out"),
        size: px_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let err_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dot-err"),
        size: err_bytes,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let palette_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("dot-pal"),
        contents: bytemuck::cast_slice(&palette_u32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dot-u"),
        size: std::mem::size_of::<Params>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("dot-bg"),
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
                binding: 4,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: err_buf.as_entire_binding(),
            },
        ],
    });
    let upload_ms = t_upload0.elapsed().as_secs_f64() * 1000.0;

    let positions = dot_diffusion_class_positions();
    let groups_x = width.div_ceil(8);
    let groups_y = height.div_ceil(8);
    let palette_len = palette_rgba8.len() as u32;
    let mut strips = 0u32;
    crate::apply_progress::report(0, 64);
    let t_d0 = Instant::now();
    for cls in 0..64u32 {
        if cancel.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        let (cls_x, cls_y) = positions[cls as usize];
        let params = Params {
            img_w: width,
            img_h: height,
            palette_len,
            cls,
            cls_x,
            cls_y,
            _p0: 0,
            _p1: 0,
        };
        // Uniform writes are ordered with later submits on this queue. One
        // submit per class so dispatch N sees class N, not the last write.
        queue.write_buffer(&uniform_buf, 0, bytemuck::bytes_of(&params));
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("dot-cls"),
        });
        {
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("dot"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(groups_x, groups_y, 1);
        }
        queue.submit(Some(enc.finish()));
        strips += 1;
        crate::apply_progress::report(strips, 64);
    }
    device.poll(wgpu::Maintain::Wait);
    let dispatch_ms = t_d0.elapsed().as_secs_f64() * 1000.0;

    let t_r0 = Instant::now();
    let out_u32 = readback_u32(device, queue, &output_buf, px_bytes)?;
    let readback_ms = t_r0.elapsed().as_secs_f64() * 1000.0;

    let t_post0 = Instant::now();
    let mut out = Vec::with_capacity(n * 4);
    for &p in &out_u32 {
        out.extend_from_slice(&p.to_le_bytes());
    }
    let post_ms = t_post0.elapsed().as_secs_f64() * 1000.0;
    Ok(GpuImageResult {
        rgba8: out,
        strips,
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

struct CachedDotPipe {
    bgl: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

fn cached_dot_pipeline(
    device: &wgpu::Device,
) -> (
    &'static wgpu::BindGroupLayout,
    &'static wgpu::ComputePipeline,
    &'static str,
) {
    static CELL: OnceLock<CachedDotPipe> = OnceLock::new();
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let pipe = CELL.get_or_init(|| {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("dot_diffusion"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("dot_bgl"),
            entries: &[
                storage_entry(0, true),
                storage_entry(1, false),
                storage_entry(2, true),
                uniform_entry(4),
                storage_entry(5, false),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("dot_pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("dot_pipe"),
            layout: Some(&pl),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        CachedDotPipe { bgl, pipeline }
    });
    let n = CALLS.fetch_add(1, Ordering::Relaxed);
    (
        &pipe.bgl,
        &pipe.pipeline,
        if n == 0 { "new" } else { "cached" },
    )
}

fn readback_u32(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    src: &wgpu::Buffer,
    byte_count: u64,
) -> Result<Vec<u32>, String> {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("dot-rb"),
        size: byte_count,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("dot-rb-enc"),
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
