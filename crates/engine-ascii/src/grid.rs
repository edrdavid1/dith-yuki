//! Canonical ASCII artifact: a character grid (not pixels).

use crate::atlas::AtlasKey;

/// How colour is stored per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GridColorMode {
    /// Single ink on paper; `fg`/`bg` from settings, ignored per cell.
    Mono,
    /// Per-cell foreground, fixed background.
    Fg,
    /// Per-cell foreground and background.
    FgBg,
}

/// Packed sRGB8 colour (or a palette index when using indexed targets later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CellColor {
    Rgb([u8; 3]),
    Indexed(u8),
}

impl CellColor {
    pub const BLACK: Self = Self::Rgb([0, 0, 0]);
    pub const WHITE: Self = Self::Rgb([255, 255, 255]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cell {
    /// Index into the atlas glyph table.
    pub glyph: u16,
    pub fg: CellColor,
    pub bg: CellColor,
    /// Mean source alpha for this cell (`0` = fully transparent, `255` = opaque).
    pub alpha: u8,
}

/// Document-sized character grid — the single source of truth for preview and export.
#[derive(Debug, Clone)]
pub struct AsciiGrid {
    pub cols: u32,
    pub rows: u32,
    pub cell_px: (u32, u32),
    pub atlas_key: AtlasKey,
    pub color: GridColorMode,
    pub cells: Vec<Cell>,
}

impl AsciiGrid {
    pub fn cell(&self, col: u32, row: u32) -> Option<&Cell> {
        if col >= self.cols || row >= self.rows {
            return None;
        }
        self.cells.get((row * self.cols + col) as usize)
    }

    pub fn char_at(&self, atlas_chars: &[char], col: u32, row: u32) -> Option<char> {
        let cell = self.cell(col, row)?;
        atlas_chars.get(cell.glyph as usize).copied()
    }
}
