# Requirements Document

## Introduction

This feature adds drag-based interactions to the docked sidebar panels in the dither-yuki-2 Tauri desktop application. Users will be able to drag a panel by its titlebar (`.docked-panel-header`) to either undock it into a floating window (by dragging far enough away from the sidebar) or reorder it within the sidebar (by dragging within the sidebar area). The floating window created by drag-to-undock will match the panel's rendered size in the sidebar. Panel order changes will persist across sessions.

## Glossary

- **Sidebar**: The right-side panel container in the main application window that holds docked panels in a vertical stack
- **Docked_Panel**: A panel (Effect Settings, Layers, or Color Lab) rendered inside the Sidebar in a flex layout
- **Panel_Header**: The `.docked-panel-header` element at the top of each Docked_Panel, used as the drag handle
- **Floating_Window**: A separate Tauri WebView window created when a panel is undocked from the Sidebar
- **Undock_Threshold**: The minimum horizontal distance (in pixels) a panel must be dragged away from the Sidebar edge to trigger undocking
- **Drop_Indicator**: A visual element displayed in the Sidebar during drag-to-reorder to show where the dragged panel will be placed
- **Panel_Order**: The ordered list of panel identifiers determining the vertical arrangement of Docked_Panels in the Sidebar
- **PanelManager**: The Rust-side state manager that tracks panel docked/floating/visible state and panel order
- **Drag_Session**: The interaction period from mousedown on a Panel_Header through mousemove to mouseup

## Requirements

### Requirement 1: Drag Initiation from Panel Header

**User Story:** As a user, I want to initiate a drag by pressing and moving on a panel's titlebar, so that I can reorder or undock panels using a natural drag gesture.

#### Acceptance Criteria

1. WHEN the user presses the mouse button on a Panel_Header and moves the pointer at least 5 pixels, THE Sidebar SHALL initiate a Drag_Session for that Docked_Panel
2. WHILE a Drag_Session is active, THE Sidebar SHALL display a visual indicator that the panel is being dragged (such as reduced opacity on the source panel)
3. WHILE a Drag_Session is active, THE Sidebar SHALL prevent text selection and other default browser drag behaviors on the Panel_Header
4. WHEN the user releases the mouse button without exceeding the 5-pixel movement threshold, THE Sidebar SHALL treat the interaction as a click (no drag occurs)

### Requirement 2: Drag-to-Undock Activation

**User Story:** As a user, I want to drag a docked panel far enough away from the sidebar to undock it into a floating window, so that I can arrange panels freely on my screen.

#### Acceptance Criteria

1. WHEN the user drags a Docked_Panel horizontally beyond 50 pixels to the left of the Sidebar's left edge during a Drag_Session, THE Sidebar SHALL trigger an undock operation for that panel
2. WHILE the user is dragging and the pointer is beyond the Undock_Threshold, THE Sidebar SHALL display a visual cue indicating that releasing will undock the panel (such as a ghost preview or cursor change)
3. WHEN the undock operation is triggered, THE PanelManager SHALL transition the panel from docked to floating state
4. IF the undock operation fails (window creation error), THEN THE Sidebar SHALL cancel the Drag_Session, keep the panel docked, and display an error notification

### Requirement 3: Floating Window Size Matches Panel Size

**User Story:** As a user, I want the floating window to open at the same size my panel occupied in the sidebar, so that the content layout remains consistent after undocking.

#### Acceptance Criteria

1. WHEN a drag-to-undock operation is triggered, THE Sidebar SHALL measure the Docked_Panel's rendered width and height before undocking
2. WHEN creating the Floating_Window, THE PanelManager SHALL set the window's inner size to match the measured width and height of the Docked_Panel in the Sidebar
3. THE Floating_Window SHALL have a width equal to the Sidebar's current width at the time of undocking
4. THE Floating_Window SHALL have a height equal to the Docked_Panel's actual rendered height (computed from the flex layout) at the time of undocking

### Requirement 4: Floating Window Position

**User Story:** As a user, I want the floating window to appear near where I released the drag, so that the undocked panel feels spatially connected to my gesture.

#### Acceptance Criteria

1. WHEN a drag-to-undock operation completes, THE Floating_Window SHALL be positioned with its top-left corner at the screen coordinates where the user released the mouse
2. IF the computed window position would place the Floating_Window partially off-screen, THEN THE PanelManager SHALL adjust the position to keep the window fully visible on the nearest monitor

### Requirement 5: Drag-to-Reorder Within Sidebar

**User Story:** As a user, I want to drag a panel within the sidebar to change its position relative to other panels, so that I can customize my workspace layout.

#### Acceptance Criteria

1. WHILE the user drags a Docked_Panel within the Sidebar area (pointer remains within Sidebar bounds and does not exceed the Undock_Threshold), THE Sidebar SHALL treat the interaction as a reorder operation
2. WHILE dragging within the Sidebar, THE Sidebar SHALL display a Drop_Indicator showing the insertion position based on the pointer's vertical position relative to other panels
3. WHEN the user releases the mouse button while the pointer is within the Sidebar area, THE Sidebar SHALL reorder the panels by moving the dragged panel to the indicated position
4. WHEN the panel order changes, THE PanelManager SHALL update the Panel_Order state to reflect the new arrangement
5. THE Sidebar SHALL render panels in the order specified by Panel_Order after a reorder operation completes

### Requirement 6: Drop Indicator Visual Feedback

**User Story:** As a user, I want to see a clear visual indicator of where my panel will land during reorder, so that I can precisely control panel arrangement.

#### Acceptance Criteria

1. WHILE a reorder Drag_Session is active, THE Sidebar SHALL display a horizontal Drop_Indicator line between panels at the calculated insertion point
2. WHEN the pointer moves vertically during a reorder drag, THE Sidebar SHALL update the Drop_Indicator position in real time based on which panel boundary the pointer is closest to
3. WHEN the Drag_Session ends (mouse release or cancellation), THE Sidebar SHALL remove the Drop_Indicator from the display

### Requirement 7: Panel Order Persistence

**User Story:** As a user, I want my custom panel order to persist across application restarts, so that I don't have to rearrange panels every session.

#### Acceptance Criteria

1. WHEN the Panel_Order changes due to a reorder operation, THE PanelManager SHALL store the updated order in its persisted state
2. WHEN the application starts, THE PanelManager SHALL restore the Panel_Order from persisted state
3. THE Sidebar SHALL render docked panels in the persisted Panel_Order on application startup
4. IF no persisted Panel_Order exists (first launch or reset), THEN THE PanelManager SHALL use the default order: effect, layers, colorlab

### Requirement 8: IPC Command for Undock with Size

**User Story:** As a developer, I want to pass the panel's measured size to the backend when undocking via drag, so that the floating window is created at the correct dimensions.

#### Acceptance Criteria

1. WHEN a drag-to-undock is triggered, THE Sidebar SHALL invoke an IPC command that includes the panel identifier, the measured width, and the measured height
2. WHEN the backend receives an undock-with-size command, THE PanelManager SHALL use the provided width and height as the Floating_Window's inner dimensions instead of default values
3. WHEN the backend receives an undock-with-size command with a screen position, THE PanelManager SHALL use the provided position as the Floating_Window's initial screen coordinates

### Requirement 9: IPC Command for Panel Reorder

**User Story:** As a developer, I want an IPC command to persist panel order changes to the backend, so that the reorder state survives application restarts.

#### Acceptance Criteria

1. WHEN a reorder operation completes in the frontend, THE Sidebar SHALL invoke an IPC command with the new ordered list of panel identifiers
2. WHEN the backend receives a reorder command, THE PanelManager SHALL validate that all provided panel identifiers are known panels
3. WHEN the backend receives a valid reorder command, THE PanelManager SHALL update the Panel_Order and emit a panel-state-changed event
4. IF the reorder command contains unknown panel identifiers, THEN THE PanelManager SHALL reject the command and return an error

### Requirement 10: Drag Cancellation

**User Story:** As a user, I want to cancel a drag operation by pressing Escape, so that I can abort an unintended drag without making changes.

#### Acceptance Criteria

1. WHEN the user presses the Escape key during a Drag_Session, THE Sidebar SHALL cancel the drag operation and restore the panel to its original position and state
2. WHEN a Drag_Session is cancelled, THE Sidebar SHALL remove all visual drag feedback (Drop_Indicator, ghost preview, opacity changes)
3. WHEN a Drag_Session is cancelled, THE Sidebar SHALL not invoke any IPC commands (no undock, no reorder)
