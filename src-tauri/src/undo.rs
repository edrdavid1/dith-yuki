//! Snapshot undo/redo for `Document` (Track N).
//!
//! History is a bounded stack of `Arc<Document>` — the same structural-sharing
//! economy as `DocumentHandle::mutate`. Handlers record via [`with_document_undo`];
//! they must not touch the stacks themselves.

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

/// Explicit history bound (Req 1.2).
pub const UNDO_MAX_DEPTH: usize = 50;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct UndoStateDto {
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct DirtyDto {
    pub dirty: bool,
}

fn has_live_document(state: &AppState) -> bool {
    !state.document_handle.snapshot().root.is_empty()
}

/// Track P: dirty iff there is a document and the live Arc is not the Saved_Mark.
pub fn is_dirty(state: &AppState) -> bool {
    if !has_live_document(state) {
        return false;
    }
    let live = state.document_handle.snapshot();
    match state.saved_snapshot.lock() {
        Ok(guard) => match guard.as_ref() {
            Some(saved) => !Arc::ptr_eq(saved, &live),
            None => true,
        },
        Err(_) => true,
    }
}

/// Remember the live Arc as clean (after save or document replace).
pub fn mark_clean(state: &AppState) {
    if let Ok(mut guard) = state.saved_snapshot.lock() {
        *guard = Some(state.document_handle.snapshot());
    }
}

pub fn emit_dirty(app: Option<&AppHandle>, state: &AppState) {
    if let Some(app) = app {
        let _ = app.emit("dirty-changed", DirtyDto { dirty: is_dirty(state) });
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
        }
    }
}

fn lock_undo(state: &AppState) -> Result<MutexGuard<'_, UndoManager>, String> {
    state
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

fn evict_layer_all(state: &AppState, layer: u32) {
    state.tile_cache.evict_layer(layer);
    state.error_residuals.evict_layer(LayerId::new(layer));
    state.block_representatives.evict_layer(layer);
}

/// Evict per-layer cache entries whose `LayerId` is in none of live + undo + redo.
fn gc_orphaned_layers(state: &AppState, undo: &UndoManager, live: &Document) {
    let referenced = referenced_layer_ids(undo, live);
    let mut candidates = HashSet::new();
    for entry in state.tile_cache.entries.iter() {
        candidates.insert(entry.key().layer);
    }
    candidates.extend(state.error_residuals.cached_layer_ids());
    candidates.extend(state.block_representatives.cached_layer_ids());
    for layer in candidates {
        if !referenced.contains(&layer) {
            evict_layer_all(state, layer);
        }
    }
}

fn sync_palette_caches(state: &AppState, live: &Document) {
    let live_ids: HashSet<u32> = live.palettes.iter().map(|p| p.id).collect();
    for id in state.palette_cache.cached_ids() {
        if !live_ids.contains(&id) {
            state.palette_cache.evict(id);
        }
    }
    for id in state.palette_lut_cache.cached_ids() {
        if !live_ids.contains(&id) {
            state.palette_lut_cache.evict(id);
        }
    }
}

fn emit_undo_state(app: Option<&AppHandle>, dto: UndoStateDto) {
    if let Some(app) = app {
        let _ = app.emit("undo-state-changed", dto);
    }
}

/// Push `before`, trim to `max_depth`, clear redo, run Orphan_GC.
pub fn record_mutation(state: &AppState, before: Arc<Document>) -> Result<UndoStateDto, String> {
    let live = state.document_handle.snapshot();
    let dto = {
        let mut undo = lock_undo(state)?;
        undo.undo_stack.push_back(before);
        if undo.undo_stack.len() > undo.max_depth {
            let _dropped = undo.undo_stack.pop_front();
        }
        undo.redo_stack.clear();
        gc_orphaned_layers(state, &undo, &live);
        undo.state_dto()
    };
    sync_palette_caches(state, &live);
    Ok(dto)
}

/// Document replace: both stacks empty, GC vs live doc, emit `{false, false}`.
pub fn clear_history(state: &AppState, app: Option<&AppHandle>) -> Result<UndoStateDto, String> {
    let live = state.document_handle.snapshot();
    let dto = {
        let mut undo = lock_undo(state)?;
        undo.undo_stack.clear();
        undo.redo_stack.clear();
        gc_orphaned_layers(state, &undo, &live);
        undo.state_dto()
    };
    sync_palette_caches(state, &live);
    emit_undo_state(app, dto);
    mark_clean(state);
    emit_dirty(app, state);
    Ok(dto)
}

/// Capture `snapshot()` before `f`; on `Ok` record history; on `Err` leave stacks unchanged.
pub fn with_document_undo<F, T>(
    state: &AppState,
    app: Option<&AppHandle>,
    f: F,
) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String>,
{
    let before = state.document_handle.snapshot();
    let result = f()?;
    let dto = record_mutation(state, before)?;
    emit_undo_state(app, dto);
    emit_dirty(app, state);
    Ok(result)
}

fn bump_live_document_gen(state: &AppState) {
    let live = state.document_handle.snapshot();
    let live_gen = live.generations.current_document_gen();
    let next = live_gen
        .max(state.tile_cache.max_generation())
        .saturating_add(1);
    live.generations.set_document_gen(next);
}

fn restore_and_invalidate(
    state: &AppState,
    app: &AppHandle,
    restored: Arc<Document>,
    kind: &str,
) -> Result<UndoStateDto, String> {
    state.document_handle.store(restored);
    bump_live_document_gen(state);
    let live = state.document_handle.snapshot();
    let dto = {
        let undo = lock_undo(state)?;
        gc_orphaned_layers(state, &undo, &live);
        undo.state_dto()
    };
    sync_palette_caches(state, &live);
    invalidate_after_document_replace(state);
    schedule_dirty_viewport_tiles(state);
    emit_document_changed(app, kind, None);
    emit_undo_state(Some(app), dto);
    emit_dirty(Some(app), state);
    Ok(dto)
}

pub fn apply_undo(state: &AppState, app: &AppHandle) -> Result<UndoStateDto, String> {
    let restored = {
        let mut undo = lock_undo(state)?;
        let Some(prev) = undo.undo_stack.pop_back() else {
            return Err("nothing to undo".to_string());
        };
        let current = state.document_handle.snapshot();
        undo.redo_stack.push(current);
        prev
    };
    restore_and_invalidate(state, app, restored, "document_undone")
}

pub fn apply_redo(state: &AppState, app: &AppHandle) -> Result<UndoStateDto, String> {
    let restored = {
        let mut undo = lock_undo(state)?;
        let Some(next) = undo.redo_stack.pop() else {
            return Err("nothing to redo".to_string());
        };
        let current = state.document_handle.snapshot();
        undo.undo_stack.push_back(current);
        next
    };
    restore_and_invalidate(state, app, restored, "document_redone")
}

#[tauri::command]
pub fn undo(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UndoStateDto, String> {
    apply_undo(&state, &app_handle)
}

#[tauri::command]
pub fn redo(
    app_handle: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<UndoStateDto, String> {
    apply_redo(&state, &app_handle)
}

#[tauri::command]
pub fn is_document_dirty(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    Ok(is_dirty(&state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_project::document::DocumentHandle;
    use engine_project::layer::Layer;
    use engine_project::types::{DocumentId, LayerKind};
    use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};

    fn dummy_mutate(state: &AppState) -> Result<(), String> {
        with_document_undo(state, None, || {
            state.document_handle.mutate(|doc| {
                doc.increment_generation();
            });
            Ok(())
        })
    }

    fn add_test_layer(state: &AppState) -> u32 {
        let mut id = 0u32;
        with_document_undo(state, None, || {
            state.document_handle.mutate(|doc| {
                id = doc
                    .root
                    .iter()
                    .map(|n| match n {
                        LayerNode::Leaf(l) => l.id.0,
                        LayerNode::Group(g) => g.id.0,
                    })
                    .max()
                    .unwrap_or(0)
                    + 1;
                let layer = Layer::new(
                    LayerId::new(id),
                    LayerKind::Raster,
                    doc.width,
                    doc.height,
                );
                doc.root.push(LayerNode::Leaf(layer));
                doc.increment_generation();
            });
            Ok(())
        })
        .unwrap();
        id
    }

    fn plant_layer_tile(state: &AppState, layer: u32) {
        let key = TileKey {
            layer,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        state
            .tile_cache
            .get_or_insert(key, Arc::new(PixelTile::new()));
        state
            .error_residuals
            .store(LayerId::new(layer), TileCoord { level: 0, x: 0, y: 0 }, Default::default());
        state.block_representatives.insert_raw(
            engine_tiles::BlockCoord {
                layer,
                block_x: 0,
                block_y: 0,
                pixel_size: 1,
            },
            [0.0, 0.0, 0.0, 1.0],
        );
    }

    fn layer_tile_count(state: &AppState, layer: u32) -> usize {
        state
            .tile_cache
            .entries
            .iter()
            .filter(|e| e.key().layer == layer)
            .count()
    }

    fn has_layer(doc: &Document, id: u32) -> bool {
        collect_layer_ids(doc).contains(&id)
    }

    #[test]
    fn successful_mutate_sets_can_undo() {
        let state = crate::commands::make_test_app_state();
        assert!(!lock_undo(&state).unwrap().state_dto().can_undo);
        dummy_mutate(&state).unwrap();
        let dto = lock_undo(&state).unwrap().state_dto();
        assert!(dto.can_undo);
        assert!(!dto.can_redo);
    }

    #[test]
    fn err_does_not_push() {
        let state = crate::commands::make_test_app_state();
        dummy_mutate(&state).unwrap();
        let before_len = lock_undo(&state).unwrap().undo_stack.len();
        let before_ptr = Arc::as_ptr(&state.document_handle.snapshot());

        let err = with_document_undo(&state, None, || Err::<(), _>("nope".into()));
        assert_eq!(err.unwrap_err(), "nope");
        assert_eq!(lock_undo(&state).unwrap().undo_stack.len(), before_len);
        assert_eq!(
            Arc::as_ptr(&state.document_handle.snapshot()),
            before_ptr,
            "failed inner fn must not change the live document"
        );
    }

    #[test]
    fn redo_break_after_new_mutation() {
        let state = crate::commands::make_test_app_state();
        dummy_mutate(&state).unwrap();
        dummy_mutate(&state).unwrap();

        {
            let mut undo = lock_undo(&state).unwrap();
            let prev = undo.undo_stack.pop_back().unwrap();
            let current = state.document_handle.snapshot();
            undo.redo_stack.push(current);
            drop(undo);
            state.document_handle.store(prev);
        }
        assert!(lock_undo(&state).unwrap().state_dto().can_redo);

        dummy_mutate(&state).unwrap();
        let dto = lock_undo(&state).unwrap().state_dto();
        assert!(!dto.can_redo);

        let err = {
            let mut undo = lock_undo(&state).unwrap();
            undo.redo_stack
                .pop()
                .ok_or_else(|| "nothing to redo".to_string())
        };
        assert_eq!(err.unwrap_err(), "nothing to redo");
    }

    #[test]
    fn max_depth_fifty_then_nothing_to_undo() {
        let state = crate::commands::make_test_app_state();
        for _ in 0..(UNDO_MAX_DEPTH + 5) {
            dummy_mutate(&state).unwrap();
        }
        assert_eq!(lock_undo(&state).unwrap().undo_stack.len(), UNDO_MAX_DEPTH);

        for _ in 0..UNDO_MAX_DEPTH {
            let mut undo = lock_undo(&state).unwrap();
            let prev = undo.undo_stack.pop_back().expect("undo available");
            let current = state.document_handle.snapshot();
            undo.redo_stack.push(current);
            drop(undo);
            state.document_handle.store(prev);
        }
        let mut undo = lock_undo(&state).unwrap();
        assert!(undo.undo_stack.pop_back().is_none());
    }

    #[test]
    fn store_restores_same_arc() {
        let handle = DocumentHandle::new(Document::new(DocumentId::new(1), 8, 8));
        let before = handle.snapshot();
        handle.mutate(|d| d.increment_generation());
        let ptr = Arc::as_ptr(&before);
        handle.store(before.clone());
        assert_eq!(Arc::as_ptr(&handle.snapshot()), ptr);
    }

    #[test]
    fn gc_orphans_layer_after_leaving_both_stacks() {
        let state = crate::commands::make_test_app_state();
        let layer = add_test_layer(&state);
        plant_layer_tile(&state, layer);
        assert_eq!(layer_tile_count(&state, layer), 1);

        // Undo the add → layer lives only on redo.
        {
            let mut undo = lock_undo(&state).unwrap();
            let prev = undo.undo_stack.pop_back().unwrap();
            let current = state.document_handle.snapshot();
            undo.redo_stack.push(current);
            drop(undo);
            state.document_handle.store(prev);
        }
        assert!(!has_layer(&state.document_handle.snapshot(), layer));
        assert_eq!(layer_tile_count(&state, layer), 1, "still referenced by redo");

        // New mutation clears redo and must GC the orphaned layer.
        dummy_mutate(&state).unwrap();
        assert_eq!(layer_tile_count(&state, layer), 0);
        assert!(!state
            .error_residuals
            .cached_layer_ids()
            .contains(&layer));
        assert!(!state
            .block_representatives
            .cached_layer_ids()
            .contains(&layer));
    }

    #[test]
    fn add_layer_undo_redo_tree() {
        let state = crate::commands::make_test_app_state();
        let before = state.document_handle.snapshot();
        let layer = add_test_layer(&state);
        assert!(has_layer(&state.document_handle.snapshot(), layer));

        {
            let mut undo = lock_undo(&state).unwrap();
            let prev = undo.undo_stack.pop_back().unwrap();
            let current = state.document_handle.snapshot();
            undo.redo_stack.push(current);
            drop(undo);
            state.document_handle.store(prev);
        }
        assert!(!has_layer(&state.document_handle.snapshot(), layer));
        assert_eq!(
            collect_layer_ids(&state.document_handle.snapshot()),
            collect_layer_ids(&before)
        );

        {
            let mut undo = lock_undo(&state).unwrap();
            let next = undo.redo_stack.pop().unwrap();
            let current = state.document_handle.snapshot();
            undo.undo_stack.push_back(current);
            drop(undo);
            state.document_handle.store(next);
        }
        assert!(has_layer(&state.document_handle.snapshot(), layer));
    }

    #[test]
    fn n_update_filter_style_mutates_are_n_undo_steps() {
        let state = crate::commands::make_test_app_state();
        let n = 7usize;
        for _ in 0..n {
            dummy_mutate(&state).unwrap();
        }
        assert_eq!(lock_undo(&state).unwrap().undo_stack.len(), n);
    }

    #[test]
    fn clear_history_zeros_flags() {
        let state = crate::commands::make_test_app_state();
        dummy_mutate(&state).unwrap();
        assert!(lock_undo(&state).unwrap().state_dto().can_undo);
        let dto = clear_history(&state, None).unwrap();
        assert!(!dto.can_undo);
        assert!(!dto.can_redo);
        assert!(lock_undo(&state).unwrap().undo_stack.is_empty());
        assert!(lock_undo(&state).unwrap().redo_stack.is_empty());
    }

    #[test]
    fn empty_doc_is_not_dirty() {
        let state = crate::commands::make_test_app_state();
        assert!(!is_dirty(&state));
    }

    #[test]
    fn mutation_dirties_after_mark_clean() {
        let state = crate::commands::make_test_app_state();
        add_test_layer(&state);
        mark_clean(&state);
        assert!(!is_dirty(&state), "install/save mark is clean");
        dummy_mutate(&state).unwrap();
        assert!(is_dirty(&state));
    }

    #[test]
    fn undo_to_saved_mark_is_clean() {
        let state = crate::commands::make_test_app_state();
        add_test_layer(&state);
        mark_clean(&state);
        dummy_mutate(&state).unwrap();
        assert!(is_dirty(&state));
        {
            let mut undo = lock_undo(&state).unwrap();
            let prev = undo.undo_stack.pop_back().unwrap();
            let current = state.document_handle.snapshot();
            undo.redo_stack.push(current);
            drop(undo);
            state.document_handle.store(prev);
        }
        assert!(!is_dirty(&state));
    }

    #[test]
    fn clear_history_marks_clean() {
        let state = crate::commands::make_test_app_state();
        add_test_layer(&state);
        dummy_mutate(&state).unwrap();
        assert!(is_dirty(&state));
        clear_history(&state, None).unwrap();
        assert!(!is_dirty(&state));
    }
}
