//! KD-tree for efficient nearest-neighbor color lookup in Oklab space.
//!
//! Provides O(n log n) construction and O(log n) average-case nearest-neighbor
//! queries in 3-dimensional Oklab (L, a, b) space using Euclidean (L2) distance.

use crate::oklab::{oklab_dist_sq, Oklab};

/// Internal node representation for the KD-tree.
///
/// Each node is either a leaf holding a single point, or an internal split
/// node that partitions space along one axis at a threshold value.
#[derive(Debug, Clone)]
enum KdNode {
    /// A leaf node referencing a single point by its index into `KdTree::points`.
    Leaf { point_idx: usize },
    /// An internal node splitting along `axis` (0=L, 1=a, 2=b).
    Split {
        axis: u8,
        threshold: f32,
        left: usize,
        right: usize,
    },
}

/// A 3-dimensional KD-tree for nearest-neighbor search in Oklab space.
///
/// The tree is built once from a set of palette colors and then queried
/// repeatedly for nearest-color lookups. Tie-breaking favors the palette
/// color with the lowest index.
#[derive(Debug, Clone)]
pub struct KdTree {
    nodes: Vec<KdNode>,
    /// Palette colors in Oklab space, kept in original palette order.
    points: Vec<Oklab>,
}

impl KdTree {
    /// Build a KD-tree from palette colors (already in Oklab space).
    ///
    /// Returns `None` if `colors` is empty.
    ///
    /// The tree is constructed by recursively splitting along the axis with
    /// greatest variance, using the median element as the split point.
    pub fn build(colors: &[Oklab]) -> Option<Self> {
        if colors.is_empty() {
            return None;
        }

        let points: Vec<Oklab> = colors.to_vec();
        // `palette_indices[i]` holds the original palette index for working slot i.
        let mut palette_indices: Vec<usize> = (0..colors.len()).collect();
        let mut nodes: Vec<KdNode> = Vec::new();

        Self::build_recursive(&points, &mut palette_indices, &mut nodes, 0, colors.len());

        Some(KdTree { nodes, points })
    }

    /// Recursively build the tree for the range `[start, end)` in `palette_indices`.
    /// Returns the index of the created node in `nodes`.
    fn build_recursive(
        points: &[Oklab],
        palette_indices: &mut [usize],
        nodes: &mut Vec<KdNode>,
        start: usize,
        end: usize,
    ) -> usize {
        let count = end - start;
        debug_assert!(count > 0);

        if count == 1 {
            let node_idx = nodes.len();
            nodes.push(KdNode::Leaf {
                point_idx: palette_indices[start],
            });
            return node_idx;
        }

        // Find the axis with greatest variance (range as proxy).
        let axis = Self::best_axis(points, &palette_indices[start..end]);

        // Sort the slice by the chosen axis to find the median.
        let slice = &mut palette_indices[start..end];
        slice.sort_by(|&a, &b| {
            let va = Self::axis_value(points, a, axis);
            let vb = Self::axis_value(points, b, axis);
            va.partial_cmp(&vb).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Median split: left gets [start, mid), right gets [mid, end).
        let mid = start + count / 2;
        let threshold = Self::axis_value(points, palette_indices[mid], axis);

        // Reserve a slot for this node, then build children.
        let node_idx = nodes.len();
        nodes.push(KdNode::Leaf { point_idx: 0 }); // placeholder

        let left = Self::build_recursive(points, palette_indices, nodes, start, mid);
        let right = Self::build_recursive(points, palette_indices, nodes, mid, end);

        nodes[node_idx] = KdNode::Split {
            axis,
            threshold,
            left,
            right,
        };

        node_idx
    }

    /// Determine the best axis to split on by finding the axis with
    /// the greatest range (max - min) across the given point indices.
    fn best_axis(points: &[Oklab], indices: &[usize]) -> u8 {
        let mut min_vals = [f32::INFINITY; 3];
        let mut max_vals = [f32::NEG_INFINITY; 3];

        for &idx in indices {
            let p = &points[idx];
            let vals = [p.l, p.a, p.b];
            for (i, &v) in vals.iter().enumerate() {
                if v < min_vals[i] {
                    min_vals[i] = v;
                }
                if v > max_vals[i] {
                    max_vals[i] = v;
                }
            }
        }

        let mut best_axis = 0u8;
        let mut best_range = max_vals[0] - min_vals[0];
        for i in 1..3 {
            let range = max_vals[i] - min_vals[i];
            if range > best_range {
                best_range = range;
                best_axis = i as u8;
            }
        }
        best_axis
    }

    /// Get the value of a point along a given axis.
    #[inline]
    fn axis_value(points: &[Oklab], idx: usize, axis: u8) -> f32 {
        let p = &points[idx];
        match axis {
            0 => p.l,
            1 => p.a,
            2 => p.b,
            _ => unreachable!(),
        }
    }

    /// Find the nearest palette color index for a query point.
    ///
    /// Uses Euclidean (L2) distance in Oklab space with standard KD-tree
    /// pruning and backtracking. Ties are broken by preferring the color
    /// with the lowest palette index.
    pub fn nearest(&self, query: Oklab) -> usize {
        let mut best_dist = f32::INFINITY;
        let mut best_idx = 0usize;

        self.search_nearest(0, query, &mut best_dist, &mut best_idx);

        best_idx
    }

    /// Recursive nearest-neighbor search with pruning.
    fn search_nearest(
        &self,
        node_idx: usize,
        query: Oklab,
        best_dist: &mut f32,
        best_idx: &mut usize,
    ) {
        match &self.nodes[node_idx] {
            KdNode::Leaf { point_idx } => {
                let dist = oklab_dist_sq(query, self.points[*point_idx]);
                let palette_idx = *point_idx;
                if dist < *best_dist || (dist == *best_dist && palette_idx < *best_idx) {
                    *best_dist = dist;
                    *best_idx = palette_idx;
                }
            }
            KdNode::Split {
                axis,
                threshold,
                left,
                right,
            } => {
                let query_val = match axis {
                    0 => query.l,
                    1 => query.a,
                    2 => query.b,
                    _ => unreachable!(),
                };

                let diff = query_val - threshold;

                // Determine which side to search first.
                let (first, second) = if diff <= 0.0 {
                    (*left, *right)
                } else {
                    (*right, *left)
                };

                // Search the closer side first.
                self.search_nearest(first, query, best_dist, best_idx);

                // Prune: only search the other side if the splitting plane
                // is closer than the current best distance.
                if diff * diff < *best_dist {
                    self.search_nearest(second, query, best_dist, best_idx);
                }
            }
        }
    }
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_palette_returns_none() {
        let tree = KdTree::build(&[]);
        assert!(tree.is_none());
    }

    #[test]
    fn test_single_color_always_returns_zero() {
        let colors = vec![Oklab {
            l: 0.5,
            a: 0.1,
            b: -0.1,
        }];
        let tree = KdTree::build(&colors).unwrap();

        // Any query should return index 0.
        let queries = [
            Oklab { l: 0.0, a: 0.0, b: 0.0 },
            Oklab { l: 1.0, a: 0.5, b: 0.5 },
            Oklab { l: 0.5, a: 0.1, b: -0.1 },
            Oklab { l: 0.3, a: -0.3, b: 0.4 },
        ];
        for q in &queries {
            assert_eq!(tree.nearest(*q), 0, "query {:?} should return 0", q);
        }
    }

    #[test]
    fn test_exact_match() {
        let colors = vec![
            Oklab { l: 0.0, a: 0.0, b: 0.0 },
            Oklab { l: 0.5, a: 0.1, b: -0.1 },
            Oklab { l: 1.0, a: -0.2, b: 0.3 },
        ];
        let tree = KdTree::build(&colors).unwrap();

        // Querying an exact point should return its index.
        assert_eq!(tree.nearest(Oklab { l: 0.0, a: 0.0, b: 0.0 }), 0);
        assert_eq!(tree.nearest(Oklab { l: 0.5, a: 0.1, b: -0.1 }), 1);
        assert_eq!(tree.nearest(Oklab { l: 1.0, a: -0.2, b: 0.3 }), 2);
    }

    #[test]
    fn test_equidistant_tie_breaking_lowest_index() {
        // Two colors equidistant from the query: should return lower index.
        let colors = vec![
            Oklab { l: 0.0, a: 0.0, b: 0.0 }, // index 0, dist_sq = 0.25
            Oklab { l: 1.0, a: 0.0, b: 0.0 }, // index 1, dist_sq = 0.25
        ];
        let tree = KdTree::build(&colors).unwrap();

        // Query at midpoint: equidistant to both.
        let query = Oklab { l: 0.5, a: 0.0, b: 0.0 };
        assert_eq!(
            tree.nearest(query),
            0,
            "equidistant query should return lowest index (0)"
        );
    }

    #[test]
    fn test_equidistant_tie_breaking_three_colors() {
        // Three colors all equidistant from the origin.
        let colors = vec![
            Oklab { l: 0.0, a: 0.0, b: 1.0 }, // index 0, dist_sq = 1.0
            Oklab { l: 0.0, a: 1.0, b: 0.0 }, // index 1, dist_sq = 1.0
            Oklab { l: 1.0, a: 0.0, b: 0.0 }, // index 2, dist_sq = 1.0
        ];
        let tree = KdTree::build(&colors).unwrap();

        let query = Oklab { l: 0.0, a: 0.0, b: 0.0 };
        assert_eq!(
            tree.nearest(query),
            0,
            "equidistant query should return lowest index (0)"
        );
    }

    #[test]
    fn test_nearest_basic() {
        let colors = vec![
            Oklab { l: 0.2, a: 0.0, b: 0.0 },
            Oklab { l: 0.5, a: 0.0, b: 0.0 },
            Oklab { l: 0.8, a: 0.0, b: 0.0 },
        ];
        let tree = KdTree::build(&colors).unwrap();

        // Query closer to index 0
        assert_eq!(tree.nearest(Oklab { l: 0.1, a: 0.0, b: 0.0 }), 0);
        // Query closer to index 1
        assert_eq!(tree.nearest(Oklab { l: 0.4, a: 0.0, b: 0.0 }), 1);
        // Query closer to index 2
        assert_eq!(tree.nearest(Oklab { l: 0.9, a: 0.0, b: 0.0 }), 2);
    }

    #[test]
    fn test_nearest_matches_brute_force() {
        // A small palette; verify KD-tree matches brute-force for several queries.
        let colors = vec![
            Oklab { l: 0.1, a: 0.2, b: -0.1 },
            Oklab { l: 0.4, a: -0.1, b: 0.3 },
            Oklab { l: 0.7, a: 0.0, b: 0.0 },
            Oklab { l: 0.9, a: -0.3, b: -0.2 },
            Oklab { l: 0.3, a: 0.4, b: 0.1 },
        ];
        let tree = KdTree::build(&colors).unwrap();

        let queries = vec![
            Oklab { l: 0.0, a: 0.0, b: 0.0 },
            Oklab { l: 0.5, a: 0.1, b: 0.1 },
            Oklab { l: 1.0, a: -0.5, b: 0.5 },
            Oklab { l: 0.35, a: 0.3, b: 0.0 },
            Oklab { l: 0.6, a: -0.2, b: -0.1 },
        ];

        for q in &queries {
            let kd_result = tree.nearest(*q);
            let bf_result = brute_force_nearest(&colors, *q);
            assert_eq!(
                kd_result, bf_result,
                "KD-tree and brute-force disagree for query {:?}: kd={}, bf={}",
                q, kd_result, bf_result
            );
        }
    }

    #[test]
    fn test_two_colors() {
        let colors = vec![
            Oklab { l: 0.0, a: 0.0, b: 0.0 },
            Oklab { l: 1.0, a: 0.0, b: 0.0 },
        ];
        let tree = KdTree::build(&colors).unwrap();

        assert_eq!(tree.nearest(Oklab { l: 0.3, a: 0.0, b: 0.0 }), 0);
        assert_eq!(tree.nearest(Oklab { l: 0.7, a: 0.0, b: 0.0 }), 1);
    }

    #[test]
    fn test_duplicate_colors() {
        // All colors are the same; should always return index 0 (lowest).
        let colors = vec![
            Oklab { l: 0.5, a: 0.0, b: 0.0 },
            Oklab { l: 0.5, a: 0.0, b: 0.0 },
            Oklab { l: 0.5, a: 0.0, b: 0.0 },
        ];
        let tree = KdTree::build(&colors).unwrap();

        assert_eq!(tree.nearest(Oklab { l: 0.5, a: 0.0, b: 0.0 }), 0);
        assert_eq!(tree.nearest(Oklab { l: 0.0, a: 0.0, b: 0.0 }), 0);
    }

    /// Brute-force nearest-neighbor for validation (tie-breaking by lowest index).
    fn brute_force_nearest(colors: &[Oklab], query: Oklab) -> usize {
        let mut best_dist = f32::INFINITY;
        let mut best_idx = 0;
        for (i, &color) in colors.iter().enumerate() {
            let dist = oklab_dist_sq(query, color);
            if dist < best_dist || (dist == best_dist && i < best_idx) {
                best_dist = dist;
                best_idx = i;
            }
        }
        best_idx
    }
}
