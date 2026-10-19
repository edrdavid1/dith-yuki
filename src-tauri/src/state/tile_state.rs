use std::sync::Arc;

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filters::{ErrorResidualsStore, FullDocumentCache};
use engine_tiles::{BlockRepresentativeCache, EdFrontier, Scheduler, TileCache};

pub use super::ed_strip_gate::{EdStripGateMap, EdStripKey};

pub struct TileState {
    pub tile_cache: Arc<TileCache>,
    pub scheduler: Scheduler,
    pub palette_cache: PaletteKdCache,
    pub palette_lut_cache: PaletteLutCache,
    pub threshold_cache: ThresholdMapCache,
    pub error_residuals: ErrorResidualsStore,
    pub block_representatives: BlockRepresentativeCache,
    pub ed_frontier: EdFrontier,
    pub full_document: FullDocumentCache,
    /// Strip ownership gates (B1). First successful `try_claim` runs the strip;
    /// waiters poll briefly then re-park (`EdStripBusy`) so workers stay free.
    pub ed_strip_gates: EdStripGateMap,
}

impl TileState {
    pub fn new(cache_bytes: usize) -> Self {
        Self {
            tile_cache: Arc::new(TileCache::new(cache_bytes)),
            scheduler: Scheduler::new(),
            palette_cache: PaletteKdCache::new(),
            palette_lut_cache: PaletteLutCache::new(),
            threshold_cache: ThresholdMapCache::new(),
            error_residuals: ErrorResidualsStore::new(),
            block_representatives: BlockRepresentativeCache::new(),
            ed_frontier: EdFrontier::new(),
            full_document: FullDocumentCache::new(),
            ed_strip_gates: EdStripGateMap::new(),
        }
    }

    pub fn strip_gate(
        &self,
        key: EdStripKey,
    ) -> std::sync::Arc<super::ed_strip_gate::EdStripGate> {
        self.ed_strip_gates.gate(key)
    }

    pub fn clear_strip_gates(&self) {
        self.ed_strip_gates.clear();
    }

    pub fn evict_strip_gates_document(&self, doc: u32) {
        self.ed_strip_gates.evict_document(doc);
    }
}
