# Implementation Plan: Cross-Platform Titlebar

## Overview

Implement a cross-platform custom titlebar architecture for the Dither Tauri v2 app. The approach uses geometric separation to prevent GPU-composited canvas layers from blocking titlebar hit-testing. A unified component architecture (WindowShell → AppTitlebar → WindowControls) wraps all windows. Platform detection occurs before React renders, enabling synchronous platform-conditional behavior throughout.

## Tasks

- [x] 1. Platform detection module and Tauri configuration
  - [x] 1.1 Create the platform detection module at `frontend/src/lib/platform.ts`
    - Export `PlatformValue` type (`'macos' | 'windows' | 'linux' | 'unknown'`)
    - Implement `initPlatform()` async function that calls `platform()` from `@tauri-apps/plugin-os` with a 500ms timeout via `Promise.race`
    - Cache result in module-level `resolvedPlatform` variable
    - Implement synchronous `getPlatform()`, `isMacOS()`, `isWindows()`, `isLinux()` functions
    - On error or unrecognized value, fall back to `'unknown'`
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6_

  - [x] 1.2 Update Tauri configuration for custom titlebar
    - Set `decorations: false` and `titleBarStyle: "Overlay"` for all windows in `tauri.conf.json`
    - Verify that runtime window creation in `panelCommands.ts` (or equivalent) also passes `decorations: false` and `titleBarStyle: "Overlay"`
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

  - [ ]* 1.3 Write unit tests for the platform detection module
    - Test that `initPlatform()` resolves to `'macos'` when `platform()` returns `'macos'`
    - Test that `initPlatform()` resolves to `'windows'` when `platform()` returns `'windows'`
    - Test that `initPlatform()` resolves to `'linux'` when `platform()` returns `'linux'`
    - Test that `initPlatform()` resolves to `'unknown'` for unrecognized values
    - Test that `initPlatform()` resolves to `'unknown'` when `platform()` throws
    - Test that `initPlatform()` resolves to `'unknown'` on timeout (>500ms)
    - Mock `@tauri-apps/plugin-os` using `vi.mock()`
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

- [x] 2. CSS architecture and titlebar styles
  - [x] 2.1 Create the titlebar CSS file at `frontend/src/styles/titlebar.css`
    - Define `:root { --titlebar-height: 32px }` CSS custom property
    - Style `.window-shell` as flex column, full width/height, `overflow: hidden`
    - Style `.content-area` with `flex: 1`, `position: relative`, `overflow-y: hidden`, `overflow-x: hidden`, `min-height: 0`
    - Style `.app-titlebar` with `height: var(--titlebar-height)`, `min-height`/`max-height` clamped, flex row, `align-items: center`, `user-select: none`, `z-index: 9999`, `pointer-events: auto`
    - Add no-drag rules for interactive elements inside `.app-titlebar` (`-webkit-app-region: no-drag`)
    - Style `.window-controls` and `.window-control-btn` for Windows/Linux buttons (46px wide, hover states, close button red hover)
    - Style `#overlay-portal` as fixed positioned, zero-size, high z-index with `pointer-events: none` on container and `pointer-events: auto` on children
    - _Requirements: 1.1, 1.2, 1.3, 1.5, 3.2, 4.3, 6.6, 9.1, 9.2, 9.3, 9.4, 9.5_

  - [x] 2.2 Import `titlebar.css` in the application entry point
    - Add `import './styles/titlebar.css'` to `frontend/src/main.tsx` (before app renders)
    - _Requirements: 9.1_

- [x] 3. Checkpoint - Ensure platform module and CSS compile
  - Ensure all tests pass, ask the user if questions arise.

- [x] 4. Core components: WindowShell and AppTitlebar
  - [x] 4.1 Create the `WindowShell` component at `frontend/src/components/WindowShell.tsx`
    - Accept `children`, optional `title`, and optional `titlebarContent` props
    - Render a `.window-shell` div containing: `<AppTitlebar>` as first child, `.content-area` div with children, and `#overlay-portal` div
    - Pass `title` and `titlebarContent` to AppTitlebar
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 2.3, 2.4, 2.6_

  - [x] 4.2 Create the `AppTitlebar` component at `frontend/src/components/AppTitlebar.tsx`
    - Accept optional `children` and `title` props
    - Apply `data-tauri-drag-region` attribute on root element
    - On macOS: apply inline `WebkitAppRegion: 'drag'` style
    - Render title span (if provided) and children, then `<WindowControls />`
    - Implement double-click handler: on Windows/Linux only, check target is not a button/input/no-drag element, then call `getCurrentWindow().toggleMaximize()` (or `isMaximized` → `unmaximize`/`maximize`)
    - On macOS: do NOT attach the double-click handler (native behavior via `-webkit-app-region`)
    - Wrap all Tauri API calls in try/catch with console.error
    - _Requirements: 2.4, 3.1, 3.2, 3.3, 4.1, 4.2, 4.3, 4.4, 4.5, 7.1, 7.2, 7.3, 7.4, 7.5_

  - [x] 4.3 Create the `WindowControls` component at `frontend/src/components/WindowControls.tsx`
    - On macOS: return `null` (native traffic lights handle controls)
    - On Windows/Linux: render minimize, maximize, and close buttons
    - Each button calls the corresponding Tauri window API (`minimize()`, `maximize()`/`unmaximize()`, `close()`)
    - Apply `data-tauri-drag-region="false"` and `WebkitAppRegion: 'no-drag'` on the controls container and each button
    - Wrap all Tauri API calls in try/catch with console.error
    - On `'unknown'` platform: render nothing (same as macOS path)
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8, 6.9, 5.4_

  - [ ]* 4.4 Write unit tests for WindowShell, AppTitlebar, and WindowControls
    - Test WindowShell renders titlebar + content area DOM structure
    - Test WindowShell content area has `position: relative` and `overflow-y: hidden`
    - Test AppTitlebar applies `data-tauri-drag-region` attribute
    - Test AppTitlebar applies `-webkit-app-region: drag` when platform is macOS (mock platform module)
    - Test AppTitlebar does NOT apply `-webkit-app-region` when platform is Windows
    - Test WindowControls returns null on macOS
    - Test WindowControls renders 3 buttons on Windows
    - Test WindowControls renders 3 buttons on Linux
    - Test WindowControls buttons have `data-tauri-drag-region="false"`
    - Test double-click handler fires maximize toggle on Windows but not on macOS
    - Test double-click handler does NOT fire when target is a button
    - Mock `@tauri-apps/api/window` and `../lib/platform` using `vi.mock()`
    - _Requirements: 1.1, 1.2, 2.4, 3.1, 4.1, 6.1, 6.7, 7.1, 7.2, 7.4_

- [x] 5. useCloseRequested hook for floating panels
  - [x] 5.1 Create the `useCloseRequested` hook at `frontend/src/hooks/useCloseRequested.ts`
    - Accept `panelId: string` parameter
    - Register `onCloseRequested` listener on `getCurrentWindow()`
    - In the listener: call `event.preventDefault()`, then `dockPanel(panelId)`, then `win.destroy()`
    - If `dockPanel` fails: log error, destroy window regardless
    - Clean up listener on unmount
    - _Requirements: 11.1, 11.2, 11.3, 11.4, 11.5, 11.6_

  - [ ]* 5.2 Write unit tests for useCloseRequested
    - Test that `event.preventDefault()` is called on close event
    - Test that `dockPanel` is invoked with the correct panelId
    - Test that `window.destroy()` is called after successful dock
    - Test that `window.destroy()` is called even when dock fails
    - Mock `@tauri-apps/api/window` and `../ipc/panelCommands`
    - _Requirements: 11.2, 11.3, 11.4, 11.5_

- [x] 6. Checkpoint - Ensure all component tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 7. Integration: wire WindowShell into main.tsx
  - [x] 7.1 Update `frontend/src/main.tsx` to call `initPlatform()` before React renders
    - Import `initPlatform` from `./lib/platform`
    - Await `initPlatform()` before `ReactDOM.createRoot(...).render(...)`
    - Wrap in an async IIFE or top-level await
    - _Requirements: 5.3_

  - [x] 7.2 Update `frontend/src/App.tsx` to wrap content in WindowShell
    - Import `WindowShell` from `./components/WindowShell`
    - Wrap the existing `app-layout` div inside `<WindowShell>`
    - Pass the `MenuBar` component as `titlebarContent` prop (or render it within the titlebar area)
    - Remove any existing manual titlebar handling that conflicts with WindowShell
    - Ensure the canvas container (`app-canvas`) starts below `--titlebar-height` (content area handles this automatically via WindowShell)
    - _Requirements: 1.1, 1.2, 1.4, 2.1, 2.3, 9.2, 9.3, 9.5_

  - [x] 7.3 Update `frontend/src/components/PanelWindow.tsx` to use WindowShell and useCloseRequested
    - Import `WindowShell` from `./WindowShell` and `useCloseRequested` from `../hooks/useCloseRequested`
    - Replace the existing `panel-window-titlebar` div with `<WindowShell title={displayName}>`
    - Call `useCloseRequested(panelId)` inside PanelWindow to handle close-as-dock
    - Remove the existing manual close/dock logic that duplicates useCloseRequested behavior
    - Ensure floating panels pass `decorations: false` and `titleBarStyle: "Overlay"` in window creation (verify in `panelCommands.ts`)
    - _Requirements: 2.2, 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 11.1, 11.6_

- [x] 8. Overlay portal integration
  - [x] 8.1 Update existing popover/dropdown/tooltip components to use `#overlay-portal`
    - Identify components that render popovers, dropdowns, or tooltips (e.g., DropdownMenu)
    - Wrap their portal rendering to target `document.getElementById('overlay-portal')` instead of `document.body`
    - Ensure graceful fallback (return null if portal target not found)
    - _Requirements: 1.5_

- [x] 9. Final checkpoint - Ensure all tests pass and components compile
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Unit tests use Vitest + React Testing Library with vi.mock() for Tauri API mocking
- No property-based tests — this feature is UI layout + platform conditionals + side-effect API calls
- The design explicitly uses TypeScript; all code should be `.ts`/`.tsx` files in `frontend/src/`
- Linux uses the same mechanism as Windows (data-tauri-drag-region + custom WindowControls)

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "2.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "2.2"] },
    { "id": 2, "tasks": ["4.1", "4.2", "4.3", "5.1"] },
    { "id": 3, "tasks": ["4.4", "5.2"] },
    { "id": 4, "tasks": ["7.1", "7.2", "7.3"] },
    { "id": 5, "tasks": ["8.1"] }
  ]
}
```
