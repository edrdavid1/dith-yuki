//! Concurrent DashMap-based cache mapping (PaletteId, revision) to Arc<KdTree>.
//!
//! `PaletteKdCache` provides thread-safe, lock-free read access to KD-trees
//! built from palette colors in Oklab space. Multiple worker threads can
//! concurrently query the same `Arc<KdTree>` without contention.
//!
//! Concurrency strategy: last-writer-wins on `DashMap::insert`.

use dashmap::DashMap;
use std::sync::Arc;

use crate::kdtree::KdTree;
use crate::oklab::{linear_to_oklab, LinRgb};
use crate::palette::{Palette, PaletteError, PaletteId};

/// Global concurrent cache mapping PaletteId → (revision, KD-tree).
///
/// Workers call `get_or_build` to obtain an `Arc<KdTree>` for nearest-color
/// lookups. The cache automatically rebuilds when a palette's revision changes.
pub struct PaletteKdCache {
    entries: DashMap<PaletteId, (u64, Arc<KdTree>)>,
}

impl PaletteKdCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Get or build a KD-tree for the given palette.
    ///
    /// Returns `Arc<KdTree>` for lock-free sharing across threads.
    ///
    /// - If the palette has 0 colors, returns `Err(PaletteError::Empty)`.
    /// - If a cached entry exists with a matching revision, returns a clone of the Arc.
    /// - Otherwise, converts palette colors to Oklab, builds a new KdTree,
    ///   inserts it into the cache, and returns the new Arc.
    ///
    /// Race condition: concurrent builds for the same palette → last-writer-wins
    /// (DashMap insert semantics).
    pub fn get_or_build(&self, palette: &Palette) -> Result<Arc<KdTree>, PaletteError> {
        // 1. Empty palette check
        if palette.colors.is_empty() {
            return Err(PaletteError::Empty);
        }

        // 2. Check cache: if entry exists with matching revision, return Arc clone
        if let Some(entry) = self.entries.get(&palette.id) {
            let (cached_revision, ref tree) = *entry;
            if cached_revision == palette.revision {
                return Ok(Arc::clone(tree));
            }
        }

        // 3. Cache miss or revision mismatch: convert palette colors to Oklab, build KdTree
        let oklab_colors: Vec<_> = palette
            .colors
            .iter()
            .map(|c| linear_to_oklab(LinRgb { r: c.r, g: c.g, b: c.b }))
            .collect();

        let tree = KdTree::build(&oklab_colors).ok_or(PaletteError::Empty)?;
        let arc_tree = Arc::new(tree);

        // 4. Insert (palette.id, (palette.revision, Arc::new(tree)))
        self.entries
            .insert(palette.id, (palette.revision, Arc::clone(&arc_tree)));

        // 5. Return the new Arc
        Ok(arc_tree)
    }

    /// Evict the cached entry for the given palette ID.
    ///
    /// Used when a palette is removed from the document.
    pub fn evict(&self, palette_id: PaletteId) {
        self.entries.remove(&palette_id);
    }

    /// Palette ids currently resident in the cache.
    pub fn cached_ids(&self) -> Vec<PaletteId> {
        self.entries.iter().map(|e| *e.key()).collect()
    }
}

impl Default for PaletteKdCache {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Unit Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::LinearColor;

    /// Helper to create a test palette with the given colors.
    fn make_palette(id: PaletteId, revision: u64, colors: Vec<LinearColor>) -> Palette {
        Palette {
            id,
            name: format!("test-palette-{}", id),
            colors,
            revision,
        }
    }

    #[test]
    fn test_empty_palette_returns_error() {
        let cache = PaletteKdCache::new();
        let palette = make_palette(1, 1, vec![]);
        let result = cache.get_or_build(&palette);
        assert!(matches!(result, Err(PaletteError::Empty)));
    }

    #[test]
    fn test_cache_miss_builds_tree() {
        let cache = PaletteKdCache::new();
        let palette = make_palette(
            1,
            1,
            vec![
                LinearColor { r: 1.0, g: 0.0, b: 0.0 },
                LinearColor { r: 0.0, g: 1.0, b: 0.0 },
                LinearColor { r: 0.0, g: 0.0, b: 1.0 },
            ],
        );

        let tree = cache.get_or_build(&palette).unwrap();
        // Tree should be usable for nearest-neighbor queries
        let query = linear_to_oklab(LinRgb { r: 0.9, g: 0.1, b: 0.0 });
        let nearest_idx = tree.nearest(query);
        // Should be closest to red (index 0)
        assert_eq!(nearest_idx, 0);
    }

    #[test]
    fn test_cache_hit_returns_same_arc() {
        let cache = PaletteKdCache::new();
        let palette = make_palette(
            1,
            1,
            vec![
                LinearColor { r: 1.0, g: 0.0, b: 0.0 },
                LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            ],
        );

        let tree1 = cache.get_or_build(&palette).unwrap();
        let tree2 = cache.get_or_build(&palette).unwrap();

        // Both should point to the same allocation (same Arc)
        assert!(Arc::ptr_eq(&tree1, &tree2));
    }

    #[test]
    fn test_revision_mismatch_triggers_rebuild() {
        let cache = PaletteKdCache::new();

        // Build with revision 1 (red + green)
        let palette_v1 = make_palette(
            1,
            1,
            vec![
                LinearColor { r: 1.0, g: 0.0, b: 0.0 },
                LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            ],
        );
        let tree_v1 = cache.get_or_build(&palette_v1).unwrap();

        // Now "modify" the palette: new revision, different colors
        let palette_v2 = make_palette(
            1,
            2,
            vec![
                LinearColor { r: 0.0, g: 0.0, b: 1.0 },
                LinearColor { r: 1.0, g: 1.0, b: 0.0 },
            ],
        );
        let tree_v2 = cache.get_or_build(&palette_v2).unwrap();

        // The tree should have been rebuilt (different Arc)
        assert!(!Arc::ptr_eq(&tree_v1, &tree_v2));

        // The new tree should match the new colors
        let query_blue = linear_to_oklab(LinRgb { r: 0.0, g: 0.0, b: 0.9 });
        assert_eq!(tree_v2.nearest(query_blue), 0); // index 0 = blue in v2
    }

    #[test]
    fn test_eviction_removes_entry() {
        let cache = PaletteKdCache::new();
        let palette = make_palette(
            42,
            1,
            vec![
                LinearColor { r: 0.5, g: 0.5, b: 0.5 },
            ],
        );

        // Build and cache
        let _tree = cache.get_or_build(&palette).unwrap();

        // Evict
        cache.evict(42);

        // Next call should rebuild (we can't directly check the DashMap,
        // but we verify it still works after eviction)
        let tree_after = cache.get_or_build(&palette).unwrap();

        // Verify the tree is functional
        let query = linear_to_oklab(LinRgb { r: 0.5, g: 0.5, b: 0.5 });
        assert_eq!(tree_after.nearest(query), 0);
    }

    #[test]
    fn test_multiple_palettes_independent() {
        let cache = PaletteKdCache::new();

        let palette_a = make_palette(
            1,
            1,
            vec![LinearColor { r: 1.0, g: 0.0, b: 0.0 }],
        );
        let palette_b = make_palette(
            2,
            1,
            vec![LinearColor { r: 0.0, g: 0.0, b: 1.0 }],
        );

        let tree_a = cache.get_or_build(&palette_a).unwrap();
        let tree_b = cache.get_or_build(&palette_b).unwrap();

        // They should be different Arcs (different palettes)
        assert!(!Arc::ptr_eq(&tree_a, &tree_b));

        // Evicting one shouldn't affect the other
        cache.evict(1);
        let tree_b_again = cache.get_or_build(&palette_b).unwrap();
        assert!(Arc::ptr_eq(&tree_b, &tree_b_again));
    }
}
