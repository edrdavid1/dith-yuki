//! Triple-buffered staging readback ring (Path B D5).

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::dispatch::CORE_SIZE;

const RING_LEN: usize = 3;

/// One tile core RGBA8 readback (`256×256×4` bytes).
pub const TILE_CORE_RGBA8_BYTES: u64 = (CORE_SIZE as u64) * (CORE_SIZE as u64) * 4;

pub struct ReadbackRing {
    buffers: [wgpu::Buffer; RING_LEN],
    byte_size: u64,
    cursor: AtomicUsize,
}

impl ReadbackRing {
    pub fn new(device: &wgpu::Device, byte_size: u64) -> Self {
        let make = |i: usize| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(&format!("gpu-readback-ring-{i}")),
                size: byte_size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        };
        Self {
            buffers: [make(0), make(1), make(2)],
            byte_size,
            cursor: AtomicUsize::new(0),
        }
    }

    pub fn for_tile_core(device: &wgpu::Device) -> Self {
        Self::new(device, TILE_CORE_RGBA8_BYTES)
    }

    pub fn byte_size(&self) -> u64 {
        self.byte_size
    }

    pub fn next_buffer(&self) -> &wgpu::Buffer {
        let idx = self.cursor.fetch_add(1, Ordering::Relaxed) % RING_LEN;
        &self.buffers[idx]
    }
}
