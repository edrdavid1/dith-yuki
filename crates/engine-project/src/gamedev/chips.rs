//! Runtime chip channel — F1 seamless evaluation + cache.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::layer::Layer;

use super::context::GamedevContext;
use super::pipeline_alpha::{count_semi_transparent_rgba8, has_semi_transparent_rgba8};
use super::pipeline_cells::cell_grid_crop_size;
use super::pipeline_frames::{filters_are_frame_stable, should_lock_pattern_phase};
use super::pipeline_tile::{
    period_snap_crop_size, pipeline_tile_fits_limits, pipeline_tile_memory_gate_error,
    PIPELINE_TILE_MAX_SOURCE_SIDE,
};
use super::seams::{SeamMode, SeamReport};

/// Stable action ids for chip buttons (FE / IPC dispatch).
pub mod chip_action {
    pub const DISABLE_TILE: &str = "disable_tile";
    pub const ENABLE_WRAP_PADDING: &str = "enable_wrap_padding";
    pub const SNAP_TO_PERIOD: &str = "snap_to_period";
    pub const CROP_TO_CELL_GRID: &str = "crop_to_cell_grid";
    pub const SWITCH_TO_FRAME_STABLE: &str = "switch_to_frame_stable";
    pub const THRESHOLD_ALPHA: &str = "threshold_alpha";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GamedevChipId {
    Seamless,
    Cells,
    Stable,
    Palette,
    Alpha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChipState {
    Pending,
    Ok,
    Warn,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GamedevChip {
    pub id: GamedevChipId,
    pub state: ChipState,
    pub label: String,
    pub tooltip: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
}

/// Runtime-only chip cache keyed by context (+ optional result) hash.
#[derive(Debug, Default, Clone)]
pub struct ChipCache {
    entries: HashMap<u64, Vec<GamedevChip>>,
    /// Bumped on invalidate — async validators compare before emit.
    epoch: u64,
}

impl ChipCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }
}

pub fn chip_cache_get(cache: &ChipCache, key: u64) -> Option<&[GamedevChip]> {
    cache.entries.get(&key).map(|v| v.as_slice())
}

pub fn chip_cache_put(cache: &mut ChipCache, key: u64, chips: Vec<GamedevChip>) {
    cache.entries.insert(key, chips);
}

pub fn chip_cache_invalidate(cache: &mut ChipCache) {
    cache.epoch = cache.epoch.wrapping_add(1);
    cache.clear();
}

pub fn chip_cache_epoch(cache: &ChipCache) -> u64 {
    cache.epoch()
}

/// Begin an async validation job: bump epoch so older in-flight results are dropped.
pub fn chip_cache_begin_job(cache: &mut ChipCache) -> u64 {
    cache.epoch = cache.epoch.wrapping_add(1);
    cache.epoch
}

/// Stable hash of context for the runtime chip cache (not cryptographic).
pub fn context_chip_hash(ctx: &GamedevContext) -> u64 {
    let bytes = serde_json::to_vec(ctx).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    hasher.finish()
}

/// Include buffer generation so chip cache invalidates with pixels.
pub fn context_result_chip_hash(ctx: &GamedevContext, result_gen: u64) -> u64 {
    let mut h = context_chip_hash(ctx);
    h ^= result_gen.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h
}

/// Relative seam fail → Warn (honest `≈`); Exact fail → Error.
pub fn chip_state_for_seam_report(report: &SeamReport) -> ChipState {
    if report.ok {
        return ChipState::Ok;
    }
    match report.mode {
        SeamMode::Exact => ChipState::Error,
        SeamMode::Relative => ChipState::Warn,
    }
}

/// Short seamless label from a ring report.
pub fn seamless_label_for_report(report: &SeamReport) -> String {
    if report.ok {
        return match report.mode {
            SeamMode::Exact => "seamless ✓".into(),
            SeamMode::Relative => {
                let r = worse_axis_ratio(report).unwrap_or(0.0);
                format!("seamless ≈ {r:.2}×")
            }
        };
    }
    match report.mode {
        SeamMode::Exact => "seam · mismatch".into(),
        SeamMode::Relative => {
            let (axis, r) = worse_axis_detail(report);
            format!("seam ≈ {r:.1}× {axis}")
        }
    }
}

fn worse_axis_ratio(report: &SeamReport) -> Option<f64> {
    let hr = report.horizontal.ratio.unwrap_or(0.0);
    let vr = report.vertical.ratio.unwrap_or(0.0);
    Some(hr.max(vr))
}

fn worse_axis_detail(report: &SeamReport) -> (&'static str, f64) {
    let hr = report.horizontal.ratio.unwrap_or(report.horizontal.r_boundary);
    let vr = report.vertical.ratio.unwrap_or(report.vertical.r_boundary);
    if hr >= vr {
        ("H", hr)
    } else {
        ("V", vr)
    }
}

fn chip(
    state: ChipState,
    label: impl Into<String>,
    tooltip: impl Into<String>,
    action_label: Option<&str>,
    action_id: Option<&str>,
) -> GamedevChip {
    GamedevChip {
        id: GamedevChipId::Seamless,
        state,
        label: label.into(),
        tooltip: tooltip.into(),
        action_label: action_label.map(str::to_string),
        action_id: action_id.map(str::to_string),
    }
}

/// Chip when Tile needs a 9× mosaic but the canvas is over the interactive limit.
pub fn seamless_chip_memory_gate(width: u32, height: u32) -> GamedevChip {
    chip(
        ChipState::Error,
        "tile · too large",
        format!(
            "Tile needs more memory than allowed for interactive preview \
             (limit {PIPELINE_TILE_MAX_SOURCE_SIDE}×{PIPELINE_TILE_MAX_SOURCE_SIDE}; \
             this canvas is {width}×{height}). Shrink the canvas, turn Tile off, \
             or use Approximate wrap-padding under Tiling advanced."
        ),
        Some("Disable Tile"),
        Some(chip_action::DISABLE_TILE),
    )
}

/// Period-mismatch warn (ordered / BN size not a multiple of the lattice).
pub fn seamless_chip_period_snap(nw: u32, nh: u32, width: u32, height: u32) -> GamedevChip {
    chip(
        ChipState::Warn,
        format!("seam · snap {nw}×{nh}"),
        format!(
            "Canvas {width}×{height} is not a multiple of the pattern period. \
             Crop to {nw}×{nh} (top-left) then re-validate."
        ),
        Some("Crop to period"),
        Some(chip_action::SNAP_TO_PERIOD),
    )
}

/// Build the seamless chip from Tile + optional ring report (no period heuristic).
pub fn seamless_chip_from_tile(
    tile_enabled: bool,
    width: u32,
    height: u32,
    report: Option<&SeamReport>,
) -> GamedevChip {
    if tile_enabled && !pipeline_tile_fits_limits(width, height) {
        return seamless_chip_memory_gate(width, height);
    }
    let Some(report) = report else {
        return chip(
            ChipState::Pending,
            "…",
            "Seam validation pending.",
            None,
            None,
        );
    };
    let state = chip_state_for_seam_report(report);
    let label = seamless_label_for_report(report);
    let tooltip = format!(
        "H: Rb={:.4} Ri={:.4}; V: Rb={:.4} Ri={:.4}; k={} ε={}",
        report.horizontal.r_boundary,
        report.horizontal.r_internal,
        report.vertical.r_boundary,
        report.vertical.r_internal,
        report.k,
        report.epsilon
    );
    let (action_label, action_id) = if state == ChipState::Ok {
        (None, None)
    } else if report.mode == SeamMode::Relative {
        (
            Some("Use wrap-padding"),
            Some(chip_action::ENABLE_WRAP_PADDING),
        )
    } else {
        (Some("Crop to period"), Some(chip_action::SNAP_TO_PERIOD))
    };
    chip(state, label, tooltip, action_label, action_id)
}

/// Full seamless chip: memory → period snap → ring report.
pub fn evaluate_seamless_chip(
    layer: &Layer,
    tile_enabled: bool,
    width: u32,
    height: u32,
    report: Option<&SeamReport>,
) -> GamedevChip {
    // Layer-aware: period-aligned ordered/BN that skips mosaic is not size-gated.
    if pipeline_tile_memory_gate_error(layer, tile_enabled, width, height).is_some() {
        return seamless_chip_memory_gate(width, height);
    }
    // Heuristic period warn takes priority over Exact ring fail (actionable snap).
    if let Some((nw, nh)) = period_snap_crop_size(layer, width, height) {
        return seamless_chip_period_snap(nw, nh, width, height);
    }
    seamless_chip_from_tile(tile_enabled, width, height, report)
}

/// Cells chip from `GamedevContext.cell` + canvas size.
pub fn evaluate_cells_chip(
    ctx: &GamedevContext,
    width: u32,
    height: u32,
) -> GamedevChip {
    let Some(cell) = ctx.cell.as_ref() else {
        return GamedevChip {
            id: GamedevChipId::Cells,
            state: ChipState::Ok,
            label: "cells · off".into(),
            tooltip: "Sprites are off. Turn on Sprites in Gamedev Settings to set cell size."
                .into(),
            action_label: None,
            action_id: None,
        };
    };
    if !cell.independent {
        return GamedevChip {
            id: GamedevChipId::Cells,
            state: ChipState::Warn,
            label: "cells · shared".into(),
            tooltip: "Keep cells separate is off — neighboring cells share one dither pass."
                .into(),
            action_label: None,
            action_id: None,
        };
    }
    if let Some((nw, nh)) = cell_grid_crop_size(width, height, cell) {
        return GamedevChip {
            id: GamedevChipId::Cells,
            state: ChipState::Warn,
            label: format!("cells · crop {nw}×{nh}"),
            tooltip: format!(
                "Canvas {width}×{height} is not a multiple of cell {}×{}. \
                 Crop top-left to {nw}×{nh}.",
                cell.w, cell.h
            ),
            action_label: Some("Crop to grid".into()),
            action_id: Some(chip_action::CROP_TO_CELL_GRID.into()),
        };
    }
    GamedevChip {
        id: GamedevChipId::Cells,
        state: ChipState::Ok,
        label: "cells independent ✓".into(),
        tooltip: format!(
            "Each {}×{} cell is dithered on its own. Preview and export use the same result.",
            cell.w, cell.h
        ),
        action_label: None,
        action_id: None,
    }
}

/// Build the gamedev chip strip (seamless + cells + stable + alpha).
pub fn evaluate_gamedev_chips(
    layer: &Layer,
    ctx: &GamedevContext,
    width: u32,
    height: u32,
    report: Option<&SeamReport>,
) -> Vec<GamedevChip> {
    evaluate_gamedev_chips_with_result(layer, ctx, width, height, report, None)
}

/// Same as [`evaluate_gamedev_chips`] with optional processed RGBA8 for alpha chip.
pub fn evaluate_gamedev_chips_with_result(
    layer: &Layer,
    ctx: &GamedevContext,
    width: u32,
    height: u32,
    report: Option<&SeamReport>,
    result_rgba8: Option<&[u8]>,
) -> Vec<GamedevChip> {
    let period_issue = period_snap_crop_size(layer, width, height).is_some();
    let seamless = if ctx.tile.enabled || report.is_some() || period_issue {
        evaluate_seamless_chip(layer, ctx.tile.enabled, width, height, report)
    } else {
        chip(
            ChipState::Ok,
            "seamless · off",
            "Tile is off. Turn on Tile in Gamedev Settings to make edges match. \
             Check seams in the preview footer only displays the current result.",
            None,
            None,
        )
    };
    vec![
        seamless,
        evaluate_cells_chip(ctx, width, height),
        evaluate_stable_chip(layer, ctx),
        evaluate_alpha_chip(ctx, result_rgba8),
    ]
}

/// Alpha chip: warn on soft edges; action thresholds per `GamedevContext.alpha`.
pub fn evaluate_alpha_chip(ctx: &GamedevContext, result_rgba8: Option<&[u8]>) -> GamedevChip {
    let soft = result_rgba8
        .map(has_semi_transparent_rgba8)
        .unwrap_or(false);
    if soft {
        let n = result_rgba8
            .map(count_semi_transparent_rgba8)
            .unwrap_or(0);
        return GamedevChip {
            id: GamedevChipId::Alpha,
            state: ChipState::Warn,
            label: format!("alpha · soft · {n}"),
            tooltip: format!(
                "{n} semi-transparent pixels. Threshold alpha flattens soft edges \
                 at {:.0}%.",
                ctx.alpha.threshold * 100.0,
            ),
            action_label: Some("Threshold alpha".into()),
            action_id: Some(chip_action::THRESHOLD_ALPHA.into()),
        };
    }
    GamedevChip {
        id: GamedevChipId::Alpha,
        state: ChipState::Ok,
        label: if ctx.alpha.no_dither_alpha {
            "alpha ✓".into()
        } else {
            "alpha · dither".into()
        },
        tooltip: if ctx.alpha.no_dither_alpha {
            "No soft (semi-transparent) edges. Alpha is not dithered."
                .into()
        } else {
            "No soft (semi-transparent) edges.".into()
        },
        action_label: None,
        action_id: None,
    }
}

/// Stable chip from filters + animation context (F3).
pub fn evaluate_stable_chip(layer: &Layer, ctx: &GamedevContext) -> GamedevChip {
    let frame_job = ctx.job == super::context::GamedevJob::Animation || !ctx.frame_set.is_empty();
    let stable_filters = filters_are_frame_stable(&layer.filters);
    let locked = should_lock_pattern_phase(ctx);

    if !frame_job && stable_filters {
        return GamedevChip {
            id: GamedevChipId::Stable,
            state: ChipState::Ok,
            label: "stable · ready".into(),
            tooltip: "Pattern is frame-stable. Enable Animation / add a frame set to apply to folders."
                .into(),
            action_label: None,
            action_id: None,
        };
    }

    if stable_filters && locked {
        let n = ctx.frame_set.len();
        return GamedevChip {
            id: GamedevChipId::Stable,
            state: ChipState::Ok,
            label: if n > 0 {
                format!("stable ✓ · {n}")
            } else {
                "stable ✓".into()
            },
            tooltip: "Ordered/blue-noise with fixed phase — identical frames produce identical bytes."
                .into(),
            action_label: None,
            action_id: None,
        };
    }

    if !stable_filters {
        return GamedevChip {
            id: GamedevChipId::Stable,
            state: ChipState::Warn,
            label: "stable · flicker".into(),
            tooltip: "Error diffusion may flicker between frames. Switch to ordered/blue noise with a fixed mask."
                .into(),
            action_label: Some("Switch to frame-stable".into()),
            action_id: Some(chip_action::SWITCH_TO_FRAME_STABLE.into()),
        };
    }

    GamedevChip {
        id: GamedevChipId::Stable,
        state: ChipState::Warn,
        label: "stable · unlock".into(),
        tooltip: "Fix the threshold mask phase (Animation job or MaskPolicy::Fixed) for locked phase across frames."
            .into(),
        action_label: Some("Switch to frame-stable".into()),
        action_id: Some(chip_action::SWITCH_TO_FRAME_STABLE.into()),
    }
}

/// Legacy stub when no layer/pixels available.
pub fn chip_stub_for_context(ctx: &GamedevContext) -> Vec<GamedevChip> {
    let seamless = if ctx.tile.enabled {
        chip(
            ChipState::Pending,
            "…",
            "Tile on — awaiting ring validation.",
            None,
            None,
        )
    } else {
        chip(
            ChipState::Ok,
            "seamless · off",
            "Tile is off. Turn it on in Gamedev Settings to make edges match.",
            None,
            None,
        )
    };
    // No layer in stub path — pending/off stable + alpha.
    let stable = GamedevChip {
        id: GamedevChipId::Stable,
        state: ChipState::Ok,
        label: "stable · off".into(),
        tooltip: "Attach a document to validate frame stability.".into(),
        action_label: None,
        action_id: None,
    };
    let alpha = evaluate_alpha_chip(ctx, None);
    vec![seamless, evaluate_cells_chip(ctx, 0, 0), stable, alpha]
}

/// Pending placeholders while a real check runs.
pub fn chip_stub_pending() -> Vec<GamedevChip> {
    stub_chips(ChipState::Pending, "…", "Validation pending.")
}

/// Shown when async validation fails (not an infinite spinner).
pub fn chip_stub_validation_failed(reason: &str) -> Vec<GamedevChip> {
    stub_chips(
        ChipState::Warn,
        "check failed",
        &format!("Gamedev validation failed: {reason}"),
    )
}

fn stub_chips(state: ChipState, label: &str, tooltip: &str) -> Vec<GamedevChip> {
    [
        GamedevChipId::Seamless,
        GamedevChipId::Cells,
        GamedevChipId::Stable,
        GamedevChipId::Alpha,
    ]
    .into_iter()
    .map(|id| GamedevChip {
        id,
        state,
        label: label.into(),
        tooltip: tooltip.into(),
        action_label: None,
        action_id: None,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{
        DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
    };
    use crate::types::{LayerId, LayerKind};

    fn bayer_layer(w: u32, h: u32) -> Layer {
        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer8x8,
                pixel_size: 1,
                ..DitherParamsV2::default()
            }),
        ));
        layer
    }

    #[test]
    fn stub_route_pending_then_ok() {
        let pending = chip_stub_pending();
        assert!(pending.iter().all(|c| c.state == ChipState::Pending));
        let ctx = GamedevContext::default();
        let done = chip_stub_for_context(&ctx);
        assert_eq!(done[0].state, ChipState::Ok);
        let key = context_chip_hash(&ctx);
        let mut cache = ChipCache::new();
        chip_cache_put(&mut cache, key, done.clone());
        assert_eq!(chip_cache_get(&cache, key), Some(done.as_slice()));
    }

    #[test]
    fn relative_fail_is_warn_not_error() {
        use crate::gamedev::seams::{AxisSeamStats, SEAM_EPSILON, SEAM_K};
        let report = SeamReport {
            mode: SeamMode::Relative,
            width: 32,
            height: 32,
            horizontal: AxisSeamStats {
                r_boundary: 0.5,
                r_internal: 0.1,
                ratio: Some(5.0),
                ok: false,
            },
            vertical: AxisSeamStats {
                r_boundary: 0.0,
                r_internal: 0.1,
                ratio: Some(0.0),
                ok: true,
            },
            ok: false,
            k: SEAM_K,
            epsilon: SEAM_EPSILON,
        };
        assert_eq!(chip_state_for_seam_report(&report), ChipState::Warn);
        let chip = seamless_chip_from_tile(true, 32, 32, Some(&report));
        assert_eq!(chip.state, ChipState::Warn);
        assert_eq!(chip.action_id.as_deref(), Some(chip_action::ENABLE_WRAP_PADDING));
    }

    #[test]
    fn memory_gate_chip_is_error() {
        let chip = seamless_chip_from_tile(true, 3000, 3000, None);
        assert_eq!(chip.state, ChipState::Error);
        assert_eq!(chip.label, "tile · too large");
        assert_eq!(chip.action_id.as_deref(), Some(chip_action::DISABLE_TILE));
    }

    #[test]
    fn period_mismatch_warns_with_snap() {
        let layer = bayer_layer(33, 32);
        let chip = evaluate_seamless_chip(&layer, true, 33, 32, None);
        assert_eq!(chip.state, ChipState::Warn);
        assert!(chip.label.contains("snap"));
        assert_eq!(chip.action_id.as_deref(), Some(chip_action::SNAP_TO_PERIOD));
    }

    #[test]
    fn period_aligned_skips_snap_chip() {
        use super::super::pipeline_tile::period_snap_crop_size;
        let layer = bayer_layer(32, 32);
        assert!(period_snap_crop_size(&layer, 32, 32).is_none());
        let chip = evaluate_seamless_chip(&layer, false, 32, 32, None);
        assert_eq!(chip.state, ChipState::Pending);
    }
}
