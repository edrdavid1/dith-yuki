//! Headless image export — no Tauri / AppState dependency.
//!
//! Used by batch processing and (later) CLI: open an image, apply a filter
//! stack + palettes, write PNG RGBA or indexed PNG8.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use engine_color::palette::{linear_to_srgb, LinearColor, Palette};
use engine_io::{atomic_write, encode_indexed_png};
use engine_tiles::decompose::decompose_image_to_tiles_at_generation;
use engine_tiles::TileCache;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::document::Document;
use crate::filter::{FilterInstance, FilterParams, PaletteDitherMode};
use crate::layer::{Layer, LayerNode};
use crate::serialize::pixels::build_processed_composite_rgba8;
use crate::types::{DocumentId, LayerId, LayerKind, PaletteId};

/// Output codec for a headless job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadlessOutputFormat {
    /// Lossless RGBA PNG.
    Png,
    /// Indexed PNG (colortype 3) — requires a palette and palette-safe filters.
    Png8,
}

/// One headless export: input image → filter stack → output file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessJob {
    pub input: PathBuf,
    pub output: PathBuf,
    pub format: HeadlessOutputFormat,
    /// Filter stack applied to the single raster layer (bottom → top).
    pub filters: Vec<FilterInstance>,
    /// Palettes referenced by `filters` (ids must match `palette_id` params).
    pub palettes: Vec<Palette>,
    /// When true (default), pattern sampling uses document origin (0,0) so
    /// ordered dither phase matches across files in a batch.
    #[serde(default = "default_true")]
    pub lock_pattern_phase: bool,
    /// Optional palette id for PNG8 indexing; defaults to the first palette.
    #[serde(default)]
    pub png8_palette_id: Option<u32>,
}

fn default_true() -> bool {
    true
}

/// Per-file outcome for batch runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadlessJobResult {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub error: Option<String>,
}

#[derive(Debug, Error)]
pub enum HeadlessError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("decode error: {0}")]
    Decode(String),
    #[error("invalid image dimensions")]
    BadDimensions,
    #[error("export error: {0}")]
    Export(String),
    #[error(
        "indexed PNG (PNG8) is disabled for Guided palette dither — \
         Guided produces colors outside the palette"
    )]
    GuidedBlocksPng8,
    #[error("PNG8 export requires a non-empty palette")]
    MissingPalette,
    #[error("cancelled")]
    Cancelled,
}

/// Decode an image file to display-referred RGBA f32 in `[0, 1]` (matches app import).
pub fn decode_image_file_to_rgba_f32(path: &Path) -> Result<(u32, u32, Vec<f32>), HeadlessError> {
    let img = image::open(path).map_err(|e| HeadlessError::Decode(e.to_string()))?;
    let rgba = img.to_rgba8();
    let width = rgba.width();
    let height = rgba.height();
    if width == 0 || height == 0 {
        return Err(HeadlessError::BadDimensions);
    }
    let mut out = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for p in rgba.pixels() {
        out.push(p[0] as f32 / 255.0);
        out.push(p[1] as f32 / 255.0);
        out.push(p[2] as f32 / 255.0);
        out.push(p[3] as f32 / 255.0);
    }
    Ok((width, height, out))
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

fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, HeadlessError> {
    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;
    use std::io::Cursor;

    let mut buf = Vec::new();
    let encoder = PngEncoder::new(Cursor::new(&mut buf));
    encoder
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| HeadlessError::Export(e.to_string()))?;
    Ok(buf)
}

/// Apply `job.filters` + palettes to `input` and write `output`.
///
/// Coordinate origin is always document (0,0). With `lock_pattern_phase`
/// (default), ordered dither phase is therefore identical across batch files.
pub fn run_headless_job(job: &HeadlessJob) -> Result<(), HeadlessError> {
    run_headless_job_cancellable(job, &AtomicBool::new(false))
}

pub fn run_headless_job_cancellable(
    job: &HeadlessJob,
    cancel: &AtomicBool,
) -> Result<(), HeadlessError> {
    let _ = job.lock_pattern_phase; // documented invariant: origin fixed at (0,0)
    if cancel.load(Ordering::Relaxed) {
        return Err(HeadlessError::Cancelled);
    }

    if job.format == HeadlessOutputFormat::Png8 && filters_have_guided(&job.filters) {
        return Err(HeadlessError::GuidedBlocksPng8);
    }

    let (width, height, rgba_f32) = decode_image_file_to_rgba_f32(&job.input)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(HeadlessError::Cancelled);
    }

    let doc_id = 1u32;
    let layer_id = 1u32;
    let live_gen = 1u64;

    // Budget: roughly enough for one full-resolution document worth of tiles.
    let budget = ((width as usize).saturating_mul(height as usize).saturating_mul(16))
        .max(50_000_000)
        .min(512_000_000);
    let cache = TileCache::new(budget);

    decompose_image_to_tiles_at_generation(
        &rgba_f32,
        width,
        height,
        doc_id,
        layer_id,
        &cache,
        live_gen,
    )
    .map_err(|e| HeadlessError::Export(format!("tile decompose: {e}")))?;

    let mut doc = Document::new(DocumentId::new(doc_id), width, height);
    doc.generations.set_document_gen(live_gen);
    doc.palettes = job.palettes.clone();

    let mut layer = Layer::new(
        LayerId::new(layer_id),
        LayerKind::Raster,
        width,
        height,
    );
    layer.filters = job.filters.clone();
    doc.root.push(LayerNode::Leaf(layer));

    if cancel.load(Ordering::Relaxed) {
        return Err(HeadlessError::Cancelled);
    }

    let rgba8 = build_processed_composite_rgba8(&cache, &doc)
        .map_err(|e| HeadlessError::Export(e.to_string()))?;

    let bytes = match job.format {
        HeadlessOutputFormat::Png => encode_rgba_png(&rgba8, width, height)?,
        HeadlessOutputFormat::Png8 => {
            let palette = resolve_png8_palette(&doc, job.png8_palette_id)?;
            let palette_srgb: Vec<(u8, u8, u8)> = palette
                .colors
                .iter()
                .map(|c| (linear_to_srgb(c.r), linear_to_srgb(c.g), linear_to_srgb(c.b)))
                .collect();
            encode_indexed_png(&rgba8, width, height, &palette_srgb)
                .map_err(|e| HeadlessError::Export(e.to_string()))?
        }
    };

    if cancel.load(Ordering::Relaxed) {
        return Err(HeadlessError::Cancelled);
    }

    if let Some(parent) = job.output.parent() {
        std::fs::create_dir_all(parent).map_err(|e| HeadlessError::Io(e.to_string()))?;
    }
    atomic_write(&job.output, &bytes).map_err(|e| HeadlessError::Io(e.to_string()))?;
    Ok(())
}

fn resolve_png8_palette(
    doc: &Document,
    palette_id: Option<u32>,
) -> Result<&Palette, HeadlessError> {
    if let Some(id) = palette_id {
        return doc
            .get_palette(PaletteId::new(id))
            .ok_or(HeadlessError::MissingPalette);
    }
    doc.palettes
        .first()
        .filter(|p| !p.colors.is_empty())
        .ok_or(HeadlessError::MissingPalette)
}

/// Expand `{name}` / `{ext}` in an output template (e.g. `{name}_dithered.png`).
pub fn expand_output_name(template: &str, input: &Path) -> String {
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext = input
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png");
    template
        .replace("{name}", stem)
        .replace("{ext}", ext)
}

/// Run jobs sequentially. One failure does not stop the batch.
///
/// `on_progress(done, total, latest)` is called after each file.
pub fn run_headless_batch(
    jobs: &[HeadlessJob],
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(usize, usize, &HeadlessJobResult),
) -> Vec<HeadlessJobResult> {
    let total = jobs.len();
    let mut results = Vec::with_capacity(total);
    for (i, job) in jobs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            let r = HeadlessJobResult {
                input: job.input.clone(),
                output: None,
                error: Some("cancelled".into()),
            };
            on_progress(i, total, &r);
            results.push(r);
            // Mark remaining as cancelled without running.
            for job in &jobs[i + 1..] {
                results.push(HeadlessJobResult {
                    input: job.input.clone(),
                    output: None,
                    error: Some("cancelled".into()),
                });
            }
            break;
        }
        let r = match run_headless_job_cancellable(job, cancel) {
            Ok(()) => HeadlessJobResult {
                input: job.input.clone(),
                output: Some(job.output.clone()),
                error: None,
            },
            Err(e) => HeadlessJobResult {
                input: job.input.clone(),
                output: None,
                error: Some(e.to_string()),
            },
        };
        on_progress(i + 1, total, &r);
        results.push(r);
    }
    results
}

/// Convenience: build a single-color-entry palette for tests / simple jobs.
pub fn palette_from_srgb(id: u32, name: &str, colors: &[(u8, u8, u8)]) -> Palette {
    use engine_color::palette::srgb_to_linear;
    Palette {
        id,
        name: name.to_string(),
        colors: colors
            .iter()
            .map(|&(r, g, b)| LinearColor {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            })
            .collect(),
        revision: 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2, FilterKind, FilterParams};
    use crate::filter::FilterInstance;
    use image::RgbImage;
    use std::sync::atomic::AtomicBool;

    fn write_test_png(path: &Path, w: u32, h: u32) {
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([
                (x * 17) as u8,
                (y * 13) as u8,
                128,
            ]);
        }
        img.save(path).unwrap();
    }

    #[test]
    fn expand_output_name_replaces_tokens() {
        let p = Path::new("/tmp/walk_01.png");
        assert_eq!(
            expand_output_name("{name}_dithered.png", p),
            "walk_01_dithered.png"
        );
    }

    #[test]
    fn headless_bayer_export_writes_png() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("frame.png");
        let output = dir.path().join("frame_dithered.png");
        write_test_png(&input, 64, 64);

        let mut filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                color_mode: DitherColorMode::Rgb,
                dither_alpha: false,
                ..DitherParamsV2::default()
            }),
        );
        filter.algorithm_id = Some("bayer_4x4".into());

        let job = HeadlessJob {
            input: input.clone(),
            output: output.clone(),
            format: HeadlessOutputFormat::Png,
            filters: vec![filter],
            palettes: vec![],
            lock_pattern_phase: true,
            png8_palette_id: None,
        };
        run_headless_job(&job).unwrap();
        assert!(output.is_file());
        let meta = std::fs::metadata(&output).unwrap();
        assert!(meta.len() > 32);
    }

    #[test]
    fn batch_continues_after_one_failure() {
        let dir = tempfile::tempdir().unwrap();
        let good1 = dir.path().join("a.png");
        let good2 = dir.path().join("c.png");
        write_test_png(&good1, 32, 32);
        write_test_png(&good2, 32, 32);
        let bad = dir.path().join("missing.png");

        let mk = |input: PathBuf| {
            let mut filter = FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: DitherModeV2::Bayer2x2,
                    levels: 2,
                    dither_alpha: false,
                    ..DitherParamsV2::default()
                }),
            );
            filter.algorithm_id = Some("bayer_2x2".into());
            let out_name = expand_output_name("{name}_dithered.png", &input);
            HeadlessJob {
                output: dir.path().join(out_name),
                input,
                format: HeadlessOutputFormat::Png,
                filters: vec![filter],
                palettes: vec![],
                lock_pattern_phase: true,
                png8_palette_id: None,
            }
        };

        let jobs = vec![mk(good1), mk(bad), mk(good2)];
        let cancel = AtomicBool::new(false);
        let results = run_headless_batch(&jobs, &cancel, |_, _, _| {});
        assert_eq!(results.len(), 3);
        assert!(results[0].error.is_none());
        assert!(results[1].error.is_some());
        assert!(results[2].error.is_none());
        assert!(results[0].output.as_ref().unwrap().is_file());
        assert!(results[2].output.as_ref().unwrap().is_file());
    }

    #[test]
    fn lock_pattern_phase_matches_across_identical_files() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.png");
        let b = dir.path().join("b.png");
        write_test_png(&a, 48, 48);
        std::fs::copy(&a, &b).unwrap();

        let mk = |input: PathBuf, output: PathBuf| {
            let mut filter = FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: DitherModeV2::Bayer8x8,
                    levels: 4,
                    dither_alpha: false,
                    pattern_angle: 15.0,
                    ..DitherParamsV2::default()
                }),
            );
            filter.algorithm_id = Some("bayer_8x8".into());
            HeadlessJob {
                input,
                output,
                format: HeadlessOutputFormat::Png,
                filters: vec![filter],
                palettes: vec![],
                lock_pattern_phase: true,
                png8_palette_id: None,
            }
        };

        let out_a = dir.path().join("a_out.png");
        let out_b = dir.path().join("b_out.png");
        run_headless_job(&mk(a, out_a.clone())).unwrap();
        run_headless_job(&mk(b, out_b.clone())).unwrap();
        let bytes_a = std::fs::read(&out_a).unwrap();
        let bytes_b = std::fs::read(&out_b).unwrap();
        assert_eq!(
            bytes_a, bytes_b,
            "locked pattern phase must match byte-for-byte on identical frames"
        );
    }
}
