//! Helpers to build Guided / Mixed pass params from palette channel ranges.

use engine_color::oklab::{linear_to_oklab, LinRgb};
use engine_color::palette::Palette;
use engine_color::palette_guided::ChannelRange;

use crate::graph::{GpuPipelineKey, PaletteGuidedPassParams, PaletteMixedPassParams};
use std::sync::Arc;

pub fn palette_guided_params(
    matrix: GpuPipelineKey,
    channel_levels: u8,
    threshold_scale: f32,
    color_mode: u32,
    threshold_bias: f32,
    pattern_angle: f32,
    ranges: [ChannelRange; 3],
) -> PaletteGuidedPassParams {
    debug_assert!(matches!(
        matrix,
        GpuPipelineKey::Bayer2 | GpuPipelineKey::Bayer4 | GpuPipelineKey::Bayer8
    ));
    PaletteGuidedPassParams {
        matrix,
        channel_levels: channel_levels.max(2),
        threshold_scale,
        color_mode,
        threshold_bias,
        pattern_angle,
        ranges: [
            [ranges[0].min, ranges[0].max],
            [ranges[1].min, ranges[1].max],
            [ranges[2].min, ranges[2].max],
        ],
    }
}

pub fn palette_mixed_params_from_palette(
    guided: PaletteGuidedPassParams,
    palette: &Palette,
) -> PaletteMixedPassParams {
    let rgb: Vec<[f32; 3]> = palette
        .colors
        .iter()
        .map(|c| [c.r, c.g, c.b])
        .collect();
    let lab: Vec<[f32; 3]> = palette
        .colors
        .iter()
        .map(|c| {
            let o = linear_to_oklab(LinRgb {
                r: c.r,
                g: c.g,
                b: c.b,
            });
            [o.l, o.a, o.b]
        })
        .collect();
    PaletteMixedPassParams {
        guided,
        palette_rgb: Arc::from(rgb.into_boxed_slice()),
        palette_lab: Arc::from(lab.into_boxed_slice()),
    }
}
