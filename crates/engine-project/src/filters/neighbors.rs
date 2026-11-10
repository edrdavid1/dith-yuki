//! P3 spatial neighbors: sample across Dithered tiles for Post blur/sharpness.
//!
//! Out-of-bounds tile offsets (beyond layer `TileBounds`) are not required.
//! When an **in-bounds** neighbor is still missing, `complete` is false and
//! Adjust falls back to in-tile clamp blur; callers may re-run Processed when
//! that neighbor’s Dithered arrives.

use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{CacheStage, TileCache};
use engine_tiles::{PixelTile, TileCoord, TileKey, HALO, TILE_SIZE};

use crate::filter::{FilterInstance, FilterParams};

const TILE_FULL_SIZE: i32 = (TILE_SIZE + 2 * HALO) as i32;
/// Must match `adjust::MAX_BLUR_RADIUS` / `BLUR_PX_PER_UNIT`.
const BLUR_PX_PER_UNIT: f32 = 12.0;
const MAX_BLUR_RADIUS: i32 = 24;
const SHARP_RADIUS: i32 = 2;

thread_local! {
    static SPATIAL_NEIGHBORS: Cell<Option<*const SpatialNeighborhood>> = const { Cell::new(None) };
}

/// Run `f` with an optional neighborhood visible to Adjust via [`current_spatial_neighbors`].
pub fn with_spatial_neighbors<R>(
    neighbors: Option<&SpatialNeighborhood>,
    f: impl FnOnce() -> R,
) -> R {
    SPATIAL_NEIGHBORS.with(|cell| {
        let prev = cell.replace(neighbors.map(|n| n as *const SpatialNeighborhood));
        let out = f();
        cell.set(prev);
        out
    })
}

pub fn current_spatial_neighbors() -> Option<&'static SpatialNeighborhood> {
    SPATIAL_NEIGHBORS.with(|cell| cell.get().map(|p| unsafe { &*p }))
}

/// Fresh Dithered tiles around `center` for cross-tile spatial Post.
pub struct SpatialNeighborhood {
    pub center: TileCoord,
    /// Absolute tile `(x, y)` → Dithered (or pointwise-prepared) pixels.
    pub tiles: HashMap<(u32, u32), Arc<PixelTile>>,
    /// Every tile in the blur footprint was fresh and present.
    pub complete: bool,
    pub radius_px: i32,
}

impl SpatialNeighborhood {
    pub fn sample_rgba(&self, gx: i32, gy: i32) -> [f32; 4] {
        let ts = TILE_SIZE as i32;
        let tx = gx.div_euclid(ts);
        let ty = gy.div_euclid(ts);
        if tx < 0 || ty < 0 {
            return self.sample_clamped_edge(gx, gy);
        }
        let key = (tx as u32, ty as u32);
        let Some(tile) = self.tiles.get(&key) else {
            return self.sample_clamped_edge(gx, gy);
        };
        let owner = TileCoord {
            level: self.center.level,
            x: key.0,
            y: key.1,
        };
        let (lx, ly) = GlobalCoordSigned { x: gx, y: gy }.to_local_with_halo(owner, HALO);
        let max = TILE_FULL_SIZE - 1;
        if lx < 0 || ly < 0 || lx > max || ly > max {
            return self.sample_clamped_edge(gx, gy);
        }
        [
            tile.at(lx as u32, ly as u32, 0),
            tile.at(lx as u32, ly as u32, 1),
            tile.at(lx as u32, ly as u32, 2),
            tile.at(lx as u32, ly as u32, 3),
        ]
    }

    fn sample_clamped_edge(&self, gx: i32, gy: i32) -> [f32; 4] {
        // Fall back to nearest available tile's edge pixel.
        let ts = TILE_SIZE as i32;
        let mut best: Option<((u32, u32), i32)> = None;
        for &(tx, ty) in self.tiles.keys() {
            let cx = tx as i32 * ts + ts / 2;
            let cy = ty as i32 * ts + ts / 2;
            let d = (cx - gx).abs() + (cy - gy).abs();
            if best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some(((tx, ty), d));
            }
        }
        let Some(((tx, ty), _)) = best else {
            return [0.0, 0.0, 0.0, 1.0];
        };
        let tile = &self.tiles[&(tx, ty)];
        let owner = TileCoord {
            level: self.center.level,
            x: tx,
            y: ty,
        };
        let (mut lx, mut ly) = GlobalCoordSigned { x: gx, y: gy }.to_local_with_halo(owner, HALO);
        let max = TILE_FULL_SIZE - 1;
        lx = lx.clamp(0, max);
        ly = ly.clamp(0, max);
        [
            tile.at(lx as u32, ly as u32, 0),
            tile.at(lx as u32, ly as u32, 1),
            tile.at(lx as u32, ly as u32, 2),
            tile.at(lx as u32, ly as u32, 3),
        ]
    }
}

/// Blur/sharpness radius in px for a single Adjust instance (0 if none).
pub fn adjust_spatial_radius_px(filter: &FilterInstance) -> i32 {
    match &filter.params {
        FilterParams::Adjust {
            blur, sharpness, ..
        } => {
            let blur_r = if *blur <= 1e-4 {
                0
            } else {
                (*blur * BLUR_PX_PER_UNIT)
                    .round()
                    .clamp(1.0, MAX_BLUR_RADIUS as f32) as i32
            };
            if blur_r > 0 {
                blur_r
            } else if *sharpness > 1e-6 {
                SHARP_RADIUS
            } else {
                0
            }
        }
        _ => 0,
    }
}

/// Max spatial radius among Post filters. Only counts Adjust that is the first
/// non-neutral Post filter (neighbor samples are Dithered-domain).
pub fn post_direct_spatial_radius(post: &[&FilterInstance]) -> i32 {
    for f in post {
        if crate::filters::neutral::filter_is_neutral_for_apply(f) {
            continue;
        }
        return adjust_spatial_radius_px(f);
    }
    0
}

/// Tile `(dx, dy)` offsets relative to center required for `radius_px`.
pub fn neighbor_tile_offsets(radius_px: i32) -> Vec<(i32, i32)> {
    if radius_px <= 0 {
        return vec![(0, 0)];
    }
    // radius < TILE_SIZE ⇒ at most ±1 tile.
    let span = 1i32;
    let mut out = Vec::with_capacity(9);
    for dy in -span..=span {
        for dx in -span..=span {
            out.push((dx, dy));
        }
    }
    out
}

/// Inclusive tile index bounds for the layer (usually `layer.bounds_l0`).
#[derive(Clone, Copy, Debug)]
pub struct NeighborTileBounds {
    pub min_x: u32,
    pub min_y: u32,
    pub max_x: u32,
    pub max_y: u32,
}

impl NeighborTileBounds {
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }
}

/// Gather fresh Dithered tiles for `center` into a [`SpatialNeighborhood`].
///
/// Tile coords outside `bounds` are ignored (edge clamp), not treated as missing.
pub fn gather_dithered_neighborhood(
    cache: &TileCache,
    doc: u32,
    layer: u32,
    center: TileCoord,
    radius_px: i32,
    bounds: NeighborTileBounds,
) -> SpatialNeighborhood {
    let mut tiles = HashMap::new();
    let mut complete = true;
    for (dx, dy) in neighbor_tile_offsets(radius_px) {
        let tx = center.x as i32 + dx;
        let ty = center.y as i32 + dy;
        if tx < 0 || ty < 0 {
            continue;
        }
        let ux = tx as u32;
        let uy = ty as u32;
        if !bounds.contains(ux, uy) {
            // Past the layer — not required.
            continue;
        }
        let coord = TileCoord {
            level: center.level,
            x: ux,
            y: uy,
        };
        let key = TileKey {
            doc,
            layer,
            coord,
            stage: CacheStage::Dithered,
        };
        if !engine_tiles::tile_fresh(cache, key) {
            if dx != 0 || dy != 0 {
                complete = false;
            }
            continue;
        }
        if let Some(tile) = cache.get_entry(key) {
            tiles.insert((coord.x, coord.y), tile);
        } else if dx != 0 || dy != 0 {
            complete = false;
        }
    }
    if !tiles.contains_key(&(center.x, center.y)) {
        complete = false;
    }
    SpatialNeighborhood {
        center,
        tiles,
        complete,
        radius_px,
    }
}

/// Absolute tile coords whose spatial Post may sample `completed` Dithered.
pub fn processed_coords_needing_spatial_upgrade(
    completed: TileCoord,
    radius_px: i32,
    bounds: NeighborTileBounds,
) -> Vec<TileCoord> {
    let mut out = Vec::new();
    for (dx, dy) in neighbor_tile_offsets(radius_px) {
        let tx = completed.x as i32 - dx;
        let ty = completed.y as i32 - dy;
        if tx < 0 || ty < 0 {
            continue;
        }
        let ux = tx as u32;
        let uy = ty as u32;
        if !bounds.contains(ux, uy) {
            continue;
        }
        out.push(TileCoord {
            level: completed.level,
            x: ux,
            y: uy,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{FilterKind, FilterParams};

    #[test]
    fn offsets_cover_moore_neighborhood() {
        let o = neighbor_tile_offsets(24);
        assert_eq!(o.len(), 9);
        assert!(o.contains(&(0, 0)));
        assert!(o.contains(&(1, 0)));
        assert!(o.contains(&(-1, -1)));
    }

    #[test]
    fn post_direct_radius_skips_neutral_then_reads_adjust() {
        let neutral = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        );
        let blurred = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 1.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        );
        assert_eq!(post_direct_spatial_radius(&[&neutral, &blurred]), 12);
    }

    #[test]
    fn curves_before_adjust_disables_direct_neighbor_radius() {
        let curves = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)],
                channel: crate::filters::curves::CurveChannel::All,
            },
        );
        let blurred = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 2.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        );
        // First non-neutral is Curves ⇒ radius 0 (neighbor path not used).
        assert_eq!(post_direct_spatial_radius(&[&curves, &blurred]), 0);
    }

    #[test]
    fn sample_across_horizontal_seam() {
        let mut left = PixelTile::new();
        let mut right = PixelTile::new();
        for y in 0..TILE_FULL_SIZE as u32 {
            for x in 0..TILE_FULL_SIZE as u32 {
                left.set(x, y, 0, 0.25);
                left.set(x, y, 1, 0.0);
                left.set(x, y, 2, 0.0);
                left.set(x, y, 3, 1.0);
                right.set(x, y, 0, 0.75);
                right.set(x, y, 1, 0.0);
                right.set(x, y, 2, 0.0);
                right.set(x, y, 3, 1.0);
            }
        }
        let center = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let mut tiles = HashMap::new();
        tiles.insert((0, 0), Arc::new(left));
        tiles.insert((1, 0), Arc::new(right));
        let nb = SpatialNeighborhood {
            center,
            tiles,
            complete: true,
            radius_px: 8,
        };
        // Last core pixel of tile 0 and first of tile 1.
        let g_left = TILE_SIZE as i32 - 1;
        let g_right = TILE_SIZE as i32;
        assert!((nb.sample_rgba(g_left, 10)[0] - 0.25).abs() < 1e-6);
        assert!((nb.sample_rgba(g_right, 10)[0] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn incomplete_neighborhood_falls_back_to_clamp_blur() {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE as u32 {
            for x in 0..TILE_FULL_SIZE as u32 {
                tile.set(
                    x,
                    y,
                    0,
                    if x < TILE_FULL_SIZE as u32 / 2 {
                        0.0
                    } else {
                        1.0
                    },
                );
                tile.set(x, y, 1, 0.0);
                tile.set(x, y, 2, 0.0);
                tile.set(x, y, 3, 1.0);
            }
        }
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let mut tiles = HashMap::new();
        tiles.insert(
            (0, 0),
            Arc::new({
                let mut t = PixelTile::new();
                t.copy_from(&tile);
                t
            }),
        );
        let incomplete = SpatialNeighborhood {
            center: coord,
            tiles,
            complete: false,
            radius_px: 12,
        };
        let mut via_incomplete = PixelTile::new();
        with_spatial_neighbors(Some(&incomplete), || {
            crate::filters::adjust::apply_adjust_into(
                &tile,
                coord,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                &mut via_incomplete,
            );
        });
        let mut clamped = PixelTile::new();
        crate::filters::adjust::apply_adjust_into(
            &tile,
            coord,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            &mut clamped,
        );
        assert_eq!(
            via_incomplete.data, clamped.data,
            "incomplete neighborhood must use in-tile clamp blur"
        );
    }

    #[test]
    fn gather_ignores_oob_neighbors_as_complete() {
        use engine_tiles::TileCache;
        let cache = TileCache::new(16 * 1024 * 1024);
        // Single-tile document: every Moore neighbor is OOB except center.
        let center = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let mut center_tile = PixelTile::new();
        center_tile.set(HALO, HALO, 0, 0.5);
        center_tile.set(HALO, HALO, 3, 1.0);
        let key = TileKey {
            doc: 1,
            layer: 1,
            coord: center,
            stage: CacheStage::Dithered,
        };
        assert!(cache.insert_fresh_gen(key, Arc::new(center_tile), 0));
        let nb = gather_dithered_neighborhood(
            &cache,
            1,
            1,
            center,
            12,
            NeighborTileBounds {
                min_x: 0,
                min_y: 0,
                max_x: 0,
                max_y: 0,
            },
        );
        assert!(
            nb.complete,
            "document-edge tiles must not require OOB neighbors"
        );
        assert_eq!(nb.tiles.len(), 1);
    }

    #[test]
    fn complete_neighbors_blur_softens_cross_tile_step() {
        let mut left = PixelTile::new();
        let mut right = PixelTile::new();
        for y in 0..TILE_FULL_SIZE as u32 {
            for x in 0..TILE_FULL_SIZE as u32 {
                left.set(x, y, 0, 0.0);
                left.set(x, y, 3, 1.0);
                right.set(x, y, 0, 1.0);
                right.set(x, y, 3, 1.0);
            }
        }
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let mut tiles = HashMap::new();
        tiles.insert(
            (0, 0),
            Arc::new({
                let mut t = PixelTile::new();
                t.copy_from(&left);
                t
            }),
        );
        tiles.insert(
            (1, 0),
            Arc::new({
                let mut t = PixelTile::new();
                t.copy_from(&right);
                t
            }),
        );
        let nb = SpatialNeighborhood {
            center: coord,
            tiles,
            complete: true,
            radius_px: 12,
        };
        let mut clamped = PixelTile::new();
        crate::filters::adjust::apply_adjust_into(
            &left,
            coord,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            &mut clamped,
        );
        let mut crossed = PixelTile::new();
        with_spatial_neighbors(Some(&nb), || {
            crate::filters::adjust::apply_adjust_into(
                &left,
                coord,
                0.0,
                0.0,
                0.0,
                1.0,
                0.0,
                0.0,
                &mut crossed,
            );
        });
        let edge_x = HALO + TILE_SIZE - 1;
        let y = HALO + 16;
        // Neighbor-aware blur pulls bright from the right tile → higher than clamp.
        assert!(
            crossed.at(edge_x, y, 0) > clamped.at(edge_x, y, 0) + 0.01,
            "neighbor blur should soften the right edge (crossed={} clamped={})",
            crossed.at(edge_x, y, 0),
            clamped.at(edge_x, y, 0)
        );
    }
}
