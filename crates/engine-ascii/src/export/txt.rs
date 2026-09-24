//! UTF-8 plain text exporter.

use crate::atlas::GlyphAtlas;
use crate::grid::AsciiGrid;

/// Bourke-style single-column ASCII ladder (dark → dense).
/// Never emit Unicode block/braille here: many editors treat those as
/// double-width, which soft-wraps mid-row and stacks vertical strips.
const ASCII_RAMP: &[u8] = b" .:-=+*#%@";

/// Max columns for plain-text export so typical editors (wrap ~80–100) keep
/// one image row on one text line instead of slicing the art into strips.
pub const TXT_EXPORT_MAX_COLS: u32 = 96;

/// UTF-8 plain text, one fixed-width line per row. Colours are dropped.
///
/// Every line is exactly `grid.cols` characters. Glyphs are mapped to the
/// single-width [`ASCII_RAMP`] by ink amount (transparent → space). Line
/// endings are `\n`.
///
/// `trim_trailing` is for log dumps only — **off** for image export.
pub fn to_txt(atlas: &GlyphAtlas, grid: &AsciiGrid, trim_trailing: bool) -> String {
    let cols = grid.cols as usize;
    let mut out = String::with_capacity((cols + 1) * grid.rows as usize);
    for row in 0..grid.rows {
        let mut line = String::with_capacity(cols);
        for col in 0..grid.cols {
            let cell = &grid.cells[(row * grid.cols + col) as usize];
            line.push(txt_char(atlas, cell));
        }
        debug_assert_eq!(line.chars().count(), cols);
        if trim_trailing {
            let end = line.trim_end_matches(' ').len();
            line.truncate(end);
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn txt_char(atlas: &GlyphAtlas, cell: &crate::grid::Cell) -> char {
    if cell.alpha == 0 {
        return ' ';
    }
    let g = &atlas.glyphs[cell.glyph as usize];
    // Prefer the glyph's own character when it is already single-width ASCII.
    if is_plain_ascii_graphic(g.ch) {
        return g.ch;
    }
    let n = g.coverage.len().max(1) as u64;
    let ink = g.ink as u64;
    let idx = ((ink * (ASCII_RAMP.len() as u64 - 1) + n / 2) / n) as usize;
    ASCII_RAMP[idx.min(ASCII_RAMP.len() - 1)] as char
}

fn is_plain_ascii_graphic(ch: char) -> bool {
    let u = ch as u32;
    (0x20..=0x7E).contains(&u)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::convert::{MatchMode, convert_mono};
    use crate::font::{BundledFont, FontFace};
    use crate::grid::{Cell, CellColor, GridColorMode};
    use crate::symbols::SymbolSet;

    fn atlas_bourke() -> GlyphAtlas {
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

    fn atlas_blocks() -> GlyphAtlas {
        GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &SymbolSet::Blocks,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap()
    }

    #[test]
    fn txt_dimensions_and_chars() {
        let atlas = atlas_bourke();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let rgba = vec![1.0f32; (cw * 2 * ch * 4) as usize];
        let grid = convert_mono(&rgba, cw * 2, ch, &atlas, MatchMode::Tone);
        let txt = to_txt(&atlas, &grid, false);
        assert_eq!(txt.chars().filter(|&c| c == '\n').count(), 1);
        assert_eq!(txt.lines().next().unwrap().chars().count(), 2);
        let trimmed = to_txt(&atlas, &grid, true);
        assert_eq!(trimmed, "\n");
    }

    #[test]
    fn txt_never_emits_wide_unicode_from_blocks() {
        let atlas = atlas_blocks();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let mut rgba = vec![1.0f32; (cw * ch * 4) as usize];
        for i in 0..(cw * ch) as usize {
            rgba[i * 4] = 0.0;
            rgba[i * 4 + 1] = 0.0;
            rgba[i * 4 + 2] = 0.0;
        }
        let grid = convert_mono(&rgba, cw, ch, &atlas, MatchMode::Tone);
        let txt = to_txt(&atlas, &grid, false);
        for c in txt.chars() {
            if c == '\n' {
                continue;
            }
            assert!(
                is_plain_ascii_graphic(c),
                "TXT must be single-width ASCII, got U+{:04X}",
                c as u32
            );
        }
    }

    #[test]
    fn txt_keeps_rectangular_grid_with_transparent_cells() {
        let atlas = atlas_bourke();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let mut cells = vec![
            Cell {
                glyph: 0,
                fg: CellColor::BLACK,
                bg: CellColor::WHITE,
                alpha: 255,
            };
            8
        ];
        for i in [2usize, 3, 6, 7] {
            cells[i].alpha = 0;
        }
        let grid = AsciiGrid {
            cols: 4,
            rows: 2,
            cell_px: (cw, ch),
            atlas_key: atlas.key,
            color: GridColorMode::Mono,
            cells,
        };
        let txt = to_txt(&atlas, &grid, false);
        let lines: Vec<&str> = txt.lines().collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].chars().count(), 4);
        assert_eq!(lines[1].chars().count(), 4);
    }
}
