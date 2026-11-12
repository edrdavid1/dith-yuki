//! Pointwise grade GPU path (Invert / Threshold / Posterize / Adjust / Levels / Curves).

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::context::GpuContext;
use crate::dispatch::{
    map_read_with_timeout, TileUniforms, CORE_SIZE, FLOATS_PER_TILE, MAP_TIMEOUT_DEFAULT,
    WORKGROUP_SIZE,
};
use crate::graph::{
    AdjustPointwisePassParams, LevelsPassParams, PointwiseOp, PointwisePassParams,
};
use crate::GpuError;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct PointwiseUniforms {
    tile: TileUniforms,
    ctrl: [u32; 4],
    p0: [f32; 4],
    p1: [f32; 4],
    p2: [f32; 4],
}

impl PointwiseUniforms {
    fn from_params(params: &PointwisePassParams, tile_x: u32, tile_y: u32) -> Self {
        let mut ctrl = [0u32; 4];
        let mut p0 = [0.0f32; 4];
        let mut p1 = [0.0f32; 4];
        let mut p2 = [0.0f32; 4];
        match &params.op {
            PointwiseOp::Invert => {
                ctrl[0] = 0;
            }
            PointwiseOp::Threshold { threshold } => {
                ctrl[0] = 1;
                p0[0] = *threshold;
            }
            PointwiseOp::Posterize { levels } => {
                ctrl[0] = 2;
                p0[0] = *levels as f32;
            }
            PointwiseOp::Adjust(a) => {
                ctrl[0] = 3;
                pack_adjust(a, &mut p0, &mut p1, &mut p2);
            }
            PointwiseOp::Levels(l) => {
                ctrl[0] = 4;
                ctrl[1] = l.channel_mask;
                pack_levels(l, &mut p0, &mut p1);
            }
            PointwiseOp::Curves(c) => {
                ctrl[0] = 5;
                ctrl[1] = c.channel;
            }
        }
        Self {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            ctrl,
            p0,
            p1,
            p2,
        }
    }
}

fn pack_adjust(
    a: &AdjustPointwisePassParams,
    p0: &mut [f32; 4],
    p1: &mut [f32; 4],
    p2: &mut [f32; 4],
) {
    let exposure = a.exposure.clamp(-5.0, 5.0);
    let contrast_m = 1.0 + a.contrast;
    let sat_m = if a.saturation >= 0.0 {
        1.0 + a.saturation * 2.0
    } else {
        1.0 + a.saturation
    };
    p0[0] = 2f32.powf(exposure);
    p0[1] = contrast_m;
    p0[2] = a.brightness;
    p0[3] = sat_m;
    p1[0] = a.shadows.clamp(-1.0, 1.0);
    p1[1] = a.highlights.clamp(-1.0, 1.0);
    p1[2] = a.temperature.clamp(-1.0, 1.0);
    p1[3] = a.tint.clamp(-1.0, 1.0);
    p2[0] = a.vibrance.clamp(-1.0, 1.0);
    p2[1] = a.hue.clamp(-180.0, 180.0);
}

fn pack_levels(l: &LevelsPassParams, p0: &mut [f32; 4], p1: &mut [f32; 4]) {
    p0[0] = l.input_black;
    p0[1] = l.input_white;
    p0[2] = l.gamma;
    p0[3] = l.output_black;
    p1[0] = l.output_white;
}

pub(crate) fn pointwise_uniform_bytes(
    params: &PointwisePassParams,
    tile_x: u32,
    tile_y: u32,
) -> PointwiseUniforms {
    PointwiseUniforms::from_params(params, tile_x, tile_y)
}

pub(crate) fn pointwise_lut_bytes(params: &PointwisePassParams) -> Arc<[f32]> {
    match &params.op {
        PointwiseOp::Curves(c) => Arc::clone(&c.lut),
        _ => Arc::from([0.0f32]),
    }
}

#[derive(Clone, Debug)]
pub struct PointwiseGpuParams {
    pub pass: PointwisePassParams,
    pub tile_x: u32,
    pub tile_y: u32,
}

pub(crate) struct PointwisePipeline {
    pub layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::ComputePipeline,
}

impl PointwisePipeline {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("pointwise-bgl"),
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
            label: Some("pointwise-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/pointwise.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pointwise-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("pointwise-main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self { layout, pipeline })
    }
}

pub fn apply_pointwise_gpu(
    ctx: &GpuContext,
    input: &[f32],
    params: PointwiseGpuParams,
) -> Result<Vec<f32>, GpuError> {
    let pipe = ctx
        .pointwise
        .as_ref()
        .ok_or(GpuError::Pipeline("pointwise"))?;
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
    let uniforms = PointwiseUniforms::from_params(&params.pass, params.tile_x, params.tile_y);
    let lut = pointwise_lut_bytes(&params.pass);

    let input_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pointwise-tile-in"),
        contents: bytemuck::cast_slice(input),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pointwise-tile-out"),
        size: (FLOATS_PER_TILE * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pointwise-uniforms"),
        contents: bytemuck::bytes_of(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let lut_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("pointwise-lut1d"),
        contents: bytemuck::cast_slice(lut.as_ref()),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("pointwise-bg"),
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
                resource: lut_buf.as_entire_binding(),
            },
        ],
    });

    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pointwise-staging"),
        size: (FLOATS_PER_TILE * std::mem::size_of::<f32>()) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("pointwise-enc"),
    });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("pointwise-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipe.pipeline);
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
