//! Viewport management for tile-based rendering.
//!
//! This module implements the viewport state tracking and tile coordinate computation
//! for the viewport-driven rendering pipeline. It determines which tiles are visible
//! at the current zoom/pan state and which tiles should be prefetched for smooth panning.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use engine_tiles::{
    CacheStage, Priority, RecomputeTask, TileCoord, TileKey, TILE_SIZE,
};

use crate::commands::AppState;

// ============================================================================
// Data Structures
// ============================================================================

/// Current viewport state for priority decisions.
///
/// Tracks the viewport rectangle the user is currently viewing, along with
/// the computed pyramid level and tile sets. Used by the scheduler to prioritize
/// tile recomputation and by eviction to preserve visible tiles.
#[derive(Debug, Clone)]
pub struct ViewportState {
    /// Current zoom factor (1.0 = 100%).
    pub zoom: f64,
    /// Document-space X of viewport top-left.
    pub x: f64,
    /// Document-space Y of viewport top-left.
    pub y: f64,
    /// Viewport width in screen pixels.
    pub width: f64,
    /// Viewport height in screen pixels.
    pub height: f64,
    /// Computed pyramid level for the current zoom.
    pub level: u8,
    /// Tile coordinates visible in the current viewport.
    pub visible_tiles: Vec<TileCoord>,
    /// Tile coordinates in the prefetch ring (one tile wide, adjacent to viewport).
    pub prefetch_tiles: Vec<TileCoord>,
}

impl Default for ViewportState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            x: 0.0,
            y: 0.0,
            width: 800.0,
            height: 600.0,
            level: 0,
            visible_tiles: Vec::new(),
            prefetch_tiles: Vec::new(),
        }
    }
}

/// Response from the `set_viewport` command.
#[derive(Debug, Clone, Serialize)]
pub struct SetViewportResponse {
    /// The computed pyramid level for the given zoom.
    pub level: u8,
    /// Number of visible tiles at this viewport.
    pub tile_count: usize,
}

// ============================================================================
// Pure Computation Functions
// ============================================================================

/// Compute pyramid level: max(0, floor(log2(1.0 / zoom))), clamped to max_level.
///
/// At zoom >= 1.0, returns level 0 (full resolution).
/// At zoom < 1.0, picks a coarser level so that tiles cover more document area.
///
/// # Arguments
///
/// * `zoom` - Current zoom factor (e.g., 0.5 means zoomed out to 50%)
/// * `max_level` - Maximum pyramid level available for the document
///
/// # Returns
///
/// The pyramid level to use, in range [0, max_level].
pub fn compute_pyramid_level(zoom: f64, max_level: u8) -> u8 {
    if zoom >= 1.0 {
        return 0;
    }
    let level = (1.0 / zoom).log2().floor() as u8;
    level.min(max_level)
}

/// Compute the maximum pyramid level for a given document size.
///
/// The max level is determined by how many times we can halve the tile grid
/// before it becomes a single tile or smaller.
pub fn compute_max_level(doc_width: u32, doc_height: u32) -> u8 {
    let max_dim = doc_width.max(doc_height);
    if max_dim <= TILE_SIZE {
        return 0;
    }
    // How many times can we divide max_dim by TILE_SIZE and still have meaningful data?
    // At level L, each tile covers TILE_SIZE * 2^L pixels.
    // Max level is floor(log2(max_dim / TILE_SIZE))
    let ratio = max_dim as f64 / TILE_SIZE as f64;
    ratio.log2().floor().max(0.0) as u8
}

/// Compute visible tile coordinates for a viewport at a given pyramid level.
///
/// Divides the viewport rectangle (in document pixels) by the tile size at the
/// given level, and clamps to the document's tile grid bounds.
///
/// # Arguments
///
/// * `zoom` - Current zoom factor
/// * `x` - Document-space X of viewport top-left
/// * `y` - Document-space Y of viewport top-left
/// * `width` - Viewport width in screen pixels
/// * `height` - Viewport height in screen pixels
/// * `level` - Pyramid level to compute tiles at
/// * `doc_width` - Document width in pixels
/// * `doc_height` - Document height in pixels
///
/// # Returns
///
/// A vector of TileCoords that are visible in the viewport.
pub fn compute_visible_tiles(
    zoom: f64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    level: u8,
    doc_width: u32,
    doc_height: u32,
) -> Vec<TileCoord> {
    let scale = 1u32 << level;
    let tile_size_at_level = (TILE_SIZE * scale) as f64;

    // Viewport bounds in document pixels
    let vp_left = x;
    let vp_top = y;
    let vp_right = x + width / zoom;
    let vp_bottom = y + height / zoom;

    // Convert to tile indices at this level
    let min_tx = (vp_left / tile_size_at_level).floor().max(0.0) as u32;
    let min_ty = (vp_top / tile_size_at_level).floor().max(0.0) as u32;
    let max_tx = (vp_right / tile_size_at_level).ceil() as u32;
    let max_ty = (vp_bottom / tile_size_at_level).ceil() as u32;

    // Clamp to grid bounds at this level
    let grid_cols = (doc_width + TILE_SIZE * scale - 1) / (TILE_SIZE * scale);
    let grid_rows = (doc_height + TILE_SIZE * scale - 1) / (TILE_SIZE * scale);

    let mut tiles = Vec::new();
    for ty in min_ty..max_ty.min(grid_rows) {
        for tx in min_tx..max_tx.min(grid_cols) {
            tiles.push(TileCoord { level, x: tx, y: ty });
        }
    }
    tiles
}

/// Compute the one-tile-wide prefetch ring adjacent to the visible tiles.
///
/// The prefetch ring consists of all tiles that are one step outside the bounding
/// box of the visible tiles, but still within the document grid bounds. This enables
/// smooth panning by pre-loading tiles the user is about to scroll into.
///
/// # Arguments
///
/// * `visible` - The set of currently visible tile coordinates
/// * `level` - Pyramid level of the visible tiles
/// * `doc_width` - Document width in pixels
/// * `doc_height` - Document height in pixels
///
/// # Returns
///
/// A vector of TileCoords forming the prefetch ring. Returns empty if `visible` is empty.
pub fn compute_prefetch_ring(
    visible: &[TileCoord],
    level: u8,
    doc_width: u32,
    doc_height: u32,
) -> Vec<TileCoord> {
    if visible.is_empty() {
        return Vec::new();
    }

    // Find bounding box of visible tiles
    let min_x = visible.iter().map(|t| t.x).min().unwrap();
    let max_x = visible.iter().map(|t| t.x).max().unwrap();
    let min_y = visible.iter().map(|t| t.y).min().unwrap();
    let max_y = visible.iter().map(|t| t.y).max().unwrap();

    // Grid bounds at this level
    let scale = 1u32 << level;
    let grid_cols = (doc_width + TILE_SIZE * scale - 1) / (TILE_SIZE * scale);
    let grid_rows = (doc_height + TILE_SIZE * scale - 1) / (TILE_SIZE * scale);

    // Expand bounding box by one tile in each direction
    let ring_min_x = if min_x > 0 { min_x - 1 } else { 0 };
    let ring_min_y = if min_y > 0 { min_y - 1 } else { 0 };
    let ring_max_x = (max_x + 1).min(grid_cols.saturating_sub(1));
    let ring_max_y = (max_y + 1).min(grid_rows.saturating_sub(1));

    // Collect all tiles in the expanded box that are NOT in the visible set
    let mut ring = Vec::new();
    for ty in ring_min_y..=ring_max_y {
        for tx in ring_min_x..=ring_max_x {
            // Skip tiles that are already visible
            let is_visible = tx >= min_x && tx <= max_x && ty >= min_y && ty <= max_y;
            if !is_visible {
                // Ensure within grid bounds
                if tx < grid_cols && ty < grid_rows {
                    ring.push(TileCoord { level, x: tx, y: ty });
                }
            }
        }
    }
    ring
}

// ============================================================================
// Priority Classification
// ============================================================================

/// Classify tile priority based on position relative to viewport center.
///
/// Inner 50% of viewport area → ViewportCenter, outer 50% → ViewportEdge.
/// This ensures tiles near the center of the user's view are loaded first.
///
/// # Arguments
///
/// * `coord` - The tile coordinate to classify
/// * `visible` - The set of currently visible tile coordinates
///
/// # Returns
///
/// `Priority::ViewportCenter` if the tile is in the inner 50% of the viewport,
/// `Priority::ViewportEdge` otherwise.
pub fn classify_priority(coord: &TileCoord, visible: &[TileCoord]) -> Priority {
    if visible.is_empty() {
        return Priority::ViewportEdge;
    }

    let min_x = visible.iter().map(|t| t.x).min().unwrap();
    let max_x = visible.iter().map(|t| t.x).max().unwrap();
    let min_y = visible.iter().map(|t| t.y).min().unwrap();
    let max_y = visible.iter().map(|t| t.y).max().unwrap();

    // Viewport center in tile coordinates
    let cx = (min_x + max_x) as f64 / 2.0;
    let cy = (min_y + max_y) as f64 / 2.0;

    // Inner 50% = 25% on each side of center
    let width = (max_x - min_x + 1) as f64;
    let height = (max_y - min_y + 1) as f64;
    let half_w = width * 0.25;
    let half_h = height * 0.25;

    let dx = (coord.x as f64 - cx).abs();
    let dy = (coord.y as f64 - cy).abs();

    if dx <= half_w && dy <= half_h {
        Priority::ViewportCenter
    } else {
        Priority::ViewportEdge
    }
}

/// Check if a tile needs recomputation (missing from cache or dirty).
///
/// # Arguments
///
/// * `state` - App state containing tile cache
/// * `key` - The TileKey to check
///
/// # Returns
///
/// `true` if the tile is missing or marked dirty, `false` if cached and clean.
fn needs_recompute(state: &AppState, key: &TileKey) -> bool {
    match state.tile_cache.entries.get(key) {
        None => true,
        Some(entry) => entry.dirty.load(Ordering::Acquire),
    }
}

// ============================================================================
// Tauri Command
// ============================================================================

/// Set the current viewport state.
///
/// Computes the pyramid level, visible tiles, and prefetch ring for the given
/// viewport parameters. Stores the viewport state for priority decisions.
///
/// # Arguments
///
/// * `zoom` - Zoom factor (0.01 to 64.0)
/// * `x` - Document-space X of viewport top-left
/// * `y` - Document-space Y of viewport top-left
/// * `width` - Viewport width in screen pixels
/// * `height` - Viewport height in screen pixels
///
/// # Returns
///
/// A `SetViewportResponse` with the computed pyramid level and visible tile count.
#[tauri::command]
pub fn set_viewport(
    zoom: f64,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    state: State<'_, Arc<AppState>>,
) -> Result<SetViewportResponse, String> {
    // Validate zoom range
    let zoom = zoom.clamp(0.01, 64.0);

    // Get document dimensions
    let snapshot = state.document_handle.snapshot();
    let doc_width = snapshot.width;
    let doc_height = snapshot.height;
    drop(snapshot);

    // Compute pyramid level
    let max_level = compute_max_level(doc_width, doc_height);
    let level = compute_pyramid_level(zoom, max_level);

    // Compute visible tiles
    let visible = compute_visible_tiles(zoom, x, y, width, height, level, doc_width, doc_height);

    // Compute prefetch ring
    let prefetch = compute_prefetch_ring(&visible, level, doc_width, doc_height);

    let tile_count = visible.len();

    // --- Scheduling Logic ---
    // NOTE: We do NOT call scheduler.clear_all() here because it would race with
    // schedule_dirty_viewport_tiles (called after filter/layer changes).
    // Instead, we only schedule tiles that need recomputation. The worker's
    // staleness check handles outdated tasks naturally.

    // Read current generation values for task scheduling
    let snapshot = state.document_handle.snapshot();
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);
    drop(snapshot);

    // Schedule missing/dirty visible tiles with classified priorities
    for coord in &visible {
        let key = TileKey {
            layer: 0, // Composite layer sentinel
            coord: *coord,
            stage: CacheStage::Composite,
        };
        if needs_recompute(&state, &key) {
            let priority = classify_priority(coord, &visible);
            let task = RecomputeTask {
                key,
                generation: doc_gen,
                layer_generation: 0,
                priority,
            };
            state.scheduler.enqueue(task);
        }
    }

    // Schedule missing/dirty prefetch tiles with Prefetch priority
    for coord in &prefetch {
        let key = TileKey {
            layer: 0, // Composite layer sentinel
            coord: *coord,
            stage: CacheStage::Composite,
        };
        if needs_recompute(&state, &key) {
            let task = RecomputeTask {
                key,
                generation: doc_gen,
                layer_generation: 0,
                priority: Priority::Prefetch,
            };
            state.scheduler.enqueue(task);
        }
    }

    // Store viewport state
    let new_viewport = ViewportState {
        zoom,
        x,
        y,
        width,
        height,
        level,
        visible_tiles: visible,
        prefetch_tiles: prefetch,
    };

    *state.viewport.lock().unwrap() = new_viewport;

    Ok(SetViewportResponse { level, tile_count })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- compute_pyramid_level tests ---

    #[test]
    fn pyramid_level_at_zoom_1_is_0() {
        assert_eq!(compute_pyramid_level(1.0, 5), 0);
    }

    #[test]
    fn pyramid_level_at_zoom_above_1_is_0() {
        assert_eq!(compute_pyramid_level(2.0, 5), 0);
        assert_eq!(compute_pyramid_level(64.0, 5), 0);
    }

    #[test]
    fn pyramid_level_at_zoom_0_5_is_1() {
        assert_eq!(compute_pyramid_level(0.5, 5), 1);
    }

    #[test]
    fn pyramid_level_at_zoom_0_25_is_2() {
        assert_eq!(compute_pyramid_level(0.25, 5), 2);
    }

    #[test]
    fn pyramid_level_at_zoom_0_125_is_3() {
        assert_eq!(compute_pyramid_level(0.125, 5), 3);
    }

    #[test]
    fn pyramid_level_clamped_to_max_level() {
        assert_eq!(compute_pyramid_level(0.01, 3), 3);
    }

    #[test]
    fn pyramid_level_between_powers_floors() {
        // zoom = 0.3: 1/0.3 ≈ 3.33, log2(3.33) ≈ 1.74, floor = 1
        assert_eq!(compute_pyramid_level(0.3, 5), 1);
    }

    // --- compute_max_level tests ---

    #[test]
    fn max_level_for_small_image() {
        // 256x256 = 1 tile at level 0, max_level = 0
        assert_eq!(compute_max_level(256, 256), 0);
    }

    #[test]
    fn max_level_for_512x512() {
        // 512/256 = 2, log2(2) = 1
        assert_eq!(compute_max_level(512, 512), 1);
    }

    #[test]
    fn max_level_for_8192x8192() {
        // 8192/256 = 32, log2(32) = 5
        assert_eq!(compute_max_level(8192, 8192), 5);
    }

    // --- compute_visible_tiles tests ---

    #[test]
    fn visible_tiles_full_viewport_small_doc() {
        // 512x512 doc, zoom 1.0, viewport covers entire doc
        let tiles = compute_visible_tiles(1.0, 0.0, 0.0, 512.0, 512.0, 0, 512, 512);
        // 2x2 grid at level 0
        assert_eq!(tiles.len(), 4);
        assert!(tiles.contains(&TileCoord { level: 0, x: 0, y: 0 }));
        assert!(tiles.contains(&TileCoord { level: 0, x: 1, y: 0 }));
        assert!(tiles.contains(&TileCoord { level: 0, x: 0, y: 1 }));
        assert!(tiles.contains(&TileCoord { level: 0, x: 1, y: 1 }));
    }

    #[test]
    fn visible_tiles_partial_viewport() {
        // 1024x1024 doc, zoom 1.0, viewport only shows top-left 256x256
        let tiles = compute_visible_tiles(1.0, 0.0, 0.0, 256.0, 256.0, 0, 1024, 1024);
        // Only the top-left tile
        assert_eq!(tiles.len(), 1);
        assert_eq!(tiles[0], TileCoord { level: 0, x: 0, y: 0 });
    }

    #[test]
    fn visible_tiles_at_higher_level() {
        // 1024x1024 doc, level 1 means tiles cover 512px each
        // Viewport covering full doc at level 1 = 2x2 grid
        let tiles = compute_visible_tiles(0.5, 0.0, 0.0, 512.0, 512.0, 1, 1024, 1024);
        // At level 1: grid is 2x2
        assert_eq!(tiles.len(), 4);
    }

    #[test]
    fn visible_tiles_clamp_to_grid_bounds() {
        // Viewport extends beyond document bounds
        let tiles = compute_visible_tiles(1.0, 0.0, 0.0, 2000.0, 2000.0, 0, 512, 512);
        // Grid is 2x2 at level 0 for 512x512, can't exceed that
        assert_eq!(tiles.len(), 4);
    }

    #[test]
    fn visible_tiles_empty_when_viewport_outside_doc() {
        // Viewport positioned far outside document
        let tiles = compute_visible_tiles(1.0, 10000.0, 10000.0, 800.0, 600.0, 0, 512, 512);
        assert_eq!(tiles.len(), 0);
    }

    // --- compute_prefetch_ring tests ---

    #[test]
    fn prefetch_ring_empty_for_empty_visible() {
        let ring = compute_prefetch_ring(&[], 0, 1024, 1024);
        assert!(ring.is_empty());
    }

    #[test]
    fn prefetch_ring_around_single_tile() {
        // Single visible tile at (1,1), grid is 4x4
        let visible = vec![TileCoord { level: 0, x: 1, y: 1 }];
        let ring = compute_prefetch_ring(&visible, 0, 1024, 1024);
        // Ring should be 8 tiles surrounding (1,1)
        assert_eq!(ring.len(), 8);
        assert!(ring.contains(&TileCoord { level: 0, x: 0, y: 0 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 1, y: 0 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 2, y: 0 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 0, y: 1 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 2, y: 1 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 0, y: 2 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 1, y: 2 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 2, y: 2 }));
    }

    #[test]
    fn prefetch_ring_clamped_to_grid_bounds() {
        // Visible tile at (0,0) in a 2x2 grid
        let visible = vec![TileCoord { level: 0, x: 0, y: 0 }];
        let ring = compute_prefetch_ring(&visible, 0, 512, 512);
        // Ring can only expand to the right and down (grid is 2x2)
        assert_eq!(ring.len(), 3);
        assert!(ring.contains(&TileCoord { level: 0, x: 1, y: 0 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 0, y: 1 }));
        assert!(ring.contains(&TileCoord { level: 0, x: 1, y: 1 }));
    }

    #[test]
    fn prefetch_ring_around_multiple_visible_tiles() {
        // Visible tiles form a 2x2 block at (1,1)-(2,2) in a 4x4 grid
        let visible = vec![
            TileCoord { level: 0, x: 1, y: 1 },
            TileCoord { level: 0, x: 2, y: 1 },
            TileCoord { level: 0, x: 1, y: 2 },
            TileCoord { level: 0, x: 2, y: 2 },
        ];
        let ring = compute_prefetch_ring(&visible, 0, 1024, 1024);
        // Ring is the border around the 2x2 block = (4x4 - 2x2) = 12 tiles
        assert_eq!(ring.len(), 12);
    }

    // --- classify_priority tests ---

    #[test]
    fn classify_priority_center_tile_in_large_viewport() {
        // 4x4 visible tiles: center is at (1.5, 1.5)
        // Inner 50% = half_w = 4*0.25 = 1.0, half_h = 4*0.25 = 1.0
        // A tile at (1, 1) has dx=0.5, dy=0.5 → within inner 50%
        let visible: Vec<TileCoord> = (0..4)
            .flat_map(|y| (0..4).map(move |x| TileCoord { level: 0, x, y }))
            .collect();
        let center_tile = TileCoord { level: 0, x: 1, y: 1 };
        assert_eq!(classify_priority(&center_tile, &visible), Priority::ViewportCenter);
    }

    #[test]
    fn classify_priority_edge_tile_in_large_viewport() {
        // 4x4 visible tiles: center is at (1.5, 1.5)
        // Inner 50%: half_w = 1.0, half_h = 1.0
        // A tile at (0, 0) has dx=1.5, dy=1.5 → outside inner 50%
        let visible: Vec<TileCoord> = (0..4)
            .flat_map(|y| (0..4).map(move |x| TileCoord { level: 0, x, y }))
            .collect();
        let edge_tile = TileCoord { level: 0, x: 0, y: 0 };
        assert_eq!(classify_priority(&edge_tile, &visible), Priority::ViewportEdge);
    }

    #[test]
    fn classify_priority_single_tile_viewport_is_center() {
        // Single tile: center = (0, 0), width=1, height=1
        // half_w = 0.25, half_h = 0.25
        // The tile at (0,0) has dx=0, dy=0 → within inner region
        let visible = vec![TileCoord { level: 0, x: 0, y: 0 }];
        assert_eq!(
            classify_priority(&visible[0], &visible),
            Priority::ViewportCenter
        );
    }

    #[test]
    fn classify_priority_empty_visible_returns_edge() {
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        assert_eq!(classify_priority(&coord, &[]), Priority::ViewportEdge);
    }

    #[test]
    fn classify_priority_2x2_viewport_all_center() {
        // 2x2 visible tiles: center is at (0.5, 0.5)
        // Inner 50%: half_w = 2*0.25 = 0.5, half_h = 0.5
        // All tiles have dx/dy ≤ 0.5, so all are ViewportCenter
        let visible = vec![
            TileCoord { level: 0, x: 0, y: 0 },
            TileCoord { level: 0, x: 1, y: 0 },
            TileCoord { level: 0, x: 0, y: 1 },
            TileCoord { level: 0, x: 1, y: 1 },
        ];
        for tile in &visible {
            assert_eq!(classify_priority(tile, &visible), Priority::ViewportCenter);
        }
    }

    #[test]
    fn classify_priority_6x6_viewport_corner_is_edge() {
        // 6x6 visible tiles: center = (2.5, 2.5), width=6, height=6
        // Inner 50%: half_w = 6*0.25=1.5, half_h = 1.5
        // Tile at (0, 0): dx=2.5, dy=2.5 → outside (> 1.5)
        let visible: Vec<TileCoord> = (0..6)
            .flat_map(|y| (0..6).map(move |x| TileCoord { level: 0, x, y }))
            .collect();
        let corner = TileCoord { level: 0, x: 0, y: 0 };
        assert_eq!(classify_priority(&corner, &visible), Priority::ViewportEdge);

        // Tile at (2, 2): dx=0.5, dy=0.5 → inside (≤ 1.5)
        let near_center = TileCoord { level: 0, x: 2, y: 2 };
        assert_eq!(classify_priority(&near_center, &visible), Priority::ViewportCenter);
    }
}
