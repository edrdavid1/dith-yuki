//! Shared export helpers.

use crate::atlas::GlyphAtlas;
use crate::grid::Cell;

/// Whether this cell should paint opaque paper (matches [`crate::render::render_rgba`]).
pub fn cell_paints_paper(atlas: &GlyphAtlas, cell: &Cell) -> bool {
    if cell.alpha == 0 {
        return false;
    }
    if cell.alpha == 255 {
        return true;
    }
    let g = &atlas.glyphs[cell.glyph as usize];
    g.coverage.iter().any(|&c| c > 0)
}

/// Glyph character, or space for fully clear cells (keeps TXT/HTML column alignment).
pub fn cell_char(atlas: &GlyphAtlas, cell: &Cell) -> char {
    if cell.alpha == 0 {
        return ' ';
    }
    atlas.glyphs[cell.glyph as usize].ch
}
