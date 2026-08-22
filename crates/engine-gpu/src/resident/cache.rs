//! GPU-resident tile cache: resident array + frame scratch (Path B D1/D2).

use std::collections::HashSet;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use dashmap::{DashMap, DashSet};
use engine_tiles::cache::{EvictContext, TILE_BYTES};
use engine_tiles::{CacheStage, PixelTile, TileKey};

use crate::context::GpuContext;
use crate::dispatch::map_read_with_timeout;
use crate::GpuError;

use super::format::{
    compute_vram_layout, create_tile_array_desc, default_vram_config, pack_tile_upload,
    unpack_tile_download, tile_row_bytes_aligned, VramBudgetConfig, VramLayout, TILE_EXTENT,
};
use super::slot::{GpuSlotMeta, SlotAllocator, SlotHandle};

const MAP_TIMEOUT: Duration = Duration::from_millis(2_000);

/// GPU-resident tiles: one resident `Texture2DArray` + frame scratch ping-pong.
pub struct GpuTileCache {
    layout: VramLayout,
    config: VramBudgetConfig,
    resident: wgpu::Texture,
    pub scratch_a: wgpu::Texture,
    pub scratch_b: wgpu::Texture,
    entries: DashMap<TileKey, GpuSlotMeta>,
    slot_to_key: DashMap<u32, TileKey>,
    allocator: SlotAllocator,
    /// Slots referenced by an in-flight frame submit — skip pressure eviction.
    in_flight: DashSet<u32>,
    live_slots: AtomicU32,
}

impl GpuTileCache {
    pub fn new(device: &wgpu::Device, config: VramBudgetConfig) -> Self {
        let limits = device.limits();
        let layout = compute_vram_layout(&config, limits.max_texture_array_layers);
        log::info!(
            "engine-gpu resident: slots={} scratch_layers={} reserved={} bytes budget={}",
            layout.max_resident_slots,
            layout.scratch_layers_per_array,
            layout.total_reserved_bytes,
            config.vram_budget_bytes
        );

        let resident = device.create_texture(&create_tile_array_desc(
            "gpu-resident-tiles",
            layout.max_resident_slots,
        ));
        let scratch_a = device.create_texture(&create_tile_array_desc(
            "gpu-scratch-a",
            layout.scratch_layers_per_array,
        ));
        let scratch_b = device.create_texture(&create_tile_array_desc(
            "gpu-scratch-b",
            layout.scratch_layers_per_array,
        ));

        Self {
            layout,
            config,
            resident,
            scratch_a,
            scratch_b,
            entries: DashMap::new(),
            slot_to_key: DashMap::new(),
            allocator: SlotAllocator::new(layout.max_resident_slots),
            in_flight: DashSet::new(),
            live_slots: AtomicU32::new(0),
        }
    }

    pub fn with_defaults(device: &wgpu::Device) -> Self {
        Self::new(device, default_vram_config())
    }

    pub fn config(&self) -> &VramBudgetConfig {
        &self.config
    }

    pub fn layout(&self) -> &VramLayout {
        &self.layout
    }

    pub fn live_slot_count(&self) -> u32 {
        self.live_slots.load(Ordering::Relaxed)
    }

    pub fn max_slots(&self) -> u32 {
        self.layout.max_resident_slots
    }

    pub fn scratch_a(&self) -> &wgpu::Texture {
        &self.scratch_a
    }

    pub fn scratch_b(&self) -> &wgpu::Texture {
        &self.scratch_b
    }

    pub fn resident_texture(&self) -> &wgpu::Texture {
        &self.resident
    }

    /// Mark slots used by the current frame submit (pressure eviction skips these).
    pub fn mark_in_flight(&self, slots: &[SlotHandle]) {
        for s in slots {
            self.in_flight.insert(s.index);
        }
    }

    pub fn clear_in_flight(&self) {
        self.in_flight.clear();
    }

    /// GPU miss → upload tile into a resident slot.
    pub fn promote(
        &self,
        ctx: &GpuContext,
        key: TileKey,
        tile: &PixelTile,
        generation: u64,
    ) -> Result<SlotHandle, GpuError> {
        if tile.data.len() * std::mem::size_of::<f32>() != TILE_BYTES {
            return Err(GpuError::Device(format!(
                "promote: expected {TILE_BYTES} bytes, got {}",
                tile.data.len() * 4
            )));
        }

        if let Some(existing) = self.entries.get(&key) {
            if existing.generation == generation {
                return Ok(existing.slot);
            }
            self.release_key(&key);
        }

        if self.allocator.free_count() == 0 {
            let empty_open = HashSet::new();
            let empty_vp = HashSet::new();
            self.evict_for_pressure(&EvictContext {
                active_doc: Some(key.doc),
                open_docs: &empty_open,
                viewport_coords: &empty_vp,
            });
        }

        let slot = self
            .allocator
            .alloc()
            .ok_or(GpuError::Device("GPU resident slots exhausted".into()))?;

        let upload = pack_tile_upload(tile);
        let row_stride = tile_row_bytes_aligned();
        ctx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.resident,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: slot.index,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &upload,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row_stride),
                rows_per_image: Some(TILE_EXTENT),
            },
            wgpu::Extent3d {
                width: TILE_EXTENT,
                height: TILE_EXTENT,
                depth_or_array_layers: 1,
            },
        );

        let meta = GpuSlotMeta {
            slot,
            generation,
            stage: key.stage,
            last_touched: Instant::now(),
        };
        self.entries.insert(key, meta);
        self.slot_to_key.insert(slot.index, key);
        self.live_slots.fetch_add(1, Ordering::Relaxed);
        Ok(slot)
    }

    /// Lookup resident slot if generation matches.
    pub fn get_slot(&self, key: &TileKey, generation: u64) -> Option<SlotHandle> {
        let meta = self.entries.get(key)?;
        if meta.generation == generation {
            Some(meta.slot)
        } else {
            None
        }
    }

    /// Read back slot → CPU `PixelTile` **without** freeing the VRAM slot (preview publish).
    pub fn download(&self, ctx: &GpuContext, key: &TileKey) -> Result<Option<PixelTile>, GpuError> {
        let Some(meta) = self.entries.get(key) else {
            return Ok(None);
        };
        let slot = meta.slot;
        drop(meta);

        let row_stride = tile_row_bytes_aligned();
        let staging_size = row_stride as u64 * TILE_EXTENT as u64;
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu-download-staging"),
            size: staging_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gpu-download-enc"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.resident,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: 0,
                    y: 0,
                    z: slot.index,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row_stride),
                    rows_per_image: Some(TILE_EXTENT),
                },
            },
            wgpu::Extent3d {
                width: TILE_EXTENT,
                height: TILE_EXTENT,
                depth_or_array_layers: 1,
            },
        );
        ctx.queue.submit(Some(encoder.finish()));

        map_read_with_timeout(ctx, &staging, MAP_TIMEOUT)?;
        let view = staging.slice(..).get_mapped_range();
        let out = unpack_tile_download(&view);
        drop(view);
        staging.unmap();

        Ok(Some(out))
    }

    /// Read back slot → CPU `PixelTile`, free VRAM slot.
    pub fn demote(&self, ctx: &GpuContext, key: &TileKey) -> Result<Option<PixelTile>, GpuError> {
        let Some(tile) = self.download(ctx, key)? else {
            return Ok(None);
        };
        self.release_key(key);
        Ok(Some(tile))
    }

    /// Unconditional doc teardown (symmetric with CPU `TileCache::evict_document`).
    pub fn evict_document(&self, doc: u32) {
        let keys: Vec<TileKey> = self
            .entries
            .iter()
            .filter(|e| e.key().doc == doc)
            .map(|e| *e.key())
            .collect();
        for key in keys {
            self.release_key(&key);
        }
    }

    /// Doc-aware pressure eviction (same `EvictContext` as CPU tier).
    pub fn evict_for_pressure(&self, ctx: &EvictContext<'_>) {
        if self.allocator.free_count() > 0 {
            return;
        }

        let stage_order = [
            CacheStage::Composite,
            CacheStage::Processed,
            CacheStage::Raw,
        ];

        for inactive_first in [true, false] {
            for stage in stage_order {
                self.evict_pass(ctx, inactive_first, stage);
                if self.allocator.free_count() > 0 {
                    return;
                }
            }
        }
    }

    fn evict_pass(&self, ctx: &EvictContext<'_>, inactive_docs: bool, stage: CacheStage) {
        let mut candidates: Vec<(Instant, TileKey)> = self
            .entries
            .iter()
            .filter(|e| {
                let key = e.key();
                if e.stage != stage {
                    return false;
                }
                if self.in_flight.contains(&e.slot.index) {
                    return false;
                }
                let is_inactive = ctx.active_doc != Some(key.doc);
                if inactive_docs != is_inactive {
                    return false;
                }
                if !is_inactive {
                    if ctx.viewport_coords.contains(&key.coord) {
                        return false;
                    }
                }
                if stage == CacheStage::Raw && ctx.open_docs.contains(&key.doc) {
                    return false;
                }
                true
            })
            .map(|e| (e.last_touched, *e.key()))
            .collect();

        candidates.sort_by_key(|(t, _)| *t);
        for (_, key) in candidates {
            self.release_key(&key);
            if self.allocator.free_count() > 0 {
                return;
            }
        }
    }

    fn take_key(&self, key: &TileKey) -> Option<(SlotHandle, GpuSlotMeta)> {
        let (_, meta) = self.entries.remove(key)?;
        self.slot_to_key.remove(&meta.slot.index);
        self.allocator.free(meta.slot);
        self.live_slots.fetch_sub(1, Ordering::Relaxed);
        self.in_flight.remove(&meta.slot.index);
        Some((meta.slot, meta))
    }

    fn release_key(&self, key: &TileKey) {
        let _ = self.take_key(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_tiles::TileCoord;

    fn key_doc(doc: u32, x: u32, y: u32, stage: CacheStage) -> TileKey {
        TileKey {
            doc,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x,
                y,
            },
            stage,
        }
    }

    fn open_set(docs: &[u32]) -> HashSet<u32> {
        docs.iter().copied().collect()
    }

    struct FakeGpu {
        cache: GpuTileCache,
        ctx: GpuContext,
    }

    fn fake_gpu() -> Option<FakeGpu> {
        let ctx = GpuContext::try_new_blocking()?;
        let cache = GpuTileCache::with_defaults(&ctx.device);
        Some(FakeGpu { cache, ctx })
    }

    fn insert_meta(cache: &GpuTileCache, key: TileKey, slot: u32, generation: u64) {
        assert!(
            cache.allocator.reserve(slot),
            "test slot {slot} not free"
        );
        let handle = SlotHandle { index: slot };
        cache.entries.insert(
            key,
            GpuSlotMeta {
                slot: handle,
                generation,
                stage: key.stage,
                last_touched: Instant::now(),
            },
        );
        cache.slot_to_key.insert(slot, key);
        cache.live_slots.fetch_add(1, Ordering::Relaxed);
    }

    #[test]
    fn evict_document_frees_all_doc_slots() {
        let Some(fg) = fake_gpu() else {
            return;
        };
        let cache = &fg.cache;
        insert_meta(cache, key_doc(1, 0, 0, CacheStage::Processed), 0, 1);
        insert_meta(cache, key_doc(1, 1, 0, CacheStage::Composite), 1, 1);
        insert_meta(cache, key_doc(2, 0, 0, CacheStage::Processed), 2, 1);
        assert_eq!(cache.live_slot_count(), 3);
        cache.evict_document(1);
        assert_eq!(cache.live_slot_count(), 1);
        assert!(cache.get_slot(&key_doc(2, 0, 0, CacheStage::Processed), 1).is_some());
        assert!(cache.entries.get(&key_doc(1, 0, 0, CacheStage::Processed)).is_none());
    }

    #[test]
    fn get_slot_rejects_stale_generation() {
        let Some(fg) = fake_gpu() else {
            return;
        };
        let cache = &fg.cache;
        let key = key_doc(1, 0, 0, CacheStage::Processed);
        insert_meta(cache, key, 0, 5);
        assert!(cache.get_slot(&key, 5).is_some());
        assert!(cache.get_slot(&key, 6).is_none());
    }

    /// TICKET-1 / industrial-gate E5: after pressure eviction, re-promote with a new
    /// `document_gen` must not surface stale pixels from a recycled slot.
    #[test]
    #[ignore = "requires GPU adapter"]
    fn promote_pressure_evict_repromote_fresh_generation() {
        let fg = fake_gpu().expect("adapter");
        let cache = &fg.cache;
        let ctx = &fg.ctx;
        let key = key_doc(1, 0, 0, CacheStage::Processed);

        let mut tile_old = PixelTile::new();
        for y in 0..TILE_EXTENT {
            for x in 0..TILE_EXTENT {
                tile_old.set(x, y, 0, 0.25);
                tile_old.set(x, y, 1, 0.25);
                tile_old.set(x, y, 2, 0.25);
                tile_old.set(x, y, 3, 1.0);
            }
        }
        cache
            .promote(ctx, key, &tile_old, 1)
            .expect("promote gen1");
        assert!(cache.get_slot(&key, 1).is_some());

        // Fill remaining slots with doc=2 tiles so the next promote triggers pressure
        // eviction; inactive doc=1 is preferred (EvictContext.active_doc = promoting doc).
        let cap = cache.max_slots();
        let mut next_slot_key = 1u32;
        while cache.allocator.free_count() > 0 {
            let filler = key_doc(2, next_slot_key, 0, CacheStage::Composite);
            let mut t = PixelTile::new();
            t.set(0, 0, 0, 0.5);
            t.set(0, 0, 3, 1.0);
            cache
                .promote(ctx, filler, &t, 1)
                .expect("fill promote");
            next_slot_key += 1;
            assert!(
                next_slot_key < cap + 8,
                "failed to fill resident cache"
            );
        }
        assert_eq!(cache.allocator.free_count(), 0);

        // One more doc=2 promote → must evict (including key from doc=1).
        let overflow = key_doc(2, next_slot_key, 0, CacheStage::Composite);
        let mut t = PixelTile::new();
        t.set(0, 0, 0, 0.5);
        t.set(0, 0, 3, 1.0);
        cache.promote(ctx, overflow, &t, 1).expect("pressure promote");
        assert!(
            cache.get_slot(&key, 1).is_none(),
            "doc1 tile must be evicted under pressure before reuse"
        );

        let mut tile_new = PixelTile::new();
        for y in 0..TILE_EXTENT {
            for x in 0..TILE_EXTENT {
                tile_new.set(x, y, 0, 0.75);
                tile_new.set(x, y, 1, 0.75);
                tile_new.set(x, y, 2, 0.75);
                tile_new.set(x, y, 3, 1.0);
            }
        }
        cache
            .promote(ctx, key, &tile_new, 2)
            .expect("promote gen2");
        assert!(cache.get_slot(&key, 1).is_none());
        assert!(cache.get_slot(&key, 2).is_some());

        let downloaded = cache
            .download(ctx, &key)
            .expect("download ok")
            .expect("slot present");
        let sample = downloaded.at(10, 10, 0);
        assert!(
            (sample - 0.75).abs() < 1e-5,
            "re-promote must write fresh gen2 pixels, got {sample} (stale would be ~0.25)"
        );
    }

    #[test]
    fn scratch_layers_match_frame_batch_cap_not_max_slots() {
        let Some(fg) = fake_gpu() else {
            return;
        };
        let layout = fg.cache.layout();
        let config = fg.cache.config();
        assert_eq!(
            layout.scratch_layers_per_array, config.frame_batch_cap,
            "scratch must be frame_batch_cap (case A), not max_slots"
        );
        assert_ne!(
            layout.scratch_layers_per_array, layout.max_resident_slots,
            "if equal, cannot distinguish scratch vs dual-full; unexpected for default budget"
        );
        assert_eq!(
            fg.cache.scratch_a().depth_or_array_layers(),
            layout.scratch_layers_per_array
        );
        assert_eq!(
            fg.cache.scratch_b().depth_or_array_layers(),
            layout.scratch_layers_per_array
        );
        assert_eq!(
            fg.cache.resident_texture().depth_or_array_layers(),
            layout.max_resident_slots
        );
    }

    #[test]
    fn evict_pressure_prefers_inactive_doc() {
        let Some(fg) = fake_gpu() else {
            return;
        };
        let cache = &fg.cache;
        // Fill all slots
        let cap = cache.max_slots();
        for i in 0..cap {
            insert_meta(
                cache,
                key_doc(1, i, 0, CacheStage::Composite),
                i,
                1,
            );
        }
        assert_eq!(cache.allocator.free_count(), 0);

        let mut viewport = HashSet::new();
        viewport.insert(TileCoord {
            level: 0,
            x: 0,
            y: 0,
        });
        let open = open_set(&[1, 2]);
        cache.evict_for_pressure(&EvictContext {
            active_doc: Some(2),
            open_docs: &open,
            viewport_coords: &viewport,
        });
        assert!(cache.allocator.free_count() > 0);
        let remaining_doc1 = cache
            .entries
            .iter()
            .filter(|e| e.key().doc == 1)
            .count();
        assert!(
            remaining_doc1 < cap as usize,
            "pressure should evict at least one inactive-doc tile before active"
        );
    }

    #[test]
    #[ignore = "requires GPU adapter"]
    fn promote_demote_roundtrip_byte_identical() {
        let fg = fake_gpu().expect("adapter");
        let mut tile = PixelTile::new();
        for y in 0..TILE_EXTENT {
            for x in 0..TILE_EXTENT {
                let v = (x + y) as f32 * 0.001;
                tile.set(x, y, 0, v);
                tile.set(x, y, 1, v);
                tile.set(x, y, 2, v);
                tile.set(x, y, 3, 1.0);
            }
        }
        let key = key_doc(9, 3, 4, CacheStage::Processed);
        let slot = fg
            .cache
            .promote(&fg.ctx, key, &tile, 42)
            .expect("promote");
        assert_eq!(slot.index, 0);
        let back = fg.cache.demote(&fg.ctx, &key).expect("demote").expect("tile");
        assert_eq!(tile.data.len(), back.data.len());
        for (a, b) in tile.data.iter().zip(back.data.iter()) {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
}
