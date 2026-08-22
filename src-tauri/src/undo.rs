//! Snapshot undo/redo for `Document` (Track N).
//!
//! History is a bounded stack of `Arc<Document>` — the same structural-sharing
//! economy as `DocumentHandle::mutate`. Handlers record via [`with_document_undo`];
//! they must not touch the stacks themselves.
//!
//! All mutation / history ops take an explicit runtime `doc_id` (VS Code URI /
//! Photoshop documentID style). Never resolve the target via `active_session()`.

use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, MutexGuard};

use engine_project::document::Document;
use engine_project::layer::LayerNode;
use engine_project::types::LayerId;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::commands::{
    emit_document_changed, invalidate_after_document_replace, schedule_dirty_viewport_tiles,
    AppState,
};
use crate::document_session::DocumentSession;

/// Explicit history bound (Req 1.2).
pub const UNDO_MAX_DEPTH: usize = 50;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct UndoStateDto {
    pub can_undo: bool,
    pub can_redo: bool,
    #[serde(default)]
    pub doc_id: u32,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct DirtyDto {
    pub dirty: bool,
    #[serde(default)]
    pub doc_id: u32,
}

fn session_is_dirty(session: &DocumentSession) -> bool {
    if session.document_handle.snapshot().root.is_empty() {
        return false;
    }
    let live = session.document_handle.snapshot();
    match session.saved_snapshot.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(saved) => !Arc::ptr_eq(saved, &live),
            None => true,
        },
        Err(_) => true,
    }
}

/// Track P: dirty for a specific session.
pub fn is_dirty_doc(state: &AppState, doc_id: u32) -> bool {
    let Ok(session) = state.require_session(doc_id) else {
        return false;
    };
    session_is_dirty(&session)
}

/// Active-tab dirty (chrome poll / welcome). Prefer [`is_dirty_doc`] when id is known.
pub fn is_dirty(state: &AppState) -> bool {
    let Some(id) = state.active_id() else {
        return false;
    };
    is_dirty_doc(state, id)
}

/// Remember the live Arc as clean for `doc_id` (after save or document replace).
pub fn mark_clean_doc(state: &AppState, doc_id: u32) {
    let session = match state.require_session(doc_id) {
        Ok(s) => s,
        Err(_) => return,
    };
    let live = session.document_handle.snapshot();
    let mut guard = match session.saved_snapshot.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    *guard = Some(live);
}

pub fn mark_clean(state: &AppState) {
    if let Some(id) = state.active_id() {
        mark_clean_doc(state, id);
    }
}

pub fn emit_dirty_doc(app: Option<&AppHandle>, state: &AppState, doc_id: u32) {
    if let Some(app) = app {
        let _ = app.emit(
            "dirty-changed",
            DirtyDto {
                dirty: is_dirty_doc(state, doc_id),
                doc_id,
            },
        );
    }
}

pub fn emit_dirty(app: Option<&AppHandle>, state: &AppState) {
    if let Some(id) = state.active_id() {
        emit_dirty_doc(app, state, id);
    }
}

pub struct UndoManager {
    undo_stack: VecDeque<Arc<Document>>,
    redo_stack: Vec<Arc<Document>>,
    max_depth: usize,
}

impl Default for UndoManager {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoManager {
    pub fn new() -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            max_depth: UNDO_MAX_DEPTH,
        }
    }

    pub fn state_dto(&self) -> UndoStateDto {
        UndoStateDto {
            can_undo: !self.undo_stack.is_empty(),
            can_redo: !self.redo_stack.is_empty(),
            doc_id: 0,
        }
    }

    pub fn state_dto_for(&self, doc_id: u32) -> UndoStateDto {
        UndoStateDto {
            can_undo: !self.undo_stack.is_empty(),
            can_redo: !self.redo_stack.is_empty(),
            doc_id,
        }
    }
}

fn lock_undo(session: &DocumentSession) -> Result<MutexGuard<'_, UndoManager>, String> {
    session
        .undo_manager
        .lock()
        .map_err(|e| format!("Undo lock poisoned: {e}"))
}

fn collect_layer_ids(doc: &Document) -> HashSet<u32> {
    fn walk(nodes: &[LayerNode], out: &mut HashSet<u32>) {
        for node in nodes {
            match node {
                LayerNode::Leaf(layer) => {
                    out.insert(layer.id.0);
                }
                LayerNode::Group(group) => {
                    out.insert(group.id.0);
                    walk(&group.children, out);
                }
            }
        }
    }
    let mut ids = HashSet::new();
    walk(&doc.root, &mut ids);
    ids
}

fn referenced_layer_ids(undo: &UndoManager, live: &Document) -> HashSet<u32> {
    let mut ids = collect_layer_ids(live);
    for doc in undo.undo_stack.iter().chain(undo.redo_stack.iter()) {
        ids.extend(collect_layer_ids(doc));
    }
    ids
}

fn evict_layer_all(state: &AppState, doc: u32, layer: u32) {
    state.tile_cache.evict_layer(doc, layer);
    state.error_residuals.evict_layer(doc, LayerId::new(layer));
    state.block_representatives.evict_layer(doc, layer);
}

/// Evict per-layer cache entries whose `LayerId` is in none of live + undo + redo.
fn gc_orphaned_layers(state: &AppState, undo: &UndoManager, live: &Document) {
    let referenced = referenced_layer_ids(undo, live);
    let mut candidates = HashSet::new();
    for entry in state.tile_cache.entries.iter() {
        if entry.key().doc == live.id.0 {
            candidates.insert(entry.key().layer);
        }
    }
    candidates.extend(state.error_residuals.cached_layer_ids());
    candidates.extend(state.block_representatives.cached_layer_ids());
    for layer in candidates {
        if !referenced.contains(&layer) {
            evict_layer_all(state, live.id.0, layer);
        }
    }
}

fn sync_palette_caches(state: &AppState, live: &Document) {
    let doc = live.id.0;
    let live_ids: HashSet<u32> = live.palettes.iter().map(|p| p.id).collect();
    for (d, id) in state.palette_cache.cached_keys() {
        if d == doc && !live_ids.contains(&id) {
            state.palette_cache.evict(d, id);
        }
    }
    for (d, id) in state.palette_lut_cache.cached_keys() {
        if d == doc && !live_ids.contains(&id) {
            state.palette_lut_cache.evict(d, id);
        }
    }
}

fn emit_undo_state(app: Option<&AppHandle>, dto: UndoStateDto) {
    if let Some(app) = app {
        let _ = app.emit("undo-state-changed", dto);
    }
}

/// Push `before`, trim to `max_depth`, clear redo, run Orphan_GC.
pub fn record_mutation(
    state: &AppState,
    doc_id: u32,
    before: Arc<Document>,
) -> Result<UndoStateDto, String> {
    let session = state.require_session(doc_id)?;
    let live = session.document_handle.snapshot();
    let dto = {
        let mut undo = lock_undo(&session)?;
        undo.undo_stack.push_back(before);
        if undo.undo_stack.len() > undo.max_depth {
            let _dropped = undo.undo_stack.pop_front();
        }
        undo.redo_stack.clear();
        gc_orphaned_layers(state, &undo, &live);
        undo.state_dto_for(doc_id)
    };
    sync_palette_caches(state, &live);
    Ok(dto)
}

/// Document replace: both stacks empty, GC vs live doc, emit `{false, false}`.
pub fn clear_history(
    state: &AppState,
    app: Option<&AppHandle>,
    doc_id: u32,
) -> Result<UndoStateDto, String> {
    let session = state.require_session(doc_id)?;
    let live = session.document_handle.snapshot();
    let dto = {
        let mut undo = lock_undo(&session)?;
        undo.undo_stack.clear();
        undo.redo_stack.clear();
        gc_orphaned_layers(state, &undo, &live);
        undo.state_dto_for(doc_id)
    };
    sync_palette_caches(state, &live);
    emit_undo_state(app, dto);
    mark_clean_doc(state, doc_id);
    emit_dirty_doc(app, state, doc_id);
    Ok(dto)
}

/// Capture `snapshot()` before `f`; on `Ok` record history; on `Err` leave stacks unchanged.
pub fn with_document_undo<F, T>(
    state: &AppState,
    app: Option<&AppHandle>,
    doc_id: u32,
    f: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let before = state.require_session(doc_id)?.document_handle.snapshot();
    let result = f()?;
    let dto = record_mutation(state, doc_id, before)?;
    emit_undo_state(app, dto);
    emit_dirty_doc(app, state, doc_id);
    Ok(result)
}

fn bump_live_document_gen(state: &AppState, doc_id: u32) {
    let Ok(session) = state.require_session(doc_id) else {
        return;
    };
    let live = session.document_handle.snapshot();
    let live_gen = live.generations.current_document_gen();
    let next = live_gen
        .max(state.tile_cache.max_generation())
        .saturating_add(1);
    live.generations.set_document_gen(next);
}

fn restore_and_invalidate(
    state: &AppState,
    app: &AppHandle,
    doc_id: u32,
    restored: Arc<Document>,
    kind: &str,
) -> Result<UndoStateDto, String> {
    let session = state.require_session(doc_id)?;
    session.document_handle.store(restored);
    bump_live_document_gen(state, doc_id);
    let live = session.document_handle.snapshot();
    let dto = {
        let undo = lock_undo(&session)?;
        gc_orphaned_layers(state, &undo, &live);
        undo.state_dto_for(doc_id)
    };
    sync_palette_caches(state, &live);
    if state.active_id() == Some(doc_id) {
        invalidate_after_document_replace(state);
        schedule_dirty_viewport_tiles(state);
    }
    emit_document_changed(app, kind, None, Some(doc_id));
    emit_undo_state(Some(app), dto);
    emit_dirty_doc(Some(app), state, doc_id);
    Ok(dto)
}

pub fn apply_undo(state: &AppState, app: &AppHandle, doc_id: u32) -> Result<UndoStateDto, String> {
    let restored = {
        let session = state.require_session(doc_id)?;
        let mut undo = lock_undo(&session)?;
        let Some(prev) = undo.undo_stack.pop_back() else {
            return Err("nothing to undo".to_string());
        };
        let current = session.document_handle.snapshot();
        undo.redo_stack.push(current);
        prev
    };
    restore_and_invalidate(state, app, doc_id, restored, "document_undone")
}

pub fn apply_redo(state: &AppState, app: &AppHandle, doc_id: u32) -> Result<UndoStateDto, String> {
    let restored = {
        let session = state.require_session(doc_id)?;
        let mut undo = lock_undo(&session)?;
        let Some(next) = undo.redo_stack.pop() else {
            return Err("nothing to redo".to_string());
        };
        let current = session.document_handle.snapshot();
        undo.undo_stack.push_back(current);
        next
    };
    restore_and_invalidate(state, app, doc_id, restored, "document_redone")
}

#[tauri::command]
pub fn undo(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UndoStateDto, String> {
    apply_undo(&state, &app_handle, doc_id)
}

#[tauri::command]
pub fn redo(
    doc_id: u32,
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UndoStateDto, String> {
    apply_redo(&state, &app_handle, doc_id)
}

#[tauri::command]
pub fn is_document_dirty(
    doc_id: Option<u32>,
    state: State<'_, Arc<AppState>>,
) -> Result<bool, String> {
    let id = match doc_id.or_else(|| state.active_id()) {
        Some(id) => id,
        None => return Ok(false),
    };
    Ok(is_dirty_doc(&state, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_project::document::DocumentHandle;
    use engine_project::layer::Layer;
    use engine_project::types::{DocumentId, LayerKind};
    use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};

    fn active_doc_id(state: &AppState) -> u32 {
        state.active_id().expect("test needs active doc")
    }

    fn dummy_mutate(state: &AppState) -> Result<(), String> {
        let doc_id = active_doc_id(state);
        with_document_undo(state, None, doc_id, || {
            state.require_session(doc_id)?.document_handle.mutate(|doc| {
                doc.increment_generation();
            });
            Ok(())
        })
    }

    fn add_test_layer(state: &AppState) -> u32 {
        let doc_id = active_doc_id(state);
        let mut id = 0u32;
        with_document_undo(state, None, doc_id, || {
            state.require_session(doc_id)?.document_handle.mutate(|doc| {
                id = doc
                    .root
                    .iter()
                    .map(|n| match n {
                        LayerNode::Leaf(l) => l.id.0,
                        LayerNode::Group(g) => g.id.0,
                    })
                    .max()
                    .unwrap_or(0)
                    .saturating_add(1);
                doc.root.push(LayerNode::Leaf(Layer::new(
                    LayerId::new(id),
                    LayerKind::Raster,
                    doc.width,
                    doc.height,
                )));
            });
            Ok(())
        })
        .unwrap();
        id
    }

    fn has_layer(doc: &Document, layer: u32) -> bool {
        collect_layer_ids(doc).contains(&layer)
    }

    fn test_state() -> Arc<AppState> {
        let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
        let doc = Document::new(DocumentId::new(1), 64, 64);
        state.spawn_session(doc);
        state
    }

    #[test]
    fn undo_redo_roundtrip_and_bounds() {
        let state = test_state();
        assert!(!lock_undo(&state.must_active()).unwrap().state_dto().can_undo);

        dummy_mutate(&state).unwrap();
        let dto = lock_undo(&state.must_active()).unwrap().state_dto();
        assert!(dto.can_undo);
        assert!(!dto.can_redo);

        // Failed mutation must not push
        let before_len = lock_undo(&state.must_active()).unwrap().undo_stack.len();
        let before_ptr = Arc::as_ptr(&state.must_active().document_handle.snapshot());
        let doc_id = active_doc_id(&state);
        let err = with_document_undo(&state, None, doc_id, || Err::<(), _>("nope".into()));
        assert!(err.is_err());
        assert_eq!(lock_undo(&state.must_active()).unwrap().undo_stack.len(), before_len);
        assert_eq!(
            Arc::as_ptr(&state.must_active().document_handle.snapshot()),
            before_ptr
        );

        // Undo then redo
        {
            let session = state.must_active();
            let mut undo = lock_undo(&session).unwrap();
            let prev = undo.undo_stack.pop_back().unwrap();
            let current = state.must_active().document_handle.snapshot();
            undo.redo_stack.push(current);
            state.must_active().document_handle.store(prev);
        }
        assert!(lock_undo(&state.must_active()).unwrap().state_dto().can_redo);

        let dto = lock_undo(&state.must_active()).unwrap().state_dto();
        assert!(!dto.can_undo);
        assert!(dto.can_redo);
        {
            let session = state.must_active();
            let mut undo = lock_undo(&session).unwrap();
            let next = undo.redo_stack.pop().unwrap();
            let current = state.must_active().document_handle.snapshot();
            undo.undo_stack.push_back(current);
            state.must_active().document_handle.store(next);
        }

        // Depth bound
        for _ in 0..UNDO_MAX_DEPTH + 5 {
            dummy_mutate(&state).unwrap();
        }
        assert_eq!(lock_undo(&state.must_active()).unwrap().undo_stack.len(), UNDO_MAX_DEPTH);
    }

    #[test]
    fn orphan_gc_drops_removed_layer_tiles() {
        let state = test_state();
        let layer = add_test_layer(&state);
        let key = TileKey {
            doc: 1,
            layer,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        state
            .tile_cache
            .insert_fresh_gen(key, Arc::new(PixelTile::new()), 1);

        // Remove layer via undoable mutate then undo → layer gone from live, tile GC'd
        {
            let session = state.must_active();
            let mut undo = lock_undo(&session).unwrap();
            let before = state.must_active().document_handle.snapshot();
            undo.undo_stack.push_back(before);
            state.must_active().document_handle.mutate(|doc| {
                doc.root.retain(|n| match n {
                    LayerNode::Leaf(l) => l.id.0 != layer,
                    LayerNode::Group(_) => true,
                });
            });
            let live = state.must_active().document_handle.snapshot();
            gc_orphaned_layers(&state, &undo, &live);
        }
        // Still referenced from undo stack — keep
        assert!(state.tile_cache.entries.contains_key(&key));

        // Pop undo (discard history of layer) — now orphan
        {
            let session = state.must_active();
            let mut undo = lock_undo(&session).unwrap();
            let _ = undo.undo_stack.pop_back();
            let live = state.must_active().document_handle.snapshot();
            gc_orphaned_layers(&state, &undo, &live);
        }
        assert!(!state.tile_cache.entries.contains_key(&key));
    }

    #[test]
    fn clear_history_empties_stacks() {
        let state = test_state();
        dummy_mutate(&state).unwrap();
        assert!(lock_undo(&state.must_active()).unwrap().state_dto().can_undo);
        clear_history(&state, None, active_doc_id(&state)).unwrap();
        assert!(lock_undo(&state.must_active()).unwrap().undo_stack.is_empty());
        assert!(lock_undo(&state.must_active()).unwrap().redo_stack.is_empty());
    }

    #[test]
    fn mutate_explicit_doc_while_other_active() {
        let state = Arc::new(AppState::empty_process(None, 64 * 1024 * 1024, false));
        let a = state.spawn_session(Document::new(DocumentId::new(1), 32, 32));
        let b = state.spawn_session(Document::new(DocumentId::new(2), 32, 32));
        assert_eq!(state.active_id(), Some(2));

        let before_b = b.document_handle.snapshot();
        with_document_undo(&state, None, 1, || {
            state.require_session(1)?.document_handle.mutate(|doc| {
                doc.root.push(LayerNode::Leaf(Layer::new(
                    LayerId::new(9),
                    LayerKind::Raster,
                    doc.width,
                    doc.height,
                )));
            });
            Ok(())
        })
        .unwrap();

        assert!(has_layer(&a.document_handle.snapshot(), 9));
        assert_eq!(
            Arc::as_ptr(&b.document_handle.snapshot()),
            Arc::as_ptr(&before_b),
            "active B must not change when mutating A"
        );
        assert!(is_dirty_doc(&state, 1));
        assert!(!is_dirty_doc(&state, 2));
    }

    #[test]
    fn require_session_gone() {
        let state = test_state();
        let err = with_document_undo(&state, None, 99, || Ok(())).unwrap_err();
        assert!(err.contains("closed") || err.contains("99"));
    }

    #[test]
    fn document_handle_unused_import_silences() {
        let _ = std::mem::size_of::<DocumentHandle>();
    }
}
