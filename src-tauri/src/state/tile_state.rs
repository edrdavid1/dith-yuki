use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::filters::ErrorResidualsStore;
use engine_tiles::{BlockRepresentativeCache, EdFrontier, Scheduler, TileCache};

pub struct TileState {
    pub tile_cache: TileCache,
    pub scheduler: Scheduler,
    pub palette_cache: PaletteKdCache,
    pub palette_lut_cache: PaletteLutCache,
    pub threshold_cache: ThresholdMapCache,
    pub error_residuals: ErrorResidualsStore,
    pub block_representatives: BlockRepresentativeCache,
    pub ed_frontier: EdFrontier,
}

impl TileState {
    pub fn new(cache_bytes: usize) -> Self {
        Self {
            tile_cache: TileCache::new(cache_bytes),
            scheduler: Scheduler::new(),
            palette_cache: PaletteKdCache::new(),
            palette_lut_cache: PaletteLutCache::new(),
            threshold_cache: ThresholdMapCache::new(),
            error_residuals: ErrorResidualsStore::new(),
            block_representatives: BlockRepresentativeCache::new(),
            ed_frontier: EdFrontier::new(),
        }
    }
}
