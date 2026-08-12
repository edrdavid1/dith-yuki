# Requirements Document

## Introduction

This feature addresses two gaps in the existing multi-window dockable panels implementation. First, floating panel windows currently render placeholder text instead of the actual interactive panel components (Effect Settings, Layers, Color Lab). Each panel must fetch its own data via IPC hooks when rendered standalone, since floating windows do not share the React component tree with the Main_Window. Second, floating panel windows currently display native OS window decorations alongside a custom titlebar. The OS decorations must be removed so that the application controls the entire window chrome — matching the Photoshop-style aesthetic with a custom CSS titlebar, drag region, and styled minimize/close buttons that integrate with the app's retro Mac OS theme.

## Glossary

- **Panel_Window**: The React component (`PanelWindow.tsx`) that renders a single panel in a standalone floating Tauri WebView window.
- **Custom_Chrome**: The application-drawn window frame (titlebar, drag region, close/minimize buttons) that replaces native OS window decorations.
- **Data_Tauri_Drag_Region**: An HTML attribute (`data-tauri-drag-region`) that marks an element as a window drag handle in Tauri frameless windows.
- **IPC_Hook**: A React hook that fetches data from the Rust backend via Tauri `invoke` commands, enabling panel components to operate independently of the App.tsx prop tree.
- **Decorations**: The Tauri window property controlling whether OS-native titlebar and borders are rendered; when `false`, the window is frameless.
- **Effect_Settings_Panel**: The panel component displaying filter parameters (dithering, curves, levels, glitch) for the currently selected effect layer.
- **Layers_Panel**: The panel component displaying the layer tree and virtual effect layers, with selection, visibility toggle, blend mode, and opacity controls.
- **Color_Lab_Panel**: The panel component for creating, importing, and editing color palettes.
- **Main_Window**: The primary application window containing the toolbar, canvas preview, and sidebar.
- **Sidebar**: The right-hand column of the Main_Window hosting docked panels.
- **PanelManager**: The Rust-side state manager holding panel configuration (from the existing multi-window-dockable-panels implementation).
- **Document_Snapshot**: The IPC response from `get_document_snapshot` containing the full layer tree with filters.

## Requirements

### Requirement 1: Frameless Floating Windows

**User Story:** As a user, I want floating panel windows to have no OS titlebar or borders, so that the app controls the entire window appearance matching its retro aesthetic.

#### Acceptance Criteria

1. WHEN the PanelManager creates a floating panel window via `undock_panel`, THE PanelManager SHALL set the Tauri window `decorations` property to `false`.
2. THE Panel_Window SHALL render a Custom_Chrome titlebar element as the topmost visual element in the window, spanning the full window width.
3. THE Custom_Chrome titlebar SHALL include a `data-tauri-drag-region` attribute on the drag area, enabling window repositioning by clicking and dragging on the titlebar.
4. THE Custom_Chrome titlebar SHALL display the panel display name as text within the drag region.
5. THE Custom_Chrome titlebar SHALL include a minimize button that minimizes the floating window at the OS level when clicked.
6. THE Custom_Chrome titlebar SHALL include a close button that invokes the `dock_panel` IPC command for the corresponding panel when clicked.
7. THE Custom_Chrome titlebar buttons SHALL NOT have the `data-tauri-drag-region` attribute, so that clicking a button does not initiate a window drag.
8. THE Custom_Chrome titlebar and buttons SHALL use styling consistent with the application's retro Mac OS theme (matching the `--color-gray`, `--color-black`, `--color-white` CSS variables and 3D embossed border pattern from App.css).

### Requirement 2: Effect Settings Panel in Floating Window

**User Story:** As a user, I want the floating Effect Settings window to show the real effect parameters UI, so that I can adjust dithering/curves/levels/glitch settings from the floating window.

#### Acceptance Criteria

1. WHEN the Panel_Window renders with `panelId = "effect"`, THE Panel_Window SHALL render the Effect_Settings_Panel component with full interactive functionality.
2. THE Effect_Settings_Panel in floating mode SHALL fetch the current document snapshot via `get_document_snapshot` IPC to determine the selected layer and filter data, without receiving props from App.tsx.
3. WHEN a filter parameter is changed in the floating Effect_Settings_Panel, THE Effect_Settings_Panel SHALL invoke the `update_filter` IPC command with the updated parameters, producing the same backend effect as when the panel is docked.
4. WHEN the document state changes (new filter added, filter removed, layer selection changed), THE floating Effect_Settings_Panel SHALL receive the updated state via Tauri event listeners and re-render with current data within 200 ms.
5. THE floating Effect_Settings_Panel SHALL support all effect types available in the docked version: Dithering, Curves, Levels, and Glitch.
6. THE floating Effect_Settings_Panel SHALL include palette selection functionality (PaletteSelector) with the same behavior as the docked version, fetching palettes via `list_palettes` IPC.

### Requirement 3: Layers Panel in Floating Window

**User Story:** As a user, I want the floating Layers window to show the real layer tree UI, so that I can select layers, toggle visibility, and manage effects from the floating window.

#### Acceptance Criteria

1. WHEN the Panel_Window renders with `panelId = "layers"`, THE Panel_Window SHALL render the Layers_Panel component with full interactive functionality.
2. THE Layers_Panel in floating mode SHALL fetch the layer tree via `get_layer_tree` IPC and document snapshot via `get_document_snapshot` IPC to populate the layer list and filter data.
3. WHEN a layer is selected in the floating Layers_Panel, THE Layers_Panel SHALL broadcast the selection via a Tauri event so that the Main_Window and other floating panels can synchronize selection state.
4. WHEN a filter visibility is toggled or reordered in the floating Layers_Panel, THE Layers_Panel SHALL invoke the corresponding IPC commands (`set_layer_props`, `reorder_filter`) producing the same backend effect as the docked version.
5. THE floating Layers_Panel SHALL display virtual effect layers (filters shown as layers) with the same visual representation and interaction behavior as the docked version.
6. WHEN the layer tree changes externally (filter added/removed from another window), THE floating Layers_Panel SHALL refresh its display to reflect the current document state within 200 ms.

### Requirement 4: Color Lab Panel in Floating Window

**User Story:** As a user, I want the floating Color Lab window to show the palette creation and editing UI, so that I can manage colors from the floating window.

#### Acceptance Criteria

1. WHEN the Panel_Window renders with `panelId = "colorlab"`, THE Panel_Window SHALL render the Color_Lab_Panel component with full palette creation, import, export, and generation functionality.
2. THE Color_Lab_Panel in floating mode SHALL invoke palette IPC commands (`add_palette`, `import_palette`, `export_palette`, `generate_palette`) directly via Tauri invoke, without requiring props from App.tsx.
3. WHEN a palette is created or imported in the floating Color_Lab_Panel, THE Color_Lab_Panel SHALL notify other windows via a Tauri event so that palette selectors in other panels refresh their palette list.
4. THE floating Color_Lab_Panel SHALL support the file dialog interactions (open/save) for palette import and export using the `@tauri-apps/plugin-dialog` API.
5. THE floating Color_Lab_Panel SHALL display the color picker, palette grid, generation controls, and import/export buttons with identical layout and behavior to the docked version.

### Requirement 5: Cross-Window Selection Synchronization

**User Story:** As a user, I want layer and filter selection to stay in sync across all windows, so that changing the selected effect in one window updates the others.

#### Acceptance Criteria

1. WHEN a layer or filter is selected in any window (Main_Window or floating panel), THE selecting window SHALL emit a `selection-changed` Tauri event containing the selected layer ID and selected filter ID.
2. WHEN a window receives a `selection-changed` event, THE window SHALL update its local selection state to match the event payload within 100 ms.
3. THE `selection-changed` event SHALL carry a payload containing: the selected layer ID (number or null) and the selected filter ID (string or null).
4. IF the `selection-changed` event contains a filter ID that does not exist in the current document snapshot, THEN THE receiving window SHALL ignore the invalid filter ID and set its filter selection to null.
5. THE Main_Window and all floating panel windows SHALL subscribe to the `selection-changed` event on mount and unsubscribe on unmount.

### Requirement 6: IPC-Based Data Hooks for Standalone Panels

**User Story:** As a developer, I want each panel to have self-contained IPC hooks, so that panels can function independently in floating windows without relying on the App.tsx prop tree.

#### Acceptance Criteria

1. THE frontend SHALL provide a `useDocumentState` hook that fetches document metadata (docId, width, height, hasDocument) via IPC and subscribes to document change events, usable by any panel in any window.
2. THE frontend SHALL provide a `useLayerState` hook that fetches the layer tree and filters via IPC and subscribes to layer change events, usable by Layers_Panel and Effect_Settings_Panel in floating windows.
3. THE frontend SHALL provide a `useSelectionState` hook that manages the current layer and filter selection, emitting and listening to `selection-changed` events for cross-window synchronization.
4. WHEN the backend document state changes (image loaded, filter modified, layer added), THE backend SHALL emit a `document-changed` Tauri event to all windows so that IPC hooks can refetch current state.
5. THE IPC hooks SHALL debounce refetch operations by 50 ms to avoid redundant IPC calls when multiple rapid events arrive.
6. THE IPC hooks SHALL handle error states (IPC failure, missing document) by returning null/empty data and setting an error field, without crashing the panel component.

### Requirement 7: Panel Window Resize Constraints

**User Story:** As a user, I want floating panel windows to have minimum sizes, so that the UI remains usable when I resize them.

#### Acceptance Criteria

1. WHEN the PanelManager creates a floating panel window, THE PanelManager SHALL set a minimum window size of 280 pixels wide and 200 pixels tall.
2. THE Panel_Window content area SHALL adapt its layout to the available window size using CSS flex layout, without horizontal overflow.
3. IF the window width is below 320 pixels, THEN THE Panel_Window content SHALL collapse secondary UI elements (descriptions, labels) while keeping controls interactive.

