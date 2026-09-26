//! SVG exporter — `<text>` cells (path outlines deferred; still font-independent via text).

use crate::atlas::GlyphAtlas;
use crate::color::{resolve_rgb, ColorTarget};
use crate::export::cell_paint::{cell_char, cell_paints_paper};
use crate::grid::{AsciiGrid, CellColor, GridColorMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SvgOptions {
    pub target: ColorTarget,
    pub mono_fg: CellColor,
    pub mono_bg: CellColor,
}

impl Default for SvgOptions {
    fn default() -> Self {
        Self {
            target: ColorTarget::TrueColor,
            mono_fg: CellColor::BLACK,
            mono_bg: CellColor::WHITE,
        }
    }
}

/// SVG with per-cell background rects + monospace `<text>` glyphs.
///
/// Transparent / soft-empty cells omit background rects (no invented paper).
/// Soft-edge cells with ink get opaque paper under the glyph, matching preview.
pub fn to_svg(atlas: &GlyphAtlas, grid: &AsciiGrid, opts: SvgOptions) -> String {
    let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
    let width = grid.cols * cw;
    let height = grid.rows * ch;
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\" style=\"background:transparent\">\n"
    );
    // Merged bg rects per row run — only where we paint paper.
    for row in 0..grid.rows {
        let mut col0 = 0u32;
        while col0 < grid.cols {
            let cell0 = &grid.cells[(row * grid.cols + col0) as usize];
            if !cell_paints_paper(atlas, cell0) {
                col0 += 1;
                continue;
            }
            let bg0 = cell_bg(grid, cell0, opts);
            let mut col1 = col0 + 1;
            while col1 < grid.cols {
                let c = &grid.cells[(row * grid.cols + col1) as usize];
                if !cell_paints_paper(atlas, c) || cell_bg(grid, c, opts) != bg0 {
                    break;
                }
                col1 += 1;
            }
            let rgb = resolve_rgb(bg0, opts.target);
            out.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#{:02x}{:02x}{:02x}\"/>\n",
                col0 * cw,
                row * ch,
                (col1 - col0) * cw,
                ch,
                rgb[0],
                rgb[1],
                rgb[2]
            ));
            col0 = col1;
        }
    }
    out.push_str(&format!(
        "<g font-family=\"monospace\" font-size=\"{}\" dominant-baseline=\"hanging\">\n",
        atlas.px
    ));
    for row in 0..grid.rows {
        for col in 0..grid.cols {
            let cell = &grid.cells[(row * grid.cols + col) as usize];
            if cell.alpha == 0 {
                continue;
            }
            if cell.alpha < 255 && !cell_paints_paper(atlas, cell) {
                continue;
            }
            let fg = resolve_rgb(cell_fg(grid, cell, opts), opts.target);
            let ch_c = cell_char(atlas, cell);
            if ch_c == ' ' && !cell_paints_paper(atlas, cell) {
                continue;
            }
            let esc = xml_escape(ch_c);
            out.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" fill=\"#{:02x}{:02x}{:02x}\">{}</text>\n",
                col * cw,
                row * ch,
                fg[0],
                fg[1],
                fg[2],
                esc
            ));
        }
    }
    out.push_str("</g>\n</svg>\n");
    out
}

fn cell_fg(grid: &AsciiGrid, cell: &crate::grid::Cell, opts: SvgOptions) -> CellColor {
    match grid.color {
        GridColorMode::Mono => opts.mono_fg,
        GridColorMode::Fg | GridColorMode::FgBg => cell.fg,
    }
}

fn cell_bg(grid: &AsciiGrid, cell: &crate::grid::Cell, opts: SvgOptions) -> CellColor {
    match grid.color {
        GridColorMode::Mono | GridColorMode::Fg => opts.mono_bg,
        GridColorMode::FgBg => cell.bg,
    }
}

fn xml_escape(c: char) -> String {
    match c {
        '&' => "&amp;".into(),
        '<' => "&lt;".into(),
        '>' => "&gt;".into(),
        '"' => "&quot;".into(),
        '\'' => "&apos;".into(),
        _ => c.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::convert::{convert_mono, MatchMode};
    use crate::font::{BundledFont, FontFace};
    use crate::grid::{Cell, CellColor, GridColorMode};
    use crate::symbols::SymbolSet;

    #[test]
    fn svg_well_formed() {
        let atlas = GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &SymbolSet::Bourke10,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let rgba = vec![1.0f32; (cw * 2 * ch * 4) as usize];
        let grid = convert_mono(&rgba, cw * 2, ch, &atlas, MatchMode::Tone);
        let svg = to_svg(&atlas, &grid, SvgOptions::default());
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("background:transparent"));
    }

    #[test]
    fn svg_omits_rects_for_fully_transparent_grid() {
        let atlas = GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &SymbolSet::Bourke10,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let grid = AsciiGrid {
            cols: 2,
            rows: 1,
            cell_px: (cw, ch),
            atlas_key: atlas.key,
            color: GridColorMode::Mono,
            cells: vec![
                Cell {
                    glyph: 0,
                    fg: CellColor::BLACK,
                    bg: CellColor::WHITE,
                    alpha: 0,
                };
                2
            ],
        };
        let svg = to_svg(&atlas, &grid, SvgOptions::default());
        assert!(!svg.contains("<rect"));
        assert!(!svg.contains("<text"));
    }
}
