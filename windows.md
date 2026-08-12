

# Implementation Plan: Tauri Multi-Window Dockable Panels

## Overview

A system where the Effect, Layers, and Color Lab panels can be:
- **Docked** – embedded in the sidebar of the main window.
- **Floating** – moved to separate operating system windows (Tauri WebViews) that can be freely positioned, including on other monitors.

The state of all panels is stored in a centralized `PanelManager` on the Rust side, synchronized across windows via Tauri events, and restored when the application restarts.

---

## Tasks

### Wave 0 – Data model and basic Rust commands

- [ ] 1. **Add `PanelManager` to `AppState`**
  - File: `crates/engine-project/src/panel.rs` (or `src-tauri/src/panel_manager.rs`)
  - Structure:
    ```rust
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct PanelInfo {
        pub id: PanelId,            // "effect" | "layers" | "colorlab"
        pub docked: bool,
        pub visible: bool,
        pub window_label: Option<String>, // Tauri window label, if floating
    }

    pub struct PanelManager {
        panels: HashMap<PanelId, PanelInfo>,
        // Mutex for interior mutability, wrapped in Arc if shared access is needed
    }
    ```
  - Methods: `new()`, `undock(id, app_handle)`, `dock(id)`, `show(id)`, `hide(id)`, `get_state() -> Vec<PanelInfo>`.
  - `PanelManager` will reside in `AppState` as `Mutex<PanelManager>`.

- [ ] 2. **Tauri command `get_panels_state`**
  - Returns the current state of all panels (`Vec<PanelInfo>`).
  - Used by the frontend at startup to initialize the UI.

- [ ] 3. **Tauri command `undock_panel`**
  - Accepts `panel_id: String`.
  - Creates a new Tauri window via `tauri::WebviewWindowBuilder`:
    - label: unique (e.g., `"panel-effect"`)
    - url: `index.html?panel=effect`
    - size, position (can save last used or set defaults)
    - decorations: true, always on top or normal
  - Updates `PanelInfo`: `docked = false`, `window_label = Some(label)`.
  - Emits a `panel-state-changed` event to all windows.

- [ ] 4. **Tauri command `dock_panel`**
  - Accepts `panel_id`.
  - If the panel has a `window_label`, closes that window (programmatically).
  - Updates `PanelInfo`: `docked = true`, `window_label = None`.
  - Emits a `panel-state-changed` event.

- [ ] 5. **Handling user window close**
  - In the Tauri setup, attach a callback to `on_window_event` for every created window.
  - On `CloseRequested` (or `Destroyed`) – automatically call `dock_panel` to keep state consistent and return the window to the dock.

---

### Wave 1 – Frontend routing by query parameter

- [ ] 6. **Refactor `main.tsx`**
  - Determine if the current window is a panel window: read `window.location.search` (e.g., `?panel=effect`).
  - If the `panel` parameter is present and matches a known identifier, render `<PanelWindow panelId={id} />` instead of the main layout.
  - If no parameter – render the standard `<App />`.

- [ ] 7. **Create `PanelWindow` component**
  - File: `frontend/src/components/PanelWindow.tsx`
  - Accepts `panelId: string`.
  - Depending on `panelId`, renders:
    - `'effect'` → `<EffectPanel />` (existing component)
    - `'layers'` → `<LayersPanel />`
    - `'colorlab'` → `<ColorLabPanel />`
  - Wraps in a minimal layout (no toolbars, no sidebar).
  - The window itself should be compact, with a title, close/dock button.

- [ ] 8. **Adapt existing panels**
  - Ensure `EffectPanel`, `LayersPanel`, `ColorLabPanel` can work as standalone pages.
  - If they rely on global context (e.g., document access via a hook), it must remain available.
  - ColorLab must stop being a modal and become a normal panel.

---

### Wave 2 – Cross-window state synchronization

- [ ] 9. **`panel-state-changed` event**
  - Emitted from Rust on every panel state change (undock, dock, show, hide).
  - Payload: the full list of `Vec<PanelInfo>`.

- [ ] 10. **Frontend listener**
  - In the main window: listen to the `panel-state-changed` event and update the local sidebar state.
  - In panel windows: also listen to this event; if their panel becomes docked (triggered from another window), they can programmatically close or show a notification.

- [ ] 11. **IPC wrappers**
  - Add frontend wrappers in `frontend/src/ipc/commands.ts`:
    - `getPanelsState()` → `invoke('get_panels_state')`
    - `undockPanel(id)` → `invoke('undock_panel', { panelId: id })`
    - `dockPanel(id)` → `invoke('dock_panel', { panelId: id })`

---

### Wave 3 – Integration into the main UI

- [ ] 12. **Dynamic sidebar**
  - Remove the hardcoded `sidebar-section-top` / `sidebar-section-bottom` structure.
  - The sidebar renders an array of panels obtained from `getPanelsState()`.
  - For each panel, check `docked` and `visible`. If both are true, render its content directly inside the sidebar (preserving scrollability).
  - A `ResizeHandle` can remain between panels.

- [ ] 13. **Control buttons in panel headers**
  - In each panel (when docked), add an "Undock" button (or icon) that calls `undockPanel(id)`.
  - When the panel is in a separate window – a "Dock" button in its header that calls `dockPanel(id)`.
  - Also a "Close" button (hide the panel without destroying state) – calls `hidePanel(id)`.

- [ ] 14. **Save window positions and sizes**
  - When moving or resizing a separate window, save its bounds (x, y, width, height) in `PanelInfo`.
  - On the next undock, the window opens with the last saved bounds.
  - Can also be saved to localStorage for faster access, but the primary store is Rust.

---

### Wave 4 – Testing and hardening

- [ ] 15. **Integration tests**
  - Full cycle: open app, all panels docked → undock effect → window appears, sidebar shows empty slot → close window (X button) → panel automatically returns to dock.
  - Verify synchronization: undock from one window, see changes in another.
  - Verify app restart with saved state (if a panel was undocked, it should reopen in a separate window).

- [ ] 16. **Edge cases**
  - What if the main window is closed? All child windows also close (managed by Tauri).
  - Double-clicking Undock does not create a second window – the command checks if the window already exists.
  - If a panel was hidden (not visible) and the user clicks Undock, it first shows in a separate window.

---

## Task Dependency Graph

```json
{
  "waves": [
    { "tasks": ["1"] },
    { "tasks": ["2", "3", "4", "5"] },
    { "tasks": ["6", "7", "8"] },
    { "tasks": ["9", "10", "11"] },
    { "tasks": ["12", "13", "14"] },
    { "tasks": ["15", "16"] }
  ]
}
```

Waves should be executed sequentially; tasks within a wave can be parallelized.

---

## Notes

- This approach provides **real native windows** that can be dragged across monitors, leveraging standard OS mechanisms.
- Window state is managed centrally in Rust, eliminating desynchronization between windows.
- The frontend does not use `position: fixed` to simulate floating – all "floating" panels are actual Tauri WebViews.
- The system is extensible: any number of panels can be added by registering them in `PanelManager` and creating a component to render by `panel_id`.
- To save window positions, use `window.outerPosition()` and `window.outerSize()` on close or move, storing them in `PanelInfo`.