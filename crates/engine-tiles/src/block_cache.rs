//! Block representative cache for mega-pixel (`pixel_size > 1`) dithering.
//!
//! Stores the raw source color of each block's top-left pixel (document-global
//! grid), computed without halo clamping. Ordered dither reads from this cache
//! instead of clamping a neighbor coordinate into the local tile buffer.
//!
//! Floyd–Steinberg also records the *dithered* RGB of each processed
//! representative so neighboring tiles can copy the true block color when the
//! representative lies outside their core (same cross-tile side-channel pattern
//! as [`ErrorResidualsStore`](../../engine-project)).

use crate::coords::GlobalCoord;
use crate::{CacheStage, TileCache, TileCoord, TileKey, HALO, TILE_SIZE};
use dashmap::DashMap;
use std::cell::Cell;
use std::sync::atomic::{AtomicU64, Ordering};

thread_local! {
    /// When false, [`BlockRepresentativeCache::get_raw`] returns None so a
    /// later stack filter (e.g. dither after Adjust) samples its input tile
    /// instead of document Raw.
    static SAMPLE_RAW_BLOCKS: Cell<bool> = const { Cell::new(true) };
}

/// Run `f` with [`get_raw`] enabled or disabled for this thread.
pub fn with_raw_block_sampling<R>(enabled: bool, f: impl FnOnce() -> R) -> R {
    SAMPLE_RAW_BLOCKS.with(|c| {
        let prev = c.replace(enabled);
        let out = f();
        c.set(prev);
        out
    })
}

/// Raw RGBA sample (linear floats), matching a single pixel in a [`PixelTile`].
pub type RawPixelValue = [f32; 4];

/// Dithered RGB produced by error diffusion at a block representative.
pub type DitheredRgb = [f32; 3];

/// Block address in the document-global mega-pixel grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockCoord {
    pub doc: u32,
    pub layer: u32,
    /// `aligned_x / pixel_size`
    pub block_x: u32,
    /// `aligned_y / pixel_size`
    pub block_y: u32,
    pub pixel_size: u32,
}

impl BlockCoord {
    #[inline]
    pub fn from_global(doc: u32, layer: u32, gx: u32, gy: u32, pixel_size: u32) -> Self {
        debug_assert!(pixel_size >= 1);
        Self {
            doc,
            layer,
            block_x: gx / pixel_size,
            block_y: gy / pixel_size,
            pixel_size,
        }
    }

    #[inline]
    pub fn origin_global(self) -> (u32, u32) {
        (self.block_x * self.pixel_size, self.block_y * self.pixel_size)
    }
}

/// Side-channel cache of block representatives (raw + optional dithered output).
pub struct BlockRepresentativeCache {
    raw: DashMap<BlockCoord, RawPixelValue>,
    dithered: DashMap<BlockCoord, DitheredRgb>,
    /// Bitmask of populated `(layer, pixel_size)` keys packed as `(layer << 8) | ps`.
    populated: DashMap<u64, ()>,
    /// Generation bumped on full invalidation (tests / diagnostics).
    generation: AtomicU64,
}

impl BlockRepresentativeCache {
    pub fn new() -> Self {
        Self {
            raw: DashMap::new(),
            dithered: DashMap::new(),
            populated: DashMap::new(),
            generation: AtomicU64::new(0),
        }
    }

    #[inline]
    fn pack_key(doc: u32, layer: u32, pixel_size: u32) -> u64 {
        ((doc as u64) << 40) | ((layer as u64) << 8) | (pixel_size as u64 & 0xff)
    }

    pub fn is_populated(&self, doc: u32, layer: u32, pixel_size: u32) -> bool {
        self.populated.contains_key(&Self::pack_key(doc, layer, pixel_size))
    }

    pub fn get_raw(&self, block: BlockCoord) -> Option<RawPixelValue> {
        if !SAMPLE_RAW_BLOCKS.with(|c| c.get()) {
            return None;
        }
        self.raw.get(&block).map(|v| *v)
    }

    pub fn insert_raw(&self, block: BlockCoord, value: RawPixelValue) {
        self.raw.insert(block, value);
    }

    pub fn get_dithered(&self, block: BlockCoord) -> Option<DitheredRgb> {
        self.dithered.get(&block).map(|v| *v)
    }

    pub fn insert_dithered(&self, block: BlockCoord, value: DitheredRgb) {
        self.dithered.insert(block, value);
    }

    /// Drop dithered outputs only (filter re-run / residuals clear). Raw stays.
    pub fn clear_dithered(&self) {
        self.dithered.clear();
    }

    /// Layer ids present in raw, dithered, or populated maps.
    pub fn cached_layer_ids(&self) -> std::collections::HashSet<u32> {
        let mut ids = std::collections::HashSet::new();
        for e in self.raw.iter() {
            ids.insert(e.key().layer);
        }
        for e in self.dithered.iter() {
            ids.insert(e.key().layer);
        }
        for e in self.populated.iter() {
            ids.insert((*e.key() >> 8) as u32);
        }
        ids
    }

    /// Drop raw, dithered, and populated entries for `layer`. Missing keys are a no-op.
    pub fn evict_layer(&self, doc: u32, layer: u32) {
        self.raw.retain(|k, _| k.doc != doc || k.layer != layer);
        self.dithered.retain(|k, _| k.doc != doc || k.layer != layer);
        self.populated.retain(|k, _| {
            let packed_doc = (*k >> 40) as u32;
            let packed_layer = ((*k >> 8) as u32) & 0xffff_ffff;
            packed_doc != doc || packed_layer != layer
        });
    }

    pub fn evict_document(&self, doc: u32) {
        self.raw.retain(|k, _| k.doc != doc);
        self.dithered.retain(|k, _| k.doc != doc);
        self.populated.retain(|k, _| (*k >> 40) as u32 != doc);
    }

    /// Full invalidate — raw image changed.
    pub fn invalidate_all(&self) {
        self.raw.clear();
        self.dithered.clear();
        self.populated.clear();
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Populate raw representatives for one `(layer, pixel_size)` from a linear
    /// RGBA buffer (same layout as `decompose_image_to_tiles`).
    pub fn populate_from_buffer(
        &self,
        rgba: &[f32],
        width: u32,
        height: u32,
        doc: u32,
        layer: u32,
        pixel_size: u32,
    ) {
        assert!(pixel_size >= 1);
        if width == 0 || height == 0 {
            return;
        }
        self.raw.retain(|k, _| !(k.doc == doc && k.layer == layer && k.pixel_size == pixel_size));

        let w = width as usize;
        let h = height as usize;
        let ps = pixel_size as usize;

        let mut gy = 0u32;
        while gy < height {
            let mut gx = 0u32;
            while gx < width {
                let idx = (gy as usize * w + gx as usize) * 4;
                let value = if idx + 3 < rgba.len() {
                    [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]]
                } else {
                    [0.0, 0.0, 0.0, 0.0]
                };
                self.insert_raw(BlockCoord::from_global(doc, layer, gx, gy, pixel_size), value);
                gx = gx.saturating_add(pixel_size);
                if gx == 0 {
                    break;
                }
            }
            gy = gy.saturating_add(pixel_size);
            if gy == 0 {
                break;
            }
        }

        let _ = (w, h, ps);

        self.populated.insert(Self::pack_key(doc, layer, pixel_size), ());
    }

    pub fn ensure_populated_from_tiles(
        &self,
        tile_cache: &TileCache,
        doc: u32,
        layer: u32,
        pixel_size: u32,
        width: u32,
        height: u32,
    ) {
        if pixel_size <= 1 || self.is_populated(doc, layer, pixel_size) {
            return;
        }
        if width == 0 || height == 0 {
            return;
        }

        self.raw.retain(|k, _| !(k.doc == doc && k.layer == layer && k.pixel_size == pixel_size));

        let mut gy = 0u32;
        while gy < height {
            let mut gx = 0u32;
            while gx < width {
                let value = read_raw_from_tiles(tile_cache, doc, layer, gx, gy)
                    .unwrap_or([0.0, 0.0, 0.0, 0.0]);
                self.insert_raw(BlockCoord::from_global(doc, layer, gx, gy, pixel_size), value);
                let next = gx.saturating_add(pixel_size);
                if next <= gx {
                    break;
                }
                gx = next;
            }
            let next = gy.saturating_add(pixel_size);
            if next <= gy {
                break;
            }
            gy = next;
        }

        self.populated.insert(Self::pack_key(doc, layer, pixel_size), ());
    }
}

impl Default for BlockRepresentativeCache {
    fn default() -> Self {
        Self::new()
    }
}

fn read_raw_from_tiles(
    tile_cache: &TileCache,
    doc: u32,
    layer: u32,
    gx: u32,
    gy: u32,
) -> Option<RawPixelValue> {
    let tx = gx / TILE_SIZE;
    let ty = gy / TILE_SIZE;
    let local_x = gx % TILE_SIZE + HALO;
    let local_y = gy % TILE_SIZE + HALO;
    let key = TileKey {
        doc,
        layer,
        coord: TileCoord {
            level: 0,
            x: tx,
            y: ty,
        },
        stage: CacheStage::Raw,
    };
    let tile = tile_cache.get_entry(key)?;
    Some([
        tile.at(local_x, local_y, 0),
        tile.at(local_x, local_y, 1),
        tile.at(local_x, local_y, 2),
        tile.at(local_x, local_y, 3),
    ])
}

/// Helper: global aligned origin for a pixel, as [`GlobalCoord`].
#[inline]
pub fn block_origin(gx: u32, gy: u32, pixel_size: u32) -> GlobalCoord {
    GlobalCoord { x: gx, y: gy }.aligned(pixel_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn populate_and_lookup() {
        let cache = BlockRepresentativeCache::new();
        // 4×4 image, pixel_size=2 → 2×2 blocks
        let mut rgba = vec![0.0f32; 4 * 4 * 4];
        // Set representative (0,0) = red, (2,0) = green
        rgba[0] = 1.0;
        rgba[4 * 2] = 0.0;
        rgba[4 * 2 + 1] = 1.0;

        cache.populate_from_buffer(&rgba, 4, 4, 1, 1, 2);
        assert!(cache.is_populated(1, 1, 2));

        let a = cache
            .get_raw(BlockCoord::from_global(1, 1, 0, 0, 2))
            .unwrap();
        assert_eq!(a[0], 1.0);

        let b = cache
            .get_raw(BlockCoord::from_global(1, 1, 2, 0, 2))
            .unwrap();
        assert_eq!(b[1], 1.0);
    }

    #[test]
    fn invalidate_clears_populated() {
        let cache = BlockRepresentativeCache::new();
        let rgba = vec![0.5f32; 8 * 8 * 4];
        cache.populate_from_buffer(&rgba, 8, 8, 1, 1, 4);
        assert!(cache.is_populated(1, 1, 4));
        let gen = cache.generation();
        cache.invalidate_all();
        assert!(!cache.is_populated(1, 1, 4));
        assert!(cache.generation() > gen);
    }

    #[test]
    fn evict_layer_removes_target_keeps_other() {
        let cache = BlockRepresentativeCache::new();
        let rgba_a = vec![0.25f32; 4 * 4 * 4];
        let rgba_b = vec![0.75f32; 4 * 4 * 4];
        cache.populate_from_buffer(&rgba_a, 4, 4, 1, 1, 2);
        cache.populate_from_buffer(&rgba_b, 4, 4, 1, 2, 2);
        cache.insert_dithered(BlockCoord::from_global(1, 1, 0, 0, 2), [0.1, 0.2, 0.3]);
        cache.insert_dithered(BlockCoord::from_global(1, 2, 0, 0, 2), [0.4, 0.5, 0.6]);

        cache.evict_layer(1, 1);

        assert!(cache.get_raw(BlockCoord::from_global(1, 1, 0, 0, 2)).is_none());
        assert!(cache.get_dithered(BlockCoord::from_global(1, 1, 0, 0, 2)).is_none());
        assert!(!cache.is_populated(1, 1, 2));
        assert!(cache.get_raw(BlockCoord::from_global(1, 2, 0, 0, 2)).is_some());
        assert!(cache.get_dithered(BlockCoord::from_global(1, 2, 0, 0, 2)).is_some());
        assert!(cache.is_populated(1, 2, 2));
    }

    #[test]
    fn edit_pixel_requires_repopulate() {
        let cache = BlockRepresentativeCache::new();
        let mut rgba = vec![0.0f32; 8 * 8 * 4];
        cache.populate_from_buffer(&rgba, 8, 8, 1, 1, 4);
        let before = cache
            .get_raw(BlockCoord::from_global(1, 1, 0, 0, 4))
            .unwrap()[0];
        assert_eq!(before, 0.0);

        rgba[0] = 0.75;
        cache.invalidate_all();
        cache.populate_from_buffer(&rgba, 8, 8, 1, 1, 4);
        let after = cache
            .get_raw(BlockCoord::from_global(1, 1, 0, 0, 4))
            .unwrap()[0];
        assert_eq!(after, 0.75);
    }

    #[test]
    fn get_raw_respects_thread_local_sampling_flag() {
        let cache = BlockRepresentativeCache::new();
        let rgba = vec![0.25f32; 4 * 4 * 4];
        cache.populate_from_buffer(&rgba, 4, 4, 1, 1, 2);
        let key = BlockCoord::from_global(1, 1, 0, 0, 2);
        assert!(cache.get_raw(key).is_some());
        with_raw_block_sampling(false, || {
            assert!(cache.get_raw(key).is_none());
        });
        assert!(cache.get_raw(key).is_some());
    }
}
