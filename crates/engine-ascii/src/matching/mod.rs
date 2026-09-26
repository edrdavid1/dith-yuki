//! Matching: pick the best atlas glyph for a cell descriptor.

mod edge_overlay;
mod mask_two_color;

pub use edge_overlay::{orient_glyph, EdgeField, EdgeOrient, EdgeOverlay};
pub use mask_two_color::{match_mask_two_color, sample_cell_rgb, TwoColorMatch};

use crate::atlas::GlyphAtlas;
use crate::descriptor::{ShapeContrastDesc, ShapeDesc, ToneDesc};

/// Nearest glyph by tone (mean coverage), using the atlas tone order.
///
/// Tie-break: lowest glyph index (stable CPU/GPU).
pub fn match_tone(atlas: &GlyphAtlas, cell: ToneDesc) -> u16 {
    let order = atlas.tone_order();
    let mut best_i = order[0];
    let mut best_d = u32::MAX;
    for &i in &order {
        let g = &atlas.glyphs[i as usize];
        let desc = ToneDesc::from_coverage(&g.coverage, atlas.cell_w(), atlas.cell_h());
        let d = cell.distance(desc);
        if d < best_d || (d == best_d && i < best_i) {
            best_d = d;
            best_i = i;
        }
    }
    best_i
}

/// Nearest glyph by 6-D shape L2 distance.
pub fn match_shape(atlas: &GlyphAtlas, cell: ShapeDesc) -> u16 {
    let mut best_i = 0u16;
    let mut best_d = u64::MAX;
    for (i, g) in atlas.glyphs.iter().enumerate() {
        let desc = ShapeDesc::from_coverage(&g.coverage, atlas.cell_w(), atlas.cell_h());
        let d = cell.distance2(desc);
        let i = i as u16;
        if d < best_d || (d == best_d && i < best_i) {
            best_d = d;
            best_i = i;
        }
    }
    best_i
}

/// Shape match after Harri contrast stretch (`gamma_8_8`, 256 = 1.0).
pub fn match_shape_contrast(atlas: &GlyphAtlas, cell: ShapeDesc, gamma_8_8: u16) -> u16 {
    let cell = ShapeContrastDesc::from_shape(cell, gamma_8_8);
    let mut best_i = 0u16;
    let mut best_d = u64::MAX;
    for (i, g) in atlas.glyphs.iter().enumerate() {
        let shape = ShapeDesc::from_coverage(&g.coverage, atlas.cell_w(), atlas.cell_h());
        let desc = ShapeContrastDesc::from_shape(shape, gamma_8_8);
        let d = cell.distance2(desc);
        let i = i as u16;
        if d < best_d || (d == best_d && i < best_i) {
            best_d = d;
            best_i = i;
        }
    }
    best_i
}

/// Precomputed glyph descriptors for repeated matching (one atlas build).
pub struct MatcherTables {
    pub tone: Vec<ToneDesc>,
    pub shape: Vec<ShapeDesc>,
}

impl MatcherTables {
    pub fn build(atlas: &GlyphAtlas) -> Self {
        let (w, h) = (atlas.cell_w(), atlas.cell_h());
        let tone = atlas
            .glyphs
            .iter()
            .map(|g| ToneDesc::from_coverage(&g.coverage, w, h))
            .collect();
        let shape = atlas
            .glyphs
            .iter()
            .map(|g| ShapeDesc::from_coverage(&g.coverage, w, h))
            .collect();
        Self { tone, shape }
    }

    pub fn match_tone(&self, cell: ToneDesc) -> u16 {
        let mut best_i = 0u16;
        let mut best_d = u32::MAX;
        for (i, &desc) in self.tone.iter().enumerate() {
            let d = cell.distance(desc);
            let i = i as u16;
            if d < best_d || (d == best_d && i < best_i) {
                best_d = d;
                best_i = i;
            }
        }
        best_i
    }

    pub fn match_shape(&self, cell: ShapeDesc) -> u16 {
        let mut best_i = 0u16;
        let mut best_d = u64::MAX;
        for (i, &desc) in self.shape.iter().enumerate() {
            let d = cell.distance2(desc);
            let i = i as u16;
            if d < best_d || (d == best_d && i < best_i) {
                best_d = d;
                best_i = i;
            }
        }
        best_i
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::font::{BundledFont, FontFace};
    use crate::symbols::SymbolSet;

    fn plex_atlas(set: SymbolSet) -> GlyphAtlas {
        GlyphAtlas::build(
            &FontFace::bundled(BundledFont::IbmPlexMono),
            &set,
            &AtlasOptions {
                size: FontSize::Px(16.0),
                antialias: true,
                hinting: true,
            },
        )
        .unwrap()
    }

    #[test]
    fn round_trip_tone_every_glyph() {
        let atlas = plex_atlas(SymbolSet::Bourke70);
        let tables = MatcherTables::build(&atlas);
        let (w, h) = (atlas.cell_w(), atlas.cell_h());
        for (i, g) in atlas.glyphs.iter().enumerate() {
            let cell = ToneDesc::from_coverage(&g.coverage, w, h);
            let got = tables.match_tone(cell);
            // Exact coverage match must recover this glyph, or a tie with lower index
            // that has identical coverage.
            assert_eq!(
                tables.tone[got as usize].coverage, cell.coverage,
                "glyph {} ({:?}) matched to {} ({:?})",
                i, g.ch, got, atlas.glyphs[got as usize].ch
            );
        }
    }

    #[test]
    fn round_trip_shape_distinct_glyphs() {
        let atlas = plex_atlas(SymbolSet::PrintableAscii);
        let tables = MatcherTables::build(&atlas);
        let (w, h) = (atlas.cell_w(), atlas.cell_h());
        let mut recovered = 0usize;
        for (i, g) in atlas.glyphs.iter().enumerate() {
            let cell = ShapeDesc::from_coverage(&g.coverage, w, h);
            let got = tables.match_shape(cell) as usize;
            if got == i {
                recovered += 1;
            } else {
                // Only accept when descriptors are identical (true ties).
                assert_eq!(
                    tables.shape[got], cell,
                    "glyph {} ({:?}) lost to {} ({:?}) with different shape",
                    i, g.ch, got, atlas.glyphs[got].ch
                );
            }
        }
        // Printable ASCII should recover the vast majority uniquely.
        assert!(recovered >= 80, "only recovered {recovered}/95 uniquely");
    }

    #[test]
    fn empty_cell_picks_space() {
        let atlas = plex_atlas(SymbolSet::Bourke10);
        let tables = MatcherTables::build(&atlas);
        let blank = vec![0u8; (atlas.cell_w() * atlas.cell_h()) as usize];
        let tone = ToneDesc::from_coverage(&blank, atlas.cell_w(), atlas.cell_h());
        let i = tables.match_tone(tone);
        assert_eq!(atlas.glyphs[i as usize].ch, ' ');
    }
}
