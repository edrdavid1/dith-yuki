//! Pipeline Tile (F1 steps 2–3): mosaic route + memory gate.
//!
//! When `GamedevContext.tile.enabled`:
//! - Period-aligned ordered/BN → **skip** 3×3 mosaic (bit-identical to normal dither).
//! - Ordered/BN when period does not divide size → assemble source 3×3 → dither → crop.
//! - **Error diffusion / ED1D → always** 3×3 mosaic (never skip); Relative seam metric later.
//!
//! Size / memory gate (mandatory): refuse expand when the source exceeds the
//! interactive side limit or 9× pixels exceed [`PIPELINE_TILE_MAX_MOSAIC_PIXELS`].
//! Period-aligned ordered/BN that **skips** the mosaic is not gated by the
//! interactive side limit (work equals a normal dither).

use crate::filter::{DitherModeV2, DitherParamsV2, FilterParams};
use crate::layer::Layer;
use crate::seamless::{
    check_seamless, ordered_matrix_side, pattern_angle_breaks_periodicity, SeamlessKind,
};

use super::seams::SeamMode;

/// Interactive ceiling for Pipeline Tile when a 3×3 mosaic must run (ED, or
/// ordered/BN that is not period-aligned). Spec acceptance target: ~1K tile.
///
/// 1024² × 9 ≈ 9.4 Mpx mosaic — past this, ED on CPU feels like a hang.
pub const PIPELINE_TILE_MAX_SOURCE_SIDE: u32 = 1024;

/// Absolute ceiling for 3×3 mosaic pixel count (RGBA f32 ≈ 16 B/px).
/// With [`PIPELINE_TILE_MAX_SOURCE_SIDE`], mosaic is ≤ 9×1024²; this remains a
/// hard RAM backstop if the side limit is raised later.
pub const PIPELINE_TILE_MAX_MOSAIC_PIXELS: usize = 9 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineTileSession {
    pub orig_w: u32,
    pub orig_h: u32,
    /// True when the buffer was expanded to 3×3 and must be cropped after filters.
    pub expanded: bool,
}

/// Why Pipeline Tile cannot allocate the mosaic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineTileError {
    /// Source exceeds [`PIPELINE_TILE_MAX_SOURCE_SIDE`] (interactive time gate).
    InteractiveLimit {
        width: u32,
        height: u32,
        max_side: u32,
    },
    /// 9× mosaic exceeds [`PIPELINE_TILE_MAX_MOSAIC_PIXELS`] (or overflow).
    MemoryGate {
        width: u32,
        height: u32,
        mosaic_pixels: Option<usize>,
        limit: usize,
    },
}

impl std::fmt::Display for PipelineTileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InteractiveLimit {
                width,
                height,
                max_side,
            } => write!(
                f,
                "Pipeline Tile mosaic is limited to {max_side}×{max_side} \
                 for interactive preview ({width}×{height} needs a 9× pass)"
            ),
            Self::MemoryGate {
                width,
                height,
                mosaic_pixels,
                limit,
            } => match mosaic_pixels {
                Some(n) => write!(
                    f,
                    "Pipeline Tile mosaic exceeds memory ceiling ({width}×{height} → {n} px > limit {limit})"
                ),
                None => write!(
                    f,
                    "Pipeline Tile mosaic size overflow ({width}×{height} × 9)"
                ),
            },
        }
    }
}

impl std::error::Error for PipelineTileError {}

/// Modes that use a fixed lattice / texture period (skip-mosaic optimization).
pub fn period_based_mode(mode: &DitherModeV2) -> bool {
    ordered_matrix_side(mode).is_some()
        || matches!(mode, DitherModeV2::BlueNoise)
        || matches!(mode, DitherModeV2::CustomPng { .. })
}

/// Lattice period in pixels, if known.
pub fn mode_period_px(
    mode: &DitherModeV2,
    pixel_size: u8,
    custom_map_size: Option<(u32, u32)>,
) -> Option<(u32, u32)> {
    if let Some((nx, ny)) = ordered_matrix_side(mode) {
        let ps = u32::from(pixel_size.max(1));
        return Some((nx.saturating_mul(ps), ny.saturating_mul(ps)));
    }
    match mode {
        DitherModeV2::BlueNoise => {
            let s = crate::algorithms::blue_noise_texture::BlueNoiseTexture::SIZE;
            let ps = u32::from(pixel_size.max(1));
            Some((s.saturating_mul(ps), s.saturating_mul(ps)))
        }
        DitherModeV2::CustomPng { .. } => custom_map_size.map(|(nx, ny)| {
            let ps = u32::from(pixel_size.max(1));
            (nx.saturating_mul(ps), ny.saturating_mul(ps))
        }),
        _ => None,
    }
}

/// True when ordered/BN seamless check says size is period-aligned (and angle ok).
pub fn dither_period_aligned(params: &DitherParamsV2, width: u32, height: u32) -> bool {
    if params.mode.is_error_diffusion() || params.mode.is_ed1d() {
        return false;
    }
    // Prefer check_seamless for Bayer-family; BlueNoise uses our period helper.
    if ordered_matrix_side(&params.mode).is_some() {
        let report = check_seamless(
            &params.mode,
            params.pixel_size,
            params.pattern_angle,
            width,
            height,
            None,
        );
        return report.kind == SeamlessKind::ExactOrdered
            && report.width_ok
            && report.height_ok
            && !report.angle_breaks_periodicity;
    }
    let Some((px, py)) = mode_period_px(&params.mode, params.pixel_size, None) else {
        return false;
    };
    if pattern_angle_breaks_periodicity(params.pattern_angle) {
        return false;
    }
    px > 0 && py > 0 && width.is_multiple_of(px) && height.is_multiple_of(py)
}

/// Enabled dither filters that are period-based (ordered / BN / custom map).
fn enabled_period_dithers(layer: &Layer) -> Vec<&DitherParamsV2> {
    layer
        .filters
        .iter()
        .filter(|f| f.enabled)
        .filter_map(|f| match &f.params {
            FilterParams::DitherV2(p) if period_based_mode(&p.mode) => Some(p),
            _ => None,
        })
        .collect()
}

fn enabled_has_ed_dither(layer: &Layer) -> bool {
    layer.filters.iter().any(|f| {
        f.enabled
            && matches!(
                &f.params,
                FilterParams::DitherV2(p) if p.mode.is_error_diffusion() || p.mode.is_ed1d()
            )
    })
}

fn enabled_has_any_dither(layer: &Layer) -> bool {
    layer.filters.iter().any(|f| {
        f.enabled && matches!(&f.params, FilterParams::DitherV2(_))
    })
}

/// Layer wants Pipeline Tile when Tile is on and a dither filter is enabled.
pub fn layer_wants_pipeline_tile(layer: &Layer, tile_enabled: bool) -> bool {
    tile_enabled && enabled_has_any_dither(layer)
}

/// Backward-compatible alias (ordered/BN-only callers from step 2).
#[inline]
pub fn layer_wants_ordered_pipeline_tile(layer: &Layer, tile_enabled: bool) -> bool {
    layer_wants_pipeline_tile(layer, tile_enabled) && !enabled_has_ed_dither(layer)
}

/// Skip 3×3 when every enabled period-dither is period-aligned and **no ED**.
///
/// ED always mosaics — sequential error history needs neighbor context.
pub fn should_skip_mosaic(layer: &Layer, width: u32, height: u32) -> bool {
    if enabled_has_ed_dither(layer) {
        return false;
    }
    let dithers = enabled_period_dithers(layer);
    if dithers.is_empty() {
        return true;
    }
    dithers
        .iter()
        .all(|p| dither_period_aligned(p, width, height))
}

pub fn mosaic_pixel_count(width: u32, height: u32) -> Option<usize> {
    let w = (width as u64).checked_mul(3)?;
    let h = (height as u64).checked_mul(3)?;
    let n = w.checked_mul(h)?;
    usize::try_from(n).ok()
}

pub fn pipeline_tile_fits_limits(width: u32, height: u32) -> bool {
    if width > PIPELINE_TILE_MAX_SOURCE_SIDE || height > PIPELINE_TILE_MAX_SOURCE_SIDE {
        return false;
    }
    match mosaic_pixel_count(width, height) {
        Some(n) => n <= PIPELINE_TILE_MAX_MOSAIC_PIXELS,
        None => false,
    }
}

/// Size/memory gate when Tile is enabled (chip / preflight; ignores skip-mosaic).
pub fn pipeline_tile_memory_blocked(width: u32, height: u32, tile_enabled: bool) -> bool {
    tile_enabled && !pipeline_tile_fits_limits(width, height)
}

/// Full-document route needed because Pipeline Tile will expand to 3×3
/// (or refuse with [`PipelineTileError`] at begin).
///
/// Oversize still selects the FD route so compute fails loudly — never a
/// silent non-tiled dither while `tile.enabled` stays on.
pub fn layer_needs_pipeline_tile_full_document(
    layer: &Layer,
    tile_enabled: bool,
    width: u32,
    height: u32,
) -> bool {
    if !layer_wants_pipeline_tile(layer, tile_enabled) {
        return false;
    }
    !should_skip_mosaic(layer, width, height)
}

/// Preflight: Tile wants a mosaic but the interactive/size gate refuses.
///
/// Period-aligned ordered/BN that skips the mosaic returns `None` even on large
/// canvases — those paths do not pay the 9× cost.
pub fn pipeline_tile_memory_gate_error(
    layer: &Layer,
    tile_enabled: bool,
    width: u32,
    height: u32,
) -> Option<PipelineTileError> {
    if !layer_wants_pipeline_tile(layer, tile_enabled) {
        return None;
    }
    if should_skip_mosaic(layer, width, height) {
        return None;
    }
    if width > PIPELINE_TILE_MAX_SOURCE_SIDE || height > PIPELINE_TILE_MAX_SOURCE_SIDE {
        return Some(PipelineTileError::InteractiveLimit {
            width,
            height,
            max_side: PIPELINE_TILE_MAX_SOURCE_SIDE,
        });
    }
    if pipeline_tile_fits_limits(width, height) {
        return None;
    }
    Some(PipelineTileError::MemoryGate {
        width,
        height,
        mosaic_pixels: mosaic_pixel_count(width, height),
        limit: PIPELINE_TILE_MAX_MOSAIC_PIXELS,
    })
}

/// Tile source into a 3×3 mosaic (row-major RGBA f32).
pub fn assemble_mosaic_rgba_f32(src: &[f32], width: u32, height: u32) -> (Vec<f32>, u32, u32) {
    let w = width as usize;
    let h = height as usize;
    let mw = w * 3;
    let mh = h * 3;
    let mut out = vec![0.0f32; mw * mh * 4];
    for ty in 0..3 {
        for tx in 0..3 {
            for y in 0..h {
                for x in 0..w {
                    let si = (y * w + x) * 4;
                    let dx = tx * w + x;
                    let dy = ty * h + y;
                    let di = (dy * mw + dx) * 4;
                    out[di..di + 4].copy_from_slice(&src[si..si + 4]);
                }
            }
        }
    }
    (out, (mw as u32), (mh as u32))
}

/// Crop the center W×H tile from a 3×3 mosaic.
pub fn crop_center_rgba_f32(
    mosaic: &[f32],
    mosaic_w: u32,
    mosaic_h: u32,
    width: u32,
    height: u32,
) -> Vec<f32> {
    let mw = mosaic_w as usize;
    let w = width as usize;
    let h = height as usize;
    debug_assert_eq!(mosaic_w, width.saturating_mul(3));
    debug_assert_eq!(mosaic_h, height.saturating_mul(3));
    let x0 = w;
    let y0 = h;
    let mut out = vec![0.0f32; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let si = ((y0 + y) * mw + (x0 + x)) * 4;
            let di = (y * w + x) * 4;
            out[di..di + 4].copy_from_slice(&mosaic[si..si + 4]);
        }
    }
    out
}

/// Expand buffer to 3×3 when Pipeline Tile mosaic is required.
pub fn begin_pipeline_tile_pass(
    rgba: &mut Vec<f32>,
    width: &mut u32,
    height: &mut u32,
    layer: &Layer,
    tile_enabled: bool,
) -> Result<PipelineTileSession, PipelineTileError> {
    let orig_w = *width;
    let orig_h = *height;

    if let Some(err) = pipeline_tile_memory_gate_error(layer, tile_enabled, orig_w, orig_h) {
        return Err(err);
    }

    if !layer_needs_pipeline_tile_full_document(layer, tile_enabled, orig_w, orig_h) {
        return Ok(PipelineTileSession {
            orig_w,
            orig_h,
            expanded: false,
        });
    }

    let (mosaic, mw, mh) = assemble_mosaic_rgba_f32(rgba, orig_w, orig_h);
    *rgba = mosaic;
    *width = mw;
    *height = mh;
    Ok(PipelineTileSession {
        orig_w,
        orig_h,
        expanded: true,
    })
}

/// Crop back to the original tile after filters.
pub fn finish_pipeline_tile_pass(
    rgba: &mut Vec<f32>,
    width: &mut u32,
    height: &mut u32,
    session: PipelineTileSession,
) {
    if !session.expanded {
        return;
    }
    *rgba = crop_center_rgba_f32(rgba, *width, *height, session.orig_w, session.orig_h);
    *width = session.orig_w;
    *height = session.orig_h;
}

/// Seam validation mode for the layer's enabled dither stack.
pub fn seam_mode_for_layer(layer: &Layer) -> SeamMode {
    if enabled_has_ed_dither(layer) {
        SeamMode::Relative
    } else {
        SeamMode::Exact
    }
}

fn gcd_u32(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

fn lcm_u32(a: u32, b: u32) -> Option<u32> {
    if a == 0 || b == 0 {
        return None;
    }
    let g = gcd_u32(a, b);
    a.checked_div(g)?.checked_mul(b)
}

/// Combined lattice period for all enabled period-based dithers (LCM per axis).
pub fn layer_combined_period_px(layer: &Layer) -> Option<(u32, u32)> {
    let mut px: Option<u32> = None;
    let mut py: Option<u32> = None;
    for p in enabled_period_dithers(layer) {
        let Some((ax, ay)) = mode_period_px(&p.mode, p.pixel_size, None) else {
            continue;
        };
        px = Some(match px {
            Some(c) => lcm_u32(c, ax)?,
            None => ax,
        });
        py = Some(match py {
            Some(c) => lcm_u32(c, ay)?,
            None => ay,
        });
    }
    match (px, py) {
        (Some(x), Some(y)) if x > 0 && y > 0 => Some((x, y)),
        _ => None,
    }
}

/// Crop-to-period target (nearest multiples at or below size, ≥ period).
///
/// `None` when already aligned, no period dither, or size &lt; period (need grow).
pub fn period_snap_crop_size(layer: &Layer, width: u32, height: u32) -> Option<(u32, u32)> {
    if enabled_has_ed_dither(layer) {
        return None;
    }
    let (px, py) = layer_combined_period_px(layer)?;
    if width.is_multiple_of(px) && height.is_multiple_of(py) {
        return None;
    }
    if width < px || height < py {
        return None;
    }
    let nw = (width / px) * px;
    let nh = (height / py) * py;
    if nw == 0 || nh == 0 || (nw == width && nh == height) {
        return None;
    }
    Some((nw, nh))
}

/// Clear `wrap_padding` on all dither filters (Tile wins).
pub fn clear_wrap_padding_on_layer(layer: &mut Layer) {
    for f in &mut layer.filters {
        if let FilterParams::DitherV2(p) = &mut f.params {
            p.wrap_padding = false;
        }
    }
}

pub fn clear_wrap_padding_in_tree(nodes: &mut [crate::layer::LayerNode]) {
    use crate::layer::LayerNode;
    for n in nodes {
        match n {
            LayerNode::Leaf(layer) => clear_wrap_padding_on_layer(layer),
            LayerNode::Group(g) => clear_wrap_padding_in_tree(&mut g.children),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams};
    use crate::layer::Layer;
    use crate::types::{LayerId, LayerKind};

    fn ed_layer(w: u32, h: u32) -> Layer {
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::FloydSteinberg,
                levels: 4,
                pixel_size: 1,
                wrap_padding: false,
                ..DitherParamsV2::default()
            }),
        ));
        layer
    }

    #[test]
    fn bayer8_32_aligned_skips() {
        let p = DitherParamsV2 {
            mode: DitherModeV2::Bayer8x8,
            pixel_size: 1,
            ..DitherParamsV2::default()
        };
        assert!(dither_period_aligned(&p, 32, 32));
        assert!(!dither_period_aligned(&p, 33, 32));
    }

    #[test]
    fn mosaic_crop_round_trip_center() {
        let w = 4u32;
        let h = 3u32;
        let mut src = vec![0.0f32; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            src[i * 4] = i as f32;
            src[i * 4 + 3] = 1.0;
        }
        let (mosaic, mw, mh) = assemble_mosaic_rgba_f32(&src, w, h);
        assert_eq!(mw, 12);
        assert_eq!(mh, 9);
        let back = crop_center_rgba_f32(&mosaic, mw, mh, w, h);
        assert_eq!(back, src);
    }

    #[test]
    fn blue_noise_period_128() {
        let p = DitherParamsV2 {
            mode: DitherModeV2::BlueNoise,
            pixel_size: 1,
            ..DitherParamsV2::default()
        };
        assert!(dither_period_aligned(&p, 128, 128));
        assert!(!dither_period_aligned(&p, 64, 64));
    }

    #[test]
    fn ed_never_skips_mosaic() {
        let layer = ed_layer(64, 64);
        assert!(!should_skip_mosaic(&layer, 64, 64));
        assert!(layer_needs_pipeline_tile_full_document(&layer, true, 64, 64));
        assert!(!layer_wants_ordered_pipeline_tile(&layer, true));
        assert!(layer_wants_pipeline_tile(&layer, true));
    }

    #[test]
    fn interactive_limit_blocks_oversize_ed() {
        // 3000 > 1024 side limit → refuse before 9× mosaic.
        assert!(!pipeline_tile_fits_limits(3000, 3000));
        assert!(!pipeline_tile_fits_limits(1025, 1025));
        assert!(pipeline_tile_fits_limits(1024, 1024));
        let layer = ed_layer(3000, 3000);
        assert!(pipeline_tile_memory_blocked(3000, 3000, true));
        // Still selects FD so begin can refuse (no silent non-tiled path).
        assert!(layer_needs_pipeline_tile_full_document(
            &layer, true, 3000, 3000
        ));
        let err = pipeline_tile_memory_gate_error(&layer, true, 3000, 3000).unwrap();
        assert!(matches!(err, PipelineTileError::InteractiveLimit { .. }));
    }

    #[test]
    fn interactive_limit_allows_1k_ed() {
        assert!(pipeline_tile_fits_limits(256, 256));
        let layer = ed_layer(256, 256);
        assert!(layer_needs_pipeline_tile_full_document(
            &layer, true, 256, 256
        ));
        assert!(pipeline_tile_memory_gate_error(&layer, true, 256, 256).is_none());
        let layer_1k = ed_layer(1024, 1024);
        assert!(pipeline_tile_memory_gate_error(&layer_1k, true, 1024, 1024).is_none());
    }

    #[test]
    fn interactive_limit_skips_period_aligned_ordered() {
        // Large Bayer that skips mosaic must not hit the interactive gate.
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 2048, 2048);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer8x8,
                pixel_size: 1,
                ..DitherParamsV2::default()
            }),
        ));
        assert!(should_skip_mosaic(&layer, 2048, 2048));
        assert!(pipeline_tile_memory_gate_error(&layer, true, 2048, 2048).is_none());
    }

    #[test]
    fn period_snap_crops_to_multiple() {
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 33, 32);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer8x8,
                pixel_size: 1,
                ..DitherParamsV2::default()
            }),
        ));
        assert_eq!(period_snap_crop_size(&layer, 33, 32), Some((32, 32)));
        assert!(period_snap_crop_size(&layer, 32, 32).is_none());
        assert_eq!(seam_mode_for_layer(&layer), SeamMode::Exact);
        assert_eq!(seam_mode_for_layer(&ed_layer(32, 32)), SeamMode::Relative);
    }
}
