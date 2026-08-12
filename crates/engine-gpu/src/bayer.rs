//! Bayer ordered dither — WGSL compute.

use bytemuck::{Pod, Zeroable};

use crate::context::GpuContext;
use crate::dispatch::{dispatch_rgba32, TileUniforms, MAP_TIMEOUT_DEFAULT};
use crate::GpuError;

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayerMatrixSize {
    Bayer2 = 2,
    Bayer4 = 4,
    Bayer8 = 8,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BayerUniforms {
    tile: TileUniforms,
    /// matrix_n, levels, threshold_scale, color_mode (0=rgb, 1=gray)
    params: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct BayerGpuParams {
    pub matrix: BayerMatrixSize,
    pub levels: u16,
    pub threshold_scale: f32,
    /// 0 = RGB, 1 = Grayscale
    pub color_mode: u32,
    pub tile_x: u32,
    pub tile_y: u32,
}

pub(crate) struct BayerPipelines {
    pub layout: wgpu::BindGroupLayout,
    pub pipe2: wgpu::ComputePipeline,
    pub pipe4: wgpu::ComputePipeline,
    pub pipe8: wgpu::ComputePipeline,
}

impl BayerPipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bayer-bgl"),
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
            label: Some("bayer-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bayer.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bayer-pl"),
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
            pipe2: make("bayer2_main"),
            pipe4: make("bayer4_main"),
            pipe8: make("bayer8_main"),
        })
    }

    fn pipeline_for(&self, m: BayerMatrixSize) -> &wgpu::ComputePipeline {
        match m {
            BayerMatrixSize::Bayer2 => &self.pipe2,
            BayerMatrixSize::Bayer4 => &self.pipe4,
            BayerMatrixSize::Bayer8 => &self.pipe8,
        }
    }
}

/// Apply Bayer dither on a core RGBA32 float buffer.
pub fn apply_bayer_gpu(
    ctx: &GpuContext,
    input: &[f32],
    params: BayerGpuParams,
) -> Result<Vec<f32>, GpuError> {
    let pipes = ctx
        .bayer
        .as_ref()
        .ok_or(GpuError::Pipeline("bayer"))?;

    let uniforms = BayerUniforms {
        tile: TileUniforms::for_tile(params.tile_x, params.tile_y),
        params: [
            params.matrix as u32 as f32,
            params.levels as f32,
            params.threshold_scale,
            params.color_mode as f32,
        ],
    };

    dispatch_rgba32(
        ctx,
        pipes.pipeline_for(params.matrix),
        &pipes.layout,
        bytemuck::bytes_of(&uniforms),
        input,
        MAP_TIMEOUT_DEFAULT,
    )
}
