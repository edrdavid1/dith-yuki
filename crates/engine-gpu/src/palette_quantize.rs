//! Helpers to build [`PaletteQuantizePassParams`] from `engine_color` LUT + palette.

use std::sync::Arc;

use engine_color::palette::Palette;
use engine_color::palette_lut::PaletteLut3D;

use crate::graph::PaletteQuantizePassParams;

/// Pack a CPU LUT + palette into graph/GPU payload (shared Arc for hash/equality).
pub fn palette_quantize_params_from_lut(
    lut: &PaletteLut3D,
    palette: &Palette,
) -> PaletteQuantizePassParams {
    let palette_rgb: Arc<[[f32; 3]]> = palette
        .colors
        .iter()
        .map(|c| [c.r, c.g, c.b])
        .collect::<Vec<_>>()
        .into();
    PaletteQuantizePassParams {
        lut_size: lut.size(),
        l_range: lut.l_range(),
        a_range: lut.a_range(),
        b_range: lut.b_range(),
        lut_grid: Arc::<[u16]>::from(lut.grid().to_vec()),
        palette_rgb,
    }
}
