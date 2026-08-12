# Implementation Plan: Multi-Window Dockable Panels

## Overview

Implement a multi-window panel system for Dither Yuki 2 that allows Effect Settings, Layers, and Color Lab panels to be undocked from the main window sidebar into independent OS-level Tauri WebView windows. Uses a centralized Rust-side PanelManager, cross-window event synchronization, and disk persistence for session restoration.

## Tasks

- [x] 1. Set up PanelManager data model and core logic
  - [x] 1.1 Create `src-tauri/src/panel_manager.rs` with PanelInfo, SavedBounds, PanelManager struct, and PanelError
    - Define `PanelId` type alias, `SavedBounds` struct (x, y, width, height), `PanelInfo` struct (id, docked, visible, window_label, saved_bounds)
    - Implement `PanelManager::new()` initializing three panels (effect, layers, colorlab) all docked and visible
    - Implement `PanelManager::from_persisted(panels: Vec<PanelInfo>)` for restoring from disk
    - Implement `validate_panel_id()`, `get_state()`, `undock()`, `dock()`, `hide()`, `show()`, `update_bounds()`, `serialize()`
    - Define `UndockResult` struct and `PanelError` enum with `thiserror`
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [x] 1.2 Create `src-tauri/src/panel_persistence.rs` for disk save/load
    - Implement `panel_state_path(app_handle)` returning `{app_data_dir}/panel_state.json`
    - Implement `load_panel_state(app_handle)` → `Option<Vec<PanelInfo>>` with graceful fallback on missing/corrupt file
    - Implement `save_panel_state(app_handle, panels)` with silent failure (log warning)
    - Use versioned JSON format: `{ "version": 1, "panels": [...] }`
    - _Requirements: 8.2, 8.3, 8.6_

  - [x] 1.3 Extend `AppState` in `src-tauri/src/commands.rs` with `panel_manager: Mutex<PanelManager>`
    - Add `use crate::panel_manager::PanelManager;` import
    - Add `pub panel_manager: Mutex<PanelManager>` field to `AppState`
    - Update `AppState` initialization in `main.rs` to create PanelManager (load from disk or default)
    - _Requirements: 1.1, 1.2_

  - [x]* 1.4 Write property tests for PanelManager (Rust — proptest)
    - **Property 1: Panel Set Invariant** — After any sequence of operations, PanelManager always contains exactly 3 panels with IDs "effect", "layers", "colorlab"
    - **Validates: Requirements 1.1**

  - [x]* 1.5 Write property test for invalid panel ID rejection
    - **Property 2: Invalid Panel ID Rejection** — Any string not in {"effect", "layers", "colorlab"} returns UnknownPanel error with no state change
    - **Validates: Requirements 1.4, 2.7, 3.5, 4.6**

  - [x]* 1.6 Write property test for state serialization round-trip
    - **Property 3: State Serialization Round-Trip** — Serialize then deserialize produces equivalent PanelInfo records
    - **Validates: Requirements 1.3, 5.4**

- [x] 2. Implement panel IPC commands
  - [x] 2.1 Create `src-tauri/src/panel_commands.rs` with Tauri command handlers
    - Implement `get_panels_state` command: acquire Mutex, return `Vec<PanelInfo>`
    - Implement `undock_panel` command: validate ID, check already-floating (focus existing), create WebviewWindow via `WebviewWindowBuilder`, update state, emit `panel-state-changed` event
    - Implement `dock_panel` command: validate ID, check already-docked (no-op), save bounds, close floating window, update state, emit event
    - Implement `hide_panel` command: validate ID, set visible=false (hide OS window if floating), emit event; no-op if already hidden
    - Implement `show_panel` command: validate ID, set visible=true (show OS window if floating), emit event; no-op if already visible
    - Implement `save_panel_bounds` command: validate ID, update saved_bounds in PanelManager
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 3.1, 3.2, 3.3, 3.5, 3.6, 4.1, 4.2, 4.3, 4.4, 4.7, 5.1, 8.1_

  - [x] 2.2 Register panel commands in `src-tauri/src/main.rs`
    - Add `mod panel_manager; mod panel_commands; mod panel_persistence;` to main.rs
    - Register all panel commands in `.invoke_handler(tauri::generate_handler![...])`
    - Set up `on_window_event` handler for CloseRequested on panel windows → auto-dock
    - Load persisted panel state on startup, restore floating windows if previously undocked
    - Save panel state on app exit (before_exit or on_event for CloseRequested on main window)
    - Close all floating windows when main window closes
    - _Requirements: 3.4, 8.3, 8.4, 9.1, 9.2, 9.3, 9.4_

  - [x]* 2.3 Write property tests for undock/dock state transitions
    - **Property 4: Undock State Transition** — Undocking a docked panel sets docked=false, window_label=Some("panel-{id}"), preserves saved_bounds
    - **Validates: Requirements 2.2, 2.3**
    - **Property 5: Dock State Transition** — Docking a floating panel sets docked=true, window_label=None, preserves visible and saved_bounds
    - **Validates: Requirements 3.1, 3.2**

  - [x]* 2.4 Write property tests for visibility and idempotency
    - **Property 6: Visibility Toggle Transitions** — Hide sets visible=false, show sets visible=true, other fields unchanged
    - **Validates: Requirements 4.1, 4.4**
    - **Property 7: Operation Idempotency** — Repeated undock/dock/hide/show on panel already in that state is a no-op
    - **Validates: Requirements 2.5, 3.6, 4.7**

  - [x]* 2.5 Write property test for persistence round-trip
    - **Property 8: Persistence Round-Trip** — Save to disk then load produces equivalent PanelInfo records for any valid state
    - **Validates: Requirements 8.3**

- [x] 3. Checkpoint - Ensure Rust backend compiles and tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 4. Create frontend types and IPC wrappers
  - [x] 4.1 Create `frontend/src/types/panels.ts` with TypeScript type definitions
    - Define `SavedBounds` interface (x, y, width, height as numbers)
    - Define `PanelInfo` interface (id, docked, visible, window_label, saved_bounds)
    - Define `PanelId` type union and `PANEL_IDS` constant array
    - Define `PANEL_DISPLAY_NAMES` and `PANEL_DEFAULT_BOUNDS` records
    - _Requirements: 10.1_

  - [x] 4.2 Create `frontend/src/ipc/panelCommands.ts` with typed invoke wrappers
    - Implement `getPanelsState()` → `invoke<PanelInfo[]>('get_panels_state')`
    - Implement `undockPanel(panelId)` → `invoke<void>('undock_panel', { panelId })`
    - Implement `dockPanel(panelId)` → `invoke<void>('dock_panel', { panelId })`
    - Implement `hidePanel(panelId)` → `invoke<void>('hide_panel', { panelId })`
    - Implement `showPanel(panelId)` → `invoke<void>('show_panel', { panelId })`
    - Implement `savePanelBounds(panelId, x, y, width, height)` → `invoke<void>('save_panel_bounds', {...})`
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5_

- [x] 5. Implement frontend routing and PanelWindow component
  - [x] 5.1 Refactor `frontend/src/main.tsx` to support query-parameter routing
    - Parse `window.location.search` for `panel` query parameter
    - If `panel` matches known IDs ("effect", "layers", "colorlab"), render `<PanelWindow panelId={...} />`
    - Otherwise render standard `<App />` layout
    - _Requirements: 6.1, 6.2, 6.5_

  - [x] 5.2 Create `frontend/src/components/PanelWindow.tsx` component
    - Render title bar with panel display name, Dock button, and Close button
    - Dock button invokes `dockPanel(panelId)` IPC command
    - Close button invokes `dockPanel(panelId)` and closes the window
    - Render the appropriate panel component based on panelId (EffectSettingsPanel, LayersPanel, or ColorLabWindow)
    - Listen to `panel-state-changed` events — close self if panel becomes docked by another action
    - Report window move/resize events to Rust via `savePanelBounds` (debounced 500ms)
    - _Requirements: 6.3, 6.4, 8.1_

  - [x]* 5.3 Write property test for routing fallback logic (Vitest + fast-check)
    - **Property 11: Unknown Panel Param Routing Fallback** — Any `panel` param not in {"effect", "layers", "colorlab"} renders standard App layout
    - **Validates: Requirements 6.5**

- [x] 6. Implement usePanels hook for cross-window state sync
  - [x] 6.1 Create `frontend/src/hooks/usePanels.ts` hook
    - Fetch initial panel state via `getPanelsState()` on mount
    - Subscribe to `panel-state-changed` Tauri event for real-time updates
    - Expose `panels`, `undock`, `dock`, `hide`, `show`, and `error` state
    - Handle IPC errors: set error state with operation name and reason
    - Unsubscribe from event listener on unmount
    - _Requirements: 5.2, 5.3, 5.5, 10.6, 10.7_

- [x] 7. Integrate dynamic sidebar into main App
  - [x] 7.1 Refactor `frontend/src/App.tsx` sidebar to use `usePanels()` hook
    - Replace hardcoded sidebar sections with dynamic panel rendering
    - Render only panels where `docked === true && visible === true`
    - Display panels in fixed order: Effect, Layers, Color Lab (top to bottom)
    - Distribute available sidebar height equally among visible docked panels
    - Collapse sidebar to zero width when no panels are docked and visible
    - Add Undock button in each panel header that calls `undock(panelId)`
    - Show error notification if undock command fails
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7_

  - [x]* 7.2 Write property test for sidebar rendering filter (Vitest + fast-check)
    - **Property 10: Sidebar Rendering Filter** — For any combination of panel states, sidebar shows exactly the subset where docked=true AND visible=true, in order [effect, layers, colorlab]
    - **Validates: Requirements 7.1**

- [x] 8. Checkpoint - Ensure frontend compiles and all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Implement window position persistence and off-screen correction
  - [x] 9.1 Add off-screen bounds correction logic to `panel_commands.rs`
    - When restoring a floating window from persisted bounds, check if window rectangle falls entirely outside available monitor boundaries
    - If off-screen, reposition to center of primary monitor preserving width and height
    - Use Tauri's monitor APIs to enumerate available screens
    - _Requirements: 8.4, 8.5_

  - [x]* 9.2 Write property test for off-screen bounds correction
    - **Property 9: Off-Screen Bounds Correction** — Any SavedBounds entirely outside all monitors is repositioned to primary monitor center while preserving width and height
    - **Validates: Requirements 8.5**

- [x] 10. Testing and hardening
  - [x]* 10.1 Write integration tests for panel lifecycle (Rust)
    - Test full cycle: all panels docked → undock → window created → dock → window closed
    - Test OS close button triggers auto-dock
    - Test main window close cascades to all floating windows
    - Test startup restoration of previously floating windows
    - _Requirements: 2.1, 3.1, 3.4, 8.4, 9.1_

  - [x]* 10.2 Write frontend integration tests for usePanels hook and event sync (Vitest)
    - Test initial state fetch and rendering
    - Test event subscription updates panel state
    - Test undock/dock/hide/show commands trigger correct IPC calls
    - Test error handling for rejected commands
    - _Requirements: 5.1, 5.2, 5.3, 10.6_

  - [x]* 10.3 Write edge case tests
    - Test double-undock doesn't create duplicate window (focuses existing)
    - Test dock on already-docked panel is a no-op
    - Test hide on already-hidden panel doesn't emit event
    - Test corrupted persistence file falls back to defaults
    - Test panel state remains consistent across rapid undock/dock sequences
    - _Requirements: 2.5, 3.6, 4.7, 8.6_

- [x] 11. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties (proptest for Rust, fast-check for TypeScript)
- Unit tests validate specific examples and edge cases
- The design uses real Tauri WebView windows — no CSS overlay simulation
- PanelManager lives behind `Mutex<PanelManager>` in AppState for thread-safe access
- Event payload is the full panel state snapshot (simple fan-out, no differential sync)

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["1.3", "1.4", "1.5", "1.6"] },
    { "id": 2, "tasks": ["2.1", "4.1"] },
    { "id": 3, "tasks": ["2.2", "2.3", "2.4", "2.5", "4.2"] },
    { "id": 4, "tasks": ["5.1", "5.2", "5.3"] },
    { "id": 5, "tasks": ["6.1"] },
    { "id": 6, "tasks": ["7.1", "7.2"] },
    { "id": 7, "tasks": ["9.1", "9.2"] },
    { "id": 8, "tasks": ["10.1", "10.2", "10.3"] }
  ]
}
```
