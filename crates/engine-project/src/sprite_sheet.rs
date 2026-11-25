//! Sprite-sheet cell processing for export.
//!
//! Delegates to [`crate::gamedev::process_independent_cells_rgba8`] (F2) so
//! preview and export share one independent-cell pipeline (ordered + ED).

use std::sync::atomic::AtomicBool;

use engine_color::palette::{linear_to_srgb, Palette};
use engine_io::encode_indexed_png;
use thiserror::Error;

use crate::filter::{FilterInstance, FilterParams, PaletteDitherMode};
use crate::gamedev::{process_independent_cells_rgba8, CellPipelineError, CellSettings};
use crate::headless::{encode_rgba_png, HeadlessError, HeadlessOutputFormat};

#[derive(Debug, Error)]
pub enum SpriteSheetError {
    #[error("{0}")]
    Headless(#[from] HeadlessError),
    #[error("cell size must be ≥ 1")]
    BadCellSize,
    #[error("image dimensions must be ≥ 1")]
    BadDimensions,
    #[error("cancelled")]
    Cancelled,
    #[error("{0}")]
    Process(String),
}

impl From<CellPipelineError> for SpriteSheetError {
    fn from(e: CellPipelineError) -> Self {
        match e {
            CellPipelineError::BadCellSize => Self::BadCellSize,
            CellPipelineError::BadDimensions => Self::BadDimensions,
            CellPipelineError::Cancelled => Self::Cancelled,
            CellPipelineError::Process(s) => Self::Process(s),
        }
    }
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
/// Incomplete edge cells are processed with remaining size; each cell is isolated
/// (ordered phase resets; ED does not leak across boundaries).
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
    process_sprite_sheet_rgba_f32_ex(
        rgba_f32,
        width,
        height,
        cell_w,
        cell_h,
        0,
        filters,
        palettes,
        format,
        png8_palette_id,
        cancel,
    )
}

/// Like [`process_sprite_sheet_rgba_f32`] with optional cell padding (F2).
pub fn process_sprite_sheet_rgba_f32_ex(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    cell_w: u32,
    cell_h: u32,
    padding: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
    format: HeadlessOutputFormat,
    png8_palette_id: Option<u32>,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, SpriteSheetError> {
    if format == HeadlessOutputFormat::Png8 && filters_have_guided(filters) {
        return Err(SpriteSheetError::Headless(HeadlessError::GuidedBlocksPng8));
    }

    let cell = CellSettings {
        w: cell_w,
        h: cell_h,
        padding,
        independent: true,
    };
    let out_rgba = process_independent_cells_rgba8(
        rgba_f32,
        width,
        height,
        &cell,
        filters,
        palettes,
        cancel,
        None,
    )?;

    match format {
        HeadlessOutputFormat::Png => Ok(encode_rgba_png(&out_rgba, width, height)?),
        HeadlessOutputFormat::Png8 => {
            let palette = resolve_palette(palettes, png8_palette_id)?;
            let palette_srgb: Vec<(u8, u8, u8)> = palette
                .colors
                .iter()
                .map(|c| {
                    (
                        linear_to_srgb(c.r),
                        linear_to_srgb(c.g),
                        linear_to_srgb(c.b),
                    )
                })
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

/// Convert RGBA8 → display-referred f32 (matches headless decode).
pub fn rgba8_to_f32(rgba: &[u8]) -> Vec<f32> {
    rgba.iter().map(|&v| v as f32 / 255.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{
        DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
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
    fn allows_error_diffusion_isolated() {
        let mut f = bayer_filter();
        if let FilterParams::DitherV2(ref mut p) = f.params {
            p.mode = DitherModeV2::FloydSteinberg;
        }
        let mut rgba = vec![0.5f32; 16];
        for i in 0..4 {
            rgba[i * 4 + 3] = 1.0;
        }
        let cancel = AtomicBool::new(false);
        let bytes = process_sprite_sheet_rgba_f32(
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
        .expect("ED cells allowed in F2");
        assert!(!bytes.is_empty());
    }
}
