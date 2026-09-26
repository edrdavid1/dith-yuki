//! 1D brightness-sorted palette lookup for Match-by-Brightness dithering.
//!
//! Finds the two palette entries whose Oklab lightness (`L`) bracket a query,
//! without a 3D LUT/KD tree. Build once per palette revision; look up with
//! binary search.

use crate::oklab::{linear_to_oklab, LinRgb};
use crate::palette::{Palette, PaletteError};

/// Palette indices sorted by Oklab lightness.
#[derive(Debug, Clone)]
pub struct BrightnessSortedPalette {
    /// `(L, palette_index)`, ascending by `L`.
    entries: Vec<(f32, usize)>,
}

impl BrightnessSortedPalette {
    /// Build a lightness-sorted index of `palette`.
    ///
    /// Duplicate `L` values are allowed (stable sort by `total_cmp`).
    pub fn build(palette: &Palette) -> Result<Self, PaletteError> {
        if palette.colors.is_empty() {
            return Err(PaletteError::Empty);
        }
        let mut entries: Vec<(f32, usize)> = palette
            .colors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let lab = linear_to_oklab(LinRgb {
                    r: c.r,
                    g: c.g,
                    b: c.b,
                });
                (lab.l, i)
            })
            .collect();
        entries.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(Self { entries })
    }

    /// Sorted `(L, palette_index)` entries (ascending `L`).
    #[inline]
    pub fn entries(&self) -> &[(f32, usize)] {
        &self.entries
    }

    /// Single nearest palette index by absolute lightness distance.
    #[inline]
    pub fn nearest(&self, pixel_l: f32) -> usize {
        let (i1, _, _) = self.two_nearest(pixel_l);
        i1
    }

    /// Two lightness neighbors and a mix fraction matching OrderedPalettePicker.
    ///
    /// `mix = d1 / (d1 + d2)` where `d1`/`d2` are absolute `L` distances to the
    /// nearer / farther neighbor. Caller picks with
    /// `idx = if t < mix { i2 } else { i1 }` (same as full-Oklab two-nearest).
    ///
    /// Outside the palette `L` range, both neighbors come from the matching edge
    /// with `mix` clamped so the edge color always wins for `t ∈ [0, 1]`.
    pub fn two_nearest(&self, pixel_l: f32) -> (usize, usize, f32) {
        let n = self.entries.len();
        debug_assert!(n >= 1);
        if n == 1 {
            let idx = self.entries[0].1;
            return (idx, idx, 0.0);
        }

        // First index with L >= pixel_l
        let i = self.entries.partition_point(|&(l, _)| l < pixel_l);

        if i == 0 {
            // At/below darkest: both neighbors from the dark edge; mix=0 → always i1.
            let i1 = self.entries[0].1;
            let i2 = self.entries[1].1;
            return (i1, i2, 0.0);
        }
        if i >= n {
            // At/above lightest: both neighbors from the light edge; mix=0 → always i1.
            let i1 = self.entries[n - 1].1;
            let i2 = self.entries[n - 2].1;
            return (i1, i2, 0.0);
        }

        let (l_left, idx_left) = self.entries[i - 1];
        let (l_right, idx_right) = self.entries[i];
        let d_left = (pixel_l - l_left).abs();
        let d_right = (pixel_l - l_right).abs();

        if d_left + d_right <= f32::EPSILON {
            return (idx_left, idx_right, 0.0);
        }

        if d_left <= d_right {
            let mix = d_left / (d_left + d_right);
            (idx_left, idx_right, mix)
        } else {
            let mix = d_right / (d_left + d_right);
            (idx_right, idx_left, mix)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::LinearColor;
    use proptest::prelude::*;

    fn palette_from_linear(colors: &[[f32; 3]]) -> Palette {
        Palette {
            id: 1,
            name: "test".into(),
            colors: colors
                .iter()
                .map(|&[r, g, b]| LinearColor { r, g, b })
                .collect(),
            revision: 1,
        }
    }

    #[test]
    fn build_rejects_empty() {
        let pal = Palette {
            id: 1,
            name: "empty".into(),
            colors: vec![],
            revision: 1,
        };
        assert!(matches!(
            BrightnessSortedPalette::build(&pal),
            Err(PaletteError::Empty)
        ));
    }

    #[test]
    fn build_sorts_by_ascending_l() {
        // Distinct luminances: black, mid gray, white (linear).
        let pal = palette_from_linear(&[[0.5, 0.5, 0.5], [1.0, 1.0, 1.0], [0.0, 0.0, 0.0]]);
        let sorted = BrightnessSortedPalette::build(&pal).unwrap();
        let ls: Vec<f32> = sorted.entries().iter().map(|(l, _)| *l).collect();
        for w in ls.windows(2) {
            assert!(w[0] <= w[1], "not sorted: {ls:?}");
        }
        assert_eq!(sorted.entries()[0].1, 2); // black
        assert_eq!(sorted.entries()[2].1, 1); // white
    }

    #[test]
    fn build_tolerates_duplicate_l() {
        let pal = palette_from_linear(&[
            [0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5], // duplicate mid gray
            [0.0, 0.0, 0.0],
        ]);
        let sorted = BrightnessSortedPalette::build(&pal).unwrap();
        assert_eq!(sorted.entries().len(), 3);
        let ls: Vec<f32> = sorted.entries().iter().map(|(l, _)| *l).collect();
        for w in ls.windows(2) {
            assert!(w[0] <= w[1]);
        }
    }

    #[test]
    fn two_nearest_below_darkest_stays_on_edge() {
        let pal = palette_from_linear(&[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]);
        let sorted = BrightnessSortedPalette::build(&pal).unwrap();
        let (i1, i2, mix) = sorted.two_nearest(-1.0);
        assert_eq!(mix, 0.0);
        // With mix=0, OrderedPalettePicker always picks i1 for t in [0,1]
        assert_eq!(i1, sorted.entries()[0].1);
        assert_eq!(i2, sorted.entries()[1].1);
        assert_eq!(sorted.nearest(-1.0), i1);
    }

    #[test]
    fn two_nearest_above_lightest_stays_on_edge() {
        let pal = palette_from_linear(&[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]);
        let sorted = BrightnessSortedPalette::build(&pal).unwrap();
        let (i1, i2, mix) = sorted.two_nearest(2.0);
        assert_eq!(mix, 0.0);
        // With mix=0, OrderedPalettePicker always picks i1 for t in [0,1]
        assert_eq!(i1, sorted.entries()[1].1); // lightest
        assert_eq!(i2, sorted.entries()[0].1);
        assert_eq!(sorted.nearest(2.0), i1);
    }

    #[test]
    fn two_nearest_midpoint_between_neighbors() {
        let pal = palette_from_linear(&[[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]);
        let sorted = BrightnessSortedPalette::build(&pal).unwrap();
        let l0 = sorted.entries()[0].0;
        let l1 = sorted.entries()[1].0;
        let mid = 0.5 * (l0 + l1);
        let (i1, i2, mix) = sorted.two_nearest(mid);
        assert!((mix - 0.5).abs() < 1e-4, "mix={mix}");
        assert_ne!(i1, i2);
    }

    #[test]
    fn single_color_palette() {
        let pal = palette_from_linear(&[[0.2, 0.3, 0.4]]);
        let sorted = BrightnessSortedPalette::build(&pal).unwrap();
        let (i1, i2, mix) = sorted.two_nearest(0.5);
        assert_eq!(i1, 0);
        assert_eq!(i2, 0);
        assert_eq!(mix, 0.0);
    }

    proptest! {
        #[test]
        fn prop_build_always_sorted(
            colors in prop::collection::vec(
                (0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0),
                1..=32
            )
        ) {
            let pal = Palette {
                id: 1,
                name: "arb".into(),
                colors: colors
                    .iter()
                    .map(|&(r, g, b)| LinearColor { r, g, b })
                    .collect(),
                revision: 1,
            };
            let sorted = BrightnessSortedPalette::build(&pal).unwrap();
            prop_assert_eq!(sorted.entries().len(), pal.colors.len());
            for w in sorted.entries().windows(2) {
                prop_assert!(w[0].0 <= w[1].0);
            }
            // Every palette index appears exactly once
            let mut idxs: Vec<usize> = sorted.entries().iter().map(|(_, i)| *i).collect();
            idxs.sort_unstable();
            prop_assert_eq!(idxs, (0..pal.colors.len()).collect::<Vec<_>>());
        }

        #[test]
        fn prop_two_nearest_indices_in_range(
            colors in prop::collection::vec(
                (0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0),
                1..=24
            ),
            pixel_l in -0.5f32..1.5f32
        ) {
            let pal = Palette {
                id: 1,
                name: "arb".into(),
                colors: colors
                    .iter()
                    .map(|&(r, g, b)| LinearColor { r, g, b })
                    .collect(),
                revision: 1,
            };
            let sorted = BrightnessSortedPalette::build(&pal).unwrap();
            let (i1, i2, mix) = sorted.two_nearest(pixel_l);
            prop_assert!(i1 < pal.colors.len());
            prop_assert!(i2 < pal.colors.len());
            prop_assert!((0.0..=1.0).contains(&mix) || mix.is_finite());
        }
    }
}
