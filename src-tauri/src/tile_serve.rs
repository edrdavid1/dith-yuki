//! Track C Phase 2: what `tile://` returns when L0 is dirty or evicted.
//!
//! Preview encoding is always display-referred sRGB (optional soft-proof CMS).
//! An LRU RGBA8 cache avoids re-encoding on Soft proof toggle revisit.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use engine_color::{soft_proof_config_hash, SoftProofConfig, SoftProofTransform};
use engine_tiles::{
    find_cached_ancestor, upsample_from_ancestor, CacheStage, PixelTile, TileCache, TileKey,
    TILE_SIZE,
};

use crate::tile_protocol::encode_preview_tile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewServe {
    Fresh { rgba8: Vec<u8>, generation: u64 },
    Stale { rgba8: Vec<u8>, generation: u64 },
    PyramidFallback { rgba8: Vec<u8> },
    Pending,
}

/// Byte budget for preview RGBA8 LRU.
///
/// A visible 4K RGBA8 framebuffer is ~33 MiB. Proof on + off both reside in this
/// cache for toggle revisit, so 64 MiB evicts the working set. 256 MiB holds
/// roughly three 4K screens of dual-state tiles with headroom.
const PROOF_CACHE_BYTE_BUDGET: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct ProofCacheKey {
    doc: u32,
    layer: u32,
    level: u8,
    x: u32,
    y: u32,
    stage: u8,
    generation: u64,
    config_hash: u64,
}

struct ProofCacheEntry {
    rgba8: Arc<[u8]>,
    last_used: u64,
}

/// Process-wide LRU for encoded preview tiles (proof on or off).
pub struct PreviewRgba8Cache {
    map: Mutex<HashMap<ProofCacheKey, ProofCacheEntry>>,
    bytes: Mutex<usize>,
    tick: AtomicU64,
}

impl Default for PreviewRgba8Cache {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewRgba8Cache {
    pub fn new() -> Self {
        Self {
            map: Mutex::new(HashMap::new()),
            bytes: Mutex::new(0),
            tick: AtomicU64::new(1),
        }
    }

    fn get(&self, key: &ProofCacheKey) -> Option<Vec<u8>> {
        let mut map = self.map.lock().ok()?;
        let entry = map.get_mut(key)?;
        entry.last_used = self.tick.fetch_add(1, Ordering::Relaxed);
        Some(entry.rgba8.to_vec())
    }

    fn insert(&self, key: ProofCacheKey, rgba8: Vec<u8>) {
        let Ok(mut map) = self.map.lock() else {
            return;
        };
        let Ok(mut bytes) = self.bytes.lock() else {
            return;
        };
        if map.contains_key(&key) {
            return;
        }
        while *bytes + rgba8.len() > PROOF_CACHE_BYTE_BUDGET && !map.is_empty() {
            let victim = map.iter().min_by_key(|(_, e)| e.last_used).map(|(k, _)| *k);
            if let Some(v) = victim {
                if let Some(old) = map.remove(&v) {
                    *bytes = bytes.saturating_sub(old.rgba8.len());
                }
            } else {
                break;
            }
        }
        *bytes += rgba8.len();
        map.insert(
            key,
            ProofCacheEntry {
                rgba8: Arc::from(rgba8.into_boxed_slice()),
                last_used: self.tick.fetch_add(1, Ordering::Relaxed),
            },
        );
    }

    pub fn clear_doc(&self, doc: u32) {
        let Ok(mut map) = self.map.lock() else {
            return;
        };
        let Ok(mut bytes) = self.bytes.lock() else {
            return;
        };
        map.retain(|k, e| {
            if k.doc == doc {
                *bytes = bytes.saturating_sub(e.rgba8.len());
                false
            } else {
                true
            }
        });
    }
}

static PREVIEW_CACHE: std::sync::OnceLock<PreviewRgba8Cache> = std::sync::OnceLock::new();

pub fn preview_rgba8_cache() -> &'static PreviewRgba8Cache {
    PREVIEW_CACHE.get_or_init(PreviewRgba8Cache::new)
}

fn stage_tag(stage: CacheStage) -> u8 {
    match stage {
        CacheStage::Raw => 0,
        CacheStage::Processed => 1,
        CacheStage::Composite => 2,
    }
}

fn cache_key(key: TileKey, generation: u64, config_hash: u64) -> ProofCacheKey {
    ProofCacheKey {
        doc: key.doc,
        layer: key.layer,
        level: key.coord.level,
        x: key.coord.x,
        y: key.coord.y,
        stage: stage_tag(key.stage),
        generation,
        config_hash,
    }
}

fn encode_tile(tile: &PixelTile, proof: Option<&Arc<SoftProofTransform>>) -> Vec<u8> {
    let t0 = Instant::now();
    let out = encode_preview_tile(tile, proof.map(|a| a.as_ref()));
    log::debug!(
        target: "soft_proof",
        "preview_encode_tile bytes={} proof={} elapsed_us={}",
        out.len(),
        proof.is_some(),
        t0.elapsed().as_micros()
    );
    out
}

#[allow(dead_code)]
pub fn resolve_preview_tile(cache: &TileCache, key: TileKey, doc_gen: u64) -> PreviewServe {
    resolve_preview_tile_proofed(cache, key, doc_gen, None, 0)
}

pub fn resolve_preview_tile_proofed(
    cache: &TileCache,
    key: TileKey,
    doc_gen: u64,
    proof: Option<&Arc<SoftProofTransform>>,
    config_hash: u64,
) -> PreviewServe {
    let t0 = Instant::now();
    let rgba_cache = preview_rgba8_cache();

    if let Some(entry) = cache.entries.get(&key) {
        let dirty = entry.dirty.load(Ordering::Acquire);
        let gen = entry.generation;
        let ck = cache_key(key, gen, config_hash);
        let rgba8 = if let Some(hit) = rgba_cache.get(&ck) {
            log::debug!(
                target: "soft_proof",
                "preview_rgba8 cache hit doc={} x={} y={}",
                key.doc,
                key.coord.x,
                key.coord.y
            );
            hit
        } else {
            let encoded = encode_tile(&entry.tile, proof);
            rgba_cache.insert(ck, encoded.clone());
            encoded
        };
        let served = if TileCache::tile_entry_is_ready(dirty, gen, doc_gen) {
            PreviewServe::Fresh {
                rgba8,
                generation: gen,
            }
        } else {
            PreviewServe::Stale {
                rgba8,
                generation: gen,
            }
        };
        log::debug!(
            target: "soft_proof",
            "tile_http_resolve elapsed_us={}",
            t0.elapsed().as_micros()
        );
        return served;
    }
    if let Some((ancestor_c, tile)) = find_cached_ancestor(cache, key) {
        let up = upsample_from_ancestor(&tile, key.coord, ancestor_c);
        return PreviewServe::PyramidFallback {
            rgba8: encode_tile(&up, proof),
        };
    }
    PreviewServe::Pending
}

/// Hash for preview cache when soft-proof config is known.
pub fn preview_config_hash(cfg: &SoftProofConfig) -> u64 {
    soft_proof_config_hash(cfg)
}

#[allow(dead_code)] // used by tile_serve tests
pub fn rgba8_len() -> usize {
    (TILE_SIZE * TILE_SIZE * 4) as usize
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_tiles::{CacheStage, PixelTile, TileCoord, HALO};
    use std::sync::Arc;

    fn key(level: u8, x: u32, y: u32) -> TileKey {
        TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord { level, x, y },
            stage: CacheStage::Composite,
        }
    }

    fn solid(r: f32) -> Arc<PixelTile> {
        let mut t = PixelTile::new();
        for y in HALO..(HALO + TILE_SIZE) {
            for x in HALO..(HALO + TILE_SIZE) {
                t.set(x, y, 0, r);
                t.set(x, y, 3, 1.0);
            }
        }
        Arc::new(t)
    }

    #[test]
    fn dirty_entry_is_stale_not_pending() {
        let cache = TileCache::new(50_000_000);
        cache.insert_fresh_gen(key(0, 0, 0), solid(0.4), 1);
        cache.mark_dirty(key(0, 0, 0));
        match resolve_preview_tile(&cache, key(0, 0, 0), 2) {
            PreviewServe::Stale { rgba8, generation } => {
                assert_eq!(generation, 1);
                assert_eq!(rgba8.len(), rgba8_len());
            }
            other => panic!("expected stale, got {other:?}"),
        }
    }

    #[test]
    fn ready_entry_is_fresh() {
        let cache = TileCache::new(50_000_000);
        cache.insert_fresh_gen(key(0, 0, 0), solid(0.2), 3);
        assert!(matches!(
            resolve_preview_tile(&cache, key(0, 0, 0), 3),
            PreviewServe::Fresh { generation: 3, .. }
        ));
    }

    #[test]
    fn evicted_l0_uses_l1_pyramid() {
        let cache = TileCache::new(50_000_000);
        cache.insert_fresh(key(1, 0, 0), solid(0.8));
        match resolve_preview_tile(&cache, key(0, 0, 0), 1) {
            PreviewServe::PyramidFallback { rgba8 } => {
                assert_eq!(rgba8.len(), rgba8_len());
                assert!(rgba8[0] > 180, "upsampled red channel from 0.8");
            }
            other => panic!("expected pyramid fallback, got {other:?}"),
        }
    }

    #[test]
    fn miss_without_ancestor_is_pending() {
        let cache = TileCache::new(50_000_000);
        assert_eq!(
            resolve_preview_tile(&cache, key(0, 3, 3), 1),
            PreviewServe::Pending
        );
    }

    #[test]
    fn rgba8_cache_hit_second_call() {
        let cache = TileCache::new(50_000_000);
        let k = key(0, 0, 0);
        cache.insert_fresh_gen(k, solid(0.5), 1);
        let a = resolve_preview_tile_proofed(&cache, k, 1, None, 99);
        let b = resolve_preview_tile_proofed(&cache, k, 1, None, 99);
        match (a, b) {
            (PreviewServe::Fresh { rgba8: x, .. }, PreviewServe::Fresh { rgba8: y, .. }) => {
                assert_eq!(x, y);
            }
            other => panic!("expected fresh pair, got {other:?}"),
        }
    }
}
