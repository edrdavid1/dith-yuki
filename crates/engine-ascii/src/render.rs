//! CPU render: [`AsciiGrid`] + atlas → linear RGBA or sRGB8.

use engine_color::palette::{linear_to_srgb, srgb_to_linear};

use crate::atlas::GlyphAtlas;
use crate::color::{resolve_rgb, ColorTarget};
use crate::grid::{AsciiGrid, CellColor, GridColorMode};

/// Render grid to linear RGBA f32 (`cols*cw × rows*ch`).
///
/// Per-cell [`Cell::alpha`](crate::grid::Cell::alpha) is preserved: fully
/// transparent cells stay clear (RGB zero, A=0); opaque cells keep A=1.
pub fn render_rgba(
    atlas: &GlyphAtlas,
    grid: &AsciiGrid,
    target: ColorTarget,
    mono_fg: CellColor,
    mono_bg: CellColor,
) -> (u32, u32, Vec<f32>) {
    let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
    let width = grid.cols * cw;
    let height = grid.rows * ch;
    let mut rgba = vec![0.0f32; (width * height * 4) as usize];

    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = &grid.cells[(row * grid.cols + col) as usize];
            let cell_a = cell.alpha as f32 / 255.0;
            if cell_a <= 0.0 {
                // Leave zeros already written (fully transparent).
                continue;
            }
            let (fg8, bg8) = match grid.color {
                GridColorMode::Mono => (
                    resolve_rgb(mono_fg, target),
                    resolve_rgb(mono_bg, target),
                ),
                GridColorMode::Fg => (
                    resolve_rgb(cell.fg, target),
                    resolve_rgb(mono_bg, target),
                ),
                GridColorMode::FgBg => (
                    resolve_rgb(cell.fg, target),
                    resolve_rgb(cell.bg, target),
                ),
            };
            let fg = [
                srgb_to_linear(fg8[0]),
                srgb_to_linear(fg8[1]),
                srgb_to_linear(fg8[2]),
            ];
            let bg = [
                srgb_to_linear(bg8[0]),
                srgb_to_linear(bg8[1]),
                srgb_to_linear(bg8[2]),
            ];
            let g = &atlas.glyphs[cell.glyph as usize];
            let ox = col * cw;
            let oy = row * ch;
            for ly in 0..ch {
                for lx in 0..cw {
                    let t = g.coverage[(ly * cw + lx) as usize] as f32 / 255.0;
                    let i = ((oy + ly) * width + (ox + lx)) * 4;
                    rgba[i as usize] = bg[0] + (fg[0] - bg[0]) * t;
                    rgba[i as usize + 1] = bg[1] + (fg[1] - bg[1]) * t;
                    rgba[i as usize + 2] = bg[2] + (fg[2] - bg[2]) * t;
                    rgba[i as usize + 3] = cell_a;
                }
            }
        }
    }
    (width, height, rgba)
}

/// Render to packed sRGB8 RGBA (`width*height*4` bytes).
pub fn render_rgba8(
    atlas: &GlyphAtlas,
    grid: &AsciiGrid,
    target: ColorTarget,
    mono_fg: CellColor,
    mono_bg: CellColor,
) -> (u32, u32, Vec<u8>) {
    let (w, h, lin) = render_rgba(atlas, grid, target, mono_fg, mono_bg);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        out[i * 4] = linear_to_srgb(lin[i * 4]);
        out[i * 4 + 1] = linear_to_srgb(lin[i * 4 + 1]);
        out[i * 4 + 2] = linear_to_srgb(lin[i * 4 + 2]);
        out[i * 4 + 3] = (lin[i * 4 + 3].clamp(0.0, 1.0) * 255.0).round() as u8;
    }
    (w, h, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::convert::{convert_mono, MatchMode};
    use crate::font::{BundledFont, FontFace};
    use crate::grid::Cell;
    use crate::symbols::SymbolSet;

    fn atlas() -> GlyphAtlas {
        GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &SymbolSet::Bourke10,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn transparent_source_stays_transparent() {
        let atlas = atlas();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        // Fully transparent black — must not paint opaque paper.
        let rgba = vec![0.0f32; (cw * ch * 4) as usize];
        let grid = convert_mono(&rgba, cw, ch, &atlas, MatchMode::Tone);
        assert_eq!(grid.cells[0].alpha, 0);
        let (_, _, out) = render_rgba8(
            &atlas,
            &grid,
            ColorTarget::TrueColor,
            CellColor::BLACK,
            CellColor::WHITE,
        );
        for i in 0..(cw * ch) as usize {
            assert_eq!(out[i * 4 + 3], 0, "pixel {i} alpha");
        }
    }

    #[test]
    fn opaque_source_stays_opaque() {
        let atlas = atlas();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let rgba = vec![1.0f32; (cw * ch * 4) as usize];
        let grid = convert_mono(&rgba, cw, ch, &atlas, MatchMode::Tone);
        assert_eq!(grid.cells[0].alpha, 255);
        let (_, _, out) = render_rgba8(
            &atlas,
            &grid,
            ColorTarget::TrueColor,
            CellColor::BLACK,
            CellColor::WHITE,
        );
        for i in 0..(cw * ch) as usize {
            assert_eq!(out[i * 4 + 3], 255, "pixel {i} alpha");
        }
    }

    #[test]
    fn cell_alpha_passthrough_in_render() {
        let atlas = atlas();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let grid = AsciiGrid {
            cols: 1,
            rows: 1,
            cell_px: (cw, ch),
            atlas_key: atlas.key,
            color: GridColorMode::Mono,
            cells: vec![Cell {
                glyph: 0,
                fg: CellColor::BLACK,
                bg: CellColor::WHITE,
                alpha: 128,
            }],
        };
        let (_, _, lin) = render_rgba(
            &atlas,
            &grid,
            ColorTarget::TrueColor,
            CellColor::BLACK,
            CellColor::WHITE,
        );
        assert!((lin[3] - 128.0 / 255.0).abs() < 1e-5);
    }
}
