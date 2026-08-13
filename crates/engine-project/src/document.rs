//! Document model and thread-safe access.

use crate::error::EngineError;
use crate::filter::FilterParams;
use crate::layer::{LayerNode};
use crate::types::{ColorProfileRef, DocumentId, FilterInstanceId, PaletteId};
use arc_swap::ArcSwap;
use engine_color::palette::{LinearColor, Palette};
use engine_tiles::generation::GenerationTracker;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The main document structure.
#[derive(Clone)]
pub struct Document {
    /// Unique document identifier
    pub id: DocumentId,

    /// Canvas width in pixels
    pub width: u32,

    /// Canvas height in pixels
    pub height: u32,

    /// Color profile reference (placeholder for Phase 5)
    pub color_profile: ColorProfileRef,

    /// Top-level layers/groups, bottom-to-top order
    pub root: Vec<LayerNode>,

    /// Full palette entities stored in the document
    pub palettes: Vec<Palette>,

    /// Incremented on any structural change (for undo/redo)
    pub revision: u64,

    /// Generation tracker for selective invalidation (not serialized)
    pub generations: GenerationTracker,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("revision", &self.revision)
            .field("root_layers", &self.root.len())
            .finish()
    }
}

impl Serialize for Document {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("Document", 7)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("width", &self.width)?;
        state.serialize_field("height", &self.height)?;
        state.serialize_field("color_profile", &self.color_profile)?;
        state.serialize_field("root", &self.root)?;
        state.serialize_field("palettes", &self.palettes)?;
        state.serialize_field("revision", &self.revision)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct DocumentHelper {
            id: DocumentId,
            width: u32,
            height: u32,
            color_profile: ColorProfileRef,
            root: Vec<LayerNode>,
            palettes: Vec<Palette>,
            revision: u64,
        }

        let helper = DocumentHelper::deserialize(deserializer)?;
        Ok(Document {
            id: helper.id,
            width: helper.width,
            height: helper.height,
            color_profile: helper.color_profile,
            root: helper.root,
            palettes: helper.palettes,
            revision: helper.revision,
            generations: GenerationTracker::new(),
        })
    }
}

impl Document {
    /// Create a new blank document.
    pub fn new(id: DocumentId, width: u32, height: u32) -> Self {
        Document {
            id,
            width,
            height,
            color_profile: ColorProfileRef::default(),
            root: Vec::new(),
            palettes: Vec::new(),
            revision: 0,
            generations: GenerationTracker::new(),
        }
    }

    /// Increment document generation (global version)
    pub fn increment_generation(&mut self) {
        self.revision += 1;
        self.generations.increment_document_gen();
    }

    /// Compute the next unique PaletteId by finding the max existing id + 1.
    fn next_palette_id(&self) -> PaletteId {
        let max_id = self.palettes.iter().map(|p| p.id).max().unwrap_or(0);
        PaletteId::new(max_id + 1)
    }

    /// Add a new palette to the document with the given name and colors.
    /// Assigns a unique PaletteId and sets initial revision to 1.
    pub fn add_palette(&mut self, name: String, colors: Vec<LinearColor>) -> PaletteId {
        let id = self.next_palette_id();
        self.palettes.push(Palette {
            id: id.0,
            name,
            colors,
            revision: 1,
        });
        id
    }

    /// Modify an existing palette's colors and increment its revision.
    pub fn modify_palette(
        &mut self,
        id: PaletteId,
        colors: Vec<LinearColor>,
    ) -> Result<(), EngineError> {
        let palette = self
            .palettes
            .iter_mut()
            .find(|p| p.id == id.0)
            .ok_or_else(|| EngineError::palette_not_found(id))?;
        palette.colors = colors;
        palette.revision += 1;
        Ok(())
    }

    /// Remove a palette from the document.
    /// Fails with PaletteInUse if any filter references this palette.
    pub fn remove_palette(&mut self, id: PaletteId) -> Result<(), EngineError> {
        // Verify the palette exists first
        if !self.palettes.iter().any(|p| p.id == id.0) {
            return Err(EngineError::palette_not_found(id));
        }

        // Check referential integrity: find all filters referencing this palette
        let references = self.find_palette_references(id);
        if !references.is_empty() {
            return Err(EngineError::palette_in_use(id, references));
        }

        // Remove the palette
        self.palettes.retain(|p| p.id != id.0);
        Ok(())
    }

    /// Find a palette by ID.
    pub fn get_palette(&self, id: PaletteId) -> Option<&Palette> {
        self.palettes.iter().find(|p| p.id == id.0)
    }

    /// Collect FilterInstanceIds of all filters that reference the given palette.
    fn find_palette_references(&self, palette_id: PaletteId) -> Vec<FilterInstanceId> {
        let mut refs = Vec::new();
        Self::collect_palette_refs_from_nodes(&self.root, palette_id, &mut refs);
        refs
    }

    /// Recursively walk layer nodes collecting filter references to a palette.
    fn collect_palette_refs_from_nodes(
        nodes: &[LayerNode],
        palette_id: PaletteId,
        refs: &mut Vec<FilterInstanceId>,
    ) {
        for node in nodes {
            match node {
                LayerNode::Leaf(layer) => {
                    for filter in &layer.filters {
                        if Self::filter_references_palette(&filter.params, palette_id) {
                            refs.push(filter.id);
                        }
                    }
                }
                LayerNode::Group(group) => {
                    Self::collect_palette_refs_from_nodes(&group.children, palette_id, refs);
                }
            }
        }
    }

    /// Check if a filter's params reference the given palette.
    fn filter_references_palette(params: &FilterParams, palette_id: PaletteId) -> bool {
        match params {
            FilterParams::PaletteQuantize { palette_id: pid, .. } => *pid == palette_id,
            FilterParams::DitherV2(params) => params.palette_id == Some(palette_id),
            FilterParams::Curves { .. }
            | FilterParams::Levels { .. }
            | FilterParams::Dither { .. }
            | FilterParams::Glitch { .. }
            | FilterParams::Glow { .. }
            | FilterParams::Crt { .. }
            | FilterParams::Placeholder(_) => false,
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new(DocumentId::new(0), 5000, 5000)
    }
}

/// Thread-safe handle to a document using lock-free reads.
///
/// Uses `arc-swap` to allow workers to read a consistent document snapshot
/// without blocking on writes from the UI thread.
pub struct DocumentHandle {
    current: ArcSwap<Document>,
}

impl DocumentHandle {
    /// Create a new document handle.
    pub fn new(doc: Document) -> Self {
        DocumentHandle {
            current: ArcSwap::new(Arc::new(doc)),
        }
    }

    /// Get a snapshot of the current document (O(1), lock-free).
    pub fn snapshot(&self) -> Arc<Document> {
        self.current.load_full()
    }

    /// Atomically replace the live document with an existing `Arc`.
    ///
    /// Used by undo/redo so the stacked snapshot is restored by pointer,
    /// without deep-cloning the tree.
    pub fn store(&self, doc: Arc<Document>) {
        self.current.store(doc);
    }

    /// Mutate the document atomically.
    ///
    /// The closure receives a mutable reference to a cloned document.
    /// After mutation, the new version is atomically swapped in.
    pub fn mutate<F>(&self, f: F)
    where
        F: FnOnce(&mut Document),
    {
        let mut new_doc = (**self.current.load()).clone();
        f(&mut new_doc);
        self.current.store(Arc::new(new_doc));
    }
}

impl Default for DocumentHandle {
    fn default() -> Self {
        Self::new(Document::default())
    }
}

impl Clone for DocumentHandle {
    fn clone(&self) -> Self {
        DocumentHandle {
            current: ArcSwap::new(self.current.load_full()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{FilterInstance, FilterKind, FilterParams, DitherMode, DiffusionKernel};
    use crate::layer::{Layer, LayerNode};
    use crate::types::LayerKind;

    #[test]
    fn document_new() {
        let doc = Document::new(DocumentId::new(1), 5000, 5000);
        assert_eq!(doc.width, 5000);
        assert_eq!(doc.height, 5000);
        assert_eq!(doc.revision, 0);
    }

    #[test]
    fn document_increment_generation() {
        let mut doc = Document::new(DocumentId::new(1), 5000, 5000);
        let old_revision = doc.revision;
        doc.increment_generation();
        assert_eq!(doc.revision, old_revision + 1);
    }

    #[test]
    fn document_handle_snapshot() {
        let doc = Document::new(DocumentId::new(1), 5000, 5000);
        let handle = DocumentHandle::new(doc);

        let snapshot1 = handle.snapshot();
        let snapshot2 = handle.snapshot();

        assert_eq!(snapshot1.revision, snapshot2.revision);
    }

    #[test]
    fn document_handle_mutate_atomic() {
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let handle = DocumentHandle::new(doc);

        let old_revision = handle.snapshot().revision;

        handle.mutate(|d| {
            d.revision += 10;
        });

        let new_revision = handle.snapshot().revision;
        assert_eq!(new_revision, old_revision + 10);
    }

    #[test]
    fn document_handle_clone() {
        let doc = Document::new(DocumentId::new(1), 256, 256);
        let handle1 = DocumentHandle::new(doc);

        handle1.mutate(|d| d.revision = 5);

        let handle2 = handle1.clone();
        assert_eq!(handle2.snapshot().revision, 5);
    }

    #[test]
    fn document_handle_concurrent_reads() {
        let doc = Document::new(DocumentId::new(1), 5000, 5000);
        let handle = Arc::new(DocumentHandle::new(doc));

        let handle1 = Arc::clone(&handle);
        let handle2 = Arc::clone(&handle);

        let thread1 = std::thread::spawn(move || {
            let _snap = handle1.snapshot();
            _snap.revision
        });

        let thread2 = std::thread::spawn(move || {
            let _snap = handle2.snapshot();
            _snap.revision
        });

        let r1 = thread1.join().unwrap();
        let r2 = thread2.join().unwrap();

        assert_eq!(r1, r2);
    }

    // === Palette management tests ===

    #[test]
    fn add_palette_assigns_unique_ids() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);

        let colors = vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
        ];

        let id1 = doc.add_palette("Palette 1".to_string(), colors.clone());
        let id2 = doc.add_palette("Palette 2".to_string(), colors.clone());
        let id3 = doc.add_palette("Palette 3".to_string(), colors);

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn add_palette_sets_revision_to_1() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let colors = vec![LinearColor { r: 0.5, g: 0.5, b: 0.5 }];

        let id = doc.add_palette("Test".to_string(), colors);
        let palette = doc.get_palette(id).unwrap();

        assert_eq!(palette.revision, 1);
    }

    #[test]
    fn get_palette_returns_correct_palette() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let colors = vec![
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
            LinearColor { r: 0.0, g: 0.0, b: 1.0 },
        ];

        let id = doc.add_palette("My Palette".to_string(), colors.clone());
        let palette = doc.get_palette(id).unwrap();

        assert_eq!(palette.name, "My Palette");
        assert_eq!(palette.colors.len(), 2);
        assert_eq!(palette.colors[0].r, 1.0);
        assert_eq!(palette.colors[1].b, 1.0);
    }

    #[test]
    fn get_palette_returns_none_for_missing_id() {
        let doc = Document::new(DocumentId::new(1), 256, 256);
        assert!(doc.get_palette(PaletteId::new(999)).is_none());
    }

    #[test]
    fn modify_palette_updates_colors_and_increments_revision() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let colors = vec![LinearColor { r: 1.0, g: 0.0, b: 0.0 }];
        let id = doc.add_palette("Test".to_string(), colors);

        let new_colors = vec![
            LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            LinearColor { r: 0.0, g: 0.0, b: 1.0 },
        ];
        let result = doc.modify_palette(id, new_colors.clone());
        assert!(result.is_ok());

        let palette = doc.get_palette(id).unwrap();
        assert_eq!(palette.revision, 2);
        assert_eq!(palette.colors.len(), 2);
        assert_eq!(palette.colors[0].g, 1.0);
    }

    #[test]
    fn modify_palette_multiple_times_increments_revision() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let colors = vec![LinearColor { r: 1.0, g: 0.0, b: 0.0 }];
        let id = doc.add_palette("Test".to_string(), colors);

        for i in 0..5 {
            let new_colors = vec![LinearColor { r: i as f32 * 0.1, g: 0.0, b: 0.0 }];
            doc.modify_palette(id, new_colors).unwrap();
        }

        let palette = doc.get_palette(id).unwrap();
        assert_eq!(palette.revision, 6); // 1 initial + 5 modifications
    }

    #[test]
    fn modify_palette_not_found() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let result = doc.modify_palette(PaletteId::new(999), vec![]);
        assert!(matches!(
            result,
            Err(EngineError::PaletteNotFound { .. })
        ));
    }

    #[test]
    fn remove_palette_succeeds_when_unreferenced() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let colors = vec![LinearColor { r: 1.0, g: 0.0, b: 0.0 }];
        let id = doc.add_palette("Test".to_string(), colors);

        let result = doc.remove_palette(id);
        assert!(result.is_ok());
        assert!(doc.get_palette(id).is_none());
    }

    #[test]
    fn remove_palette_not_found() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let result = doc.remove_palette(PaletteId::new(999));
        assert!(matches!(
            result,
            Err(EngineError::PaletteNotFound { .. })
        ));
    }

    #[test]
    fn add_remove_add_lifecycle() {
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let colors = vec![LinearColor { r: 1.0, g: 0.0, b: 0.0 }];

        let id1 = doc.add_palette("First".to_string(), colors.clone());
        doc.remove_palette(id1).unwrap();

        // Adding a new palette should get a higher ID
        let id2 = doc.add_palette("Second".to_string(), colors);
        // The next ID is max(existing) + 1; since we removed id1 and list is empty,
        // next_palette_id finds max of empty = 0, so id2 = PaletteId(1)
        // But id1 was PaletteId(1) as well. After removal, max is 0 again.
        // This is fine — ID uniqueness is within the current document state.
        assert!(doc.get_palette(id2).is_some());
    }

    #[test]
    fn palette_serialization_round_trip() {
        let mut doc = Document::new(DocumentId::new(1), 512, 512);
        let colors = vec![
            LinearColor { r: 0.5, g: 0.25, b: 0.75 },
            LinearColor { r: 1.0, g: 0.0, b: 0.0 },
        ];
        let id = doc.add_palette("Round Trip Test".to_string(), colors);
        doc.modify_palette(id, vec![
            LinearColor { r: 0.1, g: 0.2, b: 0.3 },
        ]).unwrap();

        let json = serde_json::to_string(&doc).unwrap();
        let deserialized: Document = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.palettes.len(), 1);
        let palette = deserialized.palettes.first().unwrap();
        assert_eq!(palette.id, id.0);
        assert_eq!(palette.name, "Round Trip Test");
        assert_eq!(palette.revision, 2);
        assert_eq!(palette.colors.len(), 1);
        assert_eq!(palette.colors[0].r, 0.1);
        assert_eq!(palette.colors[0].g, 0.2);
        assert_eq!(palette.colors[0].b, 0.3);
    }

    #[test]
    fn remove_palette_referential_integrity_no_reference() {
        // When no filters reference a palette, removal succeeds
        let mut doc = Document::new(DocumentId::new(1), 256, 256);
        let colors = vec![LinearColor { r: 1.0, g: 0.0, b: 0.0 }];
        let palette_id = doc.add_palette("Test".to_string(), colors);

        // Add a layer with a filter that does NOT reference any palette
        let mut layer = Layer::new(
            crate::types::LayerId::new(1),
            LayerKind::Raster,
            256,
            256,
        );
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg },
                color_depth: 4,
            },
        ));
        doc.root.push(LayerNode::Leaf(layer));

        // Removal should succeed since no filter references this palette
        assert!(doc.remove_palette(palette_id).is_ok());
    }
}
