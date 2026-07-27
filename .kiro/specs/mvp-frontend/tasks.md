# Implementation Plan: MVP Frontend

## Overview

Connect the existing React/TypeScript frontend to the Rust engine via Tauri IPC. Implement image loading, filter CRUD (Dither, Curves, Levels, Glitch), real-time preview rendering (tile pipeline → PNG → base64), and file export. The src-tauri scaffold already exists with basic document/layer/filter commands — this plan extends it with image-aware state, render pipeline, and a proper UI.

## Tasks

- [x] 1. Extend Tauri AppState and add dependencies
  - [x] 1.1 Add `image`, `base64`, and `png` crates to `src-tauri/Cargo.toml` dependencies; add `proptest` and `tempfile` to dev-dependencies
    - Add `image = { version = "0.25", features = ["png", "jpeg", "webp"] }`
    - Add `base64 = "0.22"`
    - _Requirements: 9.2, 10.3_
  - [x] 1.2 Refactor `AppState` in `src-tauri/src/commands.rs` to include `Mutex<Option<ImageData>>` field and define `ImageData` struct (width, height, tiles grid of `Arc<PixelTile>`)
    - Replace the existing `AppState` struct with the design's version
    - Update `main.rs` to initialize the new `AppState` with `image_data: Mutex::new(None)`
    - _Requirements: 9.1, 9.2_
  - [x] 1.3 Add `Dither` and `Glitch` variants to `FilterKind` enum in `crates/engine-project/src/filter.rs`
    - Add `Dither` and `Glitch` to the `FilterKind` enum
    - Add `Dither { algorithm: DitherAlgorithm, color_depth: u8 }` and `Glitch { glitch_type: GlitchType, intensity: f32, seed: u64 }` to `FilterParams`
    - Define `DitherAlgorithm` (FloydSteinberg, Ordered, Threshold) and `GlitchType` (RGBShift, BlockDisplace) enums with Serialize/Deserialize
    - Add `channel: CurveChannel` field to `FilterParams::Curves` and `gamma: f32` to `FilterParams::Levels`
    - Update `FilterKind::fmt`, `FilterInstance::validate()`, and `apply_filter_to_tile()` to handle new variants
    - Update all match arms in existing code that reference `FilterKind` and `FilterParams`
    - _Requirements: 3.1, 4.1, 4.2, 6.1, 6.2_

- [x] 2. Backend `load_image` command
  - [x] 2.1 Implement `load_image` Tauri command in `src-tauri/src/commands.rs`
    - Accept `path: String`, decode with the `image` crate (PNG/JPEG/WebP)
    - Validate dimensions ≤ 8192×8192, return `InvalidState` error if exceeded
    - Return IoError for missing file or unsupported format
    - Convert decoded image to RGBA f32 tiles (256×256 grid)
    - Store tiles in `AppState.image_data`
    - Create new `Document` with image dimensions and one raster layer
    - Return `LoadImageResponse { doc_id, width, height, tile_count }`
    - Register command in `main.rs` invoke_handler
    - _Requirements: 1.3, 1.5, 1.6, 1.7, 9.1, 9.2, 9.3, 9.4, 9.5, 9.6_
  - [ ]* 2.2 Write property test: load_image metadata correctness (Property 1)
    - **Property 1: Load image metadata correctness**
    - For any valid image (W, H) in [1, 8192], verify response has correct width, height, tile_count = ceil(W/256) × ceil(H/256)
    - **Validates: Requirements 1.3, 9.2, 9.3**
  - [ ]* 2.3 Write property test: dimension boundary validation (Property 2)
    - **Property 2: Dimension boundary validation**
    - For dimensions in [1, 8192] → success; for dimensions > 8192 → InvalidState error
    - **Validates: Requirements 1.6, 1.7, 9.6**
  - [ ]* 2.4 Write property test: invalid path and corrupt data (Property 3)
    - **Property 3: Invalid path and corrupt data error handling**
    - For non-existent paths or random bytes files → IoError with non-empty message
    - **Validates: Requirements 1.5, 9.4, 9.5**

- [x] 3. Backend `render_preview` command
  - [x] 3.1 Implement `render_preview` Tauri command in `src-tauri/src/commands.rs`
    - Accept `doc_id: u32`, read tiles from `AppState.image_data`
    - Apply all enabled filters from the document's layer in order
    - Compute preview size (≤ 2048 on longest side, preserving aspect ratio)
    - Stitch processed tiles into a single RGBA u8 buffer (f32→u8 conversion)
    - Encode buffer as PNG using the `image` crate or `png` crate
    - Return `RenderPreviewResponse { base64_png, width, height }`
    - Handle DocumentNotFound and empty-layer-data cases
    - Register command in `main.rs` invoke_handler
    - _Requirements: 2.3, 10.1, 10.2, 10.3, 10.4, 10.5, 10.6_
  - [ ]* 3.2 Write property test: preview is valid PNG (Property 9)
    - **Property 9: Preview output is valid decodable PNG**
    - For any valid document with pixel data, verify base64 decodes to valid RGBA PNG
    - **Validates: Requirements 10.3**
  - [ ]* 3.3 Write property test: preview downscale respects 2048 limit (Property 10)
    - **Property 10: Preview downscale respects 2048 limit with aspect ratio preservation**
    - For documents > 2048px, verify output max dimension ≤ 2048 and aspect ratio preserved
    - **Validates: Requirements 10.4**

- [x] 4. Backend filter CRUD commands (update_filter, remove_filter)
  - [x] 4.1 Implement `update_filter` Tauri command in `src-tauri/src/commands.rs`
    - Accept `UpdateFilterRequest { layer_id, filter_id, params }`
    - Find filter by ID on the specified layer, update its params
    - Validate params (call `FilterInstance::validate()`), return error if invalid
    - Invalidate tile cache for the affected layer
    - Register command in `main.rs` invoke_handler
    - _Requirements: 3.5, 5.3, 5.5_
  - [x] 4.2 Implement `remove_filter` Tauri command in `src-tauri/src/commands.rs`
    - Accept `RemoveFilterRequest { layer_id, filter_id }`
    - Find and remove the filter by ID from the specified layer
    - Invalidate tile cache
    - Register command in `main.rs` invoke_handler
    - _Requirements: 3.6, 3.7_
  - [x] 4.3 Extend existing `add_filter` command to handle Dither and Glitch kinds
    - Parse `DitherAlgorithm`, `color_depth`, `GlitchType`, `intensity`, `seed` from JSON params
    - Parse `channel` for Curves and `gamma` for Levels
    - Validate color_depth in [1, 8], return error otherwise
    - _Requirements: 3.2, 4.1, 4.2, 4.6, 6.1, 6.2_
  - [ ]* 4.4 Write property test: filter list ordering invariant (Property 5)
    - **Property 5: Filter list ordering invariant**
    - For N add_filter operations, verify list has N filters in same order; removing index i preserves remaining order
    - **Validates: Requirements 3.2, 3.3, 3.6**
  - [ ]* 4.5 Write property test: Dither color_depth range validation (Property 6)
    - **Property 6: Dither color_depth range validation**
    - For color_depth outside [1, 8], verify rejection with error
    - **Validates: Requirements 4.6**
  - [ ]* 4.6 Write property test: Glitch zero-intensity no-op (Property 8)
    - **Property 8: Glitch zero-intensity no-op**
    - For any PixelTile data, Glitch with intensity=0.0 → output equals input
    - **Validates: Requirements 6.6**

- [x] 5. Backend `export_image` command
  - [x] 5.1 Implement `export_image` Tauri command in `src-tauri/src/commands.rs`
    - Accept `ExportImageRequest { doc_id, path, format, quality? }`
    - Validate format is "PNG" or "JPEG", return `InvalidFilterParams` error otherwise
    - Render full-resolution image (all filters applied at original document dimensions)
    - Encode as PNG or JPEG (default quality 90 for JPEG)
    - Write to file, return IoError on write failure
    - Register command in `main.rs` invoke_handler
    - _Requirements: 7.3, 7.5, 11.1, 11.2, 11.3, 11.4, 11.5, 11.6_
  - [ ]* 5.2 Write property test: invalid export format rejection (Property 11)
    - **Property 11: Invalid export format rejection**
    - For any format string not "PNG" and not "JPEG", verify error returned and no file created
    - **Validates: Requirements 11.5**

- [x] 6. Checkpoint - Backend complete
  - Ensure all tests pass, ask the user if questions arise.

- [x] 7. Frontend layout, Toolbar, and empty state
  - [x] 7.1 Create CSS Grid layout in `frontend/src/App.css` and restructure `App.tsx`
    - Implement three-zone CSS Grid: toolbar (48px), canvas (1fr), sidebar (minmax(200px, 320px))
    - Set minimum window size 800×600
    - Add grid-template-areas: "toolbar toolbar" / "canvas sidebar"
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.6_
  - [x] 7.2 Create `frontend/src/components/Toolbar.tsx`
    - Render "Open File" and "Save" buttons
    - "Save" button disabled when no document loaded
    - "Open File" triggers Tauri file dialog (PNG/JPEG/WebP filter)
    - "Save" triggers Tauri save dialog (PNG/JPEG filter)
    - _Requirements: 1.1, 1.2, 1.8, 7.1, 7.2, 7.6, 7.7, 8.2_
  - [x] 7.3 Create empty-state placeholder in canvas area
    - Display "Drag a file or click Open" when no image loaded
    - _Requirements: 8.5_

- [x] 8. Frontend PreviewCanvas + usePreview + useDocument hooks + image loading flow
  - [x] 8.1 Create `frontend/src/types/index.ts` with TypeScript interfaces for all IPC DTOs
    - Define `LoadImageResponse`, `RenderPreviewResponse`, `FilterInfo`, `FilterKind`, `FilterParams`, `DitherAlgorithm`, `CurveChannel`, `GlitchType`, `ExportImageRequest`
    - _Requirements: 9.3, 10.1_
  - [x] 8.2 Create `frontend/src/ipc/commands.ts` with typed invoke wrappers
    - Implement `loadImage`, `renderPreview`, `addFilter`, `updateFilter`, `removeFilter`, `exportImage`
    - _Requirements: 9.1, 10.1, 11.1_
  - [x] 8.3 Create `frontend/src/hooks/useDocument.ts`
    - State: docId, width, height, layerId, loading
    - `openImage()`: call Tauri dialog → loadImage IPC → set state
    - `exportImage()`: call Tauri dialog → exportImage IPC
    - _Requirements: 1.1, 1.3, 1.4, 7.1_
  - [x] 8.4 Create `frontend/src/hooks/usePreview.ts`
    - State: previewSrc (data URL), isRendering
    - `refresh()`: call renderPreview IPC → update previewSrc
    - Auto-refresh after document load or filter changes
    - Implement `computeFitToView(imgW, imgH, vpW, vpH)` utility for aspect-ratio scaling
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_
  - [x] 8.5 Create `frontend/src/components/PreviewCanvas.tsx`
    - Render `<img>` with `src={previewSrc}` (data:image/png;base64,...)
    - Show loading spinner overlay while rendering
    - Fit-to-view with aspect ratio preservation
    - Handle resize with 200ms debounce
    - _Requirements: 2.1, 2.2, 2.4, 2.5_
  - [ ]* 8.6 Write property test: fit-to-view preserves aspect ratio (Property 4)
    - **Property 4: Fit-to-view preserves aspect ratio**
    - Use fast-check to verify computeFitToView for arbitrary dimensions
    - **Validates: Requirements 2.1**

- [x] 9. Frontend Sidebar + FilterList + filter CRUD
  - [x] 9.1 Create `frontend/src/hooks/useFilters.ts`
    - State: filters array, activeFilterId
    - `addFilter(kind)`: call addFilter IPC → append to list → set active → trigger preview refresh
    - `updateFilter(filterId, params)`: call updateFilter IPC → update list → trigger refresh (100ms debounce)
    - `removeFilter(filterId)`: call removeFilter IPC → remove from list → trigger refresh
    - Rollback on error (restore previous state)
    - _Requirements: 3.2, 3.4, 3.5, 3.6, 3.7_
  - [x] 9.2 Create `frontend/src/components/Sidebar.tsx`
    - Container with vertical scroll for overflow
    - Render FilterList + active filter's parameter editor (FilterPanel)
    - _Requirements: 8.4, 8.7_
  - [x] 9.3 Create `frontend/src/components/FilterList.tsx`
    - Show available filter types: Dither, Curves, Levels, Glitch (add buttons)
    - Show applied filters with remove button per filter
    - Click filter to set as active (show params in sidebar)
    - _Requirements: 3.1, 3.2, 3.4, 3.6_
  - [x] 9.4 Create `frontend/src/components/FilterPanel.tsx`
    - Route to correct param editor based on active filter's kind
    - _Requirements: 3.4_

- [x] 10. Frontend filter parameter editors
  - [x] 10.1 Create `frontend/src/components/common/Slider.tsx`
    - Reusable slider with label, numeric display (2 decimal places), min/max/step props
    - _Requirements: 5.4, 6.4_
  - [x] 10.2 Create `frontend/src/components/common/Notification.tsx`
    - Toast notification component, auto-hides after 5 seconds
    - Display error messages from IPC failures
    - _Requirements: 7.4, 1.5, 3.7, 4.5, 5.5, 6.5_
  - [x] 10.3 Create `frontend/src/components/filters/DitherParams.tsx`
    - Algorithm dropdown (FloydSteinberg default), color_depth integer slider [1–8] (default 4)
    - Show numeric value beside slider
    - Reject input outside [1, 8] range on frontend
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.6_
  - [x] 10.4 Create `frontend/src/components/filters/CurvesParams.tsx`
    - Channel selector (Red, Green, Blue, All, Luminance) — default All
    - Curve point editor (list of (x, y) control points in [0, 1])
    - Minimally: render editable point list or simple input fields for MVP
    - _Requirements: 5.1, 5.3, 5.4_
  - [x] 10.5 Create `frontend/src/components/filters/LevelsParams.tsx`
    - 5 sliders: input_black, input_white, gamma, output_black, output_white
    - All use 2-decimal numeric display
    - Debounce 100ms on param change → updateFilter IPC
    - _Requirements: 5.2, 5.3, 5.4_
  - [x] 10.6 Create `frontend/src/components/filters/GlitchParams.tsx`
    - Type selector (RGBShift default, BlockDisplace)
    - Intensity slider [0.0–1.0], step 0.01, default 0.5
    - Numeric display 2 decimal places
    - _Requirements: 6.1, 6.2, 6.3, 6.4_
  - [ ]* 10.7 Write property test: float display precision (Property 7)
    - **Property 7: Float display precision**
    - Use fast-check to verify formatValue produces exactly 2 decimal places for any float in [0, 10]
    - **Validates: Requirements 5.4, 6.4**

- [x] 11. Integration wiring and error handling polish
  - [x] 11.1 Wire all hooks and components together in `App.tsx`
    - Connect useDocument, useFilters, usePreview hooks
    - Pass callbacks: Toolbar → useDocument.openImage/exportImage
    - Pass callbacks: FilterList → useFilters.addFilter/removeFilter
    - Pass callbacks: FilterPanel → useFilters.updateFilter
    - Trigger preview refresh after any filter or document mutation
    - _Requirements: 1.4, 2.4, 3.3, 3.5_
  - [x] 11.2 Implement error handling with Notification toast
    - Wrap all IPC calls with try/catch → show error toast
    - On filter update error: rollback UI to previous values
    - On render error: keep last successful preview
    - _Requirements: 1.5, 2.6, 3.7, 4.5, 5.5, 6.5, 7.5_
  - [x] 11.3 Add test dependencies to `frontend/package.json` and configure Vitest
    - Add vitest, fast-check, @testing-library/react, jsdom as devDependencies
    - Create `frontend/vitest.config.ts` with jsdom environment
    - _Requirements: (testing infrastructure)_

- [x] 12. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The existing `src-tauri/` already has basic scaffold (main.rs, commands.rs with document/layer/filter commands) — tasks build on top of this
- The existing `FilterKind` enum only has Curves, Levels, Placeholder — Task 1.3 adds Dither and Glitch
- The existing `add_filter` command only handles Curves and Levels — Task 4.3 extends it

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2", "1.3"] },
    { "id": 1, "tasks": ["2.1", "7.1"] },
    { "id": 2, "tasks": ["2.2", "2.3", "2.4", "3.1", "4.1", "4.2", "4.3", "7.2", "7.3"] },
    { "id": 3, "tasks": ["3.2", "3.3", "4.4", "4.5", "4.6", "5.1", "8.1"] },
    { "id": 4, "tasks": ["5.2", "8.2", "8.3", "8.4"] },
    { "id": 5, "tasks": ["8.5", "8.6", "9.1", "10.1", "10.2"] },
    { "id": 6, "tasks": ["9.2", "9.3", "9.4", "10.3", "10.4", "10.5", "10.6"] },
    { "id": 7, "tasks": ["10.7", "11.1"] },
    { "id": 8, "tasks": ["11.2", "11.3"] }
  ]
}
```
