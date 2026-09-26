//! JSON exporter for tooling / round-trip.

use serde::Serialize;

use crate::atlas::GlyphAtlas;
use crate::grid::{AsciiGrid, CellColor, GridColorMode};

#[derive(Serialize)]
struct JsonDoc<'a> {
    cols: u32,
    rows: u32,
    cell_px: [u32; 2],
    color_mode: &'a str,
    codepoints: Vec<String>,
    cells: Vec<JsonCell>,
}

#[derive(Serialize)]
struct JsonCell {
    glyph: u16,
    ch: char,
    fg: JsonColor,
    bg: JsonColor,
    alpha: u8,
}

#[derive(Serialize)]
#[serde(untagged)]
enum JsonColor {
    Rgb { rgb: [u8; 3] },
    Indexed { index: u8 },
}

impl From<CellColor> for JsonColor {
    fn from(c: CellColor) -> Self {
        match c {
            CellColor::Rgb(rgb) => Self::Rgb { rgb },
            CellColor::Indexed(index) => Self::Indexed { index },
        }
    }
}

/// Serialize `AsciiGrid` + codepoint table as JSON.
pub fn to_json(atlas: &GlyphAtlas, grid: &AsciiGrid) -> Result<String, serde_json::Error> {
    let codepoints: Vec<String> = atlas.glyphs.iter().map(|g| g.ch.to_string()).collect();
    let cells = grid
        .cells
        .iter()
        .map(|c| JsonCell {
            glyph: c.glyph,
            ch: atlas.glyphs[c.glyph as usize].ch,
            fg: c.fg.into(),
            bg: c.bg.into(),
            alpha: c.alpha,
        })
        .collect();
    let doc = JsonDoc {
        cols: grid.cols,
        rows: grid.rows,
        cell_px: [grid.cell_px.0, grid.cell_px.1],
        color_mode: match grid.color {
            GridColorMode::Mono => "mono",
            GridColorMode::Fg => "fg",
            GridColorMode::FgBg => "fg_bg",
        },
        codepoints,
        cells,
    };
    serde_json::to_string_pretty(&doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::convert::{convert_mono, MatchMode};
    use crate::font::{BundledFont, FontFace};
    use crate::symbols::SymbolSet;

    #[test]
    fn json_round_trip_fields() {
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
        let rgba = vec![0.0f32; (cw * ch * 4) as usize];
        let grid = convert_mono(&rgba, cw, ch, &atlas, MatchMode::Tone);
        let s = to_json(&atlas, &grid).unwrap();
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["cols"], 1);
        assert_eq!(v["rows"], 1);
        assert!(v["codepoints"].as_array().unwrap().len() >= 10);
        assert_eq!(v["cells"].as_array().unwrap().len(), 1);
    }
}
