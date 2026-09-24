//! PNG exporter — render atlas at scale 1×–8× (not upscaled).

use std::io::Cursor;

use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
use crate::color::ColorTarget;
use crate::font::FontFace;
use crate::grid::{AsciiGrid, CellColor};
use crate::render::render_rgba8;
use crate::symbols::SymbolSet;

/// Encode a PNG of the grid at integer scale `1..=8`.
///
/// Scale > 1 rebuilds the atlas at `px * scale` when a font + symbol set are
/// provided; otherwise the 1× raster is nearest-neighbour expanded (fallback).
pub fn to_png(
    atlas: &GlyphAtlas,
    grid: &AsciiGrid,
    scale: u32,
    target: ColorTarget,
    mono_fg: CellColor,
    mono_bg: CellColor,
) -> Result<Vec<u8>, String> {
    let scale = scale.clamp(1, 8);
    let (w, h, rgba) = if scale == 1 {
        render_rgba8(atlas, grid, target, mono_fg, mono_bg)
    } else {
        // Fallback NN upscale of 1× render (full N× atlas rebuild needs font handle).
        let (bw, bh, base) = render_rgba8(atlas, grid, target, mono_fg, mono_bg);
        let w = bw * scale;
        let h = bh * scale;
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let sx = x / scale;
                let sy = y / scale;
                let si = ((sy * bw + sx) * 4) as usize;
                let di = ((y * w + x) * 4) as usize;
                rgba[di..di + 4].copy_from_slice(&base[si..si + 4]);
            }
        }
        (w, h, rgba)
    };
    encode_rgba8(w, h, &rgba)
}

/// Render at N× by rebuilding the atlas at `px * scale` (exact, not upscaled).
pub fn to_png_scaled(
    font: &FontFace,
    set: &SymbolSet,
    base_opts: &AtlasOptions,
    grid: &AsciiGrid,
    scale: u32,
    target: ColorTarget,
    mono_fg: CellColor,
    mono_bg: CellColor,
) -> Result<Vec<u8>, String> {
    let scale = scale.clamp(1, 8);
    if scale == 1 {
        let atlas = GlyphAtlas::build(font, set, base_opts).map_err(|e| e.to_string())?;
        return to_png(&atlas, grid, 1, target, mono_fg, mono_bg);
    }
    let base_px = match base_opts.size {
        FontSize::Px(p) => p,
        FontSize::Columns { .. } => {
            return Err("to_png_scaled requires FontSize::Px base options".into());
        }
    };
    let opts = AtlasOptions {
        size: FontSize::Px(base_px * scale as f32),
        antialias: base_opts.antialias,
        hinting: base_opts.hinting,
    };
    let atlas = GlyphAtlas::build(font, set, &opts).map_err(|e| e.to_string())?;
    // Grid cell_px changes with atlas; render uses atlas metrics directly.
    let scaled_grid = AsciiGrid {
        cols: grid.cols,
        rows: grid.rows,
        cell_px: (atlas.cell_w(), atlas.cell_h()),
        atlas_key: atlas.key,
        color: grid.color,
        cells: grid.cells.clone(),
    };
    let (w, h, rgba) = render_rgba8(&atlas, &scaled_grid, target, mono_fg, mono_bg);
    encode_rgba8(w, h, &rgba)
}

fn encode_rgba8(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut buf), w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::{MatchMode, convert_mono};
    use crate::font::BundledFont;
    use crate::symbols::SymbolSet;

    #[test]
    fn png_encodes_valid_header() {
        let font = FontFace::bundled(BundledFont::DepartureMono);
        let set = SymbolSet::Bourke10;
        let opts = AtlasOptions {
            size: FontSize::Px(11.0),
            antialias: false,
            hinting: false,
        };
        let atlas = GlyphAtlas::build(&font, &set, &opts).unwrap();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        let rgba = vec![1.0f32; (cw * ch * 4) as usize];
        let grid = convert_mono(&rgba, cw, ch, &atlas, MatchMode::Tone);
        let png = to_png(
            &atlas,
            &grid,
            1,
            ColorTarget::TrueColor,
            CellColor::BLACK,
            CellColor::WHITE,
        )
        .unwrap();
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        let png2 = to_png_scaled(
            &font,
            &set,
            &opts,
            &grid,
            2,
            ColorTarget::TrueColor,
            CellColor::BLACK,
            CellColor::WHITE,
        )
        .unwrap();
        assert!(png2.starts_with(&[0x89, b'P', b'N', b'G']));
        assert!(png2.len() > png.len());
    }
}
