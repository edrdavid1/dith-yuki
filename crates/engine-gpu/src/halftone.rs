//! CMYK Halftone GPU path.

use bytemuck::{Pod, Zeroable};

use crate::context::GpuContext;
use crate::dispatch::{dispatch_rgba32, TileUniforms, MAP_TIMEOUT_DEFAULT};
use crate::GpuError;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct HalftoneUniforms {
    tile: TileUniforms,
    /// cell_size, threshold_scale, unused, unused
    params: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct HalftoneGpuParams {
    pub cell_size: u8,
    pub threshold_scale: f32,
    pub tile_x: u32,
    pub tile_y: u32,
}

pub(crate) struct HalftonePipeline {
    pub layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::ComputePipeline,
}

impl HalftonePipeline {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("halftone-bgl"),
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
            label: Some("halftone-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/halftone.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("halftone-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("halftone-main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self { layout, pipeline })
    }
}

pub fn apply_halftone_gpu(
    ctx: &GpuContext,
    input: &[f32],
    params: HalftoneGpuParams,
) -> Result<Vec<f32>, GpuError> {
    let pipe = ctx
        .halftone
        .as_ref()
        .ok_or(GpuError::Pipeline("halftone"))?;

    let uniforms = HalftoneUniforms {
        tile: TileUniforms::for_tile(params.tile_x, params.tile_y),
        params: [
            params.cell_size as f32,
            params.threshold_scale,
            0.0,
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
