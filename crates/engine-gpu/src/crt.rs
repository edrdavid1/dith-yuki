//! CRT GPU path.

use bytemuck::{Pod, Zeroable};

use crate::context::GpuContext;
use crate::dispatch::{dispatch_rgba32, TileUniforms, MAP_TIMEOUT_DEFAULT};
use crate::graph::CrtPassParams;
use crate::GpuError;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CrtUniforms {
    tile: TileUniforms,
    p0: [f32; 4],
    p1: [f32; 4],
    p2: [f32; 4],
    p3: [f32; 4],
}

impl CrtUniforms {
    fn from_params(tile_x: u32, tile_y: u32, p: CrtPassParams) -> Self {
        Self {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            p0: [
                p.period as f32,
                p.strength,
                p.mask_strength,
                p.soft_scanlines,
            ],
            p1: [
                p.line_width as f32,
                p.orientation as f32,
                p.mask_type as f32,
                p.brightness_compensate,
            ],
            p2: [p.curvature, p.vignette, p.phosphor_bleed, p.sharpness],
            p3: [p.doc_w as f32, p.doc_h as f32, p.scale as f32, 0.0],
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CrtGpuParams {
    pub period: u8,
    pub strength: f32,
    pub mask_strength: f32,
    pub soft_scanlines: f32,
    pub line_width: u8,
    pub scale: u8,
    pub orientation: u8,
    pub mask_type: u8,
    pub brightness_compensate: f32,
    pub curvature: f32,
    pub vignette: f32,
    pub phosphor_bleed: f32,
    pub sharpness: f32,
    pub doc_w: u32,
    pub doc_h: u32,
    pub tile_x: u32,
    pub tile_y: u32,
}

impl CrtGpuParams {
    pub fn basic(period: u8, strength: f32, mask_strength: f32, tile_x: u32, tile_y: u32) -> Self {
        Self {
            period,
            strength,
            mask_strength,
            soft_scanlines: 0.0,
            line_width: 1,
            scale: 1,
            orientation: 0,
            mask_type: 0,
            brightness_compensate: 0.0,
            curvature: 0.0,
            vignette: 0.0,
            phosphor_bleed: 0.0,
            sharpness: 0.0,
            doc_w: 1024,
            doc_h: 1024,
            tile_x,
            tile_y,
        }
    }

    fn to_pass(self) -> CrtPassParams {
        CrtPassParams {
            period: self.period,
            strength: self.strength,
            mask_strength: self.mask_strength,
            soft_scanlines: self.soft_scanlines,
            line_width: self.line_width,
            scale: self.scale,
            orientation: self.orientation,
            mask_type: self.mask_type,
            brightness_compensate: self.brightness_compensate,
            curvature: self.curvature,
            vignette: self.vignette,
            phosphor_bleed: self.phosphor_bleed,
            sharpness: self.sharpness,
            doc_w: self.doc_w,
            doc_h: self.doc_h,
        }
    }
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

    let uniforms = CrtUniforms::from_params(params.tile_x, params.tile_y, params.to_pass());

    dispatch_rgba32(
        ctx,
        &pipe.pipeline,
        &pipe.layout,
        bytemuck::bytes_of(&uniforms),
        input,
        MAP_TIMEOUT_DEFAULT,
    )
}
