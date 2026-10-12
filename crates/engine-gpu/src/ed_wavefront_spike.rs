//! §0.3 spike: exact Floyd–Steinberg on GPU via wavefront `t = x + 2y`.
//!
//! Fixed-point Q8.8. One workgroup processes an entire tile-row (tiles left→right
//! with residual carry), host submits one dispatch per tile-row.
//!
//! ```bash
//! cargo run -p engine-gpu --example ed_wavefront_spike --release
//! ```

use std::io::{self, Write};
use std::time::Instant;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

const Q8: i32 = 8;
const MAX_U8_Q8: i32 = 255 << Q8;
const TILE: u32 = 64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SpikeParams {
    img_w: u32,
    img_h: u32,
    tile_y: u32,
    tile_h: u32,
    tiles_x: u32,
    top_read: u32,
    top_write: u32,
    _pad: u32,
}

/// One dispatch = one tile row. Loops tiles left→right inside the workgroup.
const WAVEFRONT_ROW_SHADER: &str = r#"
struct SpikeParams {
    img_w: u32,
    img_h: u32,
    tile_y: u32,
    tile_h: u32,
    tiles_x: u32,
    top_read: u32,
    top_write: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> params: SpikeParams;
@group(0) @binding(1) var<storage, read> input_px: array<u32>;
@group(0) @binding(2) var<storage, read_write> output_px: array<u32>;
@group(0) @binding(3) var<storage, read_write> top_res: array<i32>;

var<workgroup> err_shared: array<atomic<i32>, 4356>;
var<workgroup> left_res: array<i32, 64>;

fn err_idx(x: i32, y: i32) -> u32 {
    return u32(y + 1) * 66u + u32(x + 1);
}

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_index) li: u32) {
    let th = params.tile_h;

    // Clear left residual for start of row.
    if (li < 64u) {
        left_res[li] = 0;
    }
    workgroupBarrier();

    for (var tx = 0u; tx < params.tiles_x; tx++) {
        let tile_x = tx * 64u;
        let tw = min(64u, params.img_w - tile_x);
        let top_r = params.top_read + tx * 65u;
        let top_w = params.top_write + tx * 65u;

        for (var i = li; i < 4356u; i += 64u) {
            atomicStore(&err_shared[i], 0);
        }
        workgroupBarrier();

        if (li < th) {
            atomicAdd(&err_shared[err_idx(0, i32(li))], left_res[li]);
        }
        if (li <= tw) {
            atomicAdd(&err_shared[err_idx(i32(li), 0)], top_res[top_r + li]);
        }
        workgroupBarrier();

        let t_max = (tw - 1u) + 2u * (th - 1u);
        for (var t = 0u; t <= t_max; t++) {
            if (li < th) {
                let ly = li;
                let lx_i = i32(t) - 2 * i32(ly);
                if (lx_i >= 0 && lx_i < i32(tw)) {
                    let lx = u32(lx_i);
                    let gx = tile_x + lx;
                    let gy = params.tile_y + ly;
                    if (gx < params.img_w && gy < params.img_h) {
                        let gi = gy * params.img_w + gx;
                        let v_q8 = i32(input_px[gi]) << 8;
                        let adjusted = v_q8 + atomicLoad(&err_shared[err_idx(i32(lx), i32(ly))]);
                        let clamped = clamp(adjusted, 0, 255 << 8);
                        let color = select(0, 255, clamped >= (128 << 8));
                        let error = clamped - (color << 8);
                        output_px[gi] = u32(color);

                        let e7 = (error * 7) >> 4;
                        let e3 = (error * 3) >> 4;
                        let e5 = (error * 5) >> 4;
                        let e1 = error - e7 - e3 - e5;

                        atomicAdd(&err_shared[err_idx(i32(lx) + 1, i32(ly))], e7);
                        atomicAdd(&err_shared[err_idx(i32(lx) - 1, i32(ly) + 1)], e3);
                        atomicAdd(&err_shared[err_idx(i32(lx), i32(ly) + 1)], e5);
                        atomicAdd(&err_shared[err_idx(i32(lx) + 1, i32(ly) + 1)], e1);
                    }
                }
            }
            workgroupBarrier();
        }

        if (li < th) {
            left_res[li] = atomicLoad(&err_shared[err_idx(i32(tw), i32(li))]);
        }
        if (li < tw) {
            top_res[top_w + li] = atomicLoad(&err_shared[err_idx(i32(li), i32(th))]);
        }
        if (li == 0u) {
            top_res[top_w + tw] = atomicLoad(&err_shared[err_idx(i32(tw), i32(th))]);
        }
        workgroupBarrier();
    }
}
"#;

fn request_device() -> Option<(wgpu::Device, wgpu::Queue, String)> {
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
    let backend = format!(
        "{} ({:?}; wg_storage={} storage_bind={})",
        info.name,
        info.backend,
        al.max_compute_workgroup_storage_size,
        al.max_storage_buffer_binding_size
    );
    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("ed-wavefront-spike"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
        },
        None,
    ))
    .ok()?;
    Some((device, queue, backend))
}

/// CPU exact FS, fixed-point Q8.8, bilevel {0,255}, remainder-preserving weights.
pub fn cpu_fs_q8(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut err = vec![0i32; w * h];
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let v_q8 = (src[i] as i32) << Q8;
            let adjusted = v_q8 + err[i];
            let clamped = adjusted.clamp(0, MAX_U8_Q8);
            let color: u8 = if clamped >= (128 << Q8) { 255 } else { 0 };
            let error = clamped - ((color as i32) << Q8);
            out[i] = color;

            let e7 = (error * 7) >> 4;
            let e3 = (error * 3) >> 4;
            let e5 = (error * 5) >> 4;
            let e1 = error - e7 - e3 - e5;

            let mut add = |bx: i32, by: i32, e: i32| {
                if bx >= 0 && by >= 0 && (bx as usize) < w && (by as usize) < h {
                    err[by as usize * w + bx as usize] += e;
                }
            };
            add(x as i32 + 1, y as i32, e7);
            add(x as i32 - 1, y as i32 + 1, e3);
            add(x as i32, y as i32 + 1, e5);
            add(x as i32 + 1, y as i32 + 1, e1);
        }
    }
    out
}

/// CPU wavefront order (`t = x + 2y`) — must match row-major FS.
pub fn cpu_fs_q8_wavefront(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut err = vec![0i32; (w + 2) * (h + 2)];
    let mut out = vec![0u8; w * h];
    let stride = w + 2;
    let t_max = (w - 1) + 2 * (h - 1);
    for t in 0..=t_max {
        for y in 0..h {
            let x_i = t as i32 - 2 * y as i32;
            if x_i < 0 || x_i >= w as i32 {
                continue;
            }
            let x = x_i as usize;
            let ei = (y + 1) * stride + (x + 1);
            let v_q8 = (src[y * w + x] as i32) << Q8;
            let adjusted = v_q8 + err[ei];
            let clamped = adjusted.clamp(0, MAX_U8_Q8);
            let color: u8 = if clamped >= (128 << Q8) { 255 } else { 0 };
            let error = clamped - ((color as i32) << Q8);
            out[y * w + x] = color;

            let e7 = (error * 7) >> 4;
            let e3 = (error * 3) >> 4;
            let e5 = (error * 5) >> 4;
            let e1 = error - e7 - e3 - e5;

            err[ei + 1] += e7;
            err[ei + stride - 1] += e3;
            err[ei + stride] += e5;
            err[ei + stride + 1] += e1;
        }
    }
    out
}

fn synthetic_gradient_u8(w: usize, h: usize) -> Vec<u8> {
    let mut v = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let fx = if w > 1 {
                x as f32 / (w - 1) as f32
            } else {
                0.0
            };
            let fy = if h > 1 {
                y as f32 / (h - 1) as f32
            } else {
                0.0
            };
            v[y * w + x] = ((fx * 0.7 + fy * 0.3) * 255.0).round() as u8;
        }
    }
    v
}

fn mismatches(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

pub struct SpikeResult {
    pub label: &'static str,
    pub n: u32,
    pub cpu_ms: f64,
    pub gpu_ms: f64,
    pub mismatches: usize,
    pub backend: String,
    pub speedup: f64,
}

fn progress(msg: &str) {
    let _ = writeln!(io::stderr(), "{msg}");
    let _ = io::stderr().flush();
}

pub fn run_wavefront_tiled_gpu(n: u32) -> Option<SpikeResult> {
    progress(&format!("--- n={n}: request device ---"));
    let (device, queue, backend) = request_device()?;
    let n_usize = n as usize;
    progress(&format!("--- n={n}: build synthetic input ---"));
    let src = synthetic_gradient_u8(n_usize, n_usize);

    progress(&format!("--- n={n}: CPU reference ---"));
    let t_cpu = Instant::now();
    let cpu_out = cpu_fs_q8(&src, n_usize, n_usize);
    let cpu_ms = t_cpu.elapsed().as_secs_f64() * 1000.0;
    progress(&format!("--- n={n}: CPU done in {cpu_ms:.1}ms ---"));

    if n <= 512 {
        let cpu_wf = cpu_fs_q8_wavefront(&src, n_usize, n_usize);
        if mismatches(&cpu_out, &cpu_wf) != 0 {
            return Some(SpikeResult {
                label: "cpu-wavefront-order-broken",
                n,
                cpu_ms,
                gpu_ms: 0.0,
                mismatches: mismatches(&cpu_out, &cpu_wf),
                backend,
                speedup: 0.0,
            });
        }
    }

    let src_u32: Vec<u32> = src.iter().map(|&b| b as u32).collect();
    let byte_count = (n_usize * n_usize * 4) as u64;

    progress(&format!("--- n={n}: upload + pipeline ---"));
    let t_gpu = Instant::now();

    let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("spike-in"),
        contents: bytemuck::cast_slice(&src_u32),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spike-out"),
        size: byte_count,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spike-rb"),
        size: byte_count,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let tiles_x = n.div_ceil(TILE);
    let plane = (tiles_x * (TILE + 1)) as u64;
    let top_zeros = vec![0i32; (plane * 2) as usize];
    let top_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("spike-top"),
        contents: bytemuck::cast_slice(&top_zeros),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("spike-wf-row"),
        source: wgpu::ShaderSource::Wgsl(WAVEFRONT_ROW_SHADER.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("spike-bgl"),
        entries: &[
            bind_entry(0, true, false),
            bind_entry(1, false, true),
            bind_entry(2, false, false),
            bind_entry(3, false, false),
        ],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("spike-pl"),
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("spike-pipe"),
        layout: Some(&pl),
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("spike-u"),
        size: std::mem::size_of::<SpikeParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("spike-bg"),
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: input_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: output_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: top_buf.as_entire_binding(),
            },
        ],
    });

    let tiles_y = n.div_ceil(TILE);
    let clear_plane = vec![0i32; plane as usize];

    for ty in 0..tiles_y {
        let tile_y = ty * TILE;
        let tile_h = (n - tile_y).min(TILE);
        let read_plane = ty % 2;
        let write_plane = 1 - read_plane;

        // Clear write plane before this row deposits bottoms.
        queue.write_buffer(
            &top_buf,
            (write_plane as u64) * plane * 4,
            bytemuck::cast_slice(&clear_plane),
        );

        let params = SpikeParams {
            img_w: n,
            img_h: n,
            tile_y,
            tile_h,
            tiles_x,
            top_read: read_plane * (plane as u32),
            top_write: write_plane * (plane as u32),
            _pad: 0,
        };
        queue.write_buffer(&uniform_buf, 0, bytemuck::bytes_of(&params));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("spike-row-enc"),
        });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("spike-row"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit(Some(encoder.finish()));

        if ty == 0 || ty + 1 == tiles_y || ty % 16 == 0 {
            progress(&format!("--- n={n}: tile-row {ty}/{tiles_y} ---"));
        }
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("spike-rb-enc"),
    });
    encoder.copy_buffer_to_buffer(&output_buf, 0, &readback, 0, byte_count);
    queue.submit(Some(encoder.finish()));

    let slice = readback.slice(..);
    let (tx_c, rx_c) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx_c.send(r);
    });
    device.poll(wgpu::Maintain::Wait);
    rx_c.recv().ok()?.ok()?;
    let data = slice.get_mapped_range();
    let gpu_u32: Vec<u32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    readback.unmap();
    let gpu_ms = t_gpu.elapsed().as_secs_f64() * 1000.0;
    let gpu_out: Vec<u8> = gpu_u32.iter().map(|&v| v as u8).collect();
    let mis = mismatches(&cpu_out, &gpu_out);
    let speedup = if gpu_ms > 0.0 { cpu_ms / gpu_ms } else { 0.0 };
    progress(&format!(
        "--- n={n}: GPU done in {gpu_ms:.1}ms, mismatches={mis}, speedup={speedup:.2}x ---"
    ));
    Some(SpikeResult {
        label: "wavefront-tiled-q8",
        n,
        cpu_ms,
        gpu_ms,
        mismatches: mis,
        backend,
        speedup,
    })
}

fn bind_entry(binding: u32, uniform: bool, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: if uniform {
                wgpu::BufferBindingType::Uniform
            } else {
                wgpu::BufferBindingType::Storage { read_only }
            },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// Run spike acceptance suite.
pub fn run_spike() -> Option<Vec<SpikeResult>> {
    let mut results = Vec::new();
    results.push(run_wavefront_tiled_gpu(256)?);
    results.push(run_wavefront_tiled_gpu(4096)?);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_wavefront_tiled_gpu(16384)
    })) {
        Ok(Some(r)) => results.push(r),
        Ok(None) | Err(_) => {
            progress("16K failed (OOM/panic); trying 8K…");
            if let Some(r) = run_wavefront_tiled_gpu(8192) {
                results.push(SpikeResult {
                    label: "wavefront-tiled-q8-8k-fallback",
                    n: r.n,
                    cpu_ms: r.cpu_ms,
                    gpu_ms: r.gpu_ms,
                    mismatches: r.mismatches,
                    backend: r.backend,
                    speedup: r.speedup,
                });
            }
        }
    }
    Some(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavefront_order_matches_row_major_cpu() {
        let src = synthetic_gradient_u8(97, 64);
        let a = cpu_fs_q8(&src, 97, 64);
        let b = cpu_fs_q8_wavefront(&src, 97, 64);
        assert_eq!(mismatches(&a, &b), 0);
    }

    #[test]
    fn remainder_preserving_weights() {
        for e in [-65536i32, -1, 0, 1, 17, 255, 65280, 65536] {
            let e7 = (e * 7) >> 4;
            let e3 = (e * 3) >> 4;
            let e5 = (e * 5) >> 4;
            let e1 = e - e7 - e3 - e5;
            assert_eq!(e7 + e3 + e5 + e1, e);
        }
    }
}
