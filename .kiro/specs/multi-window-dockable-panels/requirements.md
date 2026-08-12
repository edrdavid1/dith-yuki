# Requirements Document

## Introduction

Multi-Window Dockable Panels enables the Effect Settings, Layers, and Color Lab panels to transition between two display modes: docked (embedded in the main window sidebar) and floating (rendered in independent OS-level windows via Tauri WebViews). Panel state is centralized in a Rust-side PanelManager, synchronized across all windows via Tauri events, and persisted for restoration on application restart. This feature leverages native windowing to support multi-monitor workflows without simulating floating via CSS overlays.

## Glossary

- **PanelManager**: A Rust-side state manager that holds the configuration and display mode for all registered panels.
- **Panel**: A UI component (Effect Settings, Layers, or Color Lab) that can be displayed either docked or floating.
- **PanelId**: A unique string identifier for a panel (e.g., "effect", "layers", "colorlab").
- **PanelInfo**: A data structure describing a panel's current state: identifier, docked flag, visibility flag, window label, and saved bounds.
- **Docked_Mode**: The display mode where a panel is embedded within the main window sidebar.
- **Floating_Mode**: The display mode where a panel is rendered in a separate OS-level Tauri WebView window.
- **Window_Label**: A unique Tauri window identifier assigned to a panel when it enters Floating_Mode.
- **Panel_State_Changed_Event**: A Tauri event broadcast to all windows whenever any panel's state changes.
- **Saved_Bounds**: The stored window position (x, y) and size (width, height) for a floating panel.
- **Main_Window**: The primary application window containing the toolbar, canvas, and sidebar.
- **Sidebar**: The right-hand column of the Main_Window that hosts docked panels.
- **AppState**: The centralized Rust application state that holds all shared managers including PanelManager.

## Requirements

### Requirement 1: Panel State Data Model

**User Story:** As a developer, I want a centralized data model for panel state, so that all windows share a single source of truth about panel configuration.

#### Acceptance Criteria

1. THE PanelManager SHALL maintain a PanelInfo record for each panel in the fixed set (Effect, Layers, ColorLab) containing: PanelId (string identifier), docked flag (boolean), visible flag (boolean), optional Window_Label (the Tauri window label when floating), and optional Saved_Bounds (x, y, width, height in screen pixels as integers).
2. WHEN the application starts, THE PanelManager SHALL initialize with all panels in docked state (docked = true), visible state (visible = true), Window_Label = None, and Saved_Bounds = None.
3. THE PanelManager SHALL provide a method to retrieve the full list of PanelInfo records as a JSON-serializable snapshot, returning a consistent view even if another window is concurrently mutating panel state.
4. IF a Tauri command references a PanelId that does not match any panel in the fixed set, THEN THE PanelManager SHALL return an error indicating the panel identifier is unrecognized, without modifying any state.
5. THE PanelManager SHALL serialize all read and write operations on panel state such that no caller observes a partially-updated PanelInfo record.

### Requirement 2: Undock Panel

**User Story:** As a user, I want to undock a panel from the sidebar into its own window, so that I can position it freely on any monitor.

#### Acceptance Criteria

1. WHEN the undock_panel command is invoked with a PanelId matching one of the registered panel identifiers ("effect", "layers", "colorlab"), THE PanelManager SHALL create a new Tauri WebView window with a Window_Label unique across all currently open windows.
2. WHEN the undock_panel command is invoked, THE PanelManager SHALL set the panel's docked flag to false and store the assigned Window_Label in the panel's state.
3. WHEN a panel has Saved_Bounds from a previous session, THE PanelManager SHALL open the new window at those saved coordinates and dimensions.
4. WHEN a panel has no Saved_Bounds, THE PanelManager SHALL open the new window with default dimensions of 400×600 pixels, centered on the monitor containing the main application window.
5. IF the undock_panel command is invoked for a panel that is already in Floating_Mode, THEN THE PanelManager SHALL focus the existing floating window instead of creating a duplicate.
6. WHEN the undock_panel command completes successfully, THE PanelManager SHALL emit a Panel_State_Changed_Event containing the full list of panel states to all open windows.
7. IF the undock_panel command is invoked with a PanelId that does not match any registered panel identifier, THEN THE PanelManager SHALL return an error indicating the panel identifier is unrecognized and make no changes to panel state.
8. IF the PanelManager fails to create the Tauri WebView window, THEN THE PanelManager SHALL return an error indicating window creation failed, retain the panel's docked state as true, and not emit a Panel_State_Changed_Event.

### Requirement 3: Dock Panel

**User Story:** As a user, I want to dock a floating panel back into the sidebar, so that I can consolidate my workspace.

#### Acceptance Criteria

1. WHEN the dock_panel command is invoked with a PanelId that exists in the PanelManager registry ("effect", "layers", or "colorlab"), THE PanelManager SHALL close the associated floating window and the panel SHALL reappear in its original position within the Sidebar within 500 ms.
2. WHEN the dock_panel command is invoked for a valid panel, THE PanelManager SHALL set the panel's docked flag to true and clear the Window_Label, such that a subsequent get_panels_state call returns docked=true and window_label=None for that panel.
3. WHEN the dock_panel command completes successfully, THE PanelManager SHALL emit a Panel_State_Changed_Event containing the full list of PanelInfo (id, docked, visible, window_label for each panel) to all open windows.
4. WHEN a floating panel window is closed by the user via the OS close button, THE PanelManager SHALL execute the same dock_panel logic, resulting in the panel returning to docked state with the same observable outcomes as criteria 1–3.
5. IF the dock_panel command is invoked with a PanelId that does not exist in the PanelManager registry, THEN THE PanelManager SHALL return an error indicating an unknown panel identifier and make no state changes.
6. IF the dock_panel command is invoked for a panel that is already docked, THEN THE PanelManager SHALL complete successfully without side effects (no window close attempted, no duplicate event emitted) and return the current panel state unchanged.

### Requirement 4: Panel Visibility

**User Story:** As a user, I want to show or hide panels without destroying their state, so that I can declutter my workspace temporarily.

#### Acceptance Criteria

1. WHEN the hide_panel command is invoked with a valid PanelId, THE PanelManager SHALL set the panel's visible flag to false and emit a Panel_State_Changed_Event.
2. WHEN a panel in Docked_Mode has its visible flag set to false, THE Sidebar SHALL remove that panel from its rendered content and redistribute the vacated space to the remaining visible docked panels.
3. WHEN a panel in Floating_Mode has its visible flag set to false, THE PanelManager SHALL hide the associated floating window at the OS level without destroying it, preserving the window's position, size, and internal component state.
4. WHEN the show_panel command is invoked with a valid PanelId, THE PanelManager SHALL set the panel's visible flag to true and emit a Panel_State_Changed_Event.
5. WHEN a hidden panel is made visible again, THE PanelManager SHALL restore the panel with all prior internal state intact (scroll position, form values, selected options) as it was before hiding.
6. IF the hide_panel or show_panel command is invoked with a PanelId that does not exist in the PanelManager registry, THEN THE PanelManager SHALL reject the command without modifying any state and return an error indicating an unknown panel identifier.
7. IF the hide_panel command is invoked for a panel whose visible flag is already false, or the show_panel command is invoked for a panel whose visible flag is already true, THEN THE PanelManager SHALL complete the command without emitting a Panel_State_Changed_Event and without modifying state.

### Requirement 5: Cross-Window State Synchronization

**User Story:** As a user, I want all windows to reflect panel state changes instantly, so that my workspace remains consistent across monitors.

#### Acceptance Criteria

1. WHEN any panel state changes (undock, dock, show, hide), THE PanelManager SHALL emit a Panel_State_Changed_Event containing the full list of PanelInfo records to all open windows within 100 ms of the state mutation completing.
2. WHEN the Main_Window receives a Panel_State_Changed_Event, THE Sidebar SHALL update its rendered panels to match the new state within 200 ms of receiving the event.
3. WHEN a floating panel window receives a Panel_State_Changed_Event indicating its panel has been docked, THE floating window SHALL close itself.
4. THE PanelManager SHALL serialize the Panel_State_Changed_Event payload using JSON format compatible with the frontend TypeScript PanelInfo type definition.
5. THE Panel_State_Changed_Event SHALL be delivered to all open windows including both the Main_Window and all floating panel windows.

### Requirement 6: Frontend Routing by Query Parameter

**User Story:** As a developer, I want floating panel windows to render only their panel content, so that each window has minimal overhead and focused UI.

#### Acceptance Criteria

1. WHEN a Tauri window is opened with a URL containing a `panel` query parameter with a value of "effect", "layers", or "colorlab", THE frontend SHALL render only the corresponding panel component (Effect Settings, Layers, or Color Lab respectively) without the main layout.
2. WHEN no `panel` query parameter is present in the window URL, THE frontend SHALL render the standard application layout.
3. THE PanelWindow component SHALL wrap the panel content in a title bar containing the panel name as text, a Dock button that invokes the `dock_panel` IPC command for that panel, and a Close button that invokes the `dock_panel` IPC command and closes the window.
4. THE panel components (Effect Settings, Layers, Color Lab) SHALL maintain access to shared application state (document data, layer selections, palette data) via Tauri IPC and event listeners, producing identical interactive behavior (parameter changes, selections, visual updates) whether rendered in Docked_Mode within the Sidebar or standalone in a floating window.
5. IF the `panel` query parameter is present but its value does not match any known PanelId ("effect", "layers", "colorlab"), THEN THE frontend SHALL render the standard application layout.

### Requirement 7: Dynamic Sidebar

**User Story:** As a user, I want the sidebar to adapt when panels are undocked or hidden, so that remaining panels use the available space effectively.

#### Acceptance Criteria

1. WHEN the application starts, THE Sidebar SHALL query the PanelManager state and render only panels where docked is true and visible is true, displayed in the fixed order: Effect, Layers, Color Lab (top to bottom).
2. WHEN a Panel_State_Changed_Event is received, THE Sidebar SHALL update the set of rendered panels to match the current PanelManager state (showing only panels where docked is true and visible is true) within 200 ms of receiving the event.
3. WHEN multiple panels are docked and visible, THE Sidebar SHALL distribute the available sidebar height equally among the docked panels (each panel receiving an equal share of the sidebar height).
4. WHEN only one panel remains docked and visible, THE Sidebar SHALL allocate the full sidebar height to that panel.
5. WHEN no panels are docked and visible, THE Sidebar SHALL collapse to zero width so that the Preview_Window expands to occupy the full body width.
6. THE Sidebar SHALL provide an Undock button in each docked panel header that invokes the undock_panel command for that panel.
7. IF the undock_panel command returns an error, THEN THE Sidebar SHALL display an error notification indicating the panel could not be undocked and retain the panel in its current docked state.

### Requirement 8: Panel Window Position Persistence

**User Story:** As a user, I want floating panel windows to remember their position and size, so that I do not have to rearrange them every time I undock.

#### Acceptance Criteria

1. WHEN a floating panel window is moved or resized, THE PanelManager SHALL update the Saved_Bounds for that panel with the new position (x, y) and dimensions (width, height) after a debounce interval of 500 ms from the last move or resize event.
2. WHEN the application exits, THE PanelManager SHALL persist all PanelInfo records including Saved_Bounds to disk.
3. WHEN the application starts and a previously persisted PanelInfo file exists, THE PanelManager SHALL load all PanelInfo records from disk and apply Saved_Bounds (position and dimensions) to each panel that was in Floating_Mode at last exit.
4. IF the persisted state indicates a panel was in Floating_Mode at last exit, THEN THE PanelManager SHALL automatically recreate that floating window at the Saved_Bounds position and dimensions on startup.
5. IF the Saved_Bounds position for a panel falls entirely outside all currently available screen boundaries, THEN THE PanelManager SHALL reposition that floating window to the center of the primary monitor while preserving its saved width and height.
6. IF the persisted PanelInfo file is missing or cannot be parsed, THEN THE PanelManager SHALL start all panels in their default docked state without displaying an error to the user.

### Requirement 9: Main Window Lifecycle

**User Story:** As a user, I want all floating panel windows to close when the main window closes, so that the application exits cleanly.

#### Acceptance Criteria

1. WHEN the Main_Window is closed, THE application SHALL close all floating panel windows within 3 seconds, forcibly terminating any that do not respond to the close request within that period.
2. WHEN the Main_Window is closed, THE PanelManager SHALL save the current panel state (docked/floating status, visibility, and last window position and size for each panel) before termination.
3. IF the PanelManager fails to save panel state during shutdown, THEN THE application SHALL proceed with termination without displaying an error to the user and without blocking the close operation.
4. WHEN a floating panel window is destroyed externally (e.g., OS force-close), THE PanelManager SHALL update that panel's state to Docked_Mode and emit a `panel-state-changed` event to the Main_Window without displaying errors to the user.

### Requirement 10: IPC Command Interface

**User Story:** As a frontend developer, I want typed IPC wrappers for all panel commands, so that I can interact with the PanelManager safely from TypeScript.

#### Acceptance Criteria

1. THE frontend SHALL expose a `getPanelsState()` async function that invokes the `get_panels_state` Tauri command and returns a typed array of PanelInfo objects, where each PanelInfo contains at minimum a panel identifier, a docked/undocked state, and a visible/hidden state.
2. THE frontend SHALL expose an `undockPanel(panelId: string)` async function that invokes the `undock_panel` Tauri command with the specified panel identifier and returns a Promise that resolves on success or rejects on failure.
3. THE frontend SHALL expose a `dockPanel(panelId: string)` async function that invokes the `dock_panel` Tauri command with the specified panel identifier and returns a Promise that resolves on success or rejects on failure.
4. THE frontend SHALL expose a `hidePanel(panelId: string)` async function that invokes the `hide_panel` Tauri command with the specified panel identifier and returns a Promise that resolves on success or rejects on failure.
5. THE frontend SHALL expose a `showPanel(panelId: string)` async function that invokes the `show_panel` Tauri command with the specified panel identifier and returns a Promise that resolves on success or rejects on failure.
6. IF any IPC panel command returns an error, THEN THE frontend SHALL display an error notification containing the name of the failed operation and the error reason returned by the backend.
7. IF an IPC panel command is invoked with a panel identifier that does not exist in the backend, THEN THE frontend SHALL reject the Promise with an error and display a notification indicating which panel identifier was not found.
