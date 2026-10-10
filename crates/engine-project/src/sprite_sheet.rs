//! Sprite-sheet cell processing for export (ordered dither, first iteration).
//!
//! Each cell is treated as an independent image so ordered pattern phase
//! matches across frames. Error-diffusion cell isolation is deferred.

use std::sync::atomic::{AtomicBool, Ordering};

use engine_color::palette::{linear_to_srgb, Palette};
use engine_io::encode_indexed_png;
use engine_tiles::decompose::decompose_image_to_tiles_at_generation;
use engine_tiles::TileCache;
use thiserror::Error;

use crate::document::Document;
use crate::filter::{FilterInstance, FilterParams, PaletteDitherMode};
use crate::headless::{encode_rgba_png, HeadlessError, HeadlessOutputFormat};
use crate::layer::{Layer, LayerNode};
use crate::serialize::pixels::build_processed_composite_rgba8;
use crate::types::{DocumentId, LayerId, LayerKind};

#[derive(Debug, Error)]
pub enum SpriteSheetError {
    #[error("{0}")]
    Headless(#[from] HeadlessError),
    #[error("cell size must be ≥ 1")]
    BadCellSize,
    #[error("image dimensions must be ≥ 1")]
    BadDimensions,
    #[error("sprite-sheet cells require an ordered (non–error-diffusion) filter stack")]
    ErrorDiffusionNotSupported,
    #[error("cancelled")]
    Cancelled,
}

fn filters_have_error_diffusion(filters: &[FilterInstance]) -> bool {
    filters.iter().any(|f| {
        f.enabled
            && matches!(&f.params, FilterParams::DitherV2(p) if p.mode.is_error_diffusion())
    })
}

fn filters_have_guided(filters: &[FilterInstance]) -> bool {
    filters.iter().any(|f| {
        f.enabled
            && matches!(
                &f.params,
                FilterParams::DitherV2(p)
                    if matches!(p.palette_dither_mode, PaletteDitherMode::Guided { .. })
            )
    })
}

/// Process `rgba_f32` (display-referred 0–1) as a sprite sheet of `cell_w`×`cell_h` cells.
///
/// Incomplete edge cells (width/height not divisible) are still processed with their
/// actual remaining size; pattern phase resets at each cell origin.
pub fn process_sprite_sheet_rgba_f32(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
    format: HeadlessOutputFormat,
    png8_palette_id: Option<u32>,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, SpriteSheetError> {
    if cell_w == 0 || cell_h == 0 {
        return Err(SpriteSheetError::BadCellSize);
    }
    if width == 0 || height == 0 {
        return Err(SpriteSheetError::BadDimensions);
    }
    if filters_have_error_diffusion(filters) {
        return Err(SpriteSheetError::ErrorDiffusionNotSupported);
    }
    if format == HeadlessOutputFormat::Png8 && filters_have_guided(filters) {
        return Err(SpriteSheetError::Headless(HeadlessError::GuidedBlocksPng8));
    }

    let mut out_rgba = vec![0u8; (width as usize) * (height as usize) * 4];

    let cols = width.div_ceil(cell_w);
    let rows = height.div_ceil(cell_h);

    for row in 0..rows {
        for col in 0..cols {
            if cancel.load(Ordering::Relaxed) {
                return Err(SpriteSheetError::Cancelled);
            }
            let x0 = col * cell_w;
            let y0 = row * cell_h;
            let cw = cell_w.min(width - x0);
            let ch = cell_h.min(height - y0);
            let cell = extract_cell_f32(rgba_f32, width, height, x0, y0, cw, ch);
            let cell_rgba8 = process_cell(&cell, cw, ch, filters, palettes, cancel)?;
            blit_rgba8(&mut out_rgba, width, height, x0, y0, cw, ch, &cell_rgba8);
        }
    }

    match format {
        HeadlessOutputFormat::Png => Ok(encode_rgba_png(&out_rgba, width, height)?),
        HeadlessOutputFormat::Png8 => {
            let palette = resolve_palette(palettes, png8_palette_id)?;
            let palette_srgb: Vec<(u8, u8, u8)> = palette
                .colors
                .iter()
                .map(|c| (linear_to_srgb(c.r), linear_to_srgb(c.g), linear_to_srgb(c.b)))
                .collect();
            encode_indexed_png(&out_rgba, width, height, &palette_srgb)
                .map_err(|e| SpriteSheetError::Headless(HeadlessError::Export(e.to_string())))
        }
    }
}

fn resolve_palette(
    palettes: &[Palette],
    png8_palette_id: Option<u32>,
) -> Result<&Palette, SpriteSheetError> {
    if let Some(id) = png8_palette_id {
        return palettes
            .iter()
            .find(|p| p.id == id)
            .filter(|p| !p.colors.is_empty())
            .ok_or(SpriteSheetError::Headless(HeadlessError::MissingPalette));
    }
    palettes
        .first()
        .filter(|p| !p.colors.is_empty())
        .ok_or(SpriteSheetError::Headless(HeadlessError::MissingPalette))
}

fn extract_cell_f32(
    src: &[f32],
    width: u32,
    height: u32,
    x0: u32,
    y0: u32,
    cw: u32,
    ch: u32,
) -> Vec<f32> {
    let _ = height;
    let mut out = Vec::with_capacity((cw as usize) * (ch as usize) * 4);
    for y in y0..y0 + ch {
        let row = (y as usize) * (width as usize) * 4;
        let start = row + (x0 as usize) * 4;
        let end = start + (cw as usize) * 4;
        out.extend_from_slice(&src[start..end]);
    }
    out
}

fn blit_rgba8(
    dest: &mut [u8],
    width: u32,
    _height: u32,
    x0: u32,
    y0: u32,
    cw: u32,
    ch: u32,
    src: &[u8],
) {
    for y in 0..ch {
        for x in 0..cw {
            let si = ((y as usize) * (cw as usize) + (x as usize)) * 4;
            let di = (((y0 + y) as usize) * (width as usize) + ((x0 + x) as usize)) * 4;
            dest[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
}

fn process_cell(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, SpriteSheetError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(SpriteSheetError::Cancelled);
    }
    let doc_id = 1u32;
    let layer_id = 1u32;
    let live_gen = 1u64;
    let budget = ((width as usize).saturating_mul(height as usize).saturating_mul(16))
        .max(4_000_000)
        .min(128_000_000);
    let cache = TileCache::new(budget);
    decompose_image_to_tiles_at_generation(
        rgba_f32,
        width,
        height,
        doc_id,
        layer_id,
        &cache,
        live_gen,
    )
    .map_err(|e| SpriteSheetError::Headless(HeadlessError::Export(format!("tile decompose: {e}"))))?;

    let mut doc = Document::new(DocumentId::new(doc_id), width, height);
    doc.generations.set_document_gen(live_gen);
    doc.palettes = palettes.to_vec();
    let mut layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, width, height);
    layer.filters = filters.to_vec();
    doc.root.push(LayerNode::Leaf(layer));

    build_processed_composite_rgba8(&cache, &doc)
        .map_err(|e| SpriteSheetError::Headless(HeadlessError::Export(e.to_string())))
}

/// Convert RGBA8 → display-referred f32 (matches headless decode).
pub fn rgba8_to_f32(rgba: &[u8]) -> Vec<f32> {
    rgba.iter().map(|&v| v as f32 / 255.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{
        DitherColorMode, DitherModeV2, DitherParamsV2, FilterKind, FilterParams, FilterInstance,
    };
    use std::sync::atomic::AtomicBool;

    fn bayer_filter() -> FilterInstance {
        let mut f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 2,
                color_mode: DitherColorMode::Grayscale,
                ..DitherParamsV2::default()
            }),
        );
        f.enabled = true;
        f.algorithm_id = Some("bayer_4x4".into());
        f
    }

    #[test]
    fn two_cells_share_ordered_phase() {
        // 4×2 image, cell 2×2 — both cells solid mid-gray → identical Bayer output.
        let w = 4u32;
        let h = 2u32;
        let mut rgba = vec![0.5f32; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            rgba[i * 4 + 3] = 1.0;
        }
        let cancel = AtomicBool::new(false);
        let bytes = process_sprite_sheet_rgba_f32(
            &rgba,
            w,
            h,
            2,
            2,
            &[bayer_filter()],
            &[],
            HeadlessOutputFormat::Png,
            None,
            &cancel,
        )
        .unwrap();
        let img = image::load_from_memory(&bytes).unwrap().to_rgba8();
        // Cell (0,0) vs cell (1,0) — same relative Bayer.
        for ly in 0..2u32 {
            for lx in 0..2u32 {
                let a = img.get_pixel(lx, ly);
                let b = img.get_pixel(2 + lx, ly);
                assert_eq!(a, b, "phase mismatch at ({lx},{ly})");
            }
        }
    }

    #[test]
    fn rejects_error_diffusion() {
        let mut f = bayer_filter();
        if let FilterParams::DitherV2(ref mut p) = f.params {
            p.mode = DitherModeV2::FloydSteinberg;
        }
        let rgba = vec![0.5f32; 16];
        let cancel = AtomicBool::new(false);
        let err = process_sprite_sheet_rgba_f32(
            &rgba,
            2,
            2,
            2,
            2,
            &[f],
            &[],
            HeadlessOutputFormat::Png,
            None,
            &cancel,
        )
        .unwrap_err();
        assert!(matches!(err, SpriteSheetError::ErrorDiffusionNotSupported));
    }
}
