//! GPU tile format & VRAM budget math (Path B D0/D1).

use engine_tiles::cache::TILE_BYTES;

/// Full tile edge including halo (matches CPU `PixelTile`).
pub const TILE_EXTENT: u32 = 260;

/// Default process VRAM budget for resident + scratch (256 MiB).
pub const DEFAULT_VRAM_BUDGET_BYTES: u64 = 256 * 1024 * 1024;

/// Max tiles processed in one frame job; sizes scratch ping-pong arrays.
pub const DEFAULT_FRAME_BATCH_CAP: u32 = 64;

/// Reserve for viewport texture, LUTs, alignment (4 MiB).
pub const FIXED_HEADROOM_BYTES: u64 = 4 * 1024 * 1024;

/// User / init configuration for VRAM layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramBudgetConfig {
    pub vram_budget_bytes: u64,
    pub frame_batch_cap: u32,
    pub viewport_tex_bytes: u64,
    pub fixed_headroom_bytes: u64,
}

impl Default for VramBudgetConfig {
    fn default() -> Self {
        default_vram_config()
    }
}

pub fn default_vram_config() -> VramBudgetConfig {
    VramBudgetConfig {
        vram_budget_bytes: DEFAULT_VRAM_BUDGET_BYTES,
        frame_batch_cap: DEFAULT_FRAME_BATCH_CAP,
        viewport_tex_bytes: 0,
        fixed_headroom_bytes: FIXED_HEADROOM_BYTES,
    }
}

/// Derived byte counts and slot capacity after scratch reserve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramLayout {
    pub max_resident_slots: u32,
    pub scratch_layers_per_array: u32,
    pub scratch_bytes: u64,
    pub resident_bytes: u64,
    pub total_reserved_bytes: u64,
}

/// Bytes for one `Texture2DArray` layer stack at 260×260 Rgba32Float.
pub fn tile_array_bytes(layers: u32) -> u64 {
    layers as u64 * TILE_BYTES as u64
}

/// D1 formula: reserve `2 × frame_batch_cap` scratch layers, then resident slots.
pub fn compute_vram_layout(
    config: &VramBudgetConfig,
    max_texture_array_layers: u32,
) -> VramLayout {
    let scratch_layers = config.frame_batch_cap;
    let scratch_bytes = 2 * tile_array_bytes(scratch_layers);
    let overhead = scratch_bytes
        + config.viewport_tex_bytes
        + config.fixed_headroom_bytes;

    let resident_bytes = config.vram_budget_bytes.saturating_sub(overhead);
    let mut max_resident_slots =
        (resident_bytes / TILE_BYTES as u64).min(u64::from(max_texture_array_layers)) as u32;

    if max_resident_slots == 0 && config.vram_budget_bytes > overhead {
        max_resident_slots = 1;
    }

    let total_reserved_bytes = overhead + tile_array_bytes(max_resident_slots);

    VramLayout {
        max_resident_slots,
        scratch_layers_per_array: scratch_layers,
        scratch_bytes,
        resident_bytes: tile_array_bytes(max_resident_slots),
        total_reserved_bytes,
    }
}

/// Tightly packed row bytes (260 × 4 × sizeof(f32)).
pub fn tile_row_bytes() -> u32 {
    TILE_EXTENT * 4 * 4
}

/// wgpu `write_texture` / `copy_texture_to_buffer` require 256-byte aligned rows.
pub fn tile_row_bytes_aligned() -> u32 {
    let row = tile_row_bytes();
    row.div_ceil(256) * 256
}

/// Pack `PixelTile` into upload buffer with aligned row stride.
pub fn pack_tile_upload(tile: &engine_tiles::PixelTile) -> Vec<u8> {
    let side = TILE_EXTENT as usize;
    let row = tile_row_bytes() as usize;
    let stride = tile_row_bytes_aligned() as usize;
    let mut out = vec![0u8; stride * side];
    let src = bytemuck::cast_slice(&tile.data);
    for y in 0..side {
        out[y * stride..y * stride + row].copy_from_slice(&src[y * row..(y + 1) * row]);
    }
    out
}

/// Unpack aligned download buffer into `PixelTile`.
pub fn unpack_tile_download(bytes: &[u8]) -> engine_tiles::PixelTile {
    let side = TILE_EXTENT as usize;
    let row = tile_row_bytes() as usize;
    let stride = tile_row_bytes_aligned() as usize;
    let mut tile = engine_tiles::PixelTile::new();
    let dst = bytemuck::cast_slice_mut(&mut tile.data);
    for y in 0..side {
        dst[y * row..(y + 1) * row].copy_from_slice(&bytes[y * stride..y * stride + row]);
    }
    tile
}

/// wgpu texture descriptor for one tile array (260×260 × N layers, Rgba32Float).
pub fn create_tile_array_desc(label: &'static str, layers: u32) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: TILE_EXTENT,
            height: TILE_EXTENT,
            depth_or_array_layers: layers.max(1),
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_extent_matches_engine_tiles() {
        let side = engine_tiles::TILE_SIZE + 2 * engine_tiles::HALO;
        assert_eq!(TILE_EXTENT, side);
        assert_eq!(TILE_BYTES, (TILE_EXTENT as usize) * (TILE_EXTENT as usize) * 4 * 4);
    }

    #[test]
    fn default_layout_fits_in_budget() {
        let config = default_vram_config();
        let layout = compute_vram_layout(&config, 2048);
        assert!(layout.total_reserved_bytes <= config.vram_budget_bytes);
        assert!(layout.scratch_bytes > 0);
        assert!(layout.max_resident_slots >= 1);
        // ~118 slots at 256 MiB / cap 64 (spec order of magnitude)
        assert!(layout.max_resident_slots >= 80);
        assert!(layout.max_resident_slots <= 200);
    }

    #[test]
    fn reserved_bytes_includes_headroom() {
        let config = default_vram_config();
        let layout = compute_vram_layout(&config, 2048);
        assert_eq!(
            layout.total_reserved_bytes,
            layout.scratch_bytes
                + layout.resident_bytes
                + config.viewport_tex_bytes
                + config.fixed_headroom_bytes
        );
        assert!(layout.total_reserved_bytes <= config.vram_budget_bytes);
    }
}
