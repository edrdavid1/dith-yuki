//! Document model, layer hierarchy, filter pipeline, and mask system.
//!
//! This module implements the application-level data structures that bridge
//! the UI with the Phase 1 tile engine. It provides:
//!
//! - Document: Main project structure
//! - Layer & LayerGroup: Recursive layer hierarchy
//! - FilterInstance: Filter stack with parameters
//! - MaskRef: Alpha masks (via external layers)
//! - DocumentHandle: Thread-safe access via arc-swap
//! - Invalidation: Cache coordination
//! - Commands: Document mutation operations

pub mod algorithms;
pub mod commands;
pub mod compositor;
pub mod dither_profile;
pub mod document;
pub mod dto;
pub mod error;
pub mod filter;
pub mod filters;
pub mod headless;
pub mod invalidation;
pub mod layer;
pub mod mask;
pub mod palette_gen;
pub mod pixel_pool;
pub mod post_barrier;
pub mod post_barrier_metrics;
pub mod post_cache;
pub mod seamless;
pub mod serialize;
pub mod simd;
pub mod sprite_sheet;
pub mod types;

// Public API re-exports
pub use commands::{add_layer, remove_layer, reorder_layer, set_layer_props, LayerPropsPatch};
pub use compositor::{blend_tile, composite_tile};
pub use document::{Document, DocumentHandle};
pub use error::EngineError;
pub use filter::{
    algorithm_id_for_params, filter_kind_for_algorithm_id, filter_params_to_json,
    resolve_algorithm_id, DiffusionKernel, DitherMode, FilterInstance, FilterKind, FilterParams,
    PlaceholderParams,
};
pub use filters::neutral::filter_is_neutral_for_apply;
pub use filters::FilterContext;
pub use headless::{
    expand_output_name, palette_from_srgb, run_headless_batch, run_headless_job,
    run_headless_job_cancellable, HeadlessError, HeadlessJob, HeadlessJobResult,
    HeadlessOutputFormat,
};
pub use invalidation::{
    invalidate_layer_filter_changed, invalidate_layer_props_changed,
    invalidate_layer_structure_changed, validate_document_consistency,
};
pub use layer::{
    flatten_bottom_to_top, walk_bottom_to_top, Layer, LayerGroup, LayerNode, LayerRef,
};
pub use mask::{apply_mask, MaskRef, MaskStorage};
pub use palette_gen::{generate_palette_from_layer, generate_palette_from_layer_weighted};
pub use post_barrier::{
    barrier_index, classify_change, cost_class_for_instance, cost_class_for_params,
    ensure_filter_ids, filter_fingerprint, material_stack_unchanged, post_region_indices,
    prefix_externals_for_layer, prefix_hash_through_barrier, split_enabled_at_barrier,
    stack_edit_bump, stack_hash, ChangeClass, CostClass, PrefixExternals, StackEditBump,
};
pub use post_barrier_metrics::{
    filter_index as post_filter_index, is_post_cancelled, note_stack_change, post_cancelled,
    record_clear as record_post_clear, record_ed_run_started, record_post_tile_cancelled,
    record_post_tile_discarded_after_compute, snapshot as post_barrier_counters,
    PostBarrierCounters, POST_CANCELLED_REASON,
};
pub use post_cache::{
    init_from_env as init_post_cache_from_env, neutral_verify_enabled, post_cache_enabled,
    post_verify_enabled, refresh_policy_for_change, set_neutral_verify_enabled_for_test,
    set_post_cache_enabled_for_test, set_post_verify_enabled_for_test,
};
pub use seamless::{
    check_seamless, nearest_multiples, ordered_matrix_side, pattern_angle_breaks_periodicity,
    pattern_period_px, seamless_warning_lines, SeamlessKind, SeamlessReport,
};
pub use simd::{
    blend_row_scalar, blend_row_simd, f32_to_rgba8_row_scalar, f32_to_rgba8_row_simd,
    levels_row_scalar, levels_row_simd,
};
pub use sprite_sheet::{process_sprite_sheet_rgba_f32, rgba8_to_f32, SpriteSheetError};
pub use types::{
    BlendMode, ColorProfileRef, DocumentId, FilterInstanceId, LayerId, LayerKind, PaletteId,
    TileBounds,
};

/// Library version
pub const VERSION: &str = "0.1.0";
