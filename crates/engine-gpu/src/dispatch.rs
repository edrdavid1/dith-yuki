//! Shared RGBA32 float upload / dispatch / download.

use std::sync::Arc;
use std::time::{Duration, Instant};

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::context::GpuContext;
use crate::GpuError;

/// Core tile edge length (no halo) — matches `engine_tiles::TILE_SIZE`.
pub const CORE_SIZE: u32 = 256;
pub const WORKGROUP_SIZE: u32 = 16;
pub const FLOATS_PER_TILE: usize = (CORE_SIZE as usize) * (CORE_SIZE as usize) * 4;
pub const MAP_TIMEOUT_DEFAULT: Duration = Duration::from_millis(2_000);

/// Shared tile uniforms (first fields of every pattern shader).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct TileUniforms {
    pub tile_offset: [u32; 2],
    pub size: [u32; 2],
}

impl TileUniforms {
    pub fn for_tile(tile_x: u32, tile_y: u32) -> Self {
        Self {
            tile_offset: [tile_x * CORE_SIZE, tile_y * CORE_SIZE],
            size: [CORE_SIZE, CORE_SIZE],
        }
    }
}

pub fn core_pixel_count() -> usize {
    (CORE_SIZE as usize) * (CORE_SIZE as usize)
}

/// Poll `map_async` until ready or `timeout`. On failure increments `map_timeout_counter`.
pub fn map_read_with_timeout(
    ctx: &GpuContext,
    buffer: &wgpu::Buffer,
    timeout: Duration,
) -> Result<(), GpuError> {
    if ctx
        .force_map_timeout
        .swap(false, std::sync::atomic::Ordering::Relaxed)
    {
        ctx.record_map_timeout();
        return Err(GpuError::MapTimeout);
    }

    let (tx, rx) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });

    let deadline = Instant::now() + timeout;
    loop {
        ctx.device.poll(wgpu::Maintain::Poll);
        match rx.try_recv() {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => {
                ctx.record_map_timeout();
                return Err(GpuError::Device(format!("map_async: {e}")));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                if Instant::now() >= deadline {
                    ctx.record_map_timeout();
                    return Err(GpuError::MapTimeout);
                }
                std::thread::sleep(Duration::from_micros(200));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                ctx.record_map_timeout();
                return Err(GpuError::MapTimeout);
            }
        }
    }
}

/// Forced-timeout helper for tests: poll with zero deadline → counter++.
/// Allow unused helpers kept for test/fault injection.
#[allow(dead_code)]
pub fn map_read_force_timeout(ctx: &GpuContext, buffer: &wgpu::Buffer) -> Result<(), GpuError> {
    map_read_with_timeout(ctx, buffer, Duration::ZERO)
}

/// Dispatch a compute pipeline over a core RGBA32 float tile.
///
/// `input` / returned `output` are tightly packed `width*height*4` floats (core only).
pub fn dispatch_rgba32(
    ctx: &GpuContext,
    pipeline: &wgpu::ComputePipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    uniform_bytes: &[u8],
    input: &[f32],
    timeout: Duration,
) -> Result<Vec<f32>, GpuError> {
    if input.len() != FLOATS_PER_TILE {
        return Err(GpuError::Device(format!(
            "expected {FLOATS_PER_TILE} floats, got {}",
            input.len()
        )));
    }

    let _guard = ctx
        .submit_lock
        .lock()
        .map_err(|_| GpuError::Device("submit mutex poisoned".into()))?;

    let device = &ctx.device;
    let queue = &ctx.queue;

    let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gpu-tile-in"),
        contents: bytemuck::cast_slice(input),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu-tile-out"),
        size: (FLOATS_PER_TILE * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gpu-tile-uniforms"),
        contents: uniform_bytes,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gpu-tile-bg"),
        layout: bind_group_layout,
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
        ],
    });

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu-tile-staging"),
        size: (FLOATS_PER_TILE * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("gpu-tile-enc"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gpu-tile-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }
    encoder.copy_buffer_to_buffer(
        &output_buf,
        0,
        &staging,
        0,
        (FLOATS_PER_TILE * std::mem::size_of::<f32>()) as u64,
    );
    queue.submit(Some(encoder.finish()));

    map_read_with_timeout(ctx, &staging, timeout)?;

    let view = staging.slice(..).get_mapped_range();
    let out: Vec<f32> = bytemuck::cast_slice(&view).to_vec();
    drop(view);
    staging.unmap();

    if out.len() != FLOATS_PER_TILE {
        return Err(GpuError::Device("staging size mismatch".into()));
    }
    Ok(out)
}

/// Same as [`dispatch_rgba32`] but keeps `Arc` unused placeholder for future shared buffers.
#[allow(dead_code)]
pub fn dispatch_rgba32_arc(
    ctx: &Arc<GpuContext>,
    pipeline: &wgpu::ComputePipeline,
    bind_group_layout: &wgpu::BindGroupLayout,
    uniform_bytes: &[u8],
    input: &[f32],
    timeout: Duration,
) -> Result<Vec<f32>, GpuError> {
    dispatch_rgba32(ctx, pipeline, bind_group_layout, uniform_bytes, input, timeout)
}
