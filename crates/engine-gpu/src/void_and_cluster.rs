//! Void-and-cluster ordered dither — WGSL compute (cold RGBA32 path).
//!
//! Apply-only GPU path using the precomputed 64² rank matrix. Matrix build
//! stays on CPU ([`void_and_cluster_matrix`]); Path B resident dispatch lives
//! in `resident::pipelines`.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::context::GpuContext;
use crate::dispatch::{
    map_read_with_timeout, TileUniforms, CORE_SIZE, FLOATS_PER_TILE, MAP_TIMEOUT_DEFAULT,
    WORKGROUP_SIZE,
};
use crate::void_and_cluster_matrix::{ranks, SIZE};
use crate::GpuError;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VacUniforms {
    tile: TileUniforms,
    /// levels, threshold_scale, color_mode, unused
    params: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct VoidAndClusterGpuParams {
    pub levels: u16,
    pub threshold_scale: f32,
    /// 0 = RGB, 1 = Grayscale (+2 dither_alpha)
    pub color_mode: u32,
    pub tile_x: u32,
    pub tile_y: u32,
}

pub(crate) struct VoidAndClusterPipelines {
    pub layout: wgpu::BindGroupLayout,
    pub pipe: wgpu::ComputePipeline,
    /// Persistent rank buffer (u32[4096]).
    pub ranks_buf: wgpu::Buffer,
}

impl VoidAndClusterPipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("vac-bgl"),
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
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("vac-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/void_and_cluster.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("vac-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipe = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("vac_main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("vac_main"),
            compilation_options: Default::default(),
            cache: None,
        });

        let ranks_u32: Vec<u32> = ranks().iter().map(|&r| r as u32).collect();
        debug_assert_eq!(ranks_u32.len(), SIZE * SIZE);
        let ranks_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("vac-ranks"),
            contents: bytemuck::cast_slice(&ranks_u32),
            usage: wgpu::BufferUsages::STORAGE,
        });

        Ok(Self {
            layout,
            pipe,
            ranks_buf,
        })
    }
}

/// Apply void-and-cluster dither on a core RGBA32 float buffer.
pub fn apply_void_and_cluster_gpu(
    ctx: &GpuContext,
    input: &[f32],
    params: VoidAndClusterGpuParams,
) -> Result<Vec<f32>, GpuError> {
    let pipes = ctx
        .void_and_cluster
        .as_ref()
        .ok_or(GpuError::Pipeline("void_and_cluster"))?;

    if input.len() != FLOATS_PER_TILE {
        return Err(GpuError::Device(format!(
            "expected {FLOATS_PER_TILE} floats, got {}",
            input.len()
        )));
    }

    let uniforms = VacUniforms {
        tile: TileUniforms::for_tile(params.tile_x, params.tile_y),
        params: [
            params.levels as f32,
            params.threshold_scale,
            params.color_mode as f32,
            0.0,
        ],
    };

    let _guard = ctx
        .submit_lock
        .lock()
        .map_err(|_| GpuError::Device("submit mutex poisoned".into()))?;

    let device = &ctx.device;
    let queue = &ctx.queue;

    let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vac-tile-in"),
        contents: bytemuck::cast_slice(input),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vac-tile-out"),
        size: (FLOATS_PER_TILE * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("vac-uniforms"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("vac-bg"),
        layout: &pipes.layout,
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
                resource: pipes.ranks_buf.as_entire_binding(),
            },
        ],
    });

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vac-staging"),
        size: (FLOATS_PER_TILE * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("vac-enc"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("vac-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipes.pipe);
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

    map_read_with_timeout(ctx, &staging, MAP_TIMEOUT_DEFAULT)?;

    let view = staging.slice(..).get_mapped_range();
    let out: Vec<f32> = bytemuck::cast_slice(&view).to_vec();
    drop(view);
    staging.unmap();
    Ok(out)
}

/// Process-stable rank table (same values as `engine_project::filters::void_and_cluster::ranks`).
pub fn void_and_cluster_ranks() -> &'static [u16] {
    ranks()
}
