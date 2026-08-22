//! Concurrent DashMap-based cache mapping (doc, PaletteId, revision) to Arc<KdTree>.
//!
//! `PaletteKdCache` provides thread-safe, lock-free read access to KD-trees
//! built from palette colors in Oklab space. Multiple worker threads can
//! concurrently query the same `Arc<KdTree>` without contention.
//!
//! Keys include **runtime document id** so two open docs with the same
//! file-local `PaletteId` (both often `1`) never share a tree.
//!
//! Concurrency strategy: last-writer-wins on `DashMap::insert`.

use dashmap::DashMap;
use std::sync::Arc;

use crate::kdtree::KdTree;
use crate::oklab::{linear_to_oklab, LinRgb};
use crate::palette::{Palette, PaletteError, PaletteId};

/// `(runtime_doc_id, palette_id)` — palette ids are only unique within a document.
pub type PaletteCacheKey = (u32, PaletteId);

/// Global concurrent cache mapping PaletteCacheKey → (revision, KD-tree).
pub struct PaletteKdCache {
    entries: DashMap<PaletteCacheKey, (u64, Arc<KdTree>)>,
}

impl PaletteKdCache {
    /// Create a new empty cache.
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Get or build a KD-tree for the given document-scoped palette.
    pub fn get_or_build(
        &self,
        doc_id: u32,
        palette: &Palette,
    ) -> Result<Arc<KdTree>, PaletteError> {
        if palette.colors.is_empty() {
            return Err(PaletteError::Empty);
        }

        let key = (doc_id, palette.id);
        if let Some(entry) = self.entries.get(&key) {
            let (cached_revision, ref tree) = *entry;
            if cached_revision == palette.revision {
                return Ok(Arc::clone(tree));
            }
        }

        let oklab_colors: Vec<_> = palette
            .colors
            .iter()
            .map(|c| linear_to_oklab(LinRgb { r: c.r, g: c.g, b: c.b }))
            .collect();

        let tree = KdTree::build(&oklab_colors).ok_or(PaletteError::Empty)?;
        let arc_tree = Arc::new(tree);

        self.entries
            .insert(key, (palette.revision, Arc::clone(&arc_tree)));

        Ok(arc_tree)
    }

    /// Evict one palette for one document.
    pub fn evict(&self, doc_id: u32, palette_id: PaletteId) {
        self.entries.remove(&(doc_id, palette_id));
    }

    /// Drop every cached tree belonging to a closed document session.
    pub fn evict_document(&self, doc_id: u32) {
        self.entries.retain(|&(doc, _), _| doc != doc_id);
    }

    /// Keys currently resident in the cache.
    pub fn cached_keys(&self) -> Vec<PaletteCacheKey> {
        self.entries.iter().map(|e| *e.key()).collect()
    }
}

impl Default for PaletteKdCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::LinearColor;

    fn make_palette(id: PaletteId, n: usize, revision: u64) -> Palette {
        Palette {
            id,
            name: format!("p{id}"),
            colors: (0..n)
                .map(|i| LinearColor {
                    r: i as f32 / n.max(1) as f32,
                    g: 0.0,
                    b: 0.0,
                })
                .collect(),
            revision,
        }
    }

    #[test]
    fn same_palette_id_different_docs_are_isolated() {
        let cache = PaletteKdCache::new();
        let a = make_palette(1, 50, 1);
        let b = make_palette(1, 15, 1);
        let tree_a = cache.get_or_build(1, &a).unwrap();
        let tree_b = cache.get_or_build(2, &b).unwrap();
        assert!(!Arc::ptr_eq(&tree_a, &tree_b));
        // Doc 2 must not see doc 1's larger tree indices.
        assert!(tree_b.nearest(linear_to_oklab(LinRgb {
            r: 0.5,
            g: 0.0,
            b: 0.0
        })) < 15);
    }

    #[test]
    fn revision_mismatch_rebuilds() {
        let cache = PaletteKdCache::new();
        let v1 = make_palette(1, 4, 1);
        let t1 = cache.get_or_build(1, &v1).unwrap();
        let v2 = make_palette(1, 4, 2);
        let t2 = cache.get_or_build(1, &v2).unwrap();
        assert!(!Arc::ptr_eq(&t1, &t2));
    }

    #[test]
    fn evict_document_drops_only_that_doc() {
        let cache = PaletteKdCache::new();
        let _ = cache.get_or_build(1, &make_palette(1, 3, 1)).unwrap();
        let keep = cache.get_or_build(2, &make_palette(1, 3, 1)).unwrap();
        cache.evict_document(1);
        let again = cache.get_or_build(2, &make_palette(1, 3, 1)).unwrap();
        assert!(Arc::ptr_eq(&keep, &again));
    }
}
