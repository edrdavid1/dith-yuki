//! Document model and thread-safe access.

use crate::layer::LayerNode;
use crate::types::{ColorProfileRef, DocumentId, PaletteId};
use arc_swap::ArcSwap;
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

    /// List of color palettes used in document
    pub palettes: Vec<PaletteId>,

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
            palettes: Vec<PaletteId>,
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
}
