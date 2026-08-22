//! GPU-resident compute pipelines (storage textures).

use std::sync::Mutex;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::dispatch::{TileUniforms, CORE_SIZE, WORKGROUP_SIZE};
use crate::graph::{
    BayerPassParams, CrtPassParams, HalftonePassParams, PaletteGuidedPassParams,
    PaletteMixedPassParams, PaletteQuantizePassParams, GpuPipelineKey,
};
use crate::resident::format::TILE_EXTENT;
use crate::GpuError;

/// Per-frame ring of fixed-size GPU buffers for uniforms / small storage uploads.
///
/// Each pass in one encoder must get its **own** buffer: a single shared buffer
/// rewritten with `queue.write_buffer` would show only the last write to all passes.
struct UploadBufferPool {
    label: &'static str,
    size: u64,
    usage: wgpu::BufferUsages,
    state: Mutex<(usize, Vec<wgpu::Buffer>)>,
}

impl UploadBufferPool {
    fn new(label: &'static str, size: u64, usage: wgpu::BufferUsages) -> Self {
        Self {
            label,
            size,
            usage,
            state: Mutex::new((0, Vec::new())),
        }
    }

    fn reset(&self) {
        if let Ok(mut g) = self.state.lock() {
            g.0 = 0;
        }
    }

    fn write(&self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> wgpu::Buffer {
        debug_assert_eq!(bytes.len() as u64, self.size);
        let mut g = self.state.lock().expect("upload pool");
        let i = g.0;
        if i == g.1.len() {
            g.1.push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: self.size,
                usage: self.usage,
                mapped_at_creation: false,
            }));
        }
        let buf = g.1[i].clone();
        g.0 = i + 1;
        drop(g);
        queue.write_buffer(&buf, 0, bytes);
        buf
    }
}

/// Tile + quantize params (32 bytes — fits single Metal uniform load on some drivers).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BayerUniforms {
    tile: TileUniforms,
    params: [f32; 4],
}

/// Pattern rotation + threshold bias (separate binding — avoids 32-byte struct tail truncation).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BayerPatternUniforms {
    packed: [f32; 4],
}

pub struct ResidentBayerPipelines {
    layout: wgpu::BindGroupLayout,
    pipe2: wgpu::ComputePipeline,
    pipe4: wgpu::ComputePipeline,
    pipe8: wgpu::ComputePipeline,
    uniform_pool: UploadBufferPool,
    pattern_pool: UploadBufferPool,
}

impl ResidentBayerPipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-bayer-bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
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
            label: Some("bayer-resident-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/bayer_resident.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-bayer-pl"),
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
            uniform_pool: UploadBufferPool::new(
                "resident-bayer-uniform-pool",
                std::mem::size_of::<BayerUniforms>() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            pattern_pool: UploadBufferPool::new(
                "resident-bayer-pattern-pool",
                std::mem::size_of::<BayerPatternUniforms>() as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
        })
    }

    pub fn begin_frame(&self) {
        self.uniform_pool.reset();
        self.pattern_pool.reset();
    }

    fn pipeline_for(&self, key: GpuPipelineKey) -> &wgpu::ComputePipeline {
        match key {
            GpuPipelineKey::Bayer2 => &self.pipe2,
            GpuPipelineKey::Bayer4 => &self.pipe4,
            GpuPipelineKey::Bayer8 => &self.pipe8,
            _ => &self.pipe4,
        }
    }

    fn layer_view<'a>(
        texture: &'a wgpu::Texture,
        layer: u32,
        label: &'static str,
    ) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
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

    fn pattern_trig(angle_deg: f32) -> (f32, f32) {
        let wrapped = angle_deg.rem_euclid(360.0);
        if wrapped == 0.0 {
            return (0.0, 1.0);
        }
        let theta = wrapped.to_radians();
        (theta.sin(), theta.cos())
    }

    /// Resident[slot] → scratch[scratch_layer] Bayer pass.
    pub fn encode_bayer_pass(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        resident: &wgpu::Texture,
        resident_layer: u32,
        scratch: &wgpu::Texture,
        scratch_layer: u32,
        tile_x: u32,
        tile_y: u32,
        params: BayerPassParams,
    ) {
        let (pattern_sin, pattern_cos) = Self::pattern_trig(params.pattern_angle);
        let uniforms = BayerUniforms {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            params: [
                params.levels as f32,
                params.threshold_scale,
                0.0,
                0.0,
            ],
        };
        let pattern = BayerPatternUniforms {
            packed: [
                pattern_sin,
                pattern_cos,
                params.color_mode as f32,
                params.threshold_bias,
            ],
        };

        let uniform_buf = self
            .uniform_pool
            .write(device, queue, bytemuck::bytes_of(&uniforms));
        let pattern_buf = self
            .pattern_pool
            .write(device, queue, bytemuck::bytes_of(&pattern));

        let in_view = Self::layer_view(resident, resident_layer, "bayer-res-in");
        let out_view = Self::layer_view(scratch, scratch_layer, "bayer-scratch-out");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-bayer-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: pattern_buf.as_entire_binding(),
                },
            ],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-bayer-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(self.pipeline_for(params.pipeline));
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }

    /// Copy scratch layer back into resident slot (260×260).
    pub fn copy_layer(
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::Texture,
        src_layer: u32,
        dst: &wgpu::Texture,
        dst_layer: u32,
    ) {
        let extent = wgpu::Extent3d {
            width: TILE_EXTENT,
            height: TILE_EXTENT,
            depth_or_array_layers: 1,
        };
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: src,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: src_layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: dst,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: dst_layer,
                },
                aspect: wgpu::TextureAspect::All,
            },
            extent,
        );
    }
}

/// Tile + cell params (32 bytes — Metal-safe primary uniform).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct HalftoneUniforms {
    tile: TileUniforms,
    params: [f32; 4],
}

pub struct ResidentHalftonePipelines {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl ResidentHalftonePipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-halftone-bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("halftone-resident-wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/halftone_resident.wgsl").into(),
            ),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-halftone-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("halftone-resident-main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self { layout, pipeline })
    }

    fn layer_view<'a>(
        texture: &'a wgpu::Texture,
        layer: u32,
        label: &'static str,
    ) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
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

    pub fn encode_halftone_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        resident: &wgpu::Texture,
        resident_layer: u32,
        scratch: &wgpu::Texture,
        scratch_layer: u32,
        tile_x: u32,
        tile_y: u32,
        params: HalftonePassParams,
    ) {
        let uniforms = HalftoneUniforms {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            params: [
                params.cell_size as f32,
                params.threshold_scale,
                if params.dither_alpha { 1.0 } else { 0.0 },
                if params.grayscale { 1.0 } else { 0.0 },
            ],
        };
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-halftone-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let in_view = Self::layer_view(resident, resident_layer, "halftone-res-in");
        let out_view = Self::layer_view(scratch, scratch_layer, "halftone-scratch-out");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-halftone-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
            ],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-halftone-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }
}

/// Tile + CRT params (32 bytes — Metal-safe primary uniform).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CrtUniforms {
    tile: TileUniforms,
    params: [f32; 4],
}

pub struct ResidentCrtPipelines {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl ResidentCrtPipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-crt-bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("crt-resident-wgsl"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/crt_resident.wgsl").into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-crt-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("crt-resident-main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self { layout, pipeline })
    }

    fn layer_view<'a>(
        texture: &'a wgpu::Texture,
        layer: u32,
        label: &'static str,
    ) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
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

    pub fn encode_crt_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        resident: &wgpu::Texture,
        resident_layer: u32,
        scratch: &wgpu::Texture,
        scratch_layer: u32,
        tile_x: u32,
        tile_y: u32,
        params: CrtPassParams,
    ) {
        let uniforms = CrtUniforms {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            params: [
                params.period as f32,
                params.strength,
                params.mask_strength,
                0.0,
            ],
        };
        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-crt-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let in_view = Self::layer_view(resident, resident_layer, "crt-res-in");
        let out_view = Self::layer_view(scratch, scratch_layer, "crt-scratch-out");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-crt-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
            ],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-crt-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PaletteQuantUniforms {
    tile: TileUniforms,
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PaletteMetaGpu {
    la: [f32; 4],
    b_pad: [f32; 4],
}

pub struct ResidentPalettePipelines {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
}

impl ResidentPalettePipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let storage_ro = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-palette-bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                storage_ro(3),
                storage_ro(4),
                storage_ro(5),
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palette-quantize-resident-wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/palette_quantize_resident.wgsl").into(),
            ),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-palette-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("palette-quantize-resident-main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self { layout, pipeline })
    }

    fn layer_view<'a>(
        texture: &'a wgpu::Texture,
        layer: u32,
        label: &'static str,
    ) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
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

    pub fn encode_palette_quantize_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        resident: &wgpu::Texture,
        resident_layer: u32,
        scratch: &wgpu::Texture,
        scratch_layer: u32,
        tile_x: u32,
        tile_y: u32,
        params: &PaletteQuantizePassParams,
    ) {
        let uniforms = PaletteQuantUniforms {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            params: [
                params.lut_size as f32,
                params.palette_rgb.len() as f32,
                0.0,
                0.0,
            ],
        };
        let meta = PaletteMetaGpu {
            la: [
                params.l_range.0,
                params.l_range.1,
                params.a_range.0,
                params.a_range.1,
            ],
            b_pad: [params.b_range.0, params.b_range.1, 0.0, 0.0],
        };

        let lut_u32: Vec<u32> = params.lut_grid.iter().map(|&i| i as u32).collect();
        let palette_rgba: Vec<[f32; 4]> = params
            .palette_rgb
            .iter()
            .map(|c| [c[0], c[1], c[2], 1.0])
            .collect();

        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-palette-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let meta_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-palette-meta"),
            contents: bytemuck::bytes_of(&meta),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let lut_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-palette-lut"),
            contents: bytemuck::cast_slice(&lut_u32),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let palette_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-palette-colors"),
            contents: bytemuck::cast_slice(&palette_rgba),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let in_view = Self::layer_view(resident, resident_layer, "palette-res-in");
        let out_view = Self::layer_view(scratch, scratch_layer, "palette-scratch-out");

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-palette-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: meta_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: lut_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: palette_buf.as_entire_binding(),
                },
            ],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-palette-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct GuidedUniforms {
    tile: TileUniforms,
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ChannelRangesGpu {
    rg: [f32; 4],
    b_pad: [f32; 4],
}

/// Guided channel quantize + Mixed ordered snap (Path B T7).
pub struct ResidentPaletteGuidedPipelines {
    guided_layout: wgpu::BindGroupLayout,
    snap_layout: wgpu::BindGroupLayout,
    guided2: wgpu::ComputePipeline,
    guided4: wgpu::ComputePipeline,
    guided8: wgpu::ComputePipeline,
    snap2: wgpu::ComputePipeline,
    snap4: wgpu::ComputePipeline,
    snap8: wgpu::ComputePipeline,
}

impl ResidentPaletteGuidedPipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let storage_ro = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let tex_in = wgpu::BindGroupLayoutEntry {
            binding: 1,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let tex_out = wgpu::BindGroupLayoutEntry {
            binding: 2,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba32Float,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let uniform0 = wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };

        let guided_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-palette-guided-bgl"),
            entries: &[uniform0, tex_in, tex_out, storage_ro(3), storage_ro(4)],
        });
        let snap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-palette-snap-bgl"),
            entries: &[
                uniform0,
                tex_in,
                tex_out,
                storage_ro(3),
                storage_ro(4),
                storage_ro(5),
            ],
        });

        let guided_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palette-guided-resident-wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/palette_guided_resident.wgsl").into(),
            ),
        });
        let snap_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("palette-ordered-snap-resident-wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/palette_ordered_snap_resident.wgsl").into(),
            ),
        });

        let guided_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-palette-guided-pl"),
            bind_group_layouts: &[&guided_layout],
            push_constant_ranges: &[],
        });
        let snap_pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-palette-snap-pl"),
            bind_group_layouts: &[&snap_layout],
            push_constant_ranges: &[],
        });

        let make_guided = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&guided_pl),
                module: &guided_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let make_snap = |entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&snap_pl),
                module: &snap_shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };

        Ok(Self {
            guided_layout,
            snap_layout,
            guided2: make_guided("bayer2_main"),
            guided4: make_guided("bayer4_main"),
            guided8: make_guided("bayer8_main"),
            snap2: make_snap("bayer2_main"),
            snap4: make_snap("bayer4_main"),
            snap8: make_snap("bayer8_main"),
        })
    }

    fn layer_view<'a>(
        texture: &'a wgpu::Texture,
        layer: u32,
        label: &'static str,
    ) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some(label),
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

    fn guided_pipeline(&self, matrix: GpuPipelineKey) -> &wgpu::ComputePipeline {
        match matrix {
            GpuPipelineKey::Bayer2 => &self.guided2,
            GpuPipelineKey::Bayer8 => &self.guided8,
            _ => &self.guided4,
        }
    }

    fn snap_pipeline(&self, matrix: GpuPipelineKey) -> &wgpu::ComputePipeline {
        match matrix {
            GpuPipelineKey::Bayer2 => &self.snap2,
            GpuPipelineKey::Bayer8 => &self.snap8,
            _ => &self.snap4,
        }
    }

    pub fn encode_guided_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::Texture,
        src_layer: u32,
        dst: &wgpu::Texture,
        dst_layer: u32,
        tile_x: u32,
        tile_y: u32,
        params: &PaletteGuidedPassParams,
    ) {
        let (sin_t, cos_t) = params.pattern_angle.sin_cos();
        let uniforms = GuidedUniforms {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            params: [
                params.channel_levels as f32,
                params.threshold_scale,
                0.0,
                0.0,
            ],
        };
        let pat = BayerPatternUniforms {
            packed: [
                sin_t,
                cos_t,
                params.color_mode as f32,
                params.threshold_bias,
            ],
        };
        let ranges = ChannelRangesGpu {
            rg: [
                params.ranges[0][0],
                params.ranges[0][1],
                params.ranges[1][0],
                params.ranges[1][1],
            ],
            b_pad: [params.ranges[2][0], params.ranges[2][1], 0.0, 0.0],
        };

        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-guided-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pat_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-guided-pat"),
            contents: bytemuck::bytes_of(&pat),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let ranges_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-guided-ranges"),
            contents: bytemuck::bytes_of(&ranges),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let in_view = Self::layer_view(src, src_layer, "guided-in");
        let out_view = Self::layer_view(dst, dst_layer, "guided-out");
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-guided-bg"),
            layout: &self.guided_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: pat_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: ranges_buf.as_entire_binding(),
                },
            ],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-guided-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(self.guided_pipeline(params.matrix));
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }

    pub fn encode_ordered_snap_pass(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::Texture,
        src_layer: u32,
        dst: &wgpu::Texture,
        dst_layer: u32,
        tile_x: u32,
        tile_y: u32,
        mixed: &PaletteMixedPassParams,
    ) {
        let g = &mixed.guided;
        let (sin_t, cos_t) = g.pattern_angle.sin_cos();
        let uniforms = GuidedUniforms {
            tile: TileUniforms::for_tile(tile_x, tile_y),
            params: [
                g.threshold_scale,
                mixed.palette_rgb.len() as f32,
                0.0,
                0.0,
            ],
        };
        let pat = BayerPatternUniforms {
            packed: [sin_t, cos_t, 0.0, g.threshold_bias],
        };
        let rgb: Vec<[f32; 4]> = mixed
            .palette_rgb
            .iter()
            .map(|c| [c[0], c[1], c[2], 1.0])
            .collect();
        let lab: Vec<[f32; 4]> = mixed
            .palette_lab
            .iter()
            .map(|c| [c[0], c[1], c[2], 0.0])
            .collect();

        let uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-snap-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let pat_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-snap-pat"),
            contents: bytemuck::bytes_of(&pat),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let rgb_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-snap-rgb"),
            contents: bytemuck::cast_slice(&rgb),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let lab_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident-snap-lab"),
            contents: bytemuck::cast_slice(&lab),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let in_view = Self::layer_view(src, src_layer, "snap-in");
        let out_view = Self::layer_view(dst, dst_layer, "snap-out");
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-snap-bg"),
            layout: &self.snap_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&in_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: pat_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: rgb_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: lab_buf.as_entire_binding(),
                },
            ],
        });

        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-snap-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(self.snap_pipeline(g.matrix));
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CompositeHeader {
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CompositeLayerOpGpu {
    packed: [f32; 4],
}

const COMPOSITE_MAX_STACK: usize = 16;

/// Fused multi-layer composite onto a resident Composite slot (Path B T7.5).
pub struct ResidentCompositePipelines {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    header_pool: UploadBufferPool,
    ops_pool: UploadBufferPool,
}

impl ResidentCompositePipelines {
    pub fn create(device: &wgpu::Device) -> Result<Self, GpuError> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("resident-composite-bgl"),
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("layer-composite-wgsl"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/layer_composite.wgsl").into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("resident-composite-pl"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("layer-composite-main"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

        Ok(Self {
            layout,
            pipeline,
            header_pool: UploadBufferPool::new(
                "resident-composite-header-pool",
                std::mem::size_of::<CompositeHeader>() as u64,
                wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            ),
            ops_pool: UploadBufferPool::new(
                "resident-composite-ops-pool",
                (COMPOSITE_MAX_STACK * std::mem::size_of::<CompositeLayerOpGpu>()) as u64,
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            ),
        })
    }

    pub fn begin_frame(&self) {
        self.header_pool.reset();
        self.ops_pool.reset();
    }

    fn out_layer_view(texture: &wgpu::Texture, layer: u32) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("composite-out"),
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

    fn resident_array_view(texture: &wgpu::Texture) -> wgpu::TextureView {
        texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("composite-resident-array"),
            format: Some(wgpu::TextureFormat::Rgba32Float),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_mip_level: 0,
            mip_level_count: None,
            base_array_layer: 0,
            array_layer_count: None,
            aspect: wgpu::TextureAspect::All,
            usage: None,
        })
    }

    /// Blend `src_layers` (bottom→top) from `resident` onto transparent; write `out` layer.
    ///
    /// `out` must not be the same texture as `resident` (avoid read/write hazard on one array).
    /// `src_layers` entries are `(array_layer, blend_mode, opacity)`.
    pub fn encode_stack_pass(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        resident: &wgpu::Texture,
        out: &wgpu::Texture,
        out_layer: u32,
        src_layers: &[(u32, u32, f32)],
    ) -> Result<(), GpuError> {
        if src_layers.len() > COMPOSITE_MAX_STACK {
            return Err(GpuError::Device(format!(
                "composite stack length {} exceeds max {COMPOSITE_MAX_STACK}",
                src_layers.len()
            )));
        }

        let header = CompositeHeader {
            params: [src_layers.len() as f32, 0.0, 0.0, 0.0],
        };
        let mut ops = [CompositeLayerOpGpu {
            packed: [0.0; 4],
        }; COMPOSITE_MAX_STACK];
        for (i, &(layer, mode, opacity)) in src_layers.iter().enumerate() {
            ops[i] = CompositeLayerOpGpu {
                packed: [layer as f32, mode as f32, opacity, 0.0],
            };
        }

        let header_buf = self
            .header_pool
            .write(device, queue, bytemuck::bytes_of(&header));
        let ops_buf = self
            .ops_pool
            .write(device, queue, bytemuck::bytes_of(&ops));
        let resident_view = Self::resident_array_view(resident);
        let out_view = Self::out_layer_view(out, out_layer);

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident-composite-bg"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: header_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ops_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&resident_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&out_view),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("resident-composite-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        let groups = CORE_SIZE / WORKGROUP_SIZE;
        pass.dispatch_workgroups(groups, groups, 1);
        Ok(())
    }
}
