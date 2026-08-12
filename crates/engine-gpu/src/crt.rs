//! CRT GPU path.

use bytemuck::{Pod, Zeroable};

use crate::context::GpuContext;
use crate::dispatch::{dispatch_rgba32, TileUniforms, MAP_TIMEOUT_DEFAULT};
use crate::GpuError;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CrtUniforms {
    tile: TileUniforms,
    /// period, strength, mask_strength, unused
    params: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct CrtGpuParams {
    pub period: u8,
    pub strength: f32,
    pub mask_strength: f32,
    pub tile_x: u32,
    pub tile_y: u32,
}

pub(crate) struct CrtPipeline {
    pub layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::ComputePipeline,
}

impl CrtPipeline {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("crt-bgl"),
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
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crt-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/crt.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("crt-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("crt-main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self { layout, pipeline })
    }
}

pub fn apply_crt_gpu(
    ctx: &GpuContext,
    input: &[f32],
    params: CrtGpuParams,
) -> Result<Vec<f32>, GpuError> {
    let pipe = ctx.crt.as_ref().ok_or(GpuError::Pipeline("crt"))?;

    let uniforms = CrtUniforms {
        tile: TileUniforms::for_tile(params.tile_x, params.tile_y),
        params: [
            params.period as f32,
            params.strength,
            params.mask_strength,
            0.0,
        ],
    };

    dispatch_rgba32(
        ctx,
        &pipe.pipeline,
        &pipe.layout,
        bytemuck::bytes_of(&uniforms),
        input,
        MAP_TIMEOUT_DEFAULT,
    )
}
