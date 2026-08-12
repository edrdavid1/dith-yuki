# Requirements Document

## Introduction

This feature implements a cross-platform custom titlebar architecture for the Dither Tauri v2 application (macOS, Windows, and Linux). The core problem is that on macOS, `data-tauri-drag-region` fails because the canvas element with `willChange: 'transform'` creates a hardware-accelerated CALayer that blocks hit-testing in WKWebView. The fix principle is "geometric separation" — the titlebar zone must never contain canvas or GPU-accelerated layers beneath it on any platform. A unified component architecture (WindowShell/AppTitlebar) enforces this constraint for all application windows.

**Platform scope:** macOS and Windows are the primary targets. Linux is supported via the Windows drag mechanism as fallback — WebKitGTK is Chromium-based and closer to WebView2 behavior, so `data-tauri-drag-region` with custom WindowControls is the appropriate mechanism on Linux.

## Glossary

- **WindowShell**: A reusable React layout component that wraps all window content, providing a titlebar region and a content region with geometric separation enforced via CSS custom properties.
- **AppTitlebar**: A reusable React component rendered inside WindowShell that provides the draggable titlebar area, platform-specific drag mechanism, and optional window control buttons.
- **WindowControls**: A React component that renders custom minimize/maximize/close buttons on Windows and Linux, and returns null on macOS where native traffic lights are used.
- **Geometric_Separation**: A layout constraint ensuring the titlebar DOM region and the canvas/GPU-layer region occupy strictly non-overlapping vertical zones, preventing hit-testing conflicts.
- **Titlebar_Height**: A CSS custom property (`--titlebar-height`) defining the logical pixel height of the titlebar zone, used by both the titlebar and the content area offset.
- **Platform_Detector**: A module using `@tauri-apps/plugin-os` to determine the current operating system at runtime and expose the result for conditional platform behavior.
- **Traffic_Lights**: The native macOS window control buttons (close, minimize, maximize) rendered by the system when `titleBarStyle: "Overlay"` is active.
- **Main_Window**: The primary application window containing the canvas-based image preview, sidebar panels, and menu bar.
- **Floating_Panel_Window**: A secondary window for undocked panels (effect settings, layers, color lab, preview) that also requires titlebar functionality.
- **Overlay_Portal**: A DOM node rendered as a sibling to WindowShell's content area (or at document.body level) used for popovers, tooltips, and dropdown menus that must visually escape the content area's overflow constraints.
- **onCloseRequested**: A Tauri window event listener that intercepts all window close attempts (native close button, Alt+F4, system menu close, macOS Traffic Lights red button) before the window is actually destroyed, allowing the application to execute custom logic such as docking a panel instead of closing.

## Requirements

### Requirement 1: Geometric Separation Layout

**User Story:** As a user, I want the titlebar to always be draggable regardless of canvas rendering state, so that I can reposition windows without interaction conflicts.

#### Acceptance Criteria

1. THE WindowShell SHALL render the AppTitlebar at vertical position `top: 0` with a height of `var(--titlebar-height)`.
2. THE WindowShell SHALL render the content area at vertical position `top: var(--titlebar-height)`, ensuring no content occupies the titlebar zone.
3. THE WindowShell SHALL define `--titlebar-height` as a CSS custom property with a default value of 32 logical CSS pixels.
4. WHEN a canvas element is rendered inside the content area, THE WindowShell SHALL ensure the canvas top edge starts at or below `var(--titlebar-height)` from the window top.
5. THE WindowShell content area SHALL establish a positioning context (via `position: relative`) and apply `overflow-y: hidden` so that absolutely-positioned child elements (including canvas) cannot structurally overlap the titlebar zone in the vertical axis. Popover, dropdown, and tooltip elements that need to escape content bounds SHALL be rendered via React Portal into an Overlay_Portal node outside the WindowShell content area.

### Requirement 2: Unified Window Architecture

**User Story:** As a developer, I want all application windows to use the same titlebar architecture, so that titlebar behavior is consistent and new windows automatically inherit correct drag functionality.

#### Acceptance Criteria

1. THE Main_Window SHALL render its root layout through the WindowShell component.
2. THE Floating_Panel_Window SHALL render its root layout through the WindowShell component.
3. THE WindowShell SHALL accept children as content rendered below the titlebar.
4. THE WindowShell SHALL render the AppTitlebar as the first child element in the DOM output, and the AppTitlebar element SHALL include the `data-tauri-drag-region` attribute to enable native window dragging.
5. WHEN a new window type is added to the application, THE new window SHALL use WindowShell as its root layout component so that all entry points enumerated in main.tsx route through WindowShell.
6. THE WindowShell SHALL render exactly one AppTitlebar instance per window, separate from any panel-level titlebars used for in-sidebar drag-to-reorder or undock interactions.

### Requirement 3: macOS Drag Mechanism

**User Story:** As a macOS user, I want to drag the window by the titlebar area even when the canvas is actively rendering with GPU acceleration, so that window management works reliably.

#### Acceptance Criteria

1. WHILE the application is running on macOS, THE AppTitlebar SHALL apply the CSS property `-webkit-app-region: drag` to its root element.
2. WHILE the application is running on macOS, THE AppTitlebar SHALL apply `-webkit-app-region: no-drag` to all interactive child elements (buttons, inputs, and elements with the `data-tauri-drag-region="false"` attribute).
3. WHILE the application is running on macOS, THE AppTitlebar SHALL retain the `data-tauri-drag-region` attribute on its root element as a fallback, without removing the `-webkit-app-region` CSS property.
4. WHILE the application is running on macOS, WHEN the user clicks and drags on the AppTitlebar area while canvas GPU-compositing is active (`willChange: transform` applied to canvas), THE window position SHALL update continuously following the mouse movement without requiring a minimum drag distance threshold or dropped input events.
5. WHILE the application is running on macOS, THE AppTitlebar's parent layout SHALL ensure that no canvas element or GPU-composited layer (elements with `will-change: transform` or hardware-accelerated content) geometrically overlaps the titlebar region, so that WebKit hit-testing is not intercepted by a compositing layer.
6. WHILE the canvas is actively rendering with GPU acceleration on macOS, WHEN the user clicks and drags on the AppTitlebar area, THE window SHALL move with the cursor continuously without the drag being blocked, delayed, or interrupted by the underlying canvas compositing layer.

### Requirement 4: Windows Drag Mechanism

**User Story:** As a Windows user, I want to drag the window by the titlebar area, so that window management works as expected on my platform.

#### Acceptance Criteria

1. WHILE the application is running on Windows, THE AppTitlebar SHALL use `data-tauri-drag-region` attribute on the titlebar DOM element as the drag mechanism, enabling window movement via Tauri's `WM_NCLBUTTONDOWN` hit-test integration.
2. WHILE the application is running on Windows, THE AppTitlebar SHALL ensure the titlebar DOM element is above the canvas in z-index stacking order, and the canvas element SHALL NOT geometrically overlap the titlebar region (canvas top offset must equal the titlebar height).
3. WHILE the application is running on Windows, THE AppTitlebar SHALL have `pointer-events` set to `auto` (not `none`) to ensure hit-testing succeeds.
4. WHILE the application is running on Windows, IF a user clicks an interactive element (button, input) within the titlebar, THEN THE AppTitlebar SHALL NOT initiate a window drag for that interaction, by marking interactive child elements with `data-tauri-drag-region="false"`.
5. WHEN a user double-clicks the titlebar drag region on Windows, THE AppTitlebar SHALL toggle the window between maximized and restored states.

### Requirement 5: Platform Detection

**User Story:** As a developer, I want a single platform detection module, so that platform-specific behavior is applied consistently across all components.

#### Acceptance Criteria

1. THE Platform_Detector SHALL use the `platform()` function from `@tauri-apps/plugin-os` to determine the current operating system.
2. THE Platform_Detector SHALL expose a synchronous function that returns the platform value as one of the string literals `'macos'`, `'windows'`, or `'linux'`.
3. THE Platform_Detector SHALL resolve the platform value once during application initialization, before the first React component tree render, and cache the result in module-level state so that all subsequent calls return the cached value without awaiting a Promise.
4. IF the `platform()` call from `@tauri-apps/plugin-os` throws an error or returns a value not matching `'macos'`, `'windows'`, or `'linux'`, THEN THE Platform_Detector SHALL return `'unknown'` as the platform value, and THE WindowControls SHALL NOT render custom window control buttons when the platform value is `'unknown'`.
5. THE Platform_Detector SHALL complete platform resolution within 500 ms of being invoked; IF resolution exceeds 500 ms, THEN THE Platform_Detector SHALL fall back to returning `'unknown'`.
6. WHILE the application is running on Linux, THE Platform_Detector SHALL cause the application to use the same drag mechanism and window controls as Windows (`data-tauri-drag-region` as primary drag mechanism, custom WindowControls rendered with minimize/maximize/close buttons).

### Requirement 6: Windows Window Controls

**User Story:** As a Windows user, I want minimize, maximize, and close buttons in the titlebar, so that I can control the window since native decorations are disabled.

#### Acceptance Criteria

1. WHILE the application is running on Windows, THE WindowControls SHALL render minimize, maximize, and close buttons inside the AppTitlebar, positioned at the right edge of the titlebar.
2. WHEN the minimize button is clicked, THE WindowControls SHALL minimize the current window via the Tauri window API.
3. WHEN the maximize button is clicked and the window is not maximized, THE WindowControls SHALL maximize the current window via the Tauri window API.
4. WHEN the maximize button is clicked and the window is already maximized, THE WindowControls SHALL restore the current window to its previous size via the Tauri window API.
5. WHEN the close button is clicked, THE WindowControls SHALL close the current window via the Tauri window API (triggering the window close flow, which is intercepted by onCloseRequested on Floating_Panel_Windows).
6. THE WindowControls buttons SHALL have `-webkit-app-region: no-drag` and `data-tauri-drag-region="false"` to prevent drag activation on button clicks.
7. WHILE the application is running on macOS, THE WindowControls SHALL not render any custom buttons (native Traffic_Lights are provided by the system).
8. IF a Tauri window API call (minimize, maximize, unmaximize, close) fails, THEN THE WindowControls SHALL log the error to the console and not crash the application.
9. THE WindowControls maximize button SHALL NOT integrate with Windows 11 Snap Layout in this iteration; this limitation is an accepted trade-off for fully custom window controls. Future iterations may integrate `tauri-plugin-decorum` or native window controls for Snap Layout support.

### Requirement 7: Double-Click to Maximize/Restore

**User Story:** As a user, I want to double-click the titlebar to toggle between maximized and restored window states, so that the standard window management gesture works.

#### Acceptance Criteria

1. WHEN the user double-clicks (two clicks within 500 ms) on the AppTitlebar drag region area, THE AppTitlebar SHALL toggle the window between maximized and restored states by calling the Tauri window `toggleMaximize` API.
2. IF the user double-clicks on a WindowControls button (minimize, maximize, close) or other interactive element (buttons, inputs, dropdowns) within the AppTitlebar, THEN THE AppTitlebar SHALL NOT trigger the maximize/restore toggle action and SHALL allow the interactive element to handle the event normally.
3. WHILE the application is running on Windows, THE AppTitlebar SHALL handle the double-click maximize/restore via its own event handler, since Windows frameless windows do not natively support this gesture.
4. WHILE the application is running on macOS, THE AppTitlebar SHALL rely on the native double-click behavior provided by the `-webkit-app-region: drag` CSS property, and SHALL NOT attach a redundant JavaScript double-click handler that could conflict with the native mechanism.
5. WHEN the window transitions from restored to maximized state (or vice versa) via double-click, THE AppTitlebar SHALL complete the state transition within 200 ms of the second click.

### Requirement 8: Tauri Configuration

**User Story:** As a developer, I want the Tauri configuration to support the custom titlebar on both platforms, so that native chrome does not conflict with the custom implementation.

#### Acceptance Criteria

1. THE tauri.conf.json SHALL set `decorations` to `false` for every window defined in the `app.windows` array.
2. THE tauri.conf.json SHALL set `titleBarStyle` to `"Overlay"` for every window defined in the `app.windows` array. On macOS, this enables native Traffic_Lights to appear over the custom titlebar. On Windows, this property is ignored by the runtime; the frameless behavior is achieved solely through `decorations: false`.
3. WHEN a new window is created at runtime by the application, THE window creation parameters SHALL include `decorations: false` and `titleBarStyle: "Overlay"`, matching the static configuration in tauri.conf.json.
4. IF the application is running on Windows, THEN THE window SHALL render with no native titlebar or borders (frameless) as a result of `decorations: false`, requiring no additional platform-specific configuration beyond that property.

### Requirement 9: DPI and HiDPI Scaling

**User Story:** As a user on a high-DPI display, I want the titlebar to render at the correct size without visual gaps between the titlebar and content area, so that the interface looks crisp and aligned.

#### Acceptance Criteria

1. THE WindowShell SHALL define Titlebar_Height as a CSS custom property (`--titlebar-height`) specified in logical CSS pixels with a value of 32px, not in device pixels or canvas coordinate units.
2. WHILE the display scale factor is not 100% (e.g., 125%, 150%, 200%), THE WindowShell SHALL maintain zero visible gap (0 device pixels) and zero overlap between the titlebar bottom edge and the content area top edge.
3. THE WindowShell SHALL position the content area top offset using the same `--titlebar-height` CSS variable that defines the titlebar height, so that titlebar and content boundary remain aligned by construction.
4. WHEN the window is resized on a display with a non-integer scale factor (e.g., 125%, 150%), THE WindowShell SHALL preserve zero-gap alignment between the titlebar bottom edge and the content area top edge without introducing sub-pixel rounding artifacts.
5. THE WindowShell SHALL not render any canvas element or GPU-composited layer (e.g., elements with `will-change: transform`) within the vertical bounds of the titlebar region (from the window top edge to `--titlebar-height`).

### Requirement 10: Floating Panel Window Titlebar

**User Story:** As a user, I want floating panel windows to have the same drag and window control behavior as the main window, so that all windows feel consistent.

#### Acceptance Criteria

1. WHEN a panel is undocked into a Floating_Panel_Window, THE Floating_Panel_Window SHALL render its content through WindowShell with an AppTitlebar, applying the same `--titlebar-height` (32 logical CSS pixels) and geometric separation as the Main_Window.
2. WHILE the application is running on macOS, THE Floating_Panel_Window AppTitlebar SHALL apply `-webkit-app-region: drag` to the titlebar element and `-webkit-app-region: no-drag` to all interactive child elements, and SHALL retain `data-tauri-drag-region` as a secondary mechanism.
3. WHILE the application is running on Windows, THE Floating_Panel_Window AppTitlebar SHALL use `data-tauri-drag-region` as the primary drag mechanism with the titlebar DOM element above content in z-index stacking order and `pointer-events: auto`.
4. WHILE the application is running on Windows, THE Floating_Panel_Window SHALL display WindowControls (minimize, maximize, close) in its AppTitlebar, where the close button triggers a window close (which is intercepted by the onCloseRequested listener per Requirement 11).
5. WHILE the application is running on macOS, THE Floating_Panel_Window SHALL use native Traffic_Lights provided by the system via the `titleBarStyle: "Overlay"` Tauri window configuration, where clicking the red close Traffic_Light triggers a window close event (which is intercepted by the onCloseRequested listener per Requirement 11).
6. THE Floating_Panel_Window WindowControls buttons and any interactive titlebar elements SHALL have `-webkit-app-region: no-drag` and `data-tauri-drag-region="false"` attributes to prevent drag activation on button clicks.

### Requirement 11: Floating Panel Close via onCloseRequested

**User Story:** As a user, I want closing a floating panel window (via any method — close button, keyboard shortcut, or native traffic light) to return the panel to its docked state rather than destroying the window, so that I do not accidentally lose panel state.

#### Acceptance Criteria

1. WHEN a Floating_Panel_Window is created, THE Floating_Panel_Window SHALL register a Tauri `onCloseRequested` event listener on its window instance before any user interaction is possible.
2. WHEN any close action is initiated on a Floating_Panel_Window (including: custom close button click on Windows, native Traffic_Light red button on macOS, Alt+F4 on Windows, Cmd+W on macOS, or system menu close), THE onCloseRequested listener SHALL call `event.preventDefault()` to prevent the window from being destroyed.
3. WHEN the onCloseRequested listener intercepts a close event, THE Floating_Panel_Window SHALL invoke the `dock_panel` operation for the corresponding panel, returning the panel content to its docked position in the main window.
4. AFTER the `dock_panel` operation completes successfully, THE Floating_Panel_Window SHALL close the window via the Tauri window API (the close will succeed because the panel has been returned to docked state and the listener can allow it or the window can be explicitly destroyed).
5. IF the `dock_panel` operation fails, THEN THE Floating_Panel_Window SHALL log the error to the console and close the window regardless, so that the user is not left with an unresponsive floating window.
6. THE onCloseRequested mechanism SHALL be the single unified close path for Floating_Panel_Windows on all platforms, ensuring consistent behavior regardless of how the close was initiated.
