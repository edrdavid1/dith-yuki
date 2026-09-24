//! UTF-8 plain text exporter.

use crate::atlas::GlyphAtlas;
use crate::grid::AsciiGrid;

/// UTF-8 plain text, one line per row. Colours are dropped.
///
/// `trim_trailing` removes trailing spaces on each line. Line endings are `\n`.
pub fn to_txt(atlas: &GlyphAtlas, grid: &AsciiGrid, trim_trailing: bool) -> String {
    let mut out = String::with_capacity((grid.cols as usize + 1) * grid.rows as usize);
    for row in 0..grid.rows {
        let mut line = String::with_capacity(grid.cols as usize);
        for col in 0..grid.cols {
            let cell = &grid.cells[(row * grid.cols + col) as usize];
            line.push(atlas.glyphs[cell.glyph as usize].ch);
        }
        if trim_trailing {
            let end = line.trim_end_matches(' ').len();
            line.truncate(end);
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::convert::{MatchMode, convert_mono};
    use crate::font::{BundledFont, FontFace};
    use crate::symbols::SymbolSet;

    #[test]
    fn txt_dimensions_and_chars() {
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
        let txt = to_txt(&atlas, &grid, false);
        assert_eq!(txt, "  \n");
        let trimmed = to_txt(&atlas, &grid, true);
        assert_eq!(trimmed, "\n");
    }
}
