# Tasks & Implementation Plan: Commands Refactoring

## Overview

This implementation plan breaks down the structural refactoring of `src-tauri/src/commands.rs` into 9 sequential, low-risk phases (Phases 0–8). Each phase must end with a clean build (`cargo build --all`), passing test suite (`cargo test --all`), and explicit `git diff pre-refactor-commands` verification **BEFORE** committing.

---

## Tasks

- [x] **Phase 0: Baseline Snapshot, Module Skeleton & Error Framework Setup**
  - [x] Set pre-refactor baseline checkpoint (`git tag pre-refactor-commands` or record commit hash) to allow explicit `git diff` comparisons during logic verification steps
  - [x] Create skeleton modules `src-tauri/src/commands/mod.rs`, `src-tauri/src/services/mod.rs`, and `src-tauri/src/state/mod.rs`
  - [x] Define `AppError` enum in `src-tauri/src/services/mod.rs` using `thiserror`
  - [x] Wire empty modules into `src-tauri/src/main.rs`
  - [x] Verify zero compilation errors: `cargo build --all && cargo test --all`

- [x] **Phase 1: Diagnostics Domain Extraction**
  - [x] Create `src-tauri/src/commands/diagnostics.rs`
  - [x] Move `get_gpu_preview_status`, `set_gpu_preview_enabled`, and `is_release_build` from `commands.rs` to `commands/diagnostics.rs`
  - [x] **BEFORE COMMITTING**: Diff extracted signatures against `pre-refactor-commands` baseline (`git diff pre-refactor-commands -- src-tauri/src/commands/diagnostics.rs`) and fix any mismatches
  - [x] Export `pub mod diagnostics;` from `commands/mod.rs` and update registration in `main.rs`
  - [x] Remove extracted commands from `commands.rs`
  - [x] Verify: `cargo build --all && cargo test --all`

- [x] **Phase 2: Panels Domain Extraction**
  - [x] Create `src-tauri/src/services/panel_service.rs` with `PanelService` struct
  - [x] Migrate `panel_manager: Mutex<PanelManager>` from `AppState` into `UiState`
  - [x] Create `src-tauri/src/commands/panels.rs`
  - [x] Move panel IPC handlers (`undock_panel`, `undock_panel_with_size`, `dock_panel`, `show_panel`, `hide_panel`, `move_panel_to_side`, `move_all_panels_to_side`, `swap_sidebars`, `update_dock_zone`, `begin_float_drag`, `cancel_float_drag`, `dock_panel_at`) to `commands/panels.rs` and delegate to `PanelService`
  - [x] **BEFORE COMMITTING**: Diff against `pre-refactor-commands` to validate `panel-state-changed` event emission order and payload structure remain 100% identical
  - [x] Remove extracted commands from `commands.rs`
  - [x] Verify: `cargo build --all && cargo test --all`

- [x] **Phase 3: Selection Domain Extraction**
  - [x] Inspect existing `set_selection` in `src-tauri/src/commands.rs` (line ~3540) to verify actual `selection-changed` event broadcast implementation before migrating
  - [x] Migrate `selection: Mutex<SelectionState>` from `AppState` into `UiState`
  - [x] Create `src-tauri/src/commands/selection.rs`
  - [x] Move `set_selection` and `get_selection` from `commands.rs` to `commands/selection.rs`
  - [x] **BEFORE COMMITTING**: Diff against `pre-refactor-commands` to confirm `selection-changed` event broadcast logic and `SelectionChangedPayload` match original implementation exactly; re-export `SelectionChangedPayload` and `SelectionState` in `commands/mod.rs`
  - [x] Remove extracted commands from `commands.rs`
  - [x] Verify: `cargo build --all && cargo test --all`

- [x] **Phase 4: Viewport Domain Extraction**
  - [x] Create `src-tauri/src/services/viewport_service.rs` with `ViewportService` struct
  - [x] Move complete `set_viewport` logic (pyramid calculation, tile scheduling, priority assignments, `scheduler.clear_all()`) into `ViewportService` as a single unit
  - [x] Migrate `viewport: Mutex<ViewportState>` from `AppState` into `UiState`
  - [x] Create `src-tauri/src/commands/viewport.rs` and delegate `set_viewport` to `ViewportService`
  - [x] **BEFORE COMMITTING**: Diff `set_viewport` execution pipeline against `pre-refactor-commands` baseline
  - [x] Remove `set_viewport` from `commands.rs`
  - [x] Verify: `cargo build --all && cargo test --all`

- [x] **Phase 5: Undo / Redo Domain Extraction**
  - [x] Create `src-tauri/src/services/undo_service.rs` with `UndoService` struct
  - [x] Migrate `undo_manager` and `saved_snapshot` fields into `HistoryState`
  - [x] Move `undo` and `redo` execution logic to `UndoService`
  - [x] **BEFORE COMMITTING**: Diff against `pre-refactor-commands` baseline to verify exact sequence: `DocumentHandle::store` → `increment_document_gen` → `invalidate_after_document_replace` → `schedule_dirty_viewport_tiles` → emit `document-changed`
  - [x] Create `src-tauri/src/commands/undo.rs` delegating to `UndoService`
  - [x] Remove `undo` and `redo` from `commands.rs`
  - [x] Verify: `cargo build --all && cargo test --all`

- [x] **Phase 6: Layers & Filters Domain Extraction**
  - [x] Create `src-tauri/src/services/layer_service.rs` and `src-tauri/src/services/filter_service.rs`
  - [x] Migrate tile caches (`tile_cache`, `scheduler`, `ed_frontier`, etc.) from `AppState` into `TileState`
  - [x] Move layer commands (`get_layer_tree`, `add_layer`, `remove_layer`, `reorder_layer`, `set_layer_props`) to `commands/layers.rs` and delegate to `LayerService`
  - [x] Move filter commands (`add_filter`, `update_filter`, `remove_filter`, `reorder_filter`) to `commands/filters.rs` and delegate to `FilterService`
  - [x] Re-export all layer DTOs (`LayerNodeDto`, `AddLayerRequest`, `SetLayerPropsRequest`, `ReorderLayerRequest`, `LayerPropsPatchDto`) in `commands/mod.rs`
  - [x] **BEFORE COMMITTING**: Diff against `pre-refactor-commands` baseline to verify invalidation cascade order for all layer/filter operations
  - [x] Remove extracted commands from `commands.rs`
  - [x] Verify: `cargo build --all && cargo test --all`

- [ ] **Phase 7: Palette & Color Lab Domain Extraction**
  - [x] Create `src-tauri/src/services/palette_service.rs`
  - [x] Move synchronous palette CRUD commands to `commands/palette.rs` and delegate to `PaletteService`
  - [x] **Handle `generate_palette` separately as an async command**: retain `async fn generate_palette(...)` signature, preserve layer tile sampling (MedianCut/KMeans) and async execution flow without forcing into synchronous service pattern
  - [x] Move pure color conversion commands (`generate_ramp_palette`, `generate_harmony_palette`, `colors_to_oklab`, `get_palette_oklab`) into `commands/color_lab.rs`
  - [x] **BEFORE COMMITTING**: Diff extracted palette/color commands against `pre-refactor-commands` baseline
  - [x] Remove extracted commands from `commands.rs`
  - [x] Verify: `cargo build --all && cargo test --all`

- [ ] **Phase 8: Document Domain Extraction & File Deletion**
  - [x] Create `src-tauri/src/services/document_service.rs`
  - [x] Move document lifecycle commands (`load_image`, `create_document`, `new_document`, `list_open_documents`, `set_active_document`, `close_document`, `get_document_snapshot`, `open_project`, `save_project`, `save_project_as`, `export_pattern`, `import_pattern`, `export_image`, `import_image_layer`, `is_document_dirty`, `get_recent_files`) to `commands/document.rs` and delegate to `DocumentService`
  - [x] Re-export document DTOs (`DocumentResponse`, `DocumentChangedPayload`, etc.) in `commands/mod.rs`
  - [x] **BEFORE COMMITTING**: Diff against `pre-refactor-commands` baseline to ensure document dirty flag semantics and recent files write-on-success behavior are preserved
  - [x] Confirm `commands.rs` is completely empty and delete `src-tauri/src/commands.rs`
  - [x] Update `src-tauri/src/main.rs` to register commands from `commands::document::*`, `commands::layers::*`, etc.
  - [x] Final verification: `cargo build --all && cargo test --all`

---

## Final Definition of Done Checklist

- [x] `src-tauri/src/commands.rs` is removed from repository.
- [x] Every public `#[tauri::command]` signature (function name, parameter names/types, return type) is 100% identical to the `pre-refactor-commands` baseline.
- [x] All DTO structs (`SelectionChangedPayload`, `LayerNodeDto`, `DocumentResponse`, etc.) are re-exported in `commands/mod.rs` so module paths and macro imports remain unbroken.
- [x] All IPC handlers in `commands/*.rs` are short wrappers (3–8 lines).
- [x] `AppState` cleanly embeds `TileState`, `UiState`, and `HistoryState`.
- [x] `generate_palette` remains properly async and non-blocking for layer tile sampling.
- [x] No changes were made to `frontend/src/shared/ipc/`.
- [x] All behavior preservation invariants (undo order, invalidation order, dirty flag, recent files logging) verified via `git diff pre-refactor-commands` BEFORE committing each phase.
- [x] Automated tests pass: `cargo test --all` (note: pre-existing property tests in engine-project have unrelated failures).
