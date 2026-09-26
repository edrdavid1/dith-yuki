//! Glyph atlas: one coverage raster per symbol at the cell size, plus a cache.

use std::sync::Arc;

use dashmap::DashMap;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;
use thiserror::Error;

use crate::font::{CellMetrics, FontFace, FontId, MAX_FONT_PX, MIN_FONT_PX};
use crate::symbols::{GlyphSource, SymbolSet};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontSize {
    /// Font size in pixels per em.
    Px(f32),
    /// Size chosen so that `cols` cells fit across `doc_width` pixels.
    Columns { cols: u32, doc_width: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct AtlasOptions {
    pub size: FontSize,
    /// Grayscale antialiasing. Off thresholds coverage at 50% (pixel fonts).
    pub antialias: bool,
    /// TrueType hinting (grid fitting).
    pub hinting: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AtlasKey {
    pub font: FontId,
    /// Font size in 1/64 px.
    pub px_64: u32,
    pub antialias: bool,
    pub hinting: bool,
    pub symbols: u64,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AtlasError {
    #[error("no symbols left after dropping characters missing from the font")]
    EmptySymbolSet,
    #[error("symbol set has {0} symbols; the limit is {MAX_SYMBOLS}")]
    TooManySymbols(usize),
}

/// Glyph indices are stored as `u16` in grids.
pub const MAX_SYMBOLS: usize = u16::MAX as usize;

#[derive(Debug, Clone)]
pub struct AtlasGlyph {
    pub ch: char,
    pub source: GlyphSource,
    /// Row-major `cell_w × cell_h` coverage, 0 = paper, 255 = ink.
    pub coverage: Box<[u8]>,
    /// Sum of `coverage`; the glyph's ink amount.
    pub ink: u32,
}

#[derive(Debug, Clone)]
pub struct GlyphAtlas {
    pub key: AtlasKey,
    pub px: f32,
    pub metrics: CellMetrics,
    pub glyphs: Vec<AtlasGlyph>,
    /// Characters requested by the symbol set but absent from the font.
    pub missing: Vec<char>,
}

impl GlyphAtlas {
    pub fn key_for(font: &FontFace, set: &SymbolSet, opts: &AtlasOptions) -> AtlasKey {
        AtlasKey {
            font: font.id(),
            px_64: (resolve_px(font, opts.size) * 64.0).round() as u32,
            antialias: opts.antialias,
            hinting: opts.hinting,
            symbols: set.cache_key(),
        }
    }

    pub fn build(
        font: &FontFace,
        set: &SymbolSet,
        opts: &AtlasOptions,
    ) -> Result<Self, AtlasError> {
        let key = Self::key_for(font, set, opts);
        let px = key.px_64 as f32 / 64.0;
        let metrics = font.cell_metrics(px);
        let (w, h) = (metrics.cell_w, metrics.cell_h);

        let symbols = set.symbols();
        if symbols.len() > MAX_SYMBOLS {
            return Err(AtlasError::TooManySymbols(symbols.len()));
        }

        let mut ctx = ScaleContext::new();
        let font_ref = font.font_ref();
        let mut scaler = ctx.builder(font_ref).size(px).hint(opts.hinting).build();
        let render_sources = [Source::Bitmap(StrikeWith::ExactSize), Source::Outline];
        let mut render = Render::new(&render_sources);
        render.format(Format::Alpha);

        let mut glyphs = Vec::with_capacity(symbols.len());
        let mut missing = Vec::new();
        for sym in symbols {
            let mut coverage = vec![0u8; (w * h) as usize].into_boxed_slice();
            match sym.source {
                GlyphSource::Procedural(p) => p.rasterize(w, h, &mut coverage),
                GlyphSource::Font => {
                    let gid = font.glyph_id(sym.ch);
                    if gid == 0 && !sym.ch.is_whitespace() {
                        missing.push(sym.ch);
                        continue;
                    }
                    if gid != 0 {
                        if let Some(img) = render.render(&mut scaler, gid) {
                            blit_alpha(&img, metrics, &mut coverage);
                        }
                    }
                }
            }
            if !opts.antialias {
                for v in coverage.iter_mut() {
                    *v = if *v >= 128 { 255 } else { 0 };
                }
            }
            let ink = coverage.iter().map(|&v| v as u32).sum();
            glyphs.push(AtlasGlyph {
                ch: sym.ch,
                source: sym.source,
                coverage,
                ink,
            });
        }
        if glyphs.is_empty() {
            return Err(AtlasError::EmptySymbolSet);
        }
        Ok(Self {
            key,
            px,
            metrics,
            glyphs,
            missing,
        })
    }

    pub fn cell_w(&self) -> u32 {
        self.metrics.cell_w
    }

    pub fn cell_h(&self) -> u32 {
        self.metrics.cell_h
    }

    /// Glyph indices sorted by ink (ascending); ties keep the lower index first.
    pub fn tone_order(&self) -> Vec<u16> {
        let mut order: Vec<u16> = (0..self.glyphs.len() as u16).collect();
        order.sort_by_key(|&i| (self.glyphs[i as usize].ink, i));
        order
    }
}

fn resolve_px(font: &FontFace, size: FontSize) -> f32 {
    let px = match size {
        FontSize::Px(px) => px,
        FontSize::Columns { cols, doc_width } => font.px_for_columns(cols, doc_width),
    };
    if px.is_finite() {
        px.clamp(MIN_FONT_PX, MAX_FONT_PX)
    } else {
        MIN_FONT_PX
    }
}

/// Copy a rendered glyph into the cell, pen at `(0, baseline)`; ink outside the
/// cell is clipped.
fn blit_alpha(img: &swash::scale::image::Image, metrics: CellMetrics, cell: &mut [u8]) {
    let p = img.placement;
    let (cw, ch) = (metrics.cell_w as i32, metrics.cell_h as i32);
    let x0 = p.left;
    let y0 = metrics.baseline as i32 - p.top;
    for gy in 0..p.height as i32 {
        let y = y0 + gy;
        if !(0..ch).contains(&y) {
            continue;
        }
        for gx in 0..p.width as i32 {
            let x = x0 + gx;
            if !(0..cw).contains(&x) {
                continue;
            }
            let v = img.data[(gy * p.width as i32 + gx) as usize];
            let dst = &mut cell[(y * cw + x) as usize];
            *dst = (*dst).max(v);
        }
    }
}

/// Shared atlas cache (one per app). Atlases are immutable once built.
#[derive(Default)]
pub struct AtlasCache {
    entries: DashMap<AtlasKey, Arc<GlyphAtlas>>,
}

impl AtlasCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_build(
        &self,
        font: &FontFace,
        set: &SymbolSet,
        opts: &AtlasOptions,
    ) -> Result<Arc<GlyphAtlas>, AtlasError> {
        let key = GlyphAtlas::key_for(font, set, opts);
        if let Some(hit) = self.entries.get(&key) {
            return Ok(Arc::clone(&hit));
        }
        let atlas = Arc::new(GlyphAtlas::build(font, set, opts)?);
        Ok(Arc::clone(self.entries.entry(key).or_insert(atlas).value()))
    }

    pub fn evict_font(&self, font: FontId) {
        self.entries.retain(|k, _| k.font != font);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fnv1a64;
    use crate::font::BundledFont;

    fn departure_native(set: SymbolSet) -> GlyphAtlas {
        GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &set,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap()
    }

    fn plex(set: SymbolSet, px: f32) -> GlyphAtlas {
        GlyphAtlas::build(
            &FontFace::bundled(BundledFont::IbmPlexMono),
            &set,
            &AtlasOptions {
                size: FontSize::Px(px),
                antialias: true,
                hinting: true,
            },
        )
        .unwrap()
    }

    fn glyph<'a>(atlas: &'a GlyphAtlas, ch: char) -> &'a AtlasGlyph {
        atlas.glyphs.iter().find(|g| g.ch == ch).unwrap()
    }

    #[test]
    fn pixel_font_at_native_size_is_binary() {
        let atlas = departure_native(SymbolSet::PrintableAscii);
        assert_eq!((atlas.cell_w(), atlas.cell_h()), (7, 14));
        assert_eq!(atlas.glyphs.len(), 95);
        assert!(atlas.missing.is_empty());
        for g in &atlas.glyphs {
            assert!(g.coverage.iter().all(|&v| v == 0 || v == 255), "{:?}", g.ch);
        }
        assert_eq!(glyph(&atlas, ' ').ink, 0);
        assert!(glyph(&atlas, '@').ink > glyph(&atlas, '.').ink);
    }

    #[test]
    fn every_printable_glyph_has_ink_except_space() {
        let atlas = plex(SymbolSet::PrintableAscii, 16.0);
        for g in &atlas.glyphs {
            assert_eq!(g.ink == 0, g.ch == ' ', "{:?} ink {}", g.ch, g.ink);
        }
    }

    #[test]
    fn procedural_full_block_fills_cell() {
        let atlas = plex(SymbolSet::Blocks, 16.0);
        let full = glyph(&atlas, '█');
        assert!(full.coverage.iter().all(|&v| v == 255));
        assert_eq!(full.ink, 255 * atlas.cell_w() * atlas.cell_h());
    }

    #[test]
    fn missing_glyphs_are_reported_not_fatal() {
        // Plex Mono has no Hangul; the Latin letters survive.
        let atlas = plex(SymbolSet::Custom("aㄱb".into()), 16.0);
        assert_eq!(atlas.missing, vec!['ㄱ']);
        assert_eq!(atlas.glyphs.iter().map(|g| g.ch).collect::<String>(), "ab");
        let err = GlyphAtlas::build(
            &FontFace::bundled(BundledFont::IbmPlexMono),
            &SymbolSet::Custom("ㄱㄴ".into()),
            &AtlasOptions {
                size: FontSize::Px(16.0),
                antialias: true,
                hinting: true,
            },
        )
        .unwrap_err();
        assert_eq!(err, AtlasError::EmptySymbolSet);
    }

    #[test]
    fn tone_order_is_monotonic() {
        let atlas = plex(SymbolSet::Bourke70, 16.0);
        let order = atlas.tone_order();
        assert_eq!(order.len(), 70);
        for pair in order.windows(2) {
            let (a, b) = (
                &atlas.glyphs[pair[0] as usize],
                &atlas.glyphs[pair[1] as usize],
            );
            assert!(a.ink < b.ink || (a.ink == b.ink && pair[0] < pair[1]));
        }
        assert_eq!(atlas.glyphs[order[0] as usize].ch, ' ');
    }

    #[test]
    fn braille_and_octants_need_no_font_support() {
        let atlas = departure_native(SymbolSet::Braille);
        assert_eq!(atlas.glyphs.len(), 256);
        assert!(atlas.missing.is_empty());
        let atlas = departure_native(SymbolSet::Octants);
        assert_eq!(atlas.glyphs.len(), 256);
    }

    #[test]
    fn cache_shares_atlases_by_key() {
        let cache = AtlasCache::new();
        let font = FontFace::bundled(BundledFont::IbmPlexMono);
        let opts = AtlasOptions {
            size: FontSize::Px(16.0),
            antialias: true,
            hinting: true,
        };
        let a = cache
            .get_or_build(&font, &SymbolSet::Bourke10, &opts)
            .unwrap();
        let b = cache
            .get_or_build(&font, &SymbolSet::Bourke10, &opts)
            .unwrap();
        assert!(Arc::ptr_eq(&a, &b));
        let c = cache
            .get_or_build(
                &font,
                &SymbolSet::Bourke10,
                &AtlasOptions {
                    hinting: false,
                    ..opts
                },
            )
            .unwrap();
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(cache.len(), 2);
        cache.evict_font(font.id());
        assert!(cache.is_empty());
    }

    #[test]
    fn columns_mode_resolves_to_fitting_px() {
        let font = FontFace::bundled(BundledFont::IbmPlexMono);
        let atlas = GlyphAtlas::build(
            &font,
            &SymbolSet::Bourke10,
            &AtlasOptions {
                size: FontSize::Columns {
                    cols: 120,
                    doc_width: 1920,
                },
                antialias: true,
                hinting: true,
            },
        )
        .unwrap();
        assert!(atlas.cell_w() * 120 <= 1920);
        assert_eq!(atlas.cell_w(), 16);
    }

    /// Golden rasters: guards against silent changes from rasterizer upgrades.
    /// If this fails after a deliberate dependency bump, inspect the new
    /// rasters, then update the constants.
    #[test]
    fn golden_raster_hashes() {
        let hash = |atlas: &GlyphAtlas| {
            let mut bytes = Vec::new();
            for g in &atlas.glyphs {
                bytes.extend_from_slice(&(g.ch as u32).to_le_bytes());
                bytes.extend_from_slice(&g.coverage);
            }
            fnv1a64(&bytes)
        };
        let departure = hash(&departure_native(SymbolSet::PrintableAscii));
        let plex16 = hash(&plex(SymbolSet::PrintableAscii, 16.0));
        assert_eq!(
            (departure, plex16),
            (GOLDEN_DEPARTURE_11, GOLDEN_PLEX_16),
            "departure {departure:#018x} plex16 {plex16:#018x}"
        );
    }

    #[test]
    #[ignore = "manual inspection aid"]
    fn dump_rasters() {
        for atlas in [
            departure_native(SymbolSet::Custom("Agj|@█▚⣿".into())),
            plex(SymbolSet::Custom("Agj|@─│┼".into()), 16.0),
        ] {
            let (w, h) = (atlas.cell_w() as usize, atlas.cell_h() as usize);
            println!("cell {w}x{h} baseline {}", atlas.metrics.baseline);
            for y in 0..h {
                let mut line = String::new();
                for g in &atlas.glyphs {
                    for x in 0..w {
                        line.push(match g.coverage[y * w + x] {
                            0 => '.',
                            1..=127 => '+',
                            _ => '#',
                        });
                    }
                    line.push_str("  ");
                }
                println!(
                    "{line}{}",
                    if y as u32 == atlas.metrics.baseline {
                        " <baseline"
                    } else {
                        ""
                    }
                );
            }
        }
    }

    const GOLDEN_DEPARTURE_11: u64 = 0xe71a_8d99_989d_a666;
    const GOLDEN_PLEX_16: u64 = 0x3994_6513_eb1d_45fa;
}
