# Requirements: Track N — Undo / Redo (Snapshot History)

## Introduction

Формализация [TASK_track_n_undo_redo.md](../TASK_track_n_undo_redo.md).
Цель — **Undo/Redo документа** через bounded snapshot-стек `Arc<Document>`,
одну обёртку вокруг всех мутирующих Tauri-команд, GC осиротевших тайлов
и фронтовые Edit-пункты / шорткаты.

Это **обвязка над уже существующей ArcSwap-моделью**, не новая модель
документа и не command/diff:

| Уже есть | Этот трек |
|----------|-----------|
| `DocumentHandle` (`ArcSwap`, `snapshot` / `mutate`) | Стек `Arc<Document>` до мутации |
| `document.revision` («for undo/redo») | Реальный стек, не только счётчик |
| `invalidate_after_document_replace` (`load_image` / `open_project` / `create_document`) | Тот же путь после `undo` / `redo` |
| Edit → Undo/Redo в `MenuBar` (всегда `disabled`) | `UndoStateDto` включает пункты |
| Track K: debounce 100ms в `useEffectLayer.updateParams` | Undo-шаг = тот же IPC, без второго таймера |
| `TileCache` LRU (`evict_if_over_budget`) | Первый **per-layer** эвикшен |

**Зависимость:** Track K закрыт (debounce уже в `useEffectLayer`). Независим
от H–M / C4.1. Track E/G replace-пути (`load_image` / `open_project` /
`create_document`) уже в дереве — на них вешается очистка стеков.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md).

## Glossary

- **Undo_Manager**: bounded стек снапшотов `Arc<Document>` + redo-стек в `AppState`.
- **History_Wrapper**: единственная точка, которая пушит в undo-стек; handlers
  не трогают стек напрямую.
- **Document_Replace**: смена документа целиком (`load_image`, `open_project`,
  `create_document`, `new_document`) — стеки **очищаются**, шаг не пишется.
- **UndoStateDto**: `{ can_undo, can_redo }` после мутации / undo / redo / replace.
- **Orphan_GC**: удаление per-layer записей (`TileCache` Raw/Processed/Composite,
  `ErrorResidualsStore`, `BlockRepresentativeCache`), когда `LayerId` больше
  не встречается ни в текущем `Document`, ни в undo, ни в redo.
- **Param_Debounce**: существующие 100ms в `useEffectLayer`; граница одного
  undo-шага для drag слайдера.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Snapshot-история структуры `Document` (слои, фильтры, палитры, params) | Command/diff / inverse-ops на каждую мутацию |
| Одна обёртка на все document-мутации | Undo viewport / pan / zoom / panel layout / selection / Color Lab draft |
| GC осиротевших `LayerId` (первый реальный per-layer эвикшен) | Paint/brush / raw pixel edits (их нет в модели; граница применимости) |
| Debounce undo = Track K IPC debounce | Второй undo-таймер на фронте или бэкенде |
| Edit Undo/Redo + ⌘Z/⌘⇧Z (и Ctrl) даже с фокусом в NumberInput | Нативное Tauri `Menu` с нуля (UI — кастомный `MenuBar`) |
| Очистка стеков при смене документа | Multi-document, persistent undo across restart |

---

## Requirements

### Requirement 1: Snapshot History, Not Command/Diff

**User Story:** As a developer, I want undo to restore a previous `Document` pointer, so I do not maintain an inverse of every mutation.

#### Acceptance Criteria

1. THE workspace SHALL store history as `Arc<Document>` snapshots (clone of the Arc, not a deep copy of the tree). THE design SHALL NOT implement per-command inverse operations (`add_layer` ↔ `remove_layer`, etc.).
2. `UndoManager.max_depth` SHALL be **50** (explicit constant, not a “reasonable” unnamed number). Revisiting the number after memory profiling is allowed later; this track SHALL ship 50.
3. A snapshot SHALL cover `Document` only (tree, filters, palettes, params, `revision`). Raster pixels live in `TileCache` keyed by `LayerId` and are **not** copied into the undo stack. This is sufficient while there is no paint/brush path. Pixel painting, if added later, is out of this track’s applicability (do not design it here).
4. `UndoManager` SHALL live on `AppState` in `src-tauri` (not inside `engine-project::DocumentHandle`). Workers keep lock-free `snapshot()` reads; they SHALL NOT take the undo mutex.

### Requirement 2: Single Mutation Wrapper

**User Story:** As a maintainer, I want every document mutation to record history through one function, so a new command cannot silently skip undo.

#### Acceptance Criteria

1. THE workspace SHALL add a single History_Wrapper (name locked in design: e.g. `with_document_undo`) used by **all** Tauri commands that mutate `Document`. Individual handlers SHALL NOT push to `undo_stack` / clear `redo_stack` themselves.
2. On success the wrapper SHALL: capture `document_handle.snapshot()` **before** the mutation; run the existing mutate (`DocumentHandle.mutate` and/or `engine_commands::*`); push that before-Arc onto `undo_stack`; drop the front if `len > max_depth` and run Orphan_GC; **clear `redo_stack`** (standard “new edit after undo drops redo”); emit UndoStateDto.
3. IF the inner mutation returns `Err`, THE wrapper SHALL NOT push a snapshot, SHALL NOT clear redo, and SHALL leave `document_handle` unchanged (same contract as today’s failed commands).
4. Document_Replace commands (`load_image` / `install_raster_document`, `open_project`, `create_document`, `new_document`) SHALL **clear both stacks** and emit `can_undo = false`, `can_redo = false`. They SHALL NOT record the previous document as an undo step. Undo SHALL NOT walk across a document replace.
5. THE following SHALL NOT go through the wrapper: viewport/pan/zoom, panel layout (`panel_persistence`), `set_selection` / `get_selection`, Color Lab draft, save/export (`save_project*`, `export_image`, `export_pattern`, `export_palette`), pure reads (`get_document_snapshot`, `list_palettes`, `get_palette_oklab`, …).
6. Palette CRUD that mutates `Document.palettes` (add/remove/rename/recolor/reorder/import/generate-into-document) SHALL go through the wrapper — palettes are part of `Document`.
7. `import_pattern` mutates the document and SHALL go through the wrapper.

### Requirement 3: Undo and Redo Commands

**User Story:** As a user, I want Undo to restore the last document edit and Redo to put it back, including the preview catching up.

#### Acceptance Criteria

1. Tauri commands `undo` and `redo` SHALL exist and return `UndoStateDto { can_undo: bool, can_redo: bool }`. Empty stack SHALL return a **string** error (`"nothing to undo"` / `"nothing to redo"`), not panic, matching existing `Result<T, String>` IPC. THE document SHALL be left unchanged on that error.
2. `undo` SHALL pop the back of `undo_stack`, push the current `snapshot()` onto `redo_stack`, `store` the popped Arc into `document_handle`. `redo` SHALL be the mirror.
3. After a successful undo/redo, THE backend SHALL reuse **`invalidate_after_document_replace` + `schedule_dirty_viewport_tiles` + `emit_document_changed`** (same trio as `open_project` / `install_raster_document`). THE track SHALL NOT invent a third invalidation path. Event kind locked in design (`document_undone` / `document_redone` or reuse an existing kind that already refreshes layers/filters/document in `listeners.ts`).
4. After undo/redo THE backend SHALL also drop in-flight work that belongs to the discarded side: Orphan_GC (Req 4) plus palette-cache sync for ids not present in the restored document (design locks the exact helper).
5. Frontend SHALL refresh document/layers/filters via the existing `document-changed` bridge (Req 3.3). Undo SHALL NOT require a second ad-hoc RTK path.

### Requirement 4: Orphan Layer GC (Not Optional)

**User Story:** As a user who adds and undoes layers, I do not want TileCache to grow without bound.

#### Acceptance Criteria

1. `TileCache` SHALL gain `evict_layer(layer: LayerId)` (or equivalent) that removes **all stages** (Raw, Processed, Composite) for that layer. This is the first production call site that evicts by layer — LRU budget eviction already exists and is not a substitute.
2. `ErrorResidualsStore` and `BlockRepresentativeCache` SHALL gain a per-layer remove (they are keyed by layer today; only `clear` / `invalidate_all` exist). Orphan_GC SHALL call all three.
3. A `LayerId` SHALL be evicted IFF it appears in the dropped snapshot (or, on undo/redo/replace, in the set of ids that just became unreferenced) AND it appears in **none** of: current `Document`, `undo_stack`, `redo_stack`.
4. Orphan_GC SHALL run on: (a) `max_depth` overflow when the front snapshot is dropped; (b) every successful `undo` / `redo`; (c) redo-stack clear inside the wrapper; (d) Document_Replace stack clear. Not only on overflow.
5. A test SHALL assert **absence** of `TileCache` entries for an orphaned `LayerId` after it has left both stacks and the current document — not merely “the test did not panic”.
6. After this track is merged, Track A silent-skip / missing-raw diagnostic counters MAY become reachable in real use. Re-running those diagnostics is an acceptance item of this track (Req 8), not a follow-up wish.

### Requirement 5: One History Step per Debounced IPC

**User Story:** As a user dragging a filter slider, I want one Ctrl+Z to restore the value from before the drag, not dozens of indistinguishable steps.

#### Acceptance Criteria

1. THE backend SHALL record **one undo snapshot per successful wrapper invocation**. It SHALL NOT debounce, coalesce, or timestamp-merge mutations.
2. Filter-param drags SHALL continue to coalesce at **Track K’s existing 100ms** in `useEffectLayer.updateParams` (and the existing blend debounce in the same hook). This track SHALL NOT add a second timer in Slider, NumberInput, or `UndoManager`.
3. A rapid series of `updateParams` during a drag SHALL result in **one** `update_filter` IPC after the pause and therefore one undo step. Proof: keep/extend the existing `useEffectLayer` 100ms test; do not replace it with a backend debounce test that would imply the opposite architecture.

### Requirement 6: Frontend Menu, State, and Shortcuts

**User Story:** As a user, I want Edit → Undo/Redo and the usual keyboard shortcuts to work even when a parameter field is focused.

#### Acceptance Criteria

1. `Edit → Undo` / `Edit → Redo` SHALL be `disabled` from the latest `UndoStateDto` (`!can_undo` / `!can_redo`), not hard-coded `disabled` as today. THE frontend SHALL NOT poll a getter on an interval.
2. UndoStateDto SHALL reach the UI without polling: design locks **event `undo-state-changed`** (emitted by the wrapper, undo, redo, and replace-clear) plus the `undo`/`redo` command return value. Mutating command **return types stay as they are** (do not wrap every `Result<T>` in `(T, UndoStateDto)`).
3. THE app SHALL handle **⌘Z / ⌘⇧Z** on macOS and **Ctrl+Z / Ctrl+Shift+Z** elsewhere via a **window-level keydown** on the frontend (custom `MenuBar` is not a native Tauri menu; `tauri.conf.json` has no accelerators today). Labels on the menu items SHALL show the shortcut.
4. WHEN focus is inside a text/number input (including Track K `NumberInput` / Slider text), THE document shortcut SHALL still fire (preventDefault so the field does not consume it). This is an explicit acceptance criterion, not “native accelerator if we’re lucky”.
5. Shortcuts SHALL be no-ops (or invoke and surface the string error without crashing) when `!can_undo` / `!can_redo`. They SHALL be no-ops when `!hasDocument`.

### Requirement 7: Tests

**User Story:** As a QA engineer, I want automated proof of stack semantics, GC, debounce coalescing, and replace-clears.

#### Acceptance Criteria

1. `add_layer` → `undo` → layer absent from the tree; `redo` → layer present again. Prefer asserting document structure; if an existing composite-hash helper is cheap to reuse, also assert composite identity before add vs after undo, and after add vs after redo. Do not build a new compositor test harness solely for this.
2. `undo` then a new mutation → `redo` errors with `"nothing to redo"` (`redo_stack` cleared).
3. `max_depth + 5` mutations, then `max_depth` undos → the next `undo` returns `"nothing to undo"` (no panic, no walk past the bound).
4. GC test as Req 4.5: add a layer (with at least one cache entry for that `LayerId`) → enough further mutations to drop it from both stacks while it is also absent from current → **zero** `TileCache` keys with that layer (all stages).
5. Debounce: Req 5.3 (`useEffectLayer` one IPC). Backend: N direct `update_filter` calls ⇒ N undo steps (documents that the backend is not the coalescer).
6. Frontend: menu items reflect `can_undo` / `can_redo`; keydown ⌘Z/Ctrl+Z invokes undo even when a `NumberInput` is focused (RTL with a focused input).
7. After `open_project` / `load_image` / `create_document`, both stacks are empty and the emitted/returned state is `can_undo = false`, `can_redo = false`.

### Requirement 8: Track A Eviction-Branch Follow-up

**User Story:** As a maintainer, I want the Track A “missing raw neighbor” diagnosis revisited once real eviction exists.

#### Acceptance Criteria

1. After Orphan_GC is wired, re-run the Track A diagnostic that counts the `tile_pipeline.rs` else-branch (missing left/top/diag raw) / `diffusion_skip_counter`.
2. IF the counter is no longer always zero, THE track SHALL update the recorded conclusion in [track-a-correctness/tasks.md](../track-a-correctness/tasks.md) (or a short note in this folder’s tasks §DoD) — do not leave “counter always 0” as an implicit truth.
3. This track SHALL NOT re-implement Track A waiters unless that diagnosis says the skip branch is now a user-visible seam. Updating the written conclusion is the required deliverable.

---

## Future (explicitly out of MVP)

- Persistent undo across app restart
- Named history list / branched history
- Ctrl+Y as alternate redo
- Native Tauri `Menu` accelerators in addition to the HTML menubar
- Pixel-paint undo (tile diffs or a side channel outside `Document` snapshots)
- Profiling-driven `max_depth` change
- Undo of viewport / panel / selection
