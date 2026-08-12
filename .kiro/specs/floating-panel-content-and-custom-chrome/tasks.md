# Implementation Plan: Floating Panel Content and Custom Chrome

## Overview

This implementation transforms floating panel windows from placeholder UI into fully interactive panels with self-contained IPC data hooks and custom window chrome. The work divides into: (1) Rust backend additions (selection state, document-changed events, frameless windows), (2) frontend IPC hooks for standalone panel operation, (3) PanelWindow refactoring with custom chrome and adapter components, and (4) cross-window selection synchronization.

## Tasks

- [x] 1. Rust backend: Selection state and IPC commands
  - [x] 1.1 Add SelectionState struct and field to AppState
    - Add `SelectionState` struct with `selected_layer_id: Option<u32>` and `selected_filter_id: Option<String>` to `commands.rs`
    - Add `pub selection: Mutex<SelectionState>` field to `AppState` struct
    - Initialize `selection: Mutex::new(SelectionState::default())` in `main.rs` app state construction
    - _Requirements: 5.1, 5.3, 6.3_

  - [x] 1.2 Add `set_selection` and `get_selection` IPC commands
    - Implement `set_selection` command that updates `state.selection` mutex and emits `selection-changed` event to all windows via `app_handle.emit()`
    - Implement `get_selection` command that reads and returns current `SelectionState` from mutex
    - Define `SelectionChangedPayload` struct with `selected_layer_id` and `selected_filter_id`
    - Register both commands in `main.rs` `invoke_handler`
    - _Requirements: 5.1, 5.2, 5.3, 6.3_

  - [x] 1.3 Add `emit_document_changed` helper and integrate into all mutating commands
    - Define `DocumentChangedPayload` struct with `kind: String` and `layer_id: Option<u32>`
    - Implement `emit_document_changed(app_handle, kind, layer_id)` helper function
    - Add `app_handle: AppHandle` parameter to all mutating commands: `update_filter`, `add_filter`, `remove_filter`, `reorder_filter`, `set_layer_props`, `load_image`, `add_layer`, `remove_layer`, `reorder_layer`
    - Call `emit_document_changed` after each successful mutation with appropriate `kind` string
    - _Requirements: 6.4_

  - [ ]* 1.4 Write property test for document mutation event emission
    - **Property 5: Document Mutation Event Emission**
    - **Validates: Requirements 6.4**

- [x] 2. Rust backend: Frameless windows and resize constraints
  - [x] 2.1 Change `decorations(true)` to `decorations(false)` and add min size in panel window creation
    - In `panel_commands.rs` `undock_panel`: change `.decorations(true)` to `.decorations(false)` and add `.min_inner_size(280.0, 200.0)`
    - In `main.rs` startup restoration loop: change `.decorations(true)` to `.decorations(false)` and add `.min_inner_size(280.0, 200.0)`
    - _Requirements: 1.1, 7.1_

- [x] 3. Checkpoint — Rust backend compiles
  - Ensure all Rust code compiles without errors (`cargo build`), ask the user if questions arise.

- [x] 4. Frontend: IPC data hooks
  - [x] 4.1 Create `useDocumentState` hook
    - Create `frontend/src/hooks/useDocumentState.ts`
    - Implement hook that fetches document metadata via `get_document_snapshot` IPC on mount
    - Subscribe to `document-changed` Tauri event and refetch with 50ms debounce
    - Return `{ docId, width, height, hasDocument, error }`
    - Handle IPC failures gracefully (set error field, return empty state)
    - _Requirements: 6.1, 6.4, 6.5, 6.6_

  - [x] 4.2 Create `useLayerState` hook
    - Create `frontend/src/hooks/useLayerState.ts`
    - Implement hook that fetches layer tree via `get_layer_tree` and filters via `get_document_snapshot` on mount
    - Subscribe to `document-changed` Tauri event and refetch with 50ms debounce
    - Return `{ layers, allFilters, error, refreshLayers }`
    - Handle IPC failures gracefully
    - _Requirements: 6.2, 6.4, 6.5, 6.6_

  - [x] 4.3 Create `useSelectionState` hook
    - Create `frontend/src/hooks/useSelectionState.ts`
    - Fetch initial selection via `get_selection` IPC on mount
    - Subscribe to `selection-changed` event for cross-window updates
    - Implement `setSelection(layerId, filterId)` that updates local state and invokes `set_selection` IPC
    - Use `isLocalUpdate` ref to prevent echo when receiving own events
    - _Requirements: 5.1, 5.2, 5.5, 6.3_

  - [ ]* 4.4 Write unit tests for useDocumentState debounce behavior
    - **Property 6: Event Debounce Coalescing**
    - **Validates: Requirements 6.5**

  - [ ]* 4.5 Write unit tests for useSelectionState round-trip correctness
    - **Property 8: Selection Payload Round-Trip**
    - **Validates: Requirements 5.2, 5.3**

  - [ ]* 4.6 Write unit tests for IPC error handling in hooks
    - **Property 7: Graceful IPC Error Handling**
    - **Validates: Requirements 6.6**

- [x] 5. Frontend: PanelWindow custom chrome and styling
  - [x] 5.1 Create PanelWindow.css with retro Mac OS custom chrome styles
    - Create `frontend/src/components/PanelWindow.css`
    - Implement `.panel-window` root flex container (100vh/100vw)
    - Implement `.panel-window-titlebar` with 22px height, flex row layout
    - Implement `.panel-window-titlebar-drag` area for data-tauri-drag-region
    - Implement `.panel-window-titlebar-lines` with repeating-linear-gradient stripe pattern
    - Implement `.panel-window-title` text style
    - Implement `.panel-window-btn` with 3D embossed border pattern (inset box-shadows using --color-black/--color-white)
    - Implement `.panel-window-content` flex:1 overflow hidden area
    - Use existing CSS variables (--color-gray, --color-black, --color-white, --font-family, etc.)
    - _Requirements: 1.2, 1.8, 7.2_

  - [x] 5.2 Update PanelWindow.tsx with custom chrome titlebar
    - Import PanelWindow.css
    - Replace existing `.panel-window-titlebar` markup with custom chrome structure
    - Add `.panel-window-titlebar-drag` div with `data-tauri-drag-region` attribute
    - Add stripe lines elements and panel title text inside drag region
    - Add minimize button (calls `getCurrentWindow().minimize()`)
    - Add dock button (calls existing `dockPanel` IPC)
    - Add close button (calls `dockPanel` IPC)
    - Ensure buttons do NOT have `data-tauri-drag-region` attribute
    - _Requirements: 1.2, 1.3, 1.4, 1.5, 1.6, 1.7_

  - [ ]* 5.3 Write property tests for drag region exclusion and panel name mapping
    - **Property 1: Panel Display Name Mapping**
    - **Property 2: Drag Region Exclusion on Buttons**
    - **Validates: Requirements 1.4, 1.7**

- [x] 6. Frontend: Floating panel adapter components
  - [x] 6.1 Create FloatingEffectAdapter component in PanelWindow.tsx
    - Implement adapter using `useDocumentState`, `useLayerState`, `useSelectionState` hooks
    - Build `LayerWithFilters` prop from hook data (same logic as App.tsx)
    - Wire `handleUpdateParams` to `update_filter` IPC
    - Wire `handleSelectEffect` to `add_filter` IPC
    - Pass constructed props to existing `EffectSettingsPanel` component
    - _Requirements: 2.1, 2.2, 2.3, 2.5, 2.6_

  - [x] 6.2 Create FloatingLayersAdapter component in PanelWindow.tsx
    - Implement adapter using `useLayerState`, `useSelectionState` hooks
    - Wire `onSelect` to `setSelection(layerId, null)`
    - Wire `onSelectFilter` to `setSelection(imageLayer.id, filterId)`
    - Wire `onRemoveFilter` to `remove_filter` IPC + refreshLayers
    - Wire `onReorderFilter` to `reorder_filter` IPC + refreshLayers
    - Wire `onToggleVisibility` / `onBlendModeChange` / `onOpacityChange` to `set_layer_props` IPC
    - Pass constructed props to existing `LayersPanel` component
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

  - [x] 6.3 Create FloatingColorLabAdapter component in PanelWindow.tsx
    - Implement adapter using `useDocumentState`, `useSelectionState` hooks
    - Render `ColorLabWindow` with `isOpen={true}` and `floating={true}` prop
    - Wire `onApply` to `add_palette` IPC
    - Handle cancel as form reset (no window close in floating mode)
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 6.4 Add floating mode support to ColorLabWindow
    - Add optional `floating?: boolean` prop to ColorLabWindow
    - When `floating={true}`, skip modal overlay/backdrop wrapper and render content directly
    - Preserve all palette creation, import, export, and generation functionality
    - _Requirements: 4.1, 4.5_

  - [x] 6.5 Replace placeholder content in PanelWindow.tsx renderPanelContent
    - Update `renderPanelContent` switch to return `<FloatingEffectAdapter />`, `<FloatingLayersAdapter />`, `<FloatingColorLabAdapter />` instead of placeholder divs
    - _Requirements: 2.1, 3.1, 4.1_

  - [ ]* 6.6 Write property test for effect type rendering completeness
    - **Property 3: Effect Type Rendering Completeness**
    - **Validates: Requirements 2.5**

- [x] 7. Frontend: Cross-window selection sync in App.tsx
  - [x] 7.1 Integrate useSelectionState in App.tsx for selection synchronization
    - Import and use `useSelectionState` hook in App.tsx
    - Replace or supplement existing `selectedLayerId`/`selectedFilterId` state with hook values
    - Call `setSelection()` when user changes selection in main window (layer click, filter click)
    - Listen for remote selection changes and update local UI state accordingly
    - _Requirements: 5.1, 5.2, 5.5_

  - [ ]* 7.2 Write property test for selection broadcast correctness
    - **Property 4: Selection Broadcast Correctness**
    - **Validates: Requirements 5.1, 5.3**

- [x] 8. Final checkpoint — Full integration
  - Ensure all Rust and frontend code compiles, floating panel windows render real content with custom chrome, and cross-window selection sync works. Ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The Rust backend changes (tasks 1–2) and frontend hooks (task 4) can be developed in parallel
- Adapter components (task 6) depend on hooks (task 4) being complete
- App.tsx integration (task 7) should be done last to avoid conflicts with adapter work

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "2.1", "4.1", "4.2", "5.1"] },
    { "id": 1, "tasks": ["1.2", "4.3", "5.2"] },
    { "id": 2, "tasks": ["1.3", "4.4", "4.5", "4.6", "5.3"] },
    { "id": 3, "tasks": ["1.4", "6.1", "6.2", "6.3", "6.4"] },
    { "id": 4, "tasks": ["6.5", "6.6"] },
    { "id": 5, "tasks": ["7.1"] },
    { "id": 6, "tasks": ["7.2"] }
  ]
}
```
