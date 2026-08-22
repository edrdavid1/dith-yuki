//! Viewport gather compute pass — resident tile core → RGBA8 staging.

use crate::dispatch::{CORE_SIZE, WORKGROUP_SIZE};
use crate::GpuError;

pub struct ResidentGatherPipelines {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl ResidentGatherPipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-gather-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
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
            label: Some("viewport-gather-wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/viewport_gather.wgsl").into(),
            ),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-gather-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gather_main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("gather_main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self { layout, pipeline })
    }

    fn layer_view(texture: &wgpu::Texture, layer: u32) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("gather-resident-layer"),
            format: Some(wgpu::TextureFormat::Rgba32Float),
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: layer,
            array_layer_count: Some(1),
            aspect: wgpu::TextureAspect::All,
            usage: None,
        })
    }

    /// Encode gather: resident[slot] core → `readback` storage buffer.
    pub fn encode_gather(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        resident: &wgpu::Texture,
        resident_layer: u32,
        readback: &wgpu::Buffer,
    ) {
        let in_view = Self::layer_view(resident, resident_layer);
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-gather-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: readback.as_entire_binding(),
                },
            ],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-gather-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }
}
