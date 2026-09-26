//! High-level convert: RGBA image → [`AsciiGrid`].

use crate::analyze::{analyse_cell, mean_cell_alpha};
use crate::atlas::GlyphAtlas;
use crate::color::{mean_cell_srgb8_alpha, quantize_pair, quantize_rgb, ColorTarget};
use crate::descriptor::ShapeDesc;
use crate::dither::{dither_shape, dither_tone, CellDither, ShapeErrorBuf, ToneErrorBuf};
use crate::grid::{AsciiGrid, Cell, CellColor, GridColorMode};
use crate::matching::{
    match_mask_two_color, orient_glyph, sample_cell_rgb, EdgeField, EdgeOverlay, MatcherTables,
};
use crate::render::render_rgba;

pub use crate::render::{render_rgba as render_colored, render_rgba8 as render_srgb8};

/// Matching algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    Tone,
    Shape,
    /// Shape with Harri contrast; `gamma_8_8` (`256` = 1.0).
    ShapeContrast {
        gamma_8_8: u16,
    },
    /// chafa-style mask + two-colour fit (implies [`GridColorMode::FgBg`] colours).
    MaskTwoColor,
}

/// Options for [`convert`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvertOptions {
    pub match_mode: MatchMode,
    pub color_mode: GridColorMode,
    pub color_target: ColorTarget,
    pub dither: CellDither,
    pub mono_fg: CellColor,
    pub mono_bg: CellColor,
    pub edge: EdgeOverlay,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            match_mode: MatchMode::Shape,
            color_mode: GridColorMode::Mono,
            color_target: ColorTarget::TrueColor,
            dither: CellDither::None,
            mono_fg: CellColor::BLACK,
            mono_bg: CellColor::WHITE,
            edge: EdgeOverlay::default(),
        }
    }
}

/// Convert a linear RGBA f32 image into a character grid.
pub fn convert(
    rgba: &[f32],
    width: u32,
    height: u32,
    atlas: &GlyphAtlas,
    opts: ConvertOptions,
) -> AsciiGrid {
    let tables = MatcherTables::build(atlas);
    let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
    let cols = width.div_ceil(cw.max(1));
    let rows = height.div_ceil(ch.max(1));
    let mut cells = vec![
        Cell {
            glyph: 0,
            fg: CellColor::BLACK,
            bg: CellColor::WHITE,
            alpha: 255,
        };
        (cols * rows) as usize
    ];

    let color_mode = match opts.match_mode {
        MatchMode::MaskTwoColor => GridColorMode::FgBg,
        _ => opts.color_mode,
    };

    let use_fs = matches!(opts.dither, CellDither::FloydSteinberg { .. });
    let serpentine = matches!(opts.dither, CellDither::FloydSteinberg { serpentine: true });
    let mut tone_err = if use_fs {
        Some(ToneErrorBuf::new(cols, rows))
    } else {
        None
    };
    let mut shape_err = if use_fs
        && matches!(
            opts.match_mode,
            MatchMode::Shape | MatchMode::ShapeContrast { .. }
        ) {
        Some(ShapeErrorBuf::new(cols, rows))
    } else {
        None
    };

    let edge_field = if opts.edge.enabled {
        Some(EdgeField::from_rgba(rgba, width, height))
    } else {
        None
    };

    for row in 0..rows {
        let ltr = !serpentine || row % 2 == 0;
        let col_range: Box<dyn Iterator<Item = u32>> = if ltr {
            Box::new(0..cols)
        } else {
            Box::new((0..cols).rev())
        };
        for col in col_range {
            let cell_alpha = mean_cell_alpha(rgba, width, height, col * cw, row * ch, cw, ch);
            if cell_alpha == 0 {
                cells[(row * cols + col) as usize] = Cell {
                    glyph: 0,
                    fg: opts.mono_fg,
                    bg: opts.mono_bg,
                    alpha: 0,
                };
                continue;
            }
            let (tone, shape) = analyse_cell(rgba, width, height, col * cw, row * ch, cw, ch);
            let (glyph, fg, bg) = match opts.match_mode {
                MatchMode::MaskTwoColor => {
                    let samples = sample_cell_rgb(rgba, width, height, col * cw, row * ch, cw, ch);
                    let m = match_mask_two_color(atlas, &samples);
                    let (CellColor::Rgb(fg8), CellColor::Rgb(bg8)) = (m.fg, m.bg) else {
                        unreachable!("mask_two_color always returns Rgb");
                    };
                    let (fg, bg) = quantize_pair(fg8, bg8, opts.color_target);
                    (m.glyph, fg, bg)
                }
                MatchMode::Tone => {
                    let mut t = dither_tone(tone, col, row, opts.dither);
                    if let Some(buf) = tone_err.as_mut() {
                        t = buf.take_tone(t, col, row);
                    }
                    let glyph = tables.match_tone(t);
                    if let Some(buf) = tone_err.as_mut() {
                        let chosen = tables.tone[glyph as usize];
                        buf.diffuse(t, chosen, col, row, cols, rows, serpentine, ltr);
                    }
                    let (fg, bg) =
                        colours_for_cell(rgba, width, height, col, row, cw, ch, color_mode, opts);
                    (glyph, fg, bg)
                }
                MatchMode::Shape | MatchMode::ShapeContrast { .. } => {
                    let mut s = dither_shape(shape, col, row, opts.dither);
                    if let Some(buf) = shape_err.as_mut() {
                        s = buf.take_shape(s, col, row);
                    }
                    let glyph = match opts.match_mode {
                        MatchMode::ShapeContrast { gamma_8_8 } => {
                            match_shape_contrast_tables(&tables, s, gamma_8_8)
                        }
                        _ => tables.match_shape(s),
                    };
                    if let Some(buf) = shape_err.as_mut() {
                        let chosen = tables.shape[glyph as usize];
                        buf.diffuse(s, chosen, col, row, cols, rows, serpentine, ltr);
                    }
                    let (fg, bg) =
                        colours_for_cell(rgba, width, height, col, row, cw, ch, color_mode, opts);
                    (glyph, fg, bg)
                }
            };
            let glyph = if let Some(field) = edge_field.as_ref() {
                if let Some(orient) = field.cell_orient(col * cw, row * ch, cw, ch, opts.edge.tau) {
                    orient_glyph(atlas, orient).unwrap_or(glyph)
                } else {
                    glyph
                }
            } else {
                glyph
            };
            cells[(row * cols + col) as usize] = Cell {
                glyph,
                fg,
                bg,
                alpha: cell_alpha,
            };
        }
    }

    AsciiGrid {
        cols,
        rows,
        cell_px: (cw, ch),
        atlas_key: atlas.key,
        color: color_mode,
        cells,
    }
}

fn colours_for_cell(
    rgba: &[f32],
    width: u32,
    height: u32,
    col: u32,
    row: u32,
    cw: u32,
    ch: u32,
    color_mode: GridColorMode,
    opts: ConvertOptions,
) -> (CellColor, CellColor) {
    match color_mode {
        GridColorMode::Mono => (opts.mono_fg, opts.mono_bg),
        GridColorMode::Fg => {
            let mean = mean_cell_srgb8_alpha(rgba, width, height, col * cw, row * ch, cw, ch);
            (quantize_rgb(mean, opts.color_target), opts.mono_bg)
        }
        GridColorMode::FgBg => {
            // Without MaskTwoColor, approximate with mean fg on fixed mono_bg.
            let mean = mean_cell_srgb8_alpha(rgba, width, height, col * cw, row * ch, cw, ch);
            let bg = match opts.mono_bg {
                CellColor::Rgb(c) => quantize_rgb(c, opts.color_target),
                CellColor::Indexed(i) => CellColor::Indexed(i),
            };
            (quantize_rgb(mean, opts.color_target), bg)
        }
    }
}

/// Convert a linear RGBA f32 image into a mono character grid.
pub fn convert_mono(
    rgba: &[f32],
    width: u32,
    height: u32,
    atlas: &GlyphAtlas,
    mode: MatchMode,
) -> AsciiGrid {
    convert(
        rgba,
        width,
        height,
        atlas,
        ConvertOptions {
            match_mode: mode,
            color_mode: GridColorMode::Mono,
            ..ConvertOptions::default()
        },
    )
}

fn match_shape_contrast_tables(tables: &MatcherTables, cell: ShapeDesc, gamma_8_8: u16) -> u16 {
    use crate::descriptor::ShapeContrastDesc;
    let cell = ShapeContrastDesc::from_shape(cell, gamma_8_8);
    let mut best_i = 0u16;
    let mut best_d = u64::MAX;
    for (i, &shape) in tables.shape.iter().enumerate() {
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

/// Render a mono grid back to linear RGBA f32 (ink = black on white).
pub fn render_mono(atlas: &GlyphAtlas, grid: &AsciiGrid) -> (u32, u32, Vec<f32>) {
    render_rgba(
        atlas,
        grid,
        ColorTarget::TrueColor,
        CellColor::BLACK,
        CellColor::WHITE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::descriptor::ToneDesc;
    use crate::font::{BundledFont, FontFace};
    use crate::matching::MatcherTables;
    use crate::symbols::SymbolSet;

    fn departure_bourke10() -> GlyphAtlas {
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

    #[test]
    fn convert_render_round_trip() {
        let atlas = departure_bourke10();
        let tables = MatcherTables::build(&atlas);
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let width = cw * 2;
        let height = ch;
        let mut rgba = vec![1.0f32; (width * height * 4) as usize];
        for (col, glyph_i) in [0usize, atlas.glyphs.len() - 1].into_iter().enumerate() {
            let g = &atlas.glyphs[glyph_i];
            for ly in 0..ch {
                for lx in 0..cw {
                    let ink = g.coverage[(ly * cw + lx) as usize];
                    let v = 1.0 - ink as f32 / 255.0;
                    let i = (ly * width + (col as u32 * cw + lx)) * 4;
                    rgba[i as usize] = v;
                    rgba[i as usize + 1] = v;
                    rgba[i as usize + 2] = v;
                }
            }
        }
        let grid = convert_mono(&rgba, width, height, &atlas, MatchMode::Tone);
        assert_eq!(grid.cols, 2);
        assert_eq!(grid.rows, 1);
        assert_eq!(
            tables.tone[grid.cells[0].glyph as usize].coverage,
            ToneDesc::from_coverage(&atlas.glyphs[0].coverage, cw, ch).coverage
        );
        let (rw, rh, rendered) = render_mono(&atlas, &grid);
        assert_eq!((rw, rh), (width, height));
        let grid2 = convert_mono(&rendered, rw, rh, &atlas, MatchMode::Tone);
        assert_eq!(grid2.cells[0].glyph, grid.cells[0].glyph);
        assert_eq!(grid2.cells[1].glyph, grid.cells[1].glyph);
    }

    #[test]
    fn mask_two_color_sets_fg_bg_mode() {
        let atlas = GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &SymbolSet::Blocks,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let rgba = vec![0.5f32; (cw * ch * 4) as usize];
        let grid = convert(
            &rgba,
            cw,
            ch,
            &atlas,
            ConvertOptions {
                match_mode: MatchMode::MaskTwoColor,
                color_mode: GridColorMode::Mono,
                color_target: ColorTarget::Ansi16(crate::color::Ansi16Palette::Vga),
                ..ConvertOptions::default()
            },
        );
        assert_eq!(grid.color, GridColorMode::FgBg);
        assert!(matches!(grid.cells[0].fg, CellColor::Indexed(_)));
    }
}
