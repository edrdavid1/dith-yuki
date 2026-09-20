//! `FilterContext` — the dependency bundle passed to every `FilterAlgorithm::apply` call.
//!
//! # Architectural note
//!
//! `FilterContext` is defined here in `engine-project` rather than in `engine-registry`
//! because it references concrete types from multiple crates that form the full
//! dependency graph:
//!
//! - `TileCoord`, `BlockRepresentativeCache` — `engine-tiles`
//! - `PaletteKdCache`, `PaletteLutCache`, `ThresholdMapCache` — `engine-color`
//! - `GpuContext` — `engine-gpu`
//! - `Document`, `ErrorResidualsStore` — `engine-project` (this crate)
//!
//! Since `engine-project` depends on `engine-registry` (for `FilterAlgorithm` etc.),
//! `engine-registry` cannot depend on `engine-project` without creating a circular
//! dependency.  The design document explicitly calls this out as an accepted fallback:
//!
//! > "Alternatively, if adding a full new crate is premature, the trait and types
//! > can live in `engine-project/src/registry/mod.rs` as a module rather than a
//! > separate crate. The interface is identical; the crate boundary decision does
//! > not affect the public API."
//!
//! `FilterContext` is re-exported from `engine-project`'s public API so
//! `engine-project::FilterContext` is the canonical name callers use.
//! The `FilterAlgorithm` trait (in `engine-registry`) references it via the
//! `engine-project` dependency that is always present in call-site crates.
//!
//! # Design invariant (Requirement 3.2)
//!
//! `FilterContext` is constructed from the arguments already present in
//! `apply_filter_to_tile_with_park()` — no new allocations, no new caches.
//! Every field maps directly to a parameter of that function.

use crate::document::Document;
use crate::filters::dither_residuals::ErrorResidualsStore;
use crate::types::LayerId;
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_gpu::GpuContext;
use engine_registry::FilterCtx;
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::TileCoord;

/// All dependencies that a `FilterAlgorithm::apply` implementation may need.
///
/// Constructed inside `apply_filter_to_tile_with_park()` from the arguments
/// that function already receives — zero new allocations.
///
/// # Requirement coverage
///
/// - Req 3.1 — all listed fields are present; includes `palette_cache`,
///   `lut_cache`, and `document` required by `palette_quantize`.
/// - Req 3.2 — construction is a struct literal with no additional work.
/// - Req 3.3 — `palette_quantize` can access palette data through
///   `ctx.document`, `ctx.palette_cache`, and `ctx.lut_cache`.
pub struct FilterContext<'a> {
    /// The tile coordinate being processed (level, x, y within the pyramid).
    pub coord: TileCoord,

    /// The document the tile belongs to.  Provides palette list, canvas
    /// dimensions, and layer hierarchy.
    pub document: &'a Document,

    /// Nearest-neighbour palette look-up cache (k-d tree per palette).
    /// Required by `palette_quantize` and error-diffusion dithering.
    pub palette_cache: &'a PaletteKdCache,

    /// 3-D LUT cache for fast palette colour mapping.
    /// Required by `palette_quantize`.
    pub lut_cache: &'a PaletteLutCache,

    /// Bayer / ordered dithering threshold-map cache.
    /// Required by Bayer and CMYK-halftone algorithms.
    pub threshold_cache: &'a ThresholdMapCache,

    /// Cross-tile error residuals for error-diffusion algorithms.
    /// Required by Floyd-Steinberg, Atkinson, JJN, Stucki, Burkes, Sierra.
    pub residuals: &'a ErrorResidualsStore,

    /// Block-representative pixel cache for block-granularity dithering.
    pub block_cache: &'a BlockRepresentativeCache,

    /// Optional GPU context.  `None` when no GPU adapter is available or
    /// when the algorithm is being applied on the CPU path.
    pub gpu: Option<&'a GpuContext>,

    /// Layer being processed. Required by block-representative sampling when
    /// `pixel_size > 1`.
    pub layer_id: LayerId,
}

impl<'a> FilterContext<'a> {
    /// Construct a `FilterContext` from the individual arguments that
    /// `apply_filter_to_tile_with_park` already holds.
    ///
    /// This is a zero-cost convenience constructor — it just moves references
    /// into the struct literal.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        coord: TileCoord,
        document: &'a Document,
        palette_cache: &'a PaletteKdCache,
        lut_cache: &'a PaletteLutCache,
        threshold_cache: &'a ThresholdMapCache,
        residuals: &'a ErrorResidualsStore,
        block_cache: &'a BlockRepresentativeCache,
        gpu: Option<&'a GpuContext>,
        layer_id: LayerId,
    ) -> Self {
        Self {
            coord,
            document,
            palette_cache,
            lut_cache,
            threshold_cache,
            residuals,
            block_cache,
            gpu,
            layer_id,
        }
    }

    /// Recover a typed context from the type-erased [`FilterCtx`] passed to
    /// [`engine_registry::FilterAlgorithm::apply`].
    ///
    /// # Safety
    /// `apply` is only invoked with `&FilterContext`. The pointer is valid for
    /// the duration of that call.
    pub fn from_ctx(ctx: &'a dyn FilterCtx) -> &'a FilterContext<'a> {
        // SAFETY: see above — the only `FilterCtx` impl is this type.
        unsafe { &*(ctx.as_erased_context() as *const FilterContext<'a>) }
    }
}

impl FilterCtx for FilterContext<'_> {
    fn as_erased_context(&self) -> *const () {
        self as *const Self as *const ()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::filters::dither_residuals::ErrorResidualsStore;
    use engine_color::palette_cache::PaletteKdCache;
    use engine_color::palette_lut::PaletteLutCache;
    use engine_color::threshold_map::ThresholdMapCache;
    use engine_tiles::block_cache::BlockRepresentativeCache;
    use engine_tiles::{TileCoord, TILE_SIZE};

    fn make_minimal_document() -> Document {
        use crate::types::DocumentId;
        Document::new(DocumentId::new(1), TILE_SIZE, TILE_SIZE)
    }

    /// FilterContext carries all fields required by palette_quantize
    /// (Requirement 3.1, 3.3).
    #[test]
    fn filter_context_has_palette_fields() {
        let doc = make_minimal_document();
        let pc = PaletteKdCache::new();
        let lc = PaletteLutCache::new();
        let tc = ThresholdMapCache::new();
        let rs = ErrorResidualsStore::new();
        let bc = BlockRepresentativeCache::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };

        let ctx = FilterContext::new(
            coord,
            &doc,
            &pc,
            &lc,
            &tc,
            &rs,
            &bc,
            None,
            crate::types::LayerId::new(1),
        );

        // Verify all palette-quantize-required fields are accessible.
        assert_eq!(ctx.coord, coord);
        let _ = ctx.document;
        let _ = ctx.palette_cache;
        let _ = ctx.lut_cache;
        assert!(ctx.gpu.is_none());
    }

    /// FilterContext::new is zero-cost (no allocation) — just a struct literal.
    /// Verified by construction: same address as the input references.
    #[test]
    fn filter_context_references_are_the_same_object() {
        let doc = make_minimal_document();
        let pc = PaletteKdCache::new();
        let lc = PaletteLutCache::new();
        let tc = ThresholdMapCache::new();
        let rs = ErrorResidualsStore::new();
        let bc = BlockRepresentativeCache::new();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };

        let ctx = FilterContext::new(
            coord,
            &doc,
            &pc,
            &lc,
            &tc,
            &rs,
            &bc,
            None,
            crate::types::LayerId::new(1),
        );

        assert!(std::ptr::eq(ctx.document, &doc));
        assert!(std::ptr::eq(ctx.palette_cache, &pc));
        assert!(std::ptr::eq(ctx.lut_cache, &lc));
        assert!(std::ptr::eq(ctx.threshold_cache, &tc));
        assert!(std::ptr::eq(ctx.residuals, &rs));
        assert!(std::ptr::eq(ctx.block_cache, &bc));
    }
}
