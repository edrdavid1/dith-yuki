//! Mask + two-colour fitting (chafa / libcaca style).
//!
//! For each glyph, treat its coverage as a soft mask: pixels with coverage ≥
//! mid-threshold are "ink", the rest "paper". Fit `fg` = mean of ink pixels,
//! `bg` = mean of paper pixels (in linear RGB). Pick the glyph that minimizes
//! reconstruction error in Oklab-ish luma space (weighted by coverage).

use crate::atlas::GlyphAtlas;
use crate::grid::CellColor;

/// Mid coverage threshold (128) for hard-masking soft AA glyphs.
const MASK_MID: u8 = 128;

#[derive(Debug, Clone, Copy)]
pub struct TwoColorMatch {
    pub glyph: u16,
    pub fg: CellColor,
    pub bg: CellColor,
    pub error: u64,
}

/// Mean linear RGB of pixels selected by `pred`, returned as sRGB8-ish u8
/// (linear 0..=1 mapped ×255). Empty selection → `fallback`.
fn mean_rgb(
    cell_rgba: &[[f32; 3]],
    coverage: &[u8],
    pred: impl Fn(u8) -> bool,
    fallback: [u8; 3],
) -> [u8; 3] {
    let mut sum = [0.0f64; 3];
    let mut n = 0u32;
    for (i, &cov) in coverage.iter().enumerate() {
        if !pred(cov) {
            continue;
        }
        let p = cell_rgba[i];
        sum[0] += p[0] as f64;
        sum[1] += p[1] as f64;
        sum[2] += p[2] as f64;
        n += 1;
    }
    if n == 0 {
        return fallback;
    }
    [
        ((sum[0] / n as f64) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((sum[1] / n as f64) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((sum[2] / n as f64) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Reconstruction error: Σ (actual − reconstructed)² × weight.
fn reconstruct_error(
    cell_rgba: &[[f32; 3]],
    coverage: &[u8],
    fg: [f32; 3],
    bg: [f32; 3],
) -> u64 {
    let mut err = 0u64;
    for (i, &cov) in coverage.iter().enumerate() {
        let t = cov as f32 / 255.0;
        let rec = [
            bg[0] + (fg[0] - bg[0]) * t,
            bg[1] + (fg[1] - bg[1]) * t,
            bg[2] + (fg[2] - bg[2]) * t,
        ];
        let p = cell_rgba[i];
        for c in 0..3 {
            let d = ((p[c] - rec[c]) * 255.0) as i32;
            err += (d * d) as u64;
        }
    }
    err
}

fn u8_to_lin(c: [u8; 3]) -> [f32; 3] {
    [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0]
}

/// Best glyph + fg/bg for one cell's linear RGB samples (`cell_w*cell_h` triples).
pub fn match_mask_two_color(atlas: &GlyphAtlas, cell_rgba: &[[f32; 3]]) -> TwoColorMatch {
    let n = (atlas.cell_w() * atlas.cell_h()) as usize;
    debug_assert_eq!(cell_rgba.len(), n);

    let mut best = TwoColorMatch {
        glyph: 0,
        fg: CellColor::BLACK,
        bg: CellColor::WHITE,
        error: u64::MAX,
    };

    for (i, g) in atlas.glyphs.iter().enumerate() {
        let fg8 = mean_rgb(cell_rgba, &g.coverage, |c| c >= MASK_MID, [0, 0, 0]);
        let bg8 = mean_rgb(cell_rgba, &g.coverage, |c| c < MASK_MID, [255, 255, 255]);
        let err = reconstruct_error(cell_rgba, &g.coverage, u8_to_lin(fg8), u8_to_lin(bg8));
        let i = i as u16;
        if err < best.error || (err == best.error && i < best.glyph) {
            best = TwoColorMatch {
                glyph: i,
                fg: CellColor::Rgb(fg8),
                bg: CellColor::Rgb(bg8),
                error: err,
            };
        }
    }
    best
}

/// Sample one cell's linear RGB from a full image buffer.
/// Fully transparent pixels are treated as paper white so they do not pull
/// ink/fg fits toward black; out-of-bounds samples are also paper.
pub fn sample_cell_rgb(
    rgba: &[f32],
    width: u32,
    height: u32,
    ox: u32,
    oy: u32,
    cell_w: u32,
    cell_h: u32,
) -> Vec<[f32; 3]> {
    let mut out = Vec::with_capacity((cell_w * cell_h) as usize);
    for ly in 0..cell_h {
        for lx in 0..cell_w {
            let gx = ox + lx;
            let gy = oy + ly;
            if gx < width && gy < height {
                let i = ((gy * width + gx) * 4) as usize;
                let a = rgba[i + 3].clamp(0.0, 1.0);
                if a <= 0.0 {
                    out.push([1.0, 1.0, 1.0]);
                } else {
                    out.push([rgba[i], rgba[i + 1], rgba[i + 2]]);
                }
            } else {
                out.push([1.0, 1.0, 1.0]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::font::{BundledFont, FontFace};
    use crate::symbols::SymbolSet;

    fn atlas() -> GlyphAtlas {
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
    fn solid_cell_zero_error_and_flat_colours() {
        let atlas = atlas();
        let n = (atlas.cell_w() * atlas.cell_h()) as usize;
        let white: Vec<[f32; 3]> = vec![[1.0, 1.0, 1.0]; n];
        let black: Vec<[f32; 3]> = vec![[0.0, 0.0, 0.0]; n];
        let w = match_mask_two_color(&atlas, &white);
        let b = match_mask_two_color(&atlas, &black);
        assert_eq!(w.error, 0);
        assert_eq!(b.error, 0);
        // Tie-break picks space (no ink): bg carries the flat colour; fg is unused fallback.
        assert_eq!(atlas.glyphs[w.glyph as usize].ch, ' ');
        assert_eq!(w.glyph, b.glyph);
        assert_eq!(w.bg, CellColor::Rgb([255, 255, 255]));
        assert_eq!(b.bg, CellColor::Rgb([0, 0, 0]));
    }

    #[test]
    fn half_block_recovers_left_right_split() {
        let atlas = atlas();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let mut cell = vec![[1.0f32; 3]; (cw * ch) as usize];
        // Left half black, right half white — should favour ▌ (left half block).
        for y in 0..ch {
            for x in 0..cw / 2 {
                cell[(y * cw + x) as usize] = [0.0, 0.0, 0.0];
            }
        }
        let m = match_mask_two_color(&atlas, &cell);
        let ch_picked = atlas.glyphs[m.glyph as usize].ch;
        assert!(
            matches!(ch_picked, '▌' | '▎' | '▍' | '▋' | '█' | '▉'),
            "unexpected {ch_picked:?}"
        );
        if let (CellColor::Rgb(fg), CellColor::Rgb(bg)) = (m.fg, m.bg) {
            assert!(fg[0] < 40, "fg should be dark: {fg:?}");
            assert!(bg[0] > 200, "bg should be light: {bg:?}");
        }
    }
}
