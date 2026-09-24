//! Monospace font loading, validation and cell metrics.

use std::sync::Arc;

use swash::FontRef;
use thiserror::Error;

use crate::fnv1a64;

/// Fonts shipped with the app. Both are SIL OFL 1.1 (see `assets/fonts/*-OFL.txt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BundledFont {
    IbmPlexMono,
    DepartureMono,
}

impl BundledFont {
    pub const ALL: [BundledFont; 2] = [BundledFont::IbmPlexMono, BundledFont::DepartureMono];

    pub fn bytes(self) -> &'static [u8] {
        match self {
            BundledFont::IbmPlexMono => include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf"),
            BundledFont::DepartureMono => {
                include_bytes!("../assets/fonts/DepartureMono-Regular.otf")
            }
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            BundledFont::IbmPlexMono => "IBM Plex Mono",
            BundledFont::DepartureMono => "Departure Mono",
        }
    }

    /// Pixel size at which the font's outlines land exactly on the pixel grid.
    /// Pixel fonts must be rendered at integer multiples of this size with
    /// antialiasing off to stay crisp.
    pub fn native_px(self) -> Option<f32> {
        match self {
            BundledFont::IbmPlexMono => None,
            BundledFont::DepartureMono => Some(11.0),
        }
    }
}

/// Stable content hash of the font file; identifies the font in cache keys
/// and (later) in saved projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FontId(pub u64);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FontError {
    #[error("not a readable TrueType/OpenType font")]
    Parse,
    #[error("font has no glyphs for printable ASCII")]
    NoAsciiCoverage,
    #[error("font is not monospace: printable ASCII advances differ ({min} vs {max} units)")]
    NotMonospace { min: u32, max: u32 },
}

pub const MIN_FONT_PX: f32 = 4.0;
pub const MAX_FONT_PX: f32 = 256.0;

/// Integer cell geometry for a font at a given pixel size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellMetrics {
    pub cell_w: u32,
    pub cell_h: u32,
    /// Baseline position, in pixels from the top of the cell.
    pub baseline: u32,
}

/// A validated monospace font. Cheap to clone.
#[derive(Clone)]
pub struct FontFace {
    bytes: Arc<[u8]>,
    id: FontId,
    name: String,
    units_per_em: f32,
    advance: f32,
    ascent: f32,
    descent: f32,
    leading: f32,
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFace")
            .field("id", &self.id)
            .field("name", &self.name)
            .finish()
    }
}

impl FontFace {
    pub fn bundled(font: BundledFont) -> Self {
        Self::from_bytes(font.bytes().into(), font.display_name().to_owned())
            .expect("bundled fonts are valid monospace fonts")
    }

    pub fn from_bytes(bytes: Arc<[u8]>, name: String) -> Result<Self, FontError> {
        let font = FontRef::from_index(&bytes, 0).ok_or(FontError::Parse)?;
        let metrics = font.metrics(&[]);
        if metrics.units_per_em == 0 {
            return Err(FontError::Parse);
        }
        let charmap = font.charmap();
        let glyph_metrics = font.glyph_metrics(&[]);
        let advances: Vec<u32> = (0x21u32..=0x7E)
            .map(|cp| charmap.map(cp))
            .filter(|&gid| gid != 0)
            .map(|gid| glyph_metrics.advance_width(gid).round() as u32)
            .collect();
        let advance = validate_advances(&advances)?;
        let id = FontId(fnv1a64(&bytes));
        Ok(Self {
            id,
            name,
            units_per_em: metrics.units_per_em as f32,
            advance: advance as f32,
            ascent: metrics.ascent,
            descent: metrics.descent,
            leading: metrics.leading,
            bytes,
        })
    }

    pub fn id(&self) -> FontId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn has_char(&self, ch: char) -> bool {
        self.font_ref().charmap().map(ch) != 0
    }

    pub(crate) fn font_ref(&self) -> FontRef<'_> {
        FontRef::from_index(&self.bytes, 0).expect("validated at construction")
    }

    pub(crate) fn glyph_id(&self, ch: char) -> u16 {
        self.font_ref().charmap().map(ch)
    }

    /// Cell geometry at `px` (font size in pixels per em, clamped to the supported range).
    ///
    /// Leading is split with the smaller half above the ascent so that
    /// box-drawing glyphs designed to span the line box stay connected.
    pub fn cell_metrics(&self, px: f32) -> CellMetrics {
        let s = clamp_px(px) / self.units_per_em;
        let cell_w = (self.advance * s).round().max(1.0) as u32;
        let ascent = (self.ascent * s).round().max(0.0) as u32;
        let descent = (self.descent * s).round().max(0.0) as u32;
        let leading = (self.leading * s).round().max(0.0) as u32;
        let cell_h = (ascent + descent + leading).max(1);
        CellMetrics {
            cell_w,
            cell_h,
            baseline: leading / 2 + ascent,
        }
    }

    /// Largest font size whose cell width fits `cols` columns into `doc_width` pixels.
    ///
    /// The result lies on the 1/64 px grid used by atlas keys, so keying does
    /// not round it back up past the fit.
    pub fn px_for_columns(&self, cols: u32, doc_width: u32) -> f32 {
        let target_w = (doc_width / cols.max(1)).max(1);
        // cell_w rounds to nearest, so aim for the upper edge of the rounding bucket.
        let px = (target_w as f32 + 0.499) * self.units_per_em / self.advance;
        let min_64 = (MIN_FONT_PX * 64.0) as u32;
        let mut px_64 = ((clamp_px(px) * 64.0).floor() as u32).max(min_64);
        while px_64 > min_64 && self.cell_metrics(px_64 as f32 / 64.0).cell_w > target_w {
            px_64 -= 1;
        }
        px_64 as f32 / 64.0
    }
}

fn clamp_px(px: f32) -> f32 {
    if px.is_finite() {
        px.clamp(MIN_FONT_PX, MAX_FONT_PX)
    } else {
        MIN_FONT_PX
    }
}

fn validate_advances(advances: &[u32]) -> Result<u32, FontError> {
    let min = *advances.iter().min().ok_or(FontError::NoAsciiCoverage)?;
    let max = *advances.iter().max().ok_or(FontError::NoAsciiCoverage)?;
    if min != max {
        return Err(FontError::NotMonospace { min, max });
    }
    Ok(min)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_fonts_load() {
        for font in BundledFont::ALL {
            let face = FontFace::bundled(font);
            assert!(face.has_char('A'), "{font:?} maps 'A'");
        }
    }

    #[test]
    fn departure_native_size_is_7x14() {
        let face = FontFace::bundled(BundledFont::DepartureMono);
        assert_eq!(
            face.cell_metrics(11.0),
            CellMetrics {
                cell_w: 7,
                cell_h: 14,
                baseline: 11
            }
        );
        assert_eq!(
            face.cell_metrics(22.0),
            CellMetrics {
                cell_w: 14,
                cell_h: 28,
                baseline: 22
            }
        );
    }

    #[test]
    fn plex_metrics_follow_hhea() {
        // upem 1000, advance 600, hhea ascent 1025 / descent 275 / gap 0.
        // At 20 px: 12 / 20.5 → 21 / 5.5 → 6.
        let face = FontFace::bundled(BundledFont::IbmPlexMono);
        assert_eq!(
            face.cell_metrics(20.0),
            CellMetrics {
                cell_w: 12,
                cell_h: 27,
                baseline: 21
            }
        );
    }

    #[test]
    fn font_ids_are_stable_and_distinct() {
        let a = FontFace::bundled(BundledFont::IbmPlexMono);
        let b = FontFace::bundled(BundledFont::DepartureMono);
        assert_eq!(a.id(), FontFace::bundled(BundledFont::IbmPlexMono).id());
        assert_ne!(a.id(), b.id());
    }

    #[test]
    fn garbage_is_rejected() {
        let err = FontFace::from_bytes(vec![0u8; 64].into(), "junk".into()).unwrap_err();
        assert_eq!(err, FontError::Parse);
    }

    #[test]
    fn advance_validation() {
        assert_eq!(validate_advances(&[600, 600, 600]), Ok(600));
        assert_eq!(
            validate_advances(&[600, 580]),
            Err(FontError::NotMonospace { min: 580, max: 600 })
        );
        assert_eq!(validate_advances(&[]), Err(FontError::NoAsciiCoverage));
    }

    #[test]
    fn columns_sizing_fits_width() {
        let face = FontFace::bundled(BundledFont::IbmPlexMono);
        for (cols, width) in [(80, 1920), (120, 1920), (160, 3840), (200, 1000)] {
            let px = face.px_for_columns(cols, width);
            let m = face.cell_metrics(px);
            assert!(m.cell_w * cols <= width, "{cols} cols @ {width}: cell_w {}", m.cell_w);
            assert!(m.cell_w >= width / cols - 1 || px == MIN_FONT_PX);
        }
    }

    #[test]
    fn px_is_clamped() {
        let face = FontFace::bundled(BundledFont::IbmPlexMono);
        assert_eq!(face.cell_metrics(f32::NAN), face.cell_metrics(MIN_FONT_PX));
        assert_eq!(face.cell_metrics(10_000.0), face.cell_metrics(MAX_FONT_PX));
    }
}
