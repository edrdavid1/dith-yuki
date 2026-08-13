//! 3D Oklab palette LUT: O(1) nearest-color after O(size³ · log K) build.
//!
//! `PaletteLut3D` stores a regular grid of palette indices. Build queries the
//! existing `KdTree` at each cell center. Hot-path lookup is pure index math.
//!
//! `PaletteLutCache` mirrors `PaletteKdCache` revision invalidation
//! (last-writer-wins on DashMap insert).

use dashmap::DashMap;
use std::sync::Arc;

use crate::kdtree::KdTree;
use crate::oklab::Oklab;
use crate::palette::{Palette, PaletteError, PaletteId};
use crate::palette_cache::PaletteKdCache;

/// Default grid resolution (frozen by Track B §1.5 bench, 2026-08-11).
///
/// size=32 and size=64 have ~identical lookup throughput (~23× vs KD on K=16),
/// but a dense K=64 “close colors” palette showed ~29% Cell_Boundary_Disagreement
/// at 32³. Prefer 64³ (512 KiB) for quality; no adaptive K-threshold.
pub const DEFAULT_LUT_SIZE: u32 = 64;

/// Default Oklab axis ranges covering the engine's typical gamut.
pub const DEFAULT_L_RANGE: (f32, f32) = (0.0, 1.0);
pub const DEFAULT_A_RANGE: (f32, f32) = (-0.4, 0.4);
pub const DEFAULT_B_RANGE: (f32, f32) = (-0.4, 0.4);

/// Precomputed nearest-palette index on a regular Oklab grid.
#[derive(Debug, Clone)]
pub struct PaletteLut3D {
    /// Flat `size³` grid, row-major L, a, b.
    grid: Vec<u16>,
    size: u32,
    l_range: (f32, f32),
    a_range: (f32, f32),
    b_range: (f32, f32),
}

impl PaletteLut3D {
    /// Build a LUT by querying `kdtree` at every cell center.
    ///
    /// Returns `Err(PaletteError::Empty)` if the palette has no colors or
    /// `size == 0`. Palette length must be ≤ `u16::MAX`.
    pub fn build(palette: &Palette, size: u32, kdtree: &KdTree) -> Result<Self, PaletteError> {
        Self::build_with_ranges(
            palette,
            size,
            kdtree,
            DEFAULT_L_RANGE,
            DEFAULT_A_RANGE,
            DEFAULT_B_RANGE,
        )
    }

    /// Build with explicit axis ranges (for tests / future gamut widening).
    pub fn build_with_ranges(
        palette: &Palette,
        size: u32,
        kdtree: &KdTree,
        l_range: (f32, f32),
        a_range: (f32, f32),
        b_range: (f32, f32),
    ) -> Result<Self, PaletteError> {
        if palette.colors.is_empty() || size == 0 {
            return Err(PaletteError::Empty);
        }
        debug_assert!(
            palette.colors.len() <= u16::MAX as usize,
            "palette length exceeds u16::MAX"
        );
        if palette.colors.len() > u16::MAX as usize {
            return Err(PaletteError::Empty);
        }

        let n = size as usize;
        let mut grid = vec![0u16; n * n * n];

        for i in 0..n {
            let l = cell_center(i, n, l_range);
            for j in 0..n {
                let a = cell_center(j, n, a_range);
                for k in 0..n {
                    let b = cell_center(k, n, b_range);
                    let idx = kdtree.nearest(Oklab { l, a, b });
                    grid[flat_index(i, j, k, n)] = idx as u16;
                }
            }
        }

        Ok(Self {
            grid,
            size,
            l_range,
            a_range,
            b_range,
        })
    }

    /// Map an Oklab sample to the nearest palette index via O(1) grid lookup.
    /// Out-of-range samples clamp to the grid edges.
    #[inline]
    pub fn nearest_index(&self, lab: Oklab) -> u16 {
        let n = self.size as usize;
        let i = axis_index(lab.l, self.l_range, n);
        let j = axis_index(lab.a, self.a_range, n);
        let k = axis_index(lab.b, self.b_range, n);
        self.grid[flat_index(i, j, k, n)]
    }

    #[inline]
    pub fn size(&self) -> u32 {
        self.size
    }

    #[inline]
    pub fn l_range(&self) -> (f32, f32) {
        self.l_range
    }

    #[inline]
    pub fn a_range(&self) -> (f32, f32) {
        self.a_range
    }

    #[inline]
    pub fn b_range(&self) -> (f32, f32) {
        self.b_range
    }

    /// Memory occupied by the grid in bytes (`size³ × 2`).
    pub fn grid_bytes(&self) -> usize {
        self.grid.len() * std::mem::size_of::<u16>()
    }
}

/// Concurrent cache: PaletteId → (revision, Arc<PaletteLut3D>).
pub struct PaletteLutCache {
    entries: DashMap<PaletteId, (u64, Arc<PaletteLut3D>)>,
}

impl PaletteLutCache {
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Get or build a LUT for `palette` at the given grid `size`.
    ///
    /// Uses `kd_cache` to obtain the KD-tree for cell-center queries.
    /// Rebuilds when revision mismatches. Concurrent inserts: last-writer-wins.
    pub fn get_or_build(
        &self,
        palette: &Palette,
        kd_cache: &PaletteKdCache,
        size: u32,
    ) -> Result<Arc<PaletteLut3D>, PaletteError> {
        if palette.colors.is_empty() {
            return Err(PaletteError::Empty);
        }

        if let Some(entry) = self.entries.get(&palette.id) {
            let (cached_revision, ref lut) = *entry;
            if cached_revision == palette.revision && lut.size() == size {
                return Ok(Arc::clone(lut));
            }
        }

        let tree = kd_cache.get_or_build(palette)?;
        let lut = PaletteLut3D::build(palette, size, &tree)?;
        let arc = Arc::new(lut);
        self.entries
            .insert(palette.id, (palette.revision, Arc::clone(&arc)));
        Ok(arc)
    }

    /// Evict the cached entry for the given palette ID.
    pub fn evict(&self, palette_id: PaletteId) {
        self.entries.remove(&palette_id);
    }

    /// Palette ids currently resident in the LUT cache.
    pub fn cached_ids(&self) -> Vec<PaletteId> {
        self.entries.iter().map(|e| *e.key()).collect()
    }
}

impl Default for PaletteLutCache {
    fn default() -> Self {
        Self::new()
    }
}

#[inline]
fn flat_index(i: usize, j: usize, k: usize, n: usize) -> usize {
    (i * n + j) * n + k
}

/// Cell center for index `i` in `[0, n)` along `[lo, hi]`.
#[inline]
fn cell_center(i: usize, n: usize, range: (f32, f32)) -> f32 {
    let (lo, hi) = range;
    let t = (i as f32 + 0.5) / n as f32;
    lo + t * (hi - lo)
}

/// Map axis value to grid index; clamp out-of-range to edges.
#[inline]
fn axis_index(v: f32, range: (f32, f32), n: usize) -> usize {
    let (lo, hi) = range;
    let span = hi - lo;
    if span <= 0.0 {
        return 0;
    }
    let t = ((v - lo) / span).clamp(0.0, 1.0 - f32::EPSILON);
    let i = (t * n as f32).floor() as usize;
    i.min(n - 1)
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oklab::{linear_to_oklab, LinRgb};
    use crate::palette::LinearColor;

    fn make_palette(id: PaletteId, revision: u64, colors: Vec<LinearColor>) -> Palette {
        Palette {
            id,
            name: format!("test-palette-{}", id),
            colors,
            revision,
        }
    }

    fn rgb_palette() -> Palette {
        make_palette(
            1,
            1,
            vec![
                LinearColor {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                },
                LinearColor {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                },
                LinearColor {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                },
            ],
        )
    }

    #[test]
    fn empty_palette_rejected() {
        let kd = PaletteKdCache::new();
        let palette = make_palette(1, 1, vec![]);
        let tree = kd.get_or_build(&rgb_palette()).unwrap();
        assert!(matches!(
            PaletteLut3D::build(&palette, 8, &tree),
            Err(PaletteError::Empty)
        ));

        let lut_cache = PaletteLutCache::new();
        assert!(matches!(
            lut_cache.get_or_build(&palette, &kd, 8),
            Err(PaletteError::Empty)
        ));
    }

    #[test]
    fn cell_centers_match_kdtree() {
        let palette = rgb_palette();
        let kd = PaletteKdCache::new();
        let tree = kd.get_or_build(&palette).unwrap();
        let size = 8u32;
        let lut = PaletteLut3D::build(&palette, size, &tree).unwrap();

        let n = size as usize;
        for i in 0..n {
            for j in 0..n {
                for k in 0..n {
                    let lab = Oklab {
                        l: cell_center(i, n, lut.l_range()),
                        a: cell_center(j, n, lut.a_range()),
                        b: cell_center(k, n, lut.b_range()),
                    };
                    assert_eq!(
                        lut.nearest_index(lab) as usize,
                        tree.nearest(lab),
                        "center ({i},{j},{k}) must match build-time KD"
                    );
                }
            }
        }
    }

    #[test]
    fn out_of_range_clamps() {
        let palette = rgb_palette();
        let kd = PaletteKdCache::new();
        let tree = kd.get_or_build(&palette).unwrap();
        let lut = PaletteLut3D::build(&palette, 8, &tree).unwrap();

        // Far below / above ranges — must not panic; equals corresponding edge cell.
        let below = Oklab {
            l: -10.0,
            a: -10.0,
            b: -10.0,
        };
        let above = Oklab {
            l: 10.0,
            a: 10.0,
            b: 10.0,
        };
        let edge_lo = Oklab {
            l: lut.l_range().0,
            a: lut.a_range().0,
            b: lut.b_range().0,
        };
        let edge_hi = Oklab {
            l: lut.l_range().1,
            a: lut.a_range().1,
            b: lut.b_range().1,
        };
        assert_eq!(lut.nearest_index(below), lut.nearest_index(edge_lo));
        assert_eq!(lut.nearest_index(above), lut.nearest_index(edge_hi));
    }

    #[test]
    fn cache_hit_same_arc() {
        let palette = rgb_palette();
        let kd = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let a = lut_cache
            .get_or_build(&palette, &kd, DEFAULT_LUT_SIZE)
            .unwrap();
        let b = lut_cache
            .get_or_build(&palette, &kd, DEFAULT_LUT_SIZE)
            .unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }

    #[test]
    fn revision_bump_rebuilds() {
        let kd = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();

        let v1 = make_palette(
            1,
            1,
            vec![
                LinearColor {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                },
                LinearColor {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                },
            ],
        );
        let lut1 = lut_cache
            .get_or_build(&v1, &kd, 8)
            .unwrap();

        let v2 = make_palette(
            1,
            2,
            vec![
                LinearColor {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                },
                LinearColor {
                    r: 1.0,
                    g: 1.0,
                    b: 0.0,
                },
            ],
        );
        let lut2 = lut_cache
            .get_or_build(&v2, &kd, 8)
            .unwrap();
        assert!(!Arc::ptr_eq(&lut1, &lut2));

        let query = linear_to_oklab(LinRgb {
            r: 0.0,
            g: 0.0,
            b: 0.9,
        });
        assert_eq!(lut2.nearest_index(query), 0);
    }

    #[test]
    fn random_oklab_disagreement_bounded() {
        let palette = rgb_palette();
        let kd = PaletteKdCache::new();
        let tree = kd.get_or_build(&palette).unwrap();
        let lut = PaletteLut3D::build(&palette, DEFAULT_LUT_SIZE, &tree).unwrap();
        let palette_oklab: Vec<Oklab> = palette
            .colors
            .iter()
            .map(|c| {
                linear_to_oklab(LinRgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                })
            })
            .collect();

        // Deterministic LCG samples across the LUT domain.
        let mut state = 0xC0FFEE_u64;
        let mut next = || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            (state >> 33) as f32 / (1u32 << 31) as f32
        };

        let mut disagreements = 0usize;
        let samples = 2000usize;
        let (l0, l1) = lut.l_range();
        let (a0, a1) = lut.a_range();
        let (b0, b1) = lut.b_range();

        for _ in 0..samples {
            let lab = Oklab {
                l: l0 + next() * (l1 - l0),
                a: a0 + next() * (a1 - a0),
                b: b0 + next() * (b1 - b0),
            };
            let lut_idx = lut.nearest_index(lab) as usize;
            let kd_idx = tree.nearest(lab);
            if lut_idx != kd_idx {
                // Cell-boundary disagreement: distances to both candidates
                // should be close (Voronoi boundary vs cell quantization).
                let d_lut = crate::oklab::oklab_dist_sq(lab, palette_oklab[lut_idx]);
                let d_kd = crate::oklab::oklab_dist_sq(lab, palette_oklab[kd_idx]);
                let max_d = d_lut.max(d_kd);
                let rel = if max_d > 1e-12 {
                    (d_lut - d_kd).abs() / max_d
                } else {
                    0.0
                };
                assert!(
                    rel < 0.25 || (d_lut - d_kd).abs() < 1e-4,
                    "systematic disagreement: lut={lut_idx} kd={kd_idx} lab={lab:?} d_lut={d_lut} d_kd={d_kd}"
                );
                disagreements += 1;
            }
        }

        let rate = disagreements as f64 / samples as f64;
        assert!(
            rate < 0.15,
            "disagreement rate {rate} exceeds 15% bound ({disagreements}/{samples})"
        );
    }
}
