# Implementation Plan: Track N — Undo / Redo

План: [requirements.md](./requirements.md), [design.md](./design.md).
Бриф: [TASK_track_n_undo_redo.md](../TASK_track_n_undo_redo.md).

**Gate:** Track K closed (`useEffectLayer` 100ms is the only param debounce).
No H–M / C4.1 dependency. E/G replace commands already in tree.

**Locked:** snapshot `Arc<Document>` (not command/diff); `max_depth = 50`;
wrapper captures before-Arc then runs existing mutate; replace clears stacks;
`store(Arc)` on the handle (no deep-clone on undo); invalidation =
`invalidate_after_document_replace` + schedule + `document-changed`;
Orphan_GC on overflow **and** undo/redo **and** redo-clear **and** replace;
UndoStateDto via `undo-state-changed` event (return DTO only from undo/redo);
no second debounce; custom MenuBar + window keydown (steal from NumberInput
when `can_undo`).

**Порядок:** N0 → N1 → N2 → N3 → N4 → N5. N3 can start once N1 types exist;
N4 after undo/redo IPC; N2 can land behind the wrapper as soon as N1 compiles.

---

## 0. Baseline

- [x] 0.1 Inventory
  - Every `document_handle.mutate` / `engine_commands::*` call site in
    `src-tauri/src/commands.rs` → wrapper vs `clear_history` vs skip
  - `DocumentHandle` API (`snapshot` / `mutate`; no `store` yet)
  - `invalidate_after_document_replace` + `install_raster_document` /
    `open_project` / `create_document` / `new_document`
  - `TileCache` / `ErrorResidualsStore` / `BlockRepresentativeCache` —
    confirm no `evict_layer`
  - `MenuBar` Edit Undo/Redo always disabled; no accelerators in
    `tauri.conf.json`
  - `useEffectLayer` `DEBOUNCE_MS = 100` (params + blend)
  - `listeners.ts` `document-changed` kinds that refresh layers/filters
  - Track A `diffusion_skip_counter` / skip-branch note (for N5.2)
  - _Requirements: 2, 3, 4, 5, 6, 8_

- [x] 0.2 Link docs
  - Point this folder from `TASK_track_n_undo_redo.md`, `RELEASE_TRACKS.md`,
    `tech-debit.md`
  - _Requirements: n/a (process)_

**§0.1 result (fill in):**

```
Date: 2026-08-13
mutate / engine_commands call sites:
  WRAP:
    add_layer, remove_layer, set_layer_props, reorder_layer
    add_filter, remove_filter, reorder_filter, update_filter
    import_pattern
    import_builtin_palette, import_palette, add_palette, generate_palette
    remove_palette, rename_palette, create_palette
    add_color_to_palette, update_palette_color, remove_palette_color
    reorder_palette_color, delete_palette
  CLEAR:
    install_raster_document (load_image + create_document),
    open_project, new_document
  SKIP:
    get_document_snapshot, get_layer_tree, save_project, save_project_as
    export_image, export_pattern, export_palette
    list_palettes, list_builtin_palettes, generate_ramp_palette
    generate_harmony_palette, colors_to_oklab, get_palette_oklab
    set_selection, get_selection, panel/viewport commands
DocumentHandle: snapshot + mutate; store() added in N1
Replace helpers: install_raster_document / open_project / new_document
evict_layer: TileCache / residuals / BRC = was absent (only LRU / clear / invalidate_all); added N3
MenuBar Edit: Undo/Redo were disabled hard-coded
tauri.conf.json menu/accelerators: none
Debounce: useEffectLayer DEBOUNCE_MS=100 (updateParams + updateBlend)
listeners.ts layer/filter refresh kinds:
  layers: layer_changed|reordered|added|removed, filter_updated|added|removed|reordered
  filters: filter_updated|added|removed|reordered
  (N4 added document_undone|document_redone to both)
Track A skip-branch conclusion (quote):
  "production never calls evict_* today; skip branch unreachable after full load.
   Decision: N/A for waiters-as-sole-fix."
  Revisited in N5.2: whole-layer Orphan_GC does not increment skip counter;
  neighbor-raw miss still does (lab test). Waiters unchanged.
```

---

## 1. N1 — Manager + wrapper + IPC

- [x] 1.1 `DocumentHandle::store(Arc<Document>)`
  - Used by undo/redo; must not deep-clone the stacked snapshot
  - _Requirements: 1.1, 3.2_

- [x] 1.2 `src-tauri/src/undo.rs`
  - `UndoManager` (`VecDeque` undo, `Vec` redo, `max_depth = 50`)
  - `UndoStateDto`; `state_dto()`
  - `with_document_undo` / `record_mutation` / `clear_history`
  - `undo` / `redo` helpers: store Arc, bump **live** `document_gen`, return DTO
  - Empty → `"nothing to undo"` / `"nothing to redo"`
  - `Mutex<UndoManager>` on `AppState`; register commands in `main.rs`
  - Unit: success push; Err does not push; redo-break; depth 50
  - _Requirements: 1.2–1.4, 2.1–2.3, 3.1–3.2, 7.2–7.3_

- [x] 1.3 Events
  - Emit `undo-state-changed` from wrapper / undo / redo / clear
  - Undo/redo also `emit_document_changed` (`document_undone` /
    `document_redone`) + `invalidate_after_document_replace` +
    `schedule_dirty_viewport_tiles`
  - _Requirements: 3.3, 6.2_

---

## 2. N2 — Migrate handlers

- [x] 2.1 Wrap every WRAP site from §0.1
  - Handlers must not touch `undo_stack` themselves
  - Palette writers included; `import_pattern` included
  - _Requirements: 2.1, 2.6–2.7_

- [x] 2.2 `clear_history` on replace
  - `install_raster_document`, `open_project`, `new_document`
  - After success: both flags false
  - _Requirements: 2.4, 7.7_

- [x] 2.3 Confirm SKIP list
  - save/export/reads/selection/panels/viewport/Color Lab draft stay unwrapped
  - _Requirements: 2.5_

---

## 3. N3 — Orphan GC

- [x] 3.1 `evict_layer` on `TileCache`, `ErrorResidualsStore`,
      `BlockRepresentativeCache`
  - All stages / all per-layer keys
  - Unit: target layer gone, other layer kept
  - _Requirements: 4.1–4.2_

- [x] 3.2 `gc_orphaned_layers` + `sync_palette_caches`
  - Run on: depth overflow, undo, redo, redo-clear, `clear_history`
  - Evict `LayerId` only when absent from live + both stacks
  - Palette caches: evict ids not in live `Document.palettes`
  - GC test: add layer + plant a cache entry → push it out of both stacks
    and current → **zero** keys for that layer
  - _Requirements: 4.3–4.5, 7.4_

---

## 4. N4 — Frontend

- [x] 4.1 IPC + slice + listener
  - `shared/ipc/undo.ts`; `undoSlice`; `listeners.ts` applies
    `undo-state-changed`
  - `document_undone` / `document_redone` refresh document **and**
    layers **and** filters
  - thunks `undo` / `redo`
  - _Requirements: 3.5, 6.1–6.2_

- [x] 4.2 MenuBar
  - Enable from `canUndo` / `canRedo`; click → thunks
  - Shortcut labels (⌘Z / Ctrl+Z, ⇧ variants)
  - Update `MenuBar.test.tsx` (today expects always disabled)
  - _Requirements: 6.1, 6.3, 7.6_

- [x] 4.3 Window keydown
  - Hook in `AppLayout`; steal from focused NumberInput when `canUndo`
  - RTL: focused input + chord → undo invoke
  - _Requirements: 6.3–6.5, 7.6_

---

## 5. N5 — Tests, docs, Track A note

- [x] 5.1 Remaining Req 7
  - add_layer → undo → redo tree (composite hash only if a helper is already
    cheap)
  - N× `update_filter` → N undo steps
  - Keep `useEffectLayer` 100ms test as debounce proof
  - _Requirements: 5, 7.1, 7.5_

- [x] 5.2 Track A diagnostic
  - Re-run skip-branch / `diffusion_skip_counter` after GC exists
  - Update [track-a-correctness/tasks.md](../track-a-correctness/tasks.md)
    (or §DoD here) if “always 0” is no longer true
  - Do not reimplement waiters unless the diagnosis shows a user-visible seam
  - _Requirements: 4.6, 8_

- [x] 5.3 ARCHITECTURE note
  - UndoManager, wrapper, `evict_layer`, applicability boundary (no paint)
  - _Requirements: n/a_

---

## Definition of Done

- [x] All document-mutating Tauri handlers go through `with_document_undo`;
      none push the undo stack themselves
- [x] `load_image` / `open_project` / `create_document` clear both stacks
- [x] Orphan `LayerId` tiles/residuals/BRC are actually gone (Req 4.5 test)
- [x] Slider drag = one undo step via Track K debounce, not a second timer
- [x] Edit Undo/Redo + ⌘Z/Ctrl+Z work with NumberInput focused
- [x] Track A skip-branch conclusion revisited in writing
- [x] Req 7 tests green
