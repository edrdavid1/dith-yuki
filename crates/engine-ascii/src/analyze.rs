//! Per-cell analysis: sample an RGBA image into coverage buffers and descriptors.

use crate::descriptor::{ShapeDesc, ToneDesc};

/// Linear RGB → relative luminance (Rec. 709), returned as `0..=255` coverage
/// where **ink** is dark (`255 - luma`). Matches “darker → denser glyph” ASCII.
#[inline]
pub fn ink_from_rgba(r: f32, g: f32, b: f32) -> u8 {
    let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
    ((1.0 - luma) * 255.0).round() as u8
}

/// Fill `out` (`cell_w × cell_h`) with ink coverage from an RGBA f32 buffer
/// (`width × height × 4`, linear 0..=1). Origin `(ox, oy)` is the top-left of
/// the cell in image pixels. Samples outside the image are treated as paper
/// (ink 0). Transparent source pixels contribute no ink (alpha-weighted).
pub fn sample_cell_ink(
    rgba: &[f32],
    width: u32,
    height: u32,
    ox: u32,
    oy: u32,
    cell_w: u32,
    cell_h: u32,
    out: &mut [u8],
) {
    debug_assert_eq!(out.len(), (cell_w * cell_h) as usize);
    debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
    for ly in 0..cell_h {
        for lx in 0..cell_w {
            let gx = ox + lx;
            let gy = oy + ly;
            let ink = if gx < width && gy < height {
                let i = ((gy * width + gx) * 4) as usize;
                let a = rgba[i + 3].clamp(0.0, 1.0);
                let ink = ink_from_rgba(rgba[i], rgba[i + 1], rgba[i + 2]);
                ((ink as f32) * a).round() as u8
            } else {
                0
            };
            out[(ly * cell_w + lx) as usize] = ink;
        }
    }
}

/// Alpha crumbs below this (after mean) are treated as fully clear so tiny
/// PNG anti-alias fringes do not allocate a cell.
const ALPHA_CLEAR_EPS: u8 = 8;

/// Mean source alpha of one cell (`0..=255`). Out-of-bounds samples count as 0.
/// Near-zero means snap to `0`; near-full means snap to `255` so soft-edge
/// crumbs do not force ink-only mode on otherwise solid regions.
pub fn mean_cell_alpha(
    rgba: &[f32],
    width: u32,
    height: u32,
    ox: u32,
    oy: u32,
    cell_w: u32,
    cell_h: u32,
) -> u8 {
    debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
    let n = (cell_w * cell_h) as f64;
    if n <= 0.0 {
        return 0;
    }
    let mut sum = 0.0f64;
    for ly in 0..cell_h {
        for lx in 0..cell_w {
            let gx = ox + lx;
            let gy = oy + ly;
            if gx < width && gy < height {
                let i = ((gy * width + gx) * 4) as usize;
                sum += rgba[i + 3].clamp(0.0, 1.0) as f64;
            }
        }
    }
    let mean = ((sum / n) * 255.0).round().clamp(0.0, 255.0) as u8;
    if mean < ALPHA_CLEAR_EPS {
        0
    } else if mean > 255 - ALPHA_CLEAR_EPS {
        255
    } else {
        mean
    }
}

/// Analyse one cell into tone + shape descriptors.
pub fn analyse_cell(
    rgba: &[f32],
    width: u32,
    height: u32,
    ox: u32,
    oy: u32,
    cell_w: u32,
    cell_h: u32,
) -> (ToneDesc, ShapeDesc) {
    let mut buf = vec![0u8; (cell_w * cell_h) as usize];
    sample_cell_ink(rgba, width, height, ox, oy, cell_w, cell_h, &mut buf);
    (
        ToneDesc::from_coverage(&buf, cell_w, cell_h),
        ShapeDesc::from_coverage(&buf, cell_w, cell_h),
    )
}

/// Full-image analysis: one `(ToneDesc, ShapeDesc)` per cell, row-major.
pub fn analyse_image(
    rgba: &[f32],
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
) -> (u32, u32, Vec<(ToneDesc, ShapeDesc)>) {
    let cols = width.div_ceil(cell_w.max(1));
    let rows = height.div_ceil(cell_h.max(1));
    let mut out = Vec::with_capacity((cols * rows) as usize);
    for row in 0..rows {
        for col in 0..cols {
            out.push(analyse_cell(
                rgba,
                width,
                height,
                col * cell_w,
                row * cell_h,
                cell_w,
                cell_h,
            ));
        }
    }
    (cols, rows, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::font::{BundledFont, FontFace};
    use crate::matching::MatcherTables;
    use crate::symbols::SymbolSet;

    fn solid_rgba(w: u32, h: u32, r: f32, g: f32, b: f32) -> Vec<f32> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..w * h {
            v.extend_from_slice(&[r, g, b, 1.0]);
        }
        v
    }

    #[test]
    fn white_is_no_ink_black_is_full_ink() {
        assert_eq!(ink_from_rgba(1.0, 1.0, 1.0), 0);
        assert_eq!(ink_from_rgba(0.0, 0.0, 0.0), 255);
    }

    #[test]
    fn round_trip_rasterize_then_match() {
        // Spec §6.1: render atlas glyphs into an image, analyse, match — must
        // recover the same glyph (or an equal-descriptor tie).
        let atlas = GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &SymbolSet::PrintableAscii,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap();
        let tables = MatcherTables::build(&atlas);
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let cols = 16u32;
        let rows = (atlas.glyphs.len() as u32).div_ceil(cols);
        let width = cols * cw;
        let height = rows * ch;
        let mut rgba = vec![1.0f32; (width * height * 4) as usize];

        for (i, g) in atlas.glyphs.iter().enumerate() {
            let col = (i as u32) % cols;
            let row = (i as u32) / cols;
            let ox = col * cw;
            let oy = row * ch;
            for ly in 0..ch {
                for lx in 0..cw {
                    let ink = g.coverage[(ly * cw + lx) as usize];
                    let v = 1.0 - ink as f32 / 255.0;
                    let gi = ((oy + ly) * width + (ox + lx)) * 4;
                    rgba[gi as usize] = v;
                    rgba[gi as usize + 1] = v;
                    rgba[gi as usize + 2] = v;
                }
            }
        }

        let mut ok = 0usize;
        for (i, g) in atlas.glyphs.iter().enumerate() {
            let col = (i as u32) % cols;
            let row = (i as u32) / cols;
            let (tone, shape) = analyse_cell(&rgba, width, height, col * cw, row * ch, cw, ch);
            let by_tone = tables.match_tone(tone) as usize;
            let by_shape = tables.match_shape(shape) as usize;
            assert_eq!(
                tables.tone[by_tone].coverage, tone.coverage,
                "tone miss for {:?}",
                g.ch
            );
            if by_shape == i || tables.shape[by_shape] == shape {
                ok += 1;
            } else {
                panic!(
                    "shape miss for {:?} → {:?}",
                    g.ch, atlas.glyphs[by_shape].ch
                );
            }
        }
        assert_eq!(ok, atlas.glyphs.len());
    }

    #[test]
    fn analyse_image_grid_dims() {
        let rgba = solid_rgba(100, 50, 0.5, 0.5, 0.5);
        let (cols, rows, cells) = analyse_image(&rgba, 100, 50, 7, 14);
        assert_eq!((cols, rows), (15, 4));
        assert_eq!(cells.len(), 60);
    }
}
