//! Classical Hilbert curve coordinate mapping (d2xy).
//!
//! Used by Riemersma dithering. Orientation matches the recursive construction
//! commonly used in open dithering libraries (e.g. ditherlib): pad to
//! `side = next_power_of_two(max(w,h))`, visit indices `0..side²`, skip points
//! outside the real rectangle (§0.3 of RIEMERSMA_full_document_spec).

/// Map a Hilbert index on a `side × side` grid (`side` must be a power of two) to `(x, y)`.
pub fn hilbert_point(side: u64, mut index: u128) -> (u64, u64) {
    debug_assert!(side.is_power_of_two() || side == 0);
    let (mut x, mut y) = (0u64, 0u64);
    let mut scale = 1u64;
    while scale < side {
        let right = ((index / 2) & 1) as u64;
        let up = ((index ^ u128::from(right)) & 1) as u64;
        if up == 0 {
            if right == 1 {
                x = scale - 1 - x;
                y = scale - 1 - y;
            }
            std::mem::swap(&mut x, &mut y);
        }
        x += scale * right;
        y += scale * up;
        index /= 4;
        scale *= 2;
    }
    (x, y)
}

/// Visit every cell of a `width × height` rectangle in Hilbert order.
///
/// Pads conceptually to `side = next_power_of_two(max(width, height))` and skips
/// indices whose coordinates fall outside the real rectangle.
///
/// `visit` returns `true` to continue, `false` to abort early (cancellation).
pub fn for_each_hilbert_cell(width: u64, height: u64, mut visit: impl FnMut(u64, u64) -> bool) {
    if width == 0 || height == 0 {
        return;
    }
    let side = width.max(height).next_power_of_two();
    let n = u128::from(side) * u128::from(side);
    for index in 0..n {
        let (x, y) = hilbert_point(side, index);
        if x < width && y < height && !visit(x, y) {
            return;
        }
    }
}

/// Collect Hilbert-order cells (test / oracle helper).
pub fn hilbert_cells(width: u64, height: u64) -> Vec<(u64, u64)> {
    let mut cells = Vec::with_capacity((width * height) as usize);
    for_each_hilbert_cell(width, height, |x, y| {
        cells.push((x, y));
        true
    });
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Canonical order-1 (2×2) Hilbert starting at (0,0), U-shape.
    #[test]
    fn hilbert_2x2_matches_canonical_u_shape() {
        assert_eq!(hilbert_cells(2, 2), vec![(0, 0), (0, 1), (1, 1), (1, 0)]);
    }

    /// Order-2 (4×4): recursive construction from `hilbert_point` (Hacker's Delight /
    /// Wikipedia d2xy orientation). Standalone order-1 is a vertical U; the
    /// bottom-left quadrant of order-2 is a rotated horizontal U — that is
    /// expected for this construction.
    #[test]
    fn hilbert_4x4_matches_canonical_sequence() {
        assert_eq!(
            hilbert_cells(4, 4),
            vec![
                (0, 0),
                (1, 0),
                (1, 1),
                (0, 1),
                (0, 2),
                (0, 3),
                (1, 3),
                (1, 2),
                (2, 2),
                (2, 3),
                (3, 3),
                (3, 2),
                (3, 1),
                (2, 1),
                (2, 0),
                (3, 0),
            ]
        );
    }

    #[test]
    fn hilbert_covers_all_cells_exactly_once() {
        for &(w, h) in &[(1u64, 1), (3, 5), (8, 8), (7, 2)] {
            let cells = hilbert_cells(w, h);
            assert_eq!(cells.len(), (w * h) as usize);
            let mut sorted = cells.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), cells.len());
            for &(x, y) in &cells {
                assert!(x < w && y < h);
            }
        }
    }

    #[test]
    fn successive_in_bounds_points_are_4_adjacent_or_first() {
        // After padding skips, consecutive *visited* points need not be adjacent
        // when the curve jumps over the pad — but on a filled square they are.
        let cells = hilbert_cells(8, 8);
        for w in cells.windows(2) {
            let (x0, y0) = w[0];
            let (x1, y1) = w[1];
            let d = x0.abs_diff(x1) + y0.abs_diff(y1);
            assert_eq!(d, 1, "({x0},{y0}) -> ({x1},{y1})");
        }
    }
}
