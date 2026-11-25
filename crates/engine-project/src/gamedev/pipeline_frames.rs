//! Animation / frame-set pipeline (F3).
//!
//! Fixed pattern phase + shared palette across frames; skip identical
//! consecutive sources (delta / full-frame hash).

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use engine_color::palette::Palette;
use thiserror::Error;

use crate::filter::{DitherModeV2, FilterInstance, FilterParams};
use crate::headless::{
    decode_image_file_to_rgba_f32, run_headless_job_cancellable, HeadlessError, HeadlessJob,
    HeadlessJobResult, HeadlessOutputFormat,
};

use super::context::{GamedevContext, MaskKind, MaskPolicy, PaletteSource};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FramePipelineError {
    #[error("frame set is empty")]
    EmptySet,
    #[error("{0}")]
    Headless(String),
    #[error("cancelled")]
    Cancelled,
}

impl From<HeadlessError> for FramePipelineError {
    fn from(e: HeadlessError) -> Self {
        match e {
            HeadlessError::Cancelled => Self::Cancelled,
            other => Self::Headless(other.to_string()),
        }
    }
}

#[derive(Debug, Default)]
pub struct FramePipelineMetrics {
    pub frames_processed: AtomicU64,
    pub frames_skipped_identical: AtomicU64,
    pub pixels_delta_copied: AtomicU64,
}

pub static FRAME_PIPELINE_METRICS: FramePipelineMetrics = FramePipelineMetrics {
    frames_processed: AtomicU64::new(0),
    frames_skipped_identical: AtomicU64::new(0),
    pixels_delta_copied: AtomicU64::new(0),
};

/// Modes suitable for frame-stable animation (ordered / blue-noise family).
pub fn mode_is_frame_stable(mode: &DitherModeV2) -> bool {
    !mode.is_error_diffusion() && !mode.is_ed1d()
}

pub fn filters_are_frame_stable(filters: &[FilterInstance]) -> bool {
    let mut saw_stable_dither = false;
    for f in filters {
        if !f.enabled {
            continue;
        }
        match &f.params {
            FilterParams::DitherV2(p) => {
                if !mode_is_frame_stable(&p.mode) {
                    return false;
                }
                saw_stable_dither = true;
            }
            FilterParams::Dither { .. } => {
                // Legacy dither treated as unstable unless migrated.
                return false;
            }
            _ => {}
        }
    }
    saw_stable_dither
}

/// Rewrite dither filters to Bayer8×8 + clear ED; set context mask Fixed(ordered).
pub fn switch_filters_to_frame_stable(filters: &mut [FilterInstance]) {
    for f in filters {
        if let FilterParams::DitherV2(p) = &mut f.params {
            if !mode_is_frame_stable(&p.mode) {
                p.mode = DitherModeV2::Bayer8x8;
                p.serpentine = false;
                p.wrap_padding = false;
                f.algorithm_id = Some("bayer_8x8".into());
                // Stale ED strip flags would route Bayer into apply_ed_strip → hard error.
                f.resync_topology_flags();
            }
        }
    }
}

pub fn apply_frame_stable_context(ctx: &mut GamedevContext) {
    ctx.job = super::context::GamedevJob::Animation;
    ctx.threshold_mask = MaskPolicy::Fixed {
        kind: MaskKind::Ordered,
        phase: 0,
    };
    if matches!(ctx.palette, PaletteSource::PerImage) {
        ctx.palette = PaletteSource::Shared {
            set_id: "animation".into(),
        };
    }
}

fn content_hash_f32(rgba: &[f32]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for v in rgba {
        (((*v).clamp(0.0, 1.0) * 255.0).round() as u8).hash(&mut h);
    }
    h.finish()
}

fn rgba8_unique_opaque_colors(rgba: &[u8]) -> HashSet<[u8; 3]> {
    let mut set = HashSet::new();
    for px in rgba.chunks_exact(4) {
        if px[3] == 0 {
            continue;
        }
        set.insert([px[0], px[1], px[2]]);
    }
    set
}

/// Resolve palette list for a frame job from context + document palettes.
pub fn resolve_frame_palettes<'a>(
    ctx: &GamedevContext,
    doc_palettes: &'a [Palette],
) -> Vec<Palette> {
    match &ctx.palette {
        PaletteSource::Fixed { palette_id } => doc_palettes
            .iter()
            .find(|p| p.id == *palette_id)
            .cloned()
            .into_iter()
            .collect(),
        PaletteSource::Shared { .. } | PaletteSource::PerImage => doc_palettes.to_vec(),
    }
}

/// Whether batch/frame apply should lock pattern phase.
pub fn should_lock_pattern_phase(ctx: &GamedevContext) -> bool {
    !matches!(ctx.threshold_mask, MaskPolicy::Free) || ctx.job == super::context::GamedevJob::Animation
}

/// Apply one ordered/BN frame to RGBA8 with optional delta copy from previous.
///
/// Returns `(rgba8, content_hash, pixels_copied_from_prev)`.
pub fn dither_frame_rgba8(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
    cancel: &AtomicBool,
    prev_src_hash: Option<u64>,
    prev_out: Option<&[u8]>,
    prev_src: Option<&[f32]>,
) -> Result<(Vec<u8>, u64, u64), FramePipelineError> {
    let hash = content_hash_f32(rgba_f32);
    let px = (width as usize).saturating_mul(height as usize);
    if let (Some(ph), Some(prev)) = (prev_src_hash, prev_out) {
        if ph == hash && prev.len() == px * 4 {
            FRAME_PIPELINE_METRICS
                .frames_skipped_identical
                .fetch_add(1, Ordering::Relaxed);
            FRAME_PIPELINE_METRICS
                .pixels_delta_copied
                .fetch_add(px as u64, Ordering::Relaxed);
            return Ok((prev.to_vec(), hash, px as u64));
        }
    }

    // Fast path for ordered/BN: start from previous output and only recompute
    // changed source pixels (fixed phase ⇒ identical src ⇒ identical out).
    if filters_are_frame_stable(filters) {
        if let (Some(prev), Some(prev_s)) = (prev_out, prev_src) {
            if prev.len() == px * 4 && prev_s.len() == rgba_f32.len() {
                let (out, copied) = dither_frame_delta_ordered(
                    rgba_f32, prev_s, prev, width, height, filters, palettes, cancel,
                )?;
                FRAME_PIPELINE_METRICS
                    .frames_processed
                    .fetch_add(1, Ordering::Relaxed);
                FRAME_PIPELINE_METRICS
                    .pixels_delta_copied
                    .fetch_add(copied, Ordering::Relaxed);
                return Ok((out, hash, copied));
            }
        }
    }

    let out = process_frame_in_memory(rgba_f32, width, height, filters, palettes, cancel)?;
    FRAME_PIPELINE_METRICS
        .frames_processed
        .fetch_add(1, Ordering::Relaxed);
    Ok((out, hash, 0))
}

/// Ordered/BN delta: copy unchanged pixels from `prev_out`, re-dither full frame
/// only where source differs, then merge. Correctness first; metric records copies.
fn dither_frame_delta_ordered(
    src: &[f32],
    prev_src: &[f32],
    prev_out: &[u8],
    width: u32,
    height: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
    cancel: &AtomicBool,
) -> Result<(Vec<u8>, u64), FramePipelineError> {
    let fresh = process_frame_in_memory(src, width, height, filters, palettes, cancel)?;
    let mut merged = fresh;
    let mut copied = 0u64;
    for i in 0..(width as usize * height as usize) {
        let s = i * 4;
        if src_pixel_equal(&src[s..s + 4], &prev_src[s..s + 4]) {
            merged[s..s + 4].copy_from_slice(&prev_out[s..s + 4]);
            copied += 1;
        }
    }
    Ok((merged, copied))
}

#[inline]
fn src_pixel_equal(a: &[f32], b: &[f32]) -> bool {
    // Match content_hash quantization so hash-skip and pixel-skip agree.
    for i in 0..4 {
        let qa = (a[i].clamp(0.0, 1.0) * 255.0).round() as u8;
        let qb = (b[i].clamp(0.0, 1.0) * 255.0).round() as u8;
        if qa != qb {
            return false;
        }
    }
    true
}

fn process_frame_in_memory(
    rgba_f32: &[f32],
    width: u32,
    height: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, FramePipelineError> {
    use engine_tiles::decompose::decompose_image_to_tiles_at_generation;
    use engine_tiles::TileCache;

    use crate::document::Document;
    use crate::layer::{Layer, LayerNode};
    use crate::serialize::pixels::build_processed_composite_rgba8;
    use crate::types::{DocumentId, LayerId, LayerKind};

    if cancel.load(Ordering::Relaxed) {
        return Err(FramePipelineError::Cancelled);
    }
    let doc_id = 1u32;
    let layer_id = 1u32;
    let live_gen = 1u64;
    let budget = ((width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(16))
    .clamp(4_000_000, 128_000_000);
    let cache = TileCache::new(budget);
    decompose_image_to_tiles_at_generation(
        rgba_f32, width, height, doc_id, layer_id, &cache, live_gen,
    )
    .map_err(|e| FramePipelineError::Headless(format!("tile decompose: {e}")))?;

    let mut doc = Document::new(DocumentId::new(doc_id), width, height);
    doc.generations.set_document_gen(live_gen);
    doc.palettes = palettes.to_vec();
    let mut layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, width, height);
    layer.filters = filters.to_vec();
    doc.root.push(LayerNode::Leaf(layer));

    build_processed_composite_rgba8(&cache, &doc)
        .map_err(|e| FramePipelineError::Headless(e.to_string()))
}

/// Run frame set → output folder with fixed phase + optional identical-frame skip.
pub fn process_frame_set_to_folder(
    frame_set: &[PathBuf],
    output_dir: &Path,
    name_template: &str,
    format: HeadlessOutputFormat,
    filters: &[FilterInstance],
    palettes: &[Palette],
    lock_pattern_phase: bool,
    png8_palette_id: Option<u32>,
    cancel: &AtomicBool,
) -> Result<(Vec<HeadlessJobResult>, FrameSetReport), FramePipelineError> {
    if frame_set.is_empty() {
        return Err(FramePipelineError::EmptySet);
    }
    std::fs::create_dir_all(output_dir)
        .map_err(|e| FramePipelineError::Headless(format!("create output dir: {e}")))?;

    // Snapshot global skip counter so the report is per-run, not lifetime.
    let skips_before = FRAME_PIPELINE_METRICS
        .frames_skipped_identical
        .load(Ordering::Relaxed);

    let pixels_before = FRAME_PIPELINE_METRICS
        .pixels_delta_copied
        .load(Ordering::Relaxed);

    let mut results = Vec::with_capacity(frame_set.len());
    let mut prev_hash: Option<u64> = None;
    let mut prev_out: Option<Vec<u8>> = None;
    let mut prev_src: Option<Vec<f32>> = None;
    let mut all_colors: HashSet<[u8; 3]> = HashSet::new();
    let mut max_w = 0u32;
    let mut max_h = 0u32;

    for input in frame_set {
        if cancel.load(Ordering::Relaxed) {
            return Err(FramePipelineError::Cancelled);
        }
        let name = crate::headless::expand_output_name(name_template, input);
        let output = output_dir.join(name);

        let (w, h, rgba) = decode_image_file_to_rgba_f32(input)?;
        max_w = max_w.max(w);
        max_h = max_h.max(h);
        let (out_rgba, hash, _skipped) = dither_frame_rgba8(
            &rgba,
            w,
            h,
            filters,
            palettes,
            cancel,
            prev_hash,
            prev_out.as_deref(),
            prev_src.as_deref(),
        )?;

        // If we skipped via identical hash, still need to write the file.
        let bytes = match format {
            HeadlessOutputFormat::Png => crate::headless::encode_rgba_png(&out_rgba, w, h)?,
            HeadlessOutputFormat::Png8 => {
                // Fall back to headless job for indexed encode path.
                let job = HeadlessJob {
                    input: input.clone(),
                    output: output.clone(),
                    format,
                    filters: filters.to_vec(),
                    palettes: palettes.to_vec(),
                    lock_pattern_phase,
                    png8_palette_id,
                };
                run_headless_job_cancellable(&job, cancel)?;
                results.push(HeadlessJobResult {
                    input: input.clone(),
                    output: Some(output.clone()),
                    error: None,
                });
                all_colors.extend(rgba8_unique_opaque_colors(&out_rgba));
                prev_hash = Some(hash);
                prev_out = Some(out_rgba);
                prev_src = Some(rgba);
                continue;
            }
        };

        std::fs::write(&output, &bytes)
            .map_err(|e| FramePipelineError::Headless(format!("write {}: {e}", output.display())))?;
        all_colors.extend(rgba8_unique_opaque_colors(&out_rgba));
        results.push(HeadlessJobResult {
            input: input.clone(),
            output: Some(output),
            error: None,
        });
        prev_hash = Some(hash);
        prev_out = Some(out_rgba);
        prev_src = Some(rgba);
    }
    let _pixels_copied = FRAME_PIPELINE_METRICS
        .pixels_delta_copied
        .load(Ordering::Relaxed)
        .saturating_sub(pixels_before);

    let palette_cap = palettes
        .first()
        .map(|p| p.colors.len())
        .unwrap_or(0);
    Ok((
        results,
        FrameSetReport {
            frame_count: frame_set.len(),
            unique_colors: all_colors.len(),
            shared_palette_len: palette_cap,
            frames_skipped_identical: FRAME_PIPELINE_METRICS
                .frames_skipped_identical
                .load(Ordering::Relaxed)
                .saturating_sub(skips_before),
        },
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameSetReport {
    pub frame_count: usize,
    pub unique_colors: usize,
    pub shared_palette_len: usize,
    pub frames_skipped_identical: u64,
}

impl FrameSetReport {
    pub fn palette_ok(&self) -> bool {
        self.shared_palette_len == 0 || self.unique_colors <= self.shared_palette_len
    }
}

/// Validate two in-memory frames under the same filters produce identical output.
pub fn identical_frames_bitwise_equal(
    rgba_a: &[f32],
    rgba_b: &[f32],
    width: u32,
    height: u32,
    filters: &[FilterInstance],
    palettes: &[Palette],
) -> Result<bool, FramePipelineError> {
    let cancel = AtomicBool::new(false);
    let (out_a, _, _) =
        dither_frame_rgba8(rgba_a, width, height, filters, palettes, &cancel, None, None, None)?;
    let (out_b, _, _) =
        dither_frame_rgba8(rgba_b, width, height, filters, palettes, &cancel, None, None, None)?;
    Ok(out_a == out_b)
}

/// Collect image paths from a directory (png/jpg/webp), sorted.
pub fn collect_frame_paths(dir: &Path) -> Result<Vec<PathBuf>, FramePipelineError> {
    const EXTS: &[&str] = &["png", "jpg", "jpeg", "webp"];
    let mut files = Vec::new();
    let entries = std::fs::read_dir(dir)
        .map_err(|e| FramePipelineError::Headless(format!("read dir: {e}")))?;
    for entry in entries {
        let entry =
            entry.map_err(|e| FramePipelineError::Headless(format!("read dir: {e}")))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if EXTS.contains(&ext.as_str()) {
            files.push(path);
        }
    }
    files.sort();
    if files.is_empty() {
        return Err(FramePipelineError::EmptySet);
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherColorMode, DitherParamsV2, FilterKind};

    fn bayer() -> FilterInstance {
        let mut f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer8x8,
                levels: 4,
                color_mode: DitherColorMode::Grayscale,
                ..DitherParamsV2::default()
            }),
        );
        f.enabled = true;
        f.algorithm_id = Some("bayer_8x8".into());
        f
    }

    fn solid(w: u32, h: u32, v: f32) -> Vec<f32> {
        let mut rgba = vec![v; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            rgba[i * 4 + 3] = 1.0;
        }
        rgba
    }

    #[test]
    fn identical_frames_match() {
        let w = 16u32;
        let h = 16u32;
        let a = solid(w, h, 0.4);
        let b = a.clone();
        assert!(identical_frames_bitwise_equal(&a, &b, w, h, &[bayer()], &[]).unwrap());
    }

    #[test]
    fn identical_source_skips_second_process() {
        FRAME_PIPELINE_METRICS
            .frames_skipped_identical
            .store(0, Ordering::Relaxed);
        let w = 8u32;
        let h = 8u32;
        let src = solid(w, h, 0.5);
        let cancel = AtomicBool::new(false);
        let (out1, h1, _) =
            dither_frame_rgba8(&src, w, h, &[bayer()], &[], &cancel, None, None, None).unwrap();
        let before = FRAME_PIPELINE_METRICS
            .frames_skipped_identical
            .load(Ordering::Relaxed);
        let (out2, _, copied) = dither_frame_rgba8(
            &src,
            w,
            h,
            &[bayer()],
            &[],
            &cancel,
            Some(h1),
            Some(&out1),
            Some(&src),
        )
        .unwrap();
        let after = FRAME_PIPELINE_METRICS
            .frames_skipped_identical
            .load(Ordering::Relaxed);
        assert_eq!(out1, out2);
        assert!(after > before);
        assert_eq!(copied, (w * h) as u64);
    }

    #[test]
    fn partial_frame_delta_copies_unchanged_pixels() {
        FRAME_PIPELINE_METRICS
            .pixels_delta_copied
            .store(0, Ordering::Relaxed);
        let w = 8u32;
        let h = 8u32;
        let mut a = solid(w, h, 0.4);
        let mut b = a.clone();
        // Change one pixel in b.
        b[0] = 0.9;
        b[1] = 0.9;
        b[2] = 0.9;
        let cancel = AtomicBool::new(false);
        let (out_a, h_a, _) =
            dither_frame_rgba8(&a, w, h, &[bayer()], &[], &cancel, None, None, None).unwrap();
        let (out_b, _, copied) = dither_frame_rgba8(
            &b,
            w,
            h,
            &[bayer()],
            &[],
            &cancel,
            Some(h_a),
            Some(&out_a),
            Some(&a),
        )
        .unwrap();
        assert!(copied >= (w * h - 1) as u64);
        // Unchanged pixel region (last pixel) must match.
        let last = ((w * h - 1) * 4) as usize;
        assert_eq!(&out_a[last..last + 4], &out_b[last..last + 4]);
        let _ = &mut a;
        let _ = &mut b;
    }

    #[test]
    fn switch_ed_to_bayer() {
        let mut f = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                ..DitherParamsV2::default()
            }),
        );
        f.enabled = true;
        assert!(f.requires_full_row, "ED starts with full-row topology");
        switch_filters_to_frame_stable(std::slice::from_mut(&mut f));
        assert!(matches!(
            &f.params,
            FilterParams::DitherV2(p) if matches!(p.mode, DitherModeV2::Bayer8x8)
        ));
        assert!(
            !f.requires_full_row && !f.requires_full_column,
            "frame-stable switch must clear ED strip flags"
        );
        assert!(filters_are_frame_stable(&[f]));
    }
}
