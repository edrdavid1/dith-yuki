//! Glow / bloom GPU path (full-tile storage buffers, parity tests).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::context::GpuContext;
use crate::dispatch::{map_read_with_timeout, MAP_TIMEOUT_DEFAULT};
use crate::resident::TILE_EXTENT;
use crate::GpuError;

const FLOATS_FULL: usize = (TILE_EXTENT as usize) * (TILE_EXTENT as usize) * 4;
const FLOATS_RGB: usize = (TILE_EXTENT as usize) * (TILE_EXTENT as usize) * 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GlowUniforms {
    /// radius, intensity, threshold, softness
    params: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct GlowGpuParams {
    pub radius: f32,
    pub intensity: f32,
    pub threshold: f32,
    pub softness: u8,
}

pub(crate) struct GlowPipeline {
    pub layout: wgpu::BindGroupLayout,
    pub mask: wgpu::ComputePipeline,
    pub blur_h: wgpu::ComputePipeline,
    pub blur_v: wgpu::ComputePipeline,
    pub composite: wgpu::ComputePipeline,
}

impl GlowPipeline {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glow-bgl"),
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
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glow-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/glow.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glow-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let make = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };

        Ok(Self {
            layout,
            mask: make("mask_main"),
            blur_h: make("blur_h_main"),
            blur_v: make("blur_v_main"),
            composite: make("composite_main"),
        })
    }
}

/// Apply glow on a full 260² RGBA32f tile (matches CPU `PixelTile` layout).
pub fn apply_glow_gpu(
    ctx: &GpuContext,
    input: &[f32],
    params: GlowGpuParams,
) -> Result<Vec<f32>, GpuError> {
    if input.len() != FLOATS_FULL {
        return Err(GpuError::Device(format!(
            "glow input len {} != {}",
            input.len(),
            FLOATS_FULL
        )));
    }
    let pipe = ctx.glow.as_ref().ok_or(GpuError::Pipeline("glow"))?;
    let softness = params.softness.clamp(1, 3);

    let uniforms = GlowUniforms {
        params: [
            params.radius.clamp(0.5, 16.0),
            params.intensity.clamp(0.0, 4.0),
            params.threshold.clamp(0.0, 1.0),
            softness as f32,
        ],
    };

    let _guard = ctx
        .submit_lock
        .lock()
        .map_err(|_| GpuError::Device("glow submit_lock".into()))?;

    let uniform_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("glow-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
    let input_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("glow-input"),
            contents: bytemuck::cast_slice(input),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let output_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glow-output"),
        size: (FLOATS_FULL * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let scratch_a = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glow-scratch-a"),
        size: (FLOATS_RGB * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let scratch_b = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glow-scratch-b"),
        size: (FLOATS_RGB * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let readback = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("glow-readback"),
        size: (FLOATS_FULL * 4) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let bind = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("glow-bg"),
        layout: &pipe.layout,
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
                resource: scratch_a.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: scratch_b.as_entire_binding(),
            },
        ],
    });

    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("glow-enc"),
        });
    let groups = (TILE_EXTENT + 15) / 16;

    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("glow-mask"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipe.mask);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(groups, groups, 1);
    }
    for _ in 0..softness {
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("glow-h"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipe.blur_h);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups, groups, 1);
        }
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("glow-v"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipe.blur_v);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups, groups, 1);
        }
    }
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("glow-comp"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipe.composite);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(groups, groups, 1);
    }

    encoder.copy_buffer_to_buffer(&output_buf, 0, &readback, 0, (FLOATS_FULL * 4) as u64);
    ctx.queue.submit(Some(encoder.finish()));

    map_read_with_timeout(ctx, &readback, MAP_TIMEOUT_DEFAULT)?;
    let slice = readback.slice(..);
    let data = slice.get_mapped_range();
    let out: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    readback.unmap();
    Ok(out)
}
