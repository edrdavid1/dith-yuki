# Implementation Plan: Figma UI Redesign

## Overview

Migrate the Dither Yuki 2 frontend from the current "filter list per layer" UI to the new Figma-based "one layer = one effect" model. This involves restructuring the layout (MenuBar + PreviewWindow + Sidebar), creating new components (EffectSettingsPanel, EffectChooserDialog, ColorLabWindow, LayersPanel), replacing the `useFilters` hook with `useEffectLayer`, updating zoom logic with preset navigation, and removing deprecated components (FilterList, FilterPanel, Toolbar, inline PalettePanel).

## Tasks

- [x] 1. Core types, utilities, and hook infrastructure
  - [x] 1.1 Create shared types and constants for the new effect model
    - Create `frontend/src/types/effects.ts` with `EffectType`, `EFFECT_TO_FILTER_KIND` mapping, `EFFECT_DEFAULTS`, `ZOOM_PRESETS`, `ZOOM_MIN`, `ZOOM_MAX`, `nextZoomPreset()`, `prevZoomPreset()`, and `clampParam()` utility
    - Export `LayerDisplayInfo` interface for UI display
    - _Requirements: 2.1, 2.3, 7.2, 1.6, 8.3, 8.4, 3.6_

  - [ ]* 1.2 Write property tests for zoom preset navigation (Property 1)
    - **Property 1: Zoom preset navigation is monotonic and bounded**
    - Test that `nextZoomPreset(z)` returns value > z (or 6400 at max), `prevZoomPreset(z)` returns value < z (or 1 at min), both always within [1, 6400]
    - Use fast-check with random zoom values in [1, 6400], minimum 100 iterations
    - **Validates: Requirements 1.6, 8.3, 8.4**

  - [ ]* 1.3 Write property tests for parameter clamping (Property 2)
    - **Property 2: Parameter validation always clamps to valid range**
    - Test that `clampParam(value, min, max)` always returns v where min <= v <= max for any numeric input
    - Use fast-check with random floats/ints and random bounds, minimum 100 iterations
    - **Validates: Requirements 3.6**

  - [ ]* 1.4 Write property tests for effect-to-filter mapping (Property 3)
    - **Property 3: Effect type to filter kind mapping produces valid configurations**
    - Test that all 4 EffectType values map to distinct FilterKinds with valid default params (all required fields present, values in range)
    - Exhaustive over all EffectType values + schema validation
    - **Validates: Requirements 2.3, 7.2**

  - [ ]* 1.5 Write property tests for hex color validation (Property 6)
    - **Property 6: Hex color validation correctness**
    - Test `isValidHex(s)` returns true iff `s` matches `/^#[0-9A-Fa-f]{6}$/`; roundtrip `parseHex` → `toHex` preserves value
    - Use fast-check with random ascii strings + known-good hex strings, minimum 100 iterations
    - **Validates: Requirements 6.9**

- [x] 2. Hook refactoring
  - [x] 2.1 Create `useEffectLayer` hook replacing `useFilters`
    - Create `frontend/src/hooks/useEffectLayer.ts` implementing `UseEffectLayerReturn` interface
    - Read `filters[0]` from selected layer DTO as the single effect
    - Provide debounced `updateParams()` (100ms) that calls `update_filter` IPC
    - Implement optimistic update with rollback on IPC failure
    - _Requirements: 3.1, 3.3, 3.7, 7.3_

  - [x] 2.2 Extend `useLayers` hook with effect-aware operations
    - Add `removeLayer(layerId: number)` — IPC call to `remove_layer`
    - Add `addLayerWithEffect(effectType: EffectType, position: number)` — calls `add_layer` with effect_type + initial params atomically
    - Add `toggleVisibility(layerId: number)` for eye icon toggle
    - Add document structure validation on layer tree fetch (each non-image-source layer must have exactly 1 filter)
    - _Requirements: 4.4, 4.5, 4.7, 7.2, 7.4, 7.5_

  - [x] 2.3 Update `useViewport` hook with preset-based zoom
    - Add `zoomToNextPreset()` and `zoomToPrevPreset()` methods using the `nextZoomPreset`/`prevZoomPreset` utilities
    - Update zoom bounds to [0.01, 64.0] (representing 1%–6400%)
    - _Requirements: 8.3, 8.4, 8.5, 8.6_

  - [ ]* 2.4 Write property test for document structure validation (Property 7)
    - **Property 7: Document structure validation**
    - Test `validateDocumentStructure(tree)` returns valid for correct trees, invalid with `layerId` for offending layers
    - Use fast-check to generate random layer trees with varying filter counts, minimum 100 iterations
    - **Validates: Requirements 7.4, 7.5**

- [x] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 4. Menu Bar and layout restructuring
  - [x] 4.1 Create `MenuBar` component
    - Create `frontend/src/components/MenuBar.tsx` with 5 menu items: File, Edit, Presets, Color Lab, Help
    - Implement dropdown behavior for File (Open Image, Save/Export) and Edit (Undo, Redo with disabled state)
    - Color Lab click directly opens modal (no dropdown)
    - Support hover-to-switch between open dropdowns, Escape/click-outside to close
    - Style with ChicagoFLF 12px, height 27px, bg #D9D9D9, hover: black bg + white text
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7, 9.8, 1.3_

  - [x] 4.2 Create `PreviewWindow` component with retro title bar and footer zoom
    - Create `frontend/src/components/PreviewWindow.tsx` wrapping TileCanvas
    - Title bar: 20px height, "Preview" centered with decorative horizontal lines on both sides
    - Footer: minus button, zoom percentage text, plus button — using preset navigation
    - Disable minus at 1%, plus at 6400%
    - _Requirements: 1.5, 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7_

  - [x] 4.3 Update `App.tsx` layout to new 3-zone grid
    - Replace current `app-layout` grid with: Menu_Bar (27px row) + body row split into Preview_Window (fluid) + Sidebar (332px)
    - Wire MenuBar, PreviewWindow, and Sidebar components
    - Remove old Toolbar, ZoomControls from toolbar area, FilterList/FilterPanel/PalettePanel from sidebar
    - _Requirements: 1.1, 1.2, 1.4, 8.7_

  - [x] 4.4 Update `App.css` for new layout grid and sidebar width
    - Change `.app-layout` grid to `grid-template-columns: 1fr 332px` and rows to `27px 1fr`
    - Add/update styles for MenuBar, PreviewWindow footer, sidebar split (~50/50)
    - Ensure min-width 800px, min-height 600px
    - _Requirements: 1.1, 1.2, 1.4, 9.7_

- [x] 5. Sidebar components
  - [x] 5.1 Create `EffectSettingsPanel` component
    - Create `frontend/src/components/EffectSettingsPanel.tsx`
    - Render parameter controls based on selected layer's effect type (Dithering, Glitching, Curves, RGBChannels)
    - For Dithering: palette swatches + dropdown, algorithm dropdown, pixel size slider (1–32), threshold scale slider (0.1–4.0), levels slider (2–256)
    - Show empty state (title only) when no layer selected or Image_Source_Layer selected
    - Validate inputs: clamp to valid range, reject out-of-bounds
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7_

  - [x] 5.2 Create `LayersPanel` component
    - Create `frontend/src/components/LayersPanel.tsx`
    - Header: "Layers" title, blend mode dropdown (12 modes), opacity dropdown (0–100%)
    - Layer list: each item shows eye icon, layer number/name, effect type icon; selected layer highlighted
    - Image_Source_Layer fixed at bottom with image icon, cannot be deleted or moved
    - Footer: plus button (opens EffectChooserDialog), trash button (deletes selected, disabled for Image_Source_Layer)
    - Support drag-and-drop reorder (except Image_Source_Layer)
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8, 4.9_

  - [ ]* 5.3 Write property test for image source layer position invariant (Property 4)
    - **Property 4: Image source layer position invariant**
    - Test that for any layer tree and any sequence of reorder operations, Image_Source_Layer remains at index 0 (bottom)
    - Use fast-check to generate random layer trees + random reorder sequences, minimum 100 iterations
    - **Validates: Requirements 4.3, 4.8**

- [x] 6. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 7. Modal dialogs
  - [x] 7.1 Create `EffectChooserDialog` component
    - Create `frontend/src/components/EffectChooserDialog.tsx` — modal 364×468px with "Effect" title
    - Show 4 effect types: Dithering, Glitching, Curves, RGB channels — each with icon and name
    - On select: create layer, insert above current, make selected, close dialog
    - Close on Escape, overlay click, or close button — no changes made
    - Keyboard navigation: arrow up/down for focus, Enter to confirm
    - Semi-transparent overlay blocks main UI interaction
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5_

  - [x] 7.2 Create `ColorLabWindow` component
    - Create `frontend/src/components/ColorLabWindow.tsx` — modal 692×648px
    - Auto extract section: algorithm dropdown (MedianCut, KMeans), color count slider (2–256, default 8), "Extract from row frame" and "Extract from actual frame" buttons
    - Manual editing: hex input list (#RRGGBB, max 256), delete button per color, "add color +" button
    - Preview bar: first 6 colors as swatches, "Sort by brightness" button, "Auto interpolate" button
    - Import/export buttons: ASE, GPL, HEX/TXT, JSON formats
    - "Cancel" and "Apply" buttons — Cancel discards, Apply saves palette to Document
    - Validate hex inputs: show error indicator on invalid, don't apply invalid values
    - Show error on extract with no image, disable "add color +" at 256
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 6.7, 6.8, 6.9, 6.10, 6.11_

  - [ ]* 7.3 Write property test for sort by brightness (Property 5)
    - **Property 5: Sort by brightness produces monotone Oklab lightness**
    - Test that after `sortByBrightness`, Oklab L* values are in non-decreasing order
    - Use fast-check with random RGB color lists (1–256 colors), minimum 100 iterations
    - **Validates: Requirements 6.5**

- [x] 8. Integration and wiring
  - [x] 8.1 Wire all components together in App.tsx
    - Connect MenuBar actions (onOpenImage, onSaveImage, onOpenColorLab) to state/handlers
    - Connect EffectChooserDialog to `useLayers.addLayerWithEffect`
    - Connect EffectSettingsPanel to `useEffectLayer` hook
    - Connect LayersPanel to `useLayers` hook (select, add, remove, reorder, visibility, blend, opacity)
    - Connect ColorLabWindow open/close state and apply handler
    - Connect PreviewWindow to useViewport (zoomToNextPreset, zoomToPrevPreset, pan, wheel)
    - Manage modal open/close states (effectChooserOpen, colorLabOpen)
    - _Requirements: 1.1, 2.2, 2.3, 3.1, 3.3, 4.4, 4.5, 6.1, 7.1, 7.3, 9.3_

  - [x] 8.2 Remove deprecated components and clean up imports
    - Delete `frontend/src/components/FilterList.tsx`
    - Delete `frontend/src/components/FilterPanel.tsx`
    - Delete `frontend/src/components/Toolbar.tsx`
    - Remove inline PalettePanel usage from sidebar (Color Lab replaces it)
    - Delete `frontend/src/hooks/useFilters.ts`
    - Remove old ZoomControls from toolbar (now in PreviewFooter)
    - Clean up all stale imports in App.tsx and other files
    - _Requirements: 7.1_

  - [x] 8.3 Update CSS for all new components
    - Add styles for EffectChooserDialog (modal overlay, 364×468px, effect item list)
    - Add styles for ColorLabWindow (modal overlay, 692×648px, sections)
    - Add styles for EffectSettingsPanel (slider groups, dropdowns, palette swatches)
    - Add styles for LayersPanel footer (plus/trash buttons), layer items with effect icons
    - Add styles for PreviewWindow footer (zoom +/− buttons, percentage text)
    - Ensure retro Mac OS / System 7 aesthetic consistency throughout
    - _Requirements: 1.5, 5.1, 6.1, 8.1, 8.2, 9.7_

- [x] 9. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties using fast-check via Vitest
- Unit tests validate specific examples and edge cases
- The backend API (`add_layer`, `update_filter`, `remove_filter`, `add_filter`) remains unchanged — the frontend enforces the "one effect per layer" constraint
- All components use the existing retro Mac OS / System 7 theme from App.css (ChicagoFLF font, embossed borders, gray palette)

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "1.4", "1.5", "2.1", "2.3"] },
    { "id": 2, "tasks": ["2.2", "2.4"] },
    { "id": 3, "tasks": ["4.1", "4.2", "5.1", "5.2"] },
    { "id": 4, "tasks": ["4.3", "4.4", "5.3", "7.1", "7.2"] },
    { "id": 5, "tasks": ["7.3", "8.1"] },
    { "id": 6, "tasks": ["8.2", "8.3"] }
  ]
}
```
