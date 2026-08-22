//! Off-product Floyd–Steinberg GPU prototype (Path B T8 / Phase 3).
//!
//! Two modes:
//! - **Serial** GPU (one thread, row-major): correctness reference vs CPU
//! - **Naive parallel anti-diagonal**: demonstrates same-diagonal race on FS
//!   weight `(−1,+1)` — not product-viable
//!
//! Not wired into the product graph — research only.

use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const N: u32 = 128;
const LEVELS: f32 = 1.0;
const ERR_SCALE: f32 = 65536.0;

const FS_SERIAL_SHADER: &str = r#"
struct EdMeta {
    n: u32,
    _pad: u32,
    levels: f32,
    err_scale: f32,
}

@group(0) @binding(0) var<uniform> ed: EdMeta;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> work: array<f32>;
@group(0) @binding(3) var<storage, read_write> out_buf: array<f32>;

fn idx(x: u32, y: u32) -> u32 {
    return y * ed.n + x;
}

fn add_err(x: i32, y: i32, delta: f32) {
    let n = i32(ed.n);
    if (x < 0 || y < 0 || x >= n || y >= n) {
        return;
    }
    work[idx(u32(x), u32(y))] += delta;
}

@compute @workgroup_size(1)
fn main() {
    let n = ed.n;
    // Copy src → work
    for (var i = 0u; i < n * n; i++) {
        work[i] = src[i];
    }
    for (var y = 0u; y < n; y++) {
        for (var x = 0u; x < n; x++) {
            let i = idx(x, y);
            let v = clamp(work[i], 0.0, 1.0);
            let q = clamp(round(v * ed.levels), 0.0, ed.levels) / ed.levels;
            out_buf[i] = q;
            let err_v = v - q;
            let xi = i32(x);
            let yi = i32(y);
            add_err(xi + 1, yi,     err_v * (7.0 / 16.0));
            add_err(xi - 1, yi + 1, err_v * (3.0 / 16.0));
            add_err(xi,     yi + 1, err_v * (5.0 / 16.0));
            add_err(xi + 1, yi + 1, err_v * (1.0 / 16.0));
        }
    }
}
"#;

/// Naive parallel anti-diagonal (intentionally racy for FS — research only).
const FS_PARALLEL_SHADER: &str = r#"
struct EdMeta {
    n: u32,
    diagonal: u32,
    levels: f32,
    err_scale: f32,
}

@group(0) @binding(0) var<uniform> ed: EdMeta;
@group(0) @binding(1) var<storage, read> src: array<f32>;
@group(0) @binding(2) var<storage, read_write> err: array<atomic<i32>>;
@group(0) @binding(3) var<storage, read_write> out_buf: array<f32>;

fn idx(x: u32, y: u32) -> u32 {
    return y * ed.n + x;
}

fn add_err(x: i32, y: i32, delta: f32) {
    let n = i32(ed.n);
    if (x < 0 || y < 0 || x >= n || y >= n) {
        return;
    }
    let q = i32(round(delta * ed.err_scale));
    atomicAdd(&err[idx(u32(x), u32(y))], q);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = ed.n;
    let d = ed.diagonal;
    let count = min(d + 1u, 2u * n - 1u - d);
    let start_x = select(0u, d + 1u - n, d >= n);
    if (gid.x >= count) {
        return;
    }
    let x = start_x + gid.x;
    let y = d - x;
    if (x >= n || y >= n) {
        return;
    }
    let i = idx(x, y);
    let e = f32(atomicLoad(&err[i])) / ed.err_scale;
    let v = clamp(src[i] + e, 0.0, 1.0);
    let q = clamp(round(v * ed.levels), 0.0, ed.levels) / ed.levels;
    out_buf[i] = q;
    let err_v = v - q;
    let xi = i32(x);
    let yi = i32(y);
    add_err(xi + 1, yi,     err_v * (7.0 / 16.0));
    add_err(xi - 1, yi + 1, err_v * (3.0 / 16.0));
    add_err(xi,     yi + 1, err_v * (5.0 / 16.0));
    add_err(xi + 1, yi + 1, err_v * (1.0 / 16.0));
}
"#;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct MetaGpu {
    n: u32,
    diagonal_or_pad: u32,
    levels: f32,
    err_scale: f32,
}

fn synthetic_gradient(n: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            v[y * n + x] = (x as f32 / (n - 1) as f32) * 0.7 + (y as f32 / (n - 1) as f32) * 0.3;
        }
    }
    v
}

/// CPU Floyd–Steinberg (single-channel), same kernel as `DiffusionKernel::FloydSteinberg`.
pub fn cpu_floyd_steinberg(src: &[f32], n: usize, levels: f32) -> Vec<f32> {
    let mut buf = src.to_vec();
    let mut out = vec![0.0f32; n * n];
    for y in 0..n {
        for x in 0..n {
            let i = y * n + x;
            let v = buf[i].clamp(0.0, 1.0);
            let q = (v * levels).round().clamp(0.0, levels) / levels;
            out[i] = q;
            let err = v - q;
            let mut add = |bx: i32, by: i32, w: f32| {
                if bx >= 0 && by >= 0 && (bx as usize) < n && (by as usize) < n {
                    buf[by as usize * n + bx as usize] += err * w;
                }
            };
            add(x as i32 + 1, y as i32, 7.0 / 16.0);
            add(x as i32 - 1, y as i32 + 1, 3.0 / 16.0);
            add(x as i32, y as i32 + 1, 5.0 / 16.0);
            add(x as i32 + 1, y as i32 + 1, 1.0 / 16.0);
        }
    }
    out
}

pub struct EdProtoResult {
    pub max_abs_diff: f32,
    pub cpu_ms: f64,
    pub gpu_ms: f64,
    pub n: u32,
    pub mismatches: usize,
    pub mode: &'static str,
}

fn diff_stats(cpu: &[f32], gpu: &[f32]) -> (f32, usize) {
    let mut max_abs = 0.0f32;
    let mut mismatches = 0usize;
    for i in 0..cpu.len() {
        let d = (cpu[i] - gpu[i]).abs();
        if d > max_abs {
            max_abs = d;
        }
        if d > 1e-4 {
            mismatches += 1;
        }
    }
    (max_abs, mismatches)
}

fn request_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("ed-proto"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    ))
    .ok()
}

/// Serial GPU FS — bit-close to CPU; proves shader math, not a speed win.
pub fn run_ed_serial_prototype() -> Option<EdProtoResult> {
    let (device, queue) = request_device()?;
    let n = N as usize;
    let src = synthetic_gradient(n);

    let t_cpu = Instant::now();
    let cpu_out = cpu_floyd_steinberg(&src, n, LEVELS);
    let cpu_ms = t_cpu.elapsed().as_secs_f64() * 1000.0;

    let src_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ed-src"),
        contents: bytemuck::cast_slice(&src),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let work_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ed-work"),
        size: (n * n * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ed-out"),
        size: (n * n * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ed-readback"),
        size: (n * n * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let meta = MetaGpu {
        n: N,
        diagonal_or_pad: 0,
        levels: LEVELS,
        err_scale: ERR_SCALE,
    };
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ed-uniform"),
        contents: bytemuck::bytes_of(&meta),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ed-fs-serial"),
        source: wgpu::ShaderSource::Wgsl(FS_SERIAL_SHADER.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ed-serial-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ed-serial-pl"),
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("ed-serial-pipe"),
        layout: Some(&pl),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ed-serial-bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: src_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: work_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: out_buf.as_entire_binding(),
            },
        ],
    });

    let t_gpu = Instant::now();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ed-serial-enc"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("ed-serial"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&out_buf, 0, &readback, 0, (n * n * 4) as u64);
    queue.submit(Some(encoder.finish()));
    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().ok()?.ok()?;
    let data = slice.get_mapped_range();
    let gpu_out: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    readback.unmap();
    let gpu_ms = t_gpu.elapsed().as_secs_f64() * 1000.0;

    let (max_abs, mismatches) = diff_stats(&cpu_out, &gpu_out);
    Some(EdProtoResult {
        max_abs_diff: max_abs,
        cpu_ms,
        gpu_ms,
        n: N,
        mismatches,
        mode: "serial",
    })
}

/// Naive parallel anti-diagonal — expected to diverge (same-diagonal race).
pub fn run_ed_parallel_prototype() -> Option<EdProtoResult> {
    let (device, queue) = request_device()?;
    let n = N as usize;
    let src = synthetic_gradient(n);
    let t_cpu = Instant::now();
    let cpu_out = cpu_floyd_steinberg(&src, n, LEVELS);
    let cpu_ms = t_cpu.elapsed().as_secs_f64() * 1000.0;

    let src_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ed-par-src"),
        contents: bytemuck::cast_slice(&src),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let err_zeros = vec![0i32; n * n];
    let err_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ed-par-err"),
        contents: bytemuck::cast_slice(&err_zeros),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ed-par-out"),
        size: (n * n * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ed-par-rb"),
        size: (n * n * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ed-par-u"),
        size: std::mem::size_of::<MetaGpu>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ed-fs-parallel"),
        source: wgpu::ShaderSource::Wgsl(FS_PARALLEL_SHADER.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("ed-par-bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ed-par-pl"),
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("ed-par-pipe"),
        layout: Some(&pl),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ed-par-bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: src_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: err_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: out_buf.as_entire_binding(),
            },
        ],
    });

    let t_gpu = Instant::now();
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("ed-par-enc"),
    });
    let mut stagings = Vec::new();
    let max_d = 2 * N - 2;
    for d in 0..=max_d {
        let meta = MetaGpu {
            n: N,
            diagonal_or_pad: d,
            levels: LEVELS,
            err_scale: ERR_SCALE,
        };
        let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ed-par-meta"),
            contents: bytemuck::bytes_of(&meta),
            usage: wgpu::BufferUsages::COPY_SRC,
        });
        encoder.copy_buffer_to_buffer(
            &staging,
            0,
            &uniform_buf,
            0,
            std::mem::size_of::<MetaGpu>() as u64,
        );
        stagings.push(staging);
        let count = if d < N { d + 1 } else { 2 * N - 1 - d };
        let groups = (count + 63) / 64;
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("ed-par-diag"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(groups.max(1), 1, 1);
        }
    }
    encoder.copy_buffer_to_buffer(&out_buf, 0, &readback, 0, (n * n * 4) as u64);
    queue.submit(Some(encoder.finish()));
    drop(stagings);

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv().ok()?.ok()?;
    let data = slice.get_mapped_range();
    let gpu_out: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    readback.unmap();
    let gpu_ms = t_gpu.elapsed().as_secs_f64() * 1000.0;

    let (max_abs, mismatches) = diff_stats(&cpu_out, &gpu_out);
    Some(EdProtoResult {
        max_abs_diff: max_abs,
        cpu_ms,
        gpu_ms,
        n: N,
        mismatches,
        mode: "parallel-naive",
    })
}

/// Default entry used by the example binary.
pub fn run_ed_prototype() -> Option<(EdProtoResult, EdProtoResult)> {
    let serial = run_ed_serial_prototype()?;
    let parallel = run_ed_parallel_prototype()?;
    Some((serial, parallel))
}
