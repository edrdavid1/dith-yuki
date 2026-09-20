//! Shared src copy so in-place `FilterAlgorithm::apply` never aliases ordered/ED kernels.

use engine_tiles::PixelTile;

pub(super) fn scratch_src(tile: &PixelTile) -> PixelTile {
    let mut src = PixelTile::new();
    src.copy_from(tile);
    src
}
