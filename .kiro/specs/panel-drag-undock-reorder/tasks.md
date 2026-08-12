# Implementation Plan: Panel Drag Undock & Reorder

## Overview

This plan implements drag-based panel interactions for the sidebar: drag-to-reorder (vertical repositioning within the sidebar) and drag-to-undock (horizontal drag away from the sidebar to create a floating window). The implementation spans both the React frontend (new `usePanelDrag` hook, IPC functions, CSS) and the Rust backend (new IPC commands, `PanelManager` order tracking).

## Tasks

- [x] 1. Backend: Add panel order to PanelManager
  - [x] 1.1 Add `panel_order: Vec<String>` field to `PanelManager` and update constructors
    - Add `panel_order` field to the `PanelManager` struct in `src-tauri/src/panel_manager.rs`
    - Initialize with default order `["effect", "layers", "colorlab"]` in `PanelManager::new()`
    - Restore from persisted state in `PanelManager::from_persisted()`, falling back to default if missing
    - Add `pub fn get_order(&self) -> &[String]` method
    - Add `pub fn get_state_with_order(&self) -> (Vec<PanelInfo>, Vec<String>)` method
    - Update `serialize()` to include panel_order in output
    - _Requirements: 7.2, 7.4_

  - [x] 1.2 Implement `PanelManager::reorder()` method with validation
    - Add `pub fn reorder(&mut self, order: Vec<String>) -> Result<(), PanelError>` method
    - Validate that all provided IDs are known panels (reject unknown IDs)
    - Validate that the list is a complete permutation (same count, no duplicates)
    - Update `panel_order` field on success
    - Return `PanelError::UnknownPanel` or a new `PanelError::InvalidOrder` variant on failure
    - _Requirements: 9.2, 9.3, 9.4_

  - [ ]* 1.3 Write Rust unit tests for panel order and reorder
    - Test `PanelManager::new()` initializes default order
    - Test `reorder()` accepts valid permutations
    - Test `reorder()` rejects unknown panel IDs
    - Test `reorder()` rejects incomplete or duplicate lists
    - Test `from_persisted()` restores panel_order
    - Test `get_state_with_order()` returns correct data
    - _Requirements: 9.2, 9.4, 7.2, 7.4_

- [x] 2. Backend: Implement new IPC commands
  - [x] 2.1 Implement `undock_panel_with_size` Tauri command
    - Add command in `src-tauri/src/panel_commands.rs` accepting `panel_id: String, width: u32, height: u32, x: i32, y: i32`
    - Acquire PanelManager lock, call `pm.undock(&panel_id)`
    - Create `WebviewWindow` with the provided `width` and `height` as inner size
    - Set window position to `(x, y)` coordinates
    - Apply off-screen correction using existing `correct_bounds_for_monitors()`
    - On window creation failure, revert via `pm.dock()` and return error
    - Emit `panel-state-changed` event on success
    - Register the command in the Tauri app builder (main.rs or lib.rs)
    - _Requirements: 8.1, 8.2, 8.3, 3.2, 4.1, 4.2_

  - [x] 2.2 Implement `reorder_panels` Tauri command
    - Add command in `src-tauri/src/panel_commands.rs` accepting `order: Vec<String>`
    - Acquire PanelManager lock, call `pm.reorder(order)`
    - Emit `panel-state-changed` event with updated state including panel_order
    - Return error string if validation fails
    - Register the command in the Tauri app builder
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

  - [x] 2.3 Update `panel-state-changed` event payload to include `panel_order`
    - Update `emit_panel_state()` helper to emit a struct/object containing both `panels: Vec<PanelInfo>` and `panel_order: Vec<String>`
    - Update all existing event emission sites to use the new payload shape
    - _Requirements: 7.1, 5.5_

- [x] 3. Checkpoint - Backend compiles and tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 4. Frontend: Add IPC functions and update types
  - [x] 4.1 Add `undockPanelWithSize` and `reorderPanels` IPC functions
    - Add `undockPanelWithSize(panelId, width, height, x, y)` to `frontend/src/ipc/panelCommands.ts`
    - Add `reorderPanels(order: string[])` to `frontend/src/ipc/panelCommands.ts`
    - Both call `invoke()` with appropriate command names and parameters
    - _Requirements: 8.1, 9.1_

  - [x] 4.2 Update panel types and event listener for new payload shape
    - Add `PanelStateSnapshot` interface to `frontend/src/types/panels.ts` with `panels` and `panel_order` fields
    - Update `usePanels` hook event listener to handle the new `panel-state-changed` payload shape (object with `panels` and `panel_order`)
    - Expose `panelOrder: PanelId[]` from the `usePanels` hook return
    - _Requirements: 7.3, 5.5_

- [x] 5. Frontend: Implement `usePanelDrag` hook
  - [x] 5.1 Create the `usePanelDrag` hook with drag session lifecycle
    - Create `frontend/src/hooks/usePanelDrag.ts`
    - Implement `DragState` interface tracking: active, panelId, mode, start/current positions, dropIndex, sourceIndex
    - On mousedown: record start position, attach document-level `mousemove` and `mouseup` listeners
    - On mousemove: check 5px Euclidean distance threshold to activate drag session
    - Prevent text selection and default drag behavior during active drag
    - On mouseup or Escape: clean up listeners, reset state
    - _Requirements: 1.1, 1.3, 1.4, 10.1, 10.2, 10.3_

  - [x] 5.2 Implement mode detection (undock vs reorder) in `usePanelDrag`
    - On each mousemove during active drag: compare cursor X to `sidebarRef.left - 50`
    - Set mode to `'undock'` when cursor is beyond threshold, `'reorder'` otherwise
    - _Requirements: 2.1, 5.1_

  - [x] 5.3 Implement drop index calculation for reorder mode
    - Calculate panel midpoints from DOM elements (top + height/2), excluding the source panel
    - Determine drop index as the count of midpoints <= cursor Y
    - Update `dropIndex` in state on each mousemove when in reorder mode
    - _Requirements: 5.2, 6.2_

  - [x] 5.4 Implement mouseup handlers: dispatch undock or reorder
    - In undock mode on mouseup: measure source panel's DOM width/height, call `onUndock(panelId, width, height, screenX, screenY)`
    - In reorder mode on mouseup: compute new order array by removing source and inserting at dropIndex, call `onReorder(newOrder)`
    - In idle mode (threshold not met): treat as click, do nothing
    - _Requirements: 2.1, 3.1, 5.3, 4.1_

  - [x] 5.5 Implement Escape key cancellation
    - Listen for `keydown` Escape during active drag session
    - On Escape: reset all drag state, remove visual feedback, do not call any callbacks
    - _Requirements: 10.1, 10.2, 10.3_

  - [ ]* 5.6 Write property tests for drag threshold detection
    - **Property 1: Drag Threshold Detection**
    - Use fast-check to generate random start/current positions
    - Assert drag activates iff Euclidean distance >= 5px
    - **Validates: Requirements 1.1, 1.4**

  - [ ]* 5.7 Write property tests for mode detection
    - **Property 2: Mode Detection — Undock vs Reorder**
    - Use fast-check to generate random cursor X positions and sidebar left edge values
    - Assert mode is "undock" iff cursorX < sidebarLeft - 50
    - **Validates: Requirements 2.1, 5.1**

  - [ ]* 5.8 Write property tests for drop index calculation
    - **Property 3: Drop Index Calculation**
    - Use fast-check to generate arrays of panel positions (top, height) and cursor Y
    - Assert drop index equals count of midpoints <= cursorY, excluding source panel
    - **Validates: Requirements 5.2, 6.2**

  - [ ]* 5.9 Write property tests for array reorder
    - **Property 4: Array Reorder Preserves Elements**
    - Use fast-check to generate arrays and valid sourceIndex/dropIndex pairs
    - Assert reorder result is a permutation of original with source at target position
    - **Validates: Requirements 5.3**

  - [ ]* 5.10 Write property tests for panel ID validation
    - **Property 5: Panel ID Validation**
    - Use fast-check to generate lists of strings (mix of valid/invalid panel IDs)
    - Assert validation passes iff list is exact permutation of known panel IDs
    - **Validates: Requirements 9.2, 9.4**

- [x] 6. Checkpoint - Hook logic complete and property tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 7. Frontend: CSS and visual feedback
  - [x] 7.1 Add drag feedback and drop indicator CSS classes
    - Add `.docked-panel-header--dragging` with `cursor: grabbing` to `frontend/src/App.css`
    - Add `.docked-panel--dragging` with `opacity: 0.4; pointer-events: none`
    - Add `.docked-panel--undock-preview` with `opacity: 0.3; border: 1px dashed var(--border-color)`
    - Add `.panel-drop-indicator` with `height: 2px; background: var(--color-highlight, #4a90d9); margin: 0 4px; border-radius: 1px; flex-shrink: 0`
    - _Requirements: 1.2, 2.2, 6.1_

- [x] 8. Frontend: Integrate `usePanelDrag` into App.tsx
  - [x] 8.1 Wire `usePanelDrag` hook into App.tsx sidebar rendering
    - Import and initialize `usePanelDrag` with `sidebarRef`, `panelOrder` from `usePanels`, and callback handlers
    - Replace the static `PANEL_IDS` ordering in `visibleDockedPanels` with `panelOrder` from the hook
    - Pass `handleMouseDown` from drag hook to each `.docked-panel-header` (onMouseDown)
    - Apply `getPanelStyle` from drag hook to each panel wrapper for opacity during drag
    - _Requirements: 5.5, 1.1_

  - [x] 8.2 Render drop indicator in sidebar during reorder drag
    - In the panel map loop, conditionally render `<div className="panel-drop-indicator" />` at the `dropIndicatorIndex` position
    - Only render when `dragState.active && dragState.mode === 'reorder'`
    - Remove indicator when drag ends
    - _Requirements: 6.1, 6.2, 6.3_

  - [x] 8.3 Implement undock and reorder callbacks that call IPC
    - Create `handleDragUndock` callback: calls `undockPanelWithSize(panelId, width, height, screenX, screenY)`
    - Create `handleDragReorder` callback: calls `reorderPanels(newOrder)`
    - Handle errors from IPC calls: surface via existing error notification system
    - _Requirements: 8.1, 9.1, 2.4_

  - [ ]* 8.4 Write unit tests for App.tsx drag integration
    - Test that mousedown on panel header initiates drag tracking
    - Test that drop indicator renders at correct position during reorder
    - Test that undock IPC is called with correct size/position on undock drag
    - Test that reorder IPC is called with correct order on reorder drag
    - Test Escape cancellation produces no IPC calls
    - _Requirements: 1.1, 5.3, 8.1, 9.1, 10.3_

- [x] 9. Final checkpoint - Full integration works end-to-end
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document (Properties 1-5)
- Property 6 (undock window position matches release point) is validated implicitly through the integration of `undockPanelWithSize` passing screen coordinates directly to the backend
- The `panel-state-changed` event payload change (task 2.3) affects both new and existing event listeners — task 4.2 handles the frontend migration
- The existing `undock_panel` command remains unchanged for button-based undocking

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "4.1", "7.1"] },
    { "id": 1, "tasks": ["1.2", "4.2"] },
    { "id": 2, "tasks": ["1.3", "2.1", "2.2", "2.3"] },
    { "id": 3, "tasks": ["5.1"] },
    { "id": 4, "tasks": ["5.2", "5.3", "5.5"] },
    { "id": 5, "tasks": ["5.4", "5.6", "5.7", "5.8", "5.9", "5.10"] },
    { "id": 6, "tasks": ["8.1"] },
    { "id": 7, "tasks": ["8.2", "8.3"] },
    { "id": 8, "tasks": ["8.4"] }
  ]
}
```
