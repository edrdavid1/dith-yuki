//! Gamedev workspace document context, chip stubs, and cache-invalidation contract.
//!
//! Spec: `.local-doc/gamedev-2.md` (Foundation). Pipeline Tile / ring checks live in later
//! phases — this module only persists context and classifies invalidation.

mod chips;
mod cli_apply;
mod context;
mod invalidation;
mod pipeline_alpha;
mod pipeline_cells;
mod pipeline_frames;
mod pipeline_tile;
mod seams;

pub use chips::{
    chip_action, chip_cache_begin_job, chip_cache_epoch, chip_cache_get, chip_cache_invalidate,
    chip_cache_put,
    chip_state_for_seam_report, chip_stub_for_context, chip_stub_pending,
    chip_stub_validation_failed, context_chip_hash, context_result_chip_hash,
    evaluate_alpha_chip, evaluate_cells_chip, evaluate_gamedev_chips,
    evaluate_gamedev_chips_with_result, evaluate_seamless_chip, evaluate_stable_chip,
    seamless_chip_from_tile,
    seamless_chip_memory_gate, seamless_chip_period_snap, seamless_label_for_report, ChipCache,
    ChipState, GamedevChip, GamedevChipId,
};
pub use cli_apply::{
    parse_cell_wh, resolve_cli_preset, run_cli_apply, CliApplyError, CliApplyOptions,
};
pub use context::{
    gamedev_extra_key, lift_gamedev_from_extra, AlphaPolicy, CellSettings, ExportDefaults,
    GamedevContext, GamedevJob, MaskKind, MaskPolicy, PaletteSource, PixelPerfect, TileSettings,
    GAMEDEV_EXTRA_KEY,
};
pub use invalidation::{gamedev_cache_invalidation, GamedevCacheLayer};
pub use pipeline_alpha::{
    apply_alpha_policy_rgba8, clear_transparent_rgb_rgba8, clear_transparent_rgb_rgba_f32,
    count_semi_transparent_rgba8, effective_upscale, has_semi_transparent_rgba8,
    integer_nearest_upscale_rgba8, integer_nearest_upscale_rgba_f32, nearest_downsample_rgba_f32,
    pixel_perfect_active, resolve_pixel_perfect_work_size, threshold_alpha_rgba8,
    threshold_alpha_rgba_f32,
};
pub use pipeline_cells::{
    cell_grid_crop_size, layer_needs_independent_cells, process_independent_cells_rgba8,
    process_independent_cells_rgba_f32, CellPipelineError, CellPipelineMetrics, CellResultCache,
    CELL_PIPELINE_METRICS,
};
pub use pipeline_frames::{
    apply_frame_stable_context, collect_frame_paths, dither_frame_rgba8, filters_are_frame_stable,
    identical_frames_bitwise_equal, mode_is_frame_stable, process_frame_set_to_folder,
    resolve_frame_palettes, should_lock_pattern_phase, switch_filters_to_frame_stable,
    FramePipelineError, FramePipelineMetrics, FrameSetReport, FRAME_PIPELINE_METRICS,
};
pub use pipeline_tile::{
    assemble_mosaic_rgba_f32, begin_pipeline_tile_pass, clear_wrap_padding_in_tree,
    clear_wrap_padding_on_layer, crop_center_rgba_f32, dither_period_aligned,
    finish_pipeline_tile_pass, layer_combined_period_px, layer_needs_pipeline_tile_full_document,
    layer_wants_ordered_pipeline_tile, layer_wants_pipeline_tile, mosaic_pixel_count,
    period_snap_crop_size, pipeline_tile_fits_limits, pipeline_tile_memory_blocked,
    pipeline_tile_memory_gate_error, seam_mode_for_layer, should_skip_mosaic, PipelineTileError,
    PipelineTileSession, PIPELINE_TILE_MAX_MOSAIC_PIXELS, PIPELINE_TILE_MAX_SOURCE_SIDE,
};
pub use seams::{
    pixels_match_index, pixels_match_rgba8, validate_seams, validate_seams_indexed,
    validate_seams_rgba8, AxisSeamStats, SeamMode, SeamPixelFormat, SeamReport, SeamValidateError,
    SEAM_EPSILON, SEAM_K,
};
