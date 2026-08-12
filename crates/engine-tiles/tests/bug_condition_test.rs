//! Bug Condition Exploration Test — Raster-Order Enqueue
//!
//! **Validates: Requirements 1.2, 1.3**
//!
//! This test verifies that tile grids produced in row-major order (as
//! `compute_visible_tiles` does) are NOT already sorted in center-out
//! manhattan distance order.
//!
//! On UNFIXED code, this test FAILS because tiles are in raster order
//! (which is NOT center-out). The test asserts they ARE in center-out
//! order — that assertion fails, confirming the bug exists.
//!
//! DO NOT fix the code or the test when it fails — failure confirms the bug.

use engine_tiles::TileCoord;
use proptest::prelude::*;

/// Simulate what `compute_visible_tiles` produces: a rectangular grid of
/// TileCoords in row-major (raster) order, just like the nested
/// `for ty { for tx { ... } }` loops in viewport.rs.
fn generate_raster_order_grid(width: u32, height: u32) -> Vec<TileCoord> {
    let mut tiles = Vec::new();
    for ty in 0..height {
        for tx in 0..width {
            tiles.push(TileCoord { level: 0, x: tx, y: ty });
        }
    }
    tiles
}

/// Compute manhattan distance from a tile to the grid center.
fn manhattan_distance_from_center(coord: &TileCoord, tiles: &[TileCoord]) -> f64 {
    let min_x = tiles.iter().map(|t| t.x).min().unwrap();
    let max_x = tiles.iter().map(|t| t.x).max().unwrap();
    let min_y = tiles.iter().map(|t| t.y).min().unwrap();
    let max_y = tiles.iter().map(|t| t.y).max().unwrap();

    let center_x = (min_x + max_x) as f64 / 2.0;
    let center_y = (min_y + max_y) as f64 / 2.0;

    (coord.x as f64 - center_x).abs() + (coord.y as f64 - center_y).abs()
}

/// Check if a sequence of tiles is in non-decreasing manhattan distance
/// from the grid center (i.e., center-out order).
fn is_center_out_ordered(tiles: &[TileCoord]) -> bool {
    if tiles.len() <= 1 {
        return true;
    }

    for i in 0..tiles.len() - 1 {
        let dist_i = manhattan_distance_from_center(&tiles[i], tiles);
        let dist_next = manhattan_distance_from_center(&tiles[i + 1], tiles);
        if dist_i > dist_next {
            return false;
        }
    }
    true
}

proptest! {
    /// Property: For any tile grid with >1 tile generated in raster order,
    /// the tiles should already be in center-out manhattan distance order.
    ///
    /// This FAILS on unfixed code because raster order ≠ center-out order
    /// for any non-trivial grid (width > 1 AND height > 1).
    #[test]
    fn raster_order_grid_is_center_out_ordered(
        width in 2u32..=10,
        height in 2u32..=10,
    ) {
        let tiles = generate_raster_order_grid(width, height);

        // Assert that raster-order tiles are in center-out order.
        // On UNFIXED code, this will FAIL because raster order starts at (0,0)
        // which is a corner, not the center.
        prop_assert!(
            is_center_out_ordered(&tiles),
            "Grid {}x{} is in raster order, NOT center-out. \
             First tile ({},{}) is a corner, not the center tile.",
            width, height, tiles[0].x, tiles[0].y
        );
    }
}
