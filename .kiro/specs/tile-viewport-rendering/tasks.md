# Implementation Plan: Tile Viewport Rendering

## Overview

This plan migrates Dither Yuki 2 from the current full-image render pipeline (`render_preview` → PNG → base64 → `<img>`) to a viewport-driven tile rendering architecture. The implementation follows the design document's recommended work order: backend tile pipeline first, then frontend canvas, then zoom/pan, then compositing and layer panel.

## Tasks

- [x] 1. Image decomposition and Raw tile storage
  - [x] 1.1 Implement `decompose_image_to_tiles` in `engine-tiles`
    - Create a new module `crates/engine-tiles/src/decompose.rs`
    - Implement `decompose_image_to_tiles(rgba_f32, width, height, layer_id, cache) -> Result<TileGrid, EngineError>`
    - Tile the image left-to-right, top-to-bottom in 256×256 blocks
    - Edge tiles zero-filled for out-of-bounds regions
    - Populate 2px halo region from adjacent pixel data
    - Return `TileGrid { cols, rows }` struct
    - _Requirements: 1.1, 1.2_

  - [x]* 1.2 Write property test for image decomposition (Property 1)
    - **Property 1: Image decomposition produces correct tile grid**
    - Test with random (width, height) ∈ [1, 8192]: verify exactly `ceil(w/256) × ceil(h/256)` tiles produced
    - Verify edge tiles have zero-filled out-of-bounds regions
    - Verify tile coordinates range correctness
    - **Validates: Requirements 1.1, 1.2**

  - [x] 1.3 Rewrite `load_image` command to use `decompose_image_to_tiles`
    - Modify `src-tauri/src/commands.rs` `load_image` to call `decompose_image_to_tiles` after decoding
    - Store Raw-stage tiles in TileCache instead of `AppState.image_data`
    - Remove the `Mutex<Option<ImageData>>` field from `AppState`
    - Update `AppState` struct: remove `image_data`, add `scheduler: Scheduler` and `viewport: Mutex<ViewportState>`
    - _Requirements: 1.1, 1.3_

  - [x]* 1.4 Write property test for f32→u8 encoding (Property 3)
    - **Property 3: Tile protocol f32→u8 encoding correctness**
    - Test with random f32 values including edge cases (0.0, 1.0, negatives, >1.0)
    - Verify each byte equals `round(clamp(value, 0.0, 1.0) * 255.0)`
    - Verify output is exactly 262,144 bytes (256×256×4)
    - **Validates: Requirements 2.2**

- [x] 2. Tile protocol handler
  - [x] 2.1 Implement tile URL parser
    - Create `src-tauri/src/tile_protocol.rs`
    - Implement `parse_tile_url(uri) -> Result<ParsedTileUrl, TileProtocolError>`
    - Handle URL format: `tile://doc/{doc_id}/layer/{layer_id}/stage/{stage}/l/{level}/{x}/{y}`
    - Validate each path segment; return 400 for malformed URLs
    - _Requirements: 2.1, 2.7_

  - [x] 2.2 Implement `f32_tile_to_rgba8` conversion function
    - Implement in `src-tauri/src/tile_protocol.rs`
    - Convert the 256×256 main region (skipping halo) from f32 to u8
    - Clamp values to [0.0, 1.0], multiply by 255, round
    - Return exactly 262,144 bytes (RGBA8, row-major)
    - _Requirements: 2.2_

  - [x] 2.3 Register `tile://` custom protocol with Tauri
    - In `src-tauri/src/main.rs`, register the protocol via `tauri::Builder::register_asynchronous_uri_scheme_protocol`
    - On cache hit (not dirty): return 200 + RGBA8 bytes
    - On cache miss or dirty: schedule Immediate task, return 202
    - On out-of-bounds coord or missing layer/doc: return 404
    - On malformed URL: return 400
    - _Requirements: 2.1, 2.2, 2.3, 2.5, 2.6, 2.7_

  - [x]* 2.4 Write property test for tile protocol error responses (Property 4)
    - **Property 4: Tile protocol error responses**
    - Test with random OOB coordinates → verify 404
    - Test with random malformed URLs → verify 400
    - Test with valid URLs referencing nonexistent doc/layer → verify 404
    - **Validates: Requirements 2.5, 2.6, 2.7**

- [x] 3. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 4. Viewport manager and priority scheduling
  - [x] 4.1 Implement `set_viewport` command
    - Create `src-tauri/src/viewport.rs`
    - Implement `ViewportState` struct with zoom, x, y, width, height, level, visible_tiles, prefetch_tiles
    - Implement `compute_pyramid_level(zoom, max_level) -> u8` using `max(0, floor(log2(1.0/zoom)))`
    - Implement `compute_visible_tiles(zoom, x, y, width, height, level, doc_width, doc_height) -> Vec<TileCoord>`
    - Implement `compute_prefetch_ring` for one-tile-wide adjacent ring
    - Register `set_viewport` as Tauri command in `main.rs`
    - _Requirements: 3.1, 3.2, 3.5_

  - [x] 4.2 Implement priority classification and scheduling logic
    - Implement `classify_priority(coord, visible) -> Priority` (inner 50% → ViewportCenter, outer 50% → ViewportEdge)
    - Schedule missing/dirty tiles via Scheduler with correct priorities
    - Cancel stale tasks that are no longer in viewport+prefetch ring
    - _Requirements: 3.3, 3.4, 3.6_

  - [x]* 4.3 Write property test for viewport-to-tiles computation (Property 5)
    - **Property 5: Viewport-to-tiles computation**
    - Test with random viewport params (zoom ∈ [0.01, 64.0], arbitrary x/y/width/height) and doc dimensions up to 8192×8192
    - Verify computed tiles equal all TileCoords whose 256×scale region intersects the viewport
    - **Validates: Requirements 3.1, 3.2**

  - [x]* 4.4 Write property test for priority assignment (Property 6)
    - **Property 6: Selective scheduling with correct priority assignment**
    - Verify inner 50% tiles get ViewportCenter, outer 50% get ViewportEdge, prefetch ring gets Prefetch
    - Verify cached clean tiles are NOT scheduled
    - **Validates: Requirements 3.3, 3.4, 3.5**

- [x] 5. Lazy filter pipeline and invalidation
  - [x] 5.1 Implement on-demand Processed tile computation
    - Create `src-tauri/src/tile_pipeline.rs`
    - Implement `compute_processed_tile(key, state) -> Result<PixelTile, EngineError>`
    - Fetch Raw tile from cache, apply layer's filter stack via `apply_filter_to_tile`
    - Read halo pixels from neighboring Raw tiles for error diffusion correctness
    - Store result in cache at Processed stage
    - _Requirements: 4.1, 4.2_

  - [x] 5.2 Implement invalidation on filter/layer changes
    - Modify `update_filter` command: increment layer generation, mark Processed+Composite dirty for affected layer
    - Mark Composite dirty for all layers above the changed layer
    - Schedule viewport-visible dirty tiles for recomputation
    - Emit `tile-ready` event after each tile is recomputed
    - _Requirements: 4.3, 4.4, 10.1, 10.2_

  - [x]* 5.3 Write property test for invalidation scope (Property 8)
    - **Property 8: Invalidation scope correctness**
    - For filter change: verify Processed+Composite dirty for target layer, Composite dirty for layers above, Raw unchanged
    - For property change: verify only Composite dirty for affected layer and above
    - **Validates: Requirements 4.3, 10.1, 10.2**

  - [x]* 5.4 Write property test for filter pipeline equivalence (Property 7)
    - **Property 7: Lazy filter pipeline equivalence**
    - Verify on-demand Processed tile is byte-identical to calling `apply_filter_to_tile` directly
    - Test with random tile data + random filter stacks (1–4 filters)
    - **Validates: Requirements 4.1**

- [x] 6. Worker pool and tile-ready events
  - [x] 6.1 Implement tile worker loop
    - Create `src-tauri/src/worker.rs`
    - Implement `tile_worker_loop(state, app_handle)` that dequeues from Scheduler
    - Perform staleness check against GenerationTracker before execution
    - Execute Raw/Processed/Composite computation based on task stage
    - Insert fresh tile into cache and emit `tile-ready` event
    - _Requirements: 2.4, 10.4, 10.5, 10.6_

  - [x] 6.2 Spawn worker threads on app startup
    - In `main.rs`, spawn N worker threads (N = available_parallelism or 4 fallback)
    - Pass `Arc<AppState>` and `AppHandle` to each worker
    - _Requirements: 11.1, 11.2_

  - [x]* 6.3 Write property test for stale task discard (Property 14)
    - **Property 14: Stale task discard**
    - Verify tasks with non-matching generation or layer_generation are discarded
    - Verify no cache write or event emission for stale tasks
    - **Validates: Requirements 10.5**

- [x] 7. Remove legacy render pipeline
  - [x] 7.1 Remove `render_preview` command and `usePreview` hook
    - Delete `render_preview` IPC command from `src-tauri/src/commands.rs`
    - Remove `render_preview` from `tauri::generate_handler!` in `main.rs`
    - Delete `frontend/src/hooks/usePreview.ts` (keep `computeFitToView` utility if needed elsewhere, or inline into viewport controller)
    - Remove `PreviewCanvas` component from `frontend/src/components/PreviewCanvas.tsx`
    - Remove imports and usage from `App.tsx`
    - _Requirements: 1.3 (TileCache is sole source)_

- [x] 8. Checkpoint - Ensure backend compiles and all Rust tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. TileCanvas frontend component
  - [x] 9.1 Create Tile Web Worker
    - Create `frontend/src/workers/tileWorker.ts`
    - Implement message handlers: `request-tiles` (batch fetch), `fetch-tile` (single fetch)
    - Fetch from `tile://` URL, decode raw RGBA8 to ImageData, create ImageBitmap
    - Post decoded bitmaps back to main thread via transferable objects
    - _Requirements: 5.4_

  - [x] 9.2 Implement TileCanvas component
    - Create `frontend/src/components/TileCanvas.tsx`
    - Manage HTML5 `<canvas>` element with 2D context
    - Initialize Web Worker on mount, terminate on unmount
    - Maintain a `Map<string, ImageBitmap>` for decoded tiles
    - Listen for `tile-ready` Tauri events, trigger re-fetch of updated tiles
    - Draw tiles at correct screen positions based on viewport state
    - _Requirements: 5.1, 5.2, 5.3_

  - [x] 9.3 Implement fallback display (pending tiles and error handling)
    - Display nearest lower-resolution pyramid tile scaled up while higher-res tiles load
    - Display neutral gray placeholder if no pyramid tile available
    - Show error indicator for failed tile fetches; retry up to 2× with 500ms delay
    - _Requirements: 5.5, 5.6_

  - [x] 9.4 Wire TileCanvas into App.tsx
    - Replace `PreviewCanvas` usage in `App.tsx` with `TileCanvas`
    - Pass docId, docWidth, docHeight, viewport state, and onViewportChange callback
    - Remove `usePreview` hook usage
    - _Requirements: 5.1_

- [x] 10. Viewport controller (zoom and pan)
  - [x] 10.1 Implement `useViewport` hook
    - Create `frontend/src/hooks/useViewport.ts`
    - Manage `ViewportState { zoom, panX, panY, canvasWidth, canvasHeight }`
    - Implement `handleWheel`: zoom 2× per scroll detent centered on cursor, clamp [0.01, 64.0]
    - Implement `handlePanDrag`: update pan offset by `delta_screen / zoom`
    - Implement `fitToView`: compute zoom as `min(canvasWidth / docWidth, canvasHeight / docHeight)`
    - Implement `constrainPan`: viewport center cannot exceed 50% beyond document bounds
    - Call `set_viewport` IPC after each viewport change (debounced)
    - _Requirements: 6.1, 6.2, 6.7, 7.1, 7.2, 7.4_

  - [x] 10.2 Implement zoom indicator and presets UI
    - Add zoom percentage display (rounded to nearest integer)
    - Add preset buttons: Fit, 25%, 50%, 100%, 200%, 400%
    - Add editable zoom input (validated to 1%–6400% range)
    - _Requirements: 6.6_

  - [x] 10.3 Implement pan mode activation and cursor handling
    - Activate pan on middle mouse button hold or Space+left mouse
    - Change cursor to grab/hand icon during pan mode
    - Restore previous cursor on release
    - Reposition already-fetched tiles within one animation frame during pan
    - _Requirements: 7.1, 7.3, 7.5_

  - [x]* 10.4 Write property test for zoom transform (Property 9)
    - **Property 9: Zoom preserves document point under cursor**
    - For random viewport states and cursor positions, verify the document point under cursor stays at same screen position after zoom (±0.5px tolerance)
    - **Validates: Requirements 6.1, 6.2, 6.7**

  - [x]* 10.5 Write property test for pan constraint (Property 10)
    - **Property 10: Pan transform with boundary constraint**
    - For random viewport states and drag deltas, verify pan updates by `delta_screen / zoom` and center is constrained within 50% of viewport width/height beyond document bounds
    - **Validates: Requirements 7.2, 7.4**

- [x] 11. Checkpoint - Verify frontend renders tiles and zoom/pan works end-to-end
  - Ensure all tests pass, ask the user if questions arise.

- [x] 12. Multi-layer compositor
  - [x] 12.1 Implement `composite_tile` function
    - Create `crates/engine-project/src/compositor.rs`
    - Walk layer tree bottom-to-top via `walk_bottom_to_top`
    - For each visible leaf layer: get Processed tile, apply mask, blend into composite
    - Implement `blend_tile(dst, src, mode, opacity)` with Porter-Duff "over" compositing
    - Implement all 12 `apply_blend_mode` formulas (Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion)
    - Handle group isolation: push/pop composite stack at GroupStart/GroupEnd
    - Skip invisible layers/groups and their descendants
    - Return fully transparent tile if no visible layers contribute content
    - _Requirements: 8.1, 8.2, 8.5, 8.6, 8.7_

  - [x] 12.2 Implement layer mask application
    - Implement `apply_layer_mask(layer, tile, coord, cache) -> PixelTile`
    - Multiply layer alpha by mask luminance (0.2126R + 0.7152G + 0.0722B)
    - Support inverted masks: use `1.0 - luminance`
    - Skip masking if mask is None or disabled
    - _Requirements: 8.4_

  - [x] 12.3 Wire compositor into worker pipeline
    - In `tile_pipeline.rs`, implement `compute_composite_tile(key, state)`
    - Call `composite_tile` with document snapshot and tile coord
    - Store Composite tile in cache; this is what `tile://` serves for composite stage
    - _Requirements: 8.1, 8.3_

  - [x]* 12.4 Write property test for compositor blending (Property 11)
    - **Property 11: Compositor blending correctness**
    - Test with random 1–8 layer stacks, random blend modes, arbitrary opacity and pixel values
    - Verify composite equals manual bottom-to-top blend with Porter-Duff "over"
    - **Validates: Requirements 8.1, 8.2, 8.5, 8.7**

  - [x]* 12.5 Write property test for mask alpha multiplication (Property 12)
    - **Property 12: Mask alpha multiplication**
    - Test with random mask luminance and layer alpha values
    - Verify effective alpha = `layer_alpha × mask_luminance` (or `1.0 - luminance` if inverted)
    - **Validates: Requirements 8.4**

  - [x]* 12.6 Write property test for structure invalidation (Property 13)
    - **Property 13: Structure invalidation marks all Composite tiles dirty**
    - For layer add/remove/reorder: verify ALL Composite-stage tiles dirty, Raw/Processed unchanged
    - **Validates: Requirements 10.3**

- [x] 13. Layer panel UI
  - [x] 13.1 Create LayerPanel component with tree rendering
    - Create `frontend/src/components/LayerPanel.tsx`
    - Render layer tree structure reflecting document hierarchy (bottom-to-top visual order)
    - Show layer name, indentation for group children
    - Display 32×32 thumbnail per layer from highest pyramid level tile
    - _Requirements: 9.1, 9.2_

  - [x] 13.2 Implement per-layer controls
    - Visibility toggle (eye icon)
    - Opacity slider (0.0–1.0, display as 0–100%)
    - Blend mode dropdown (12 variants)
    - Editable layer name field (1–64 chars, trimmed, reject empty)
    - Invoke `set_layer_props` on changes
    - _Requirements: 9.4, 9.7_

  - [x] 13.3 Implement drag-and-drop layer reordering
    - Show drop-position indicator on drag
    - On drop, invoke `reorder_layer` with layer ID, target parent, target index
    - Revert UI on IPC error
    - _Requirements: 9.3, 9.6_

  - [x] 13.4 Implement add-layer button
    - Invoke `add_layer` to create new Raster layer above selected (or at top of root)
    - Handle error with notification and UI revert
    - _Requirements: 9.5, 9.6_

  - [x]* 13.5 Write property test for layer name validation (Property 15)
    - **Property 15: Layer name validation**
    - Test with random strings: verify trimming, empty rejection, 1–64 char acceptance
    - **Validates: Requirements 9.7**

  - [x] 13.6 Wire LayerPanel into App.tsx sidebar
    - Add LayerPanel to sidebar, pass layer tree data from document snapshot
    - Create `useLayer` hook or extend `useDocument` for layer operations
    - Wire `add_layer`, `reorder_layer`, `set_layer_props` IPC calls
    - _Requirements: 9.1_

- [x] 14. New Tauri commands for layer management
  - [x] 14.1 Expose `add_layer`, `reorder_layer`, `set_layer_props` as Tauri IPC commands
    - Create typed IPC wrappers in `src-tauri/src/commands.rs` delegating to `engine_project::commands`
    - Add DTOs: `AddLayerRequest`, `LayerPropsPatchDto`, `LayerNodeDto`
    - Register in `tauri::generate_handler!`
    - Add `get_layer_tree` command returning `Vec<LayerNodeDto>` for frontend consumption
    - _Requirements: 9.3, 9.4, 9.5_

- [x] 15. Viewport-aware eviction
  - [x] 15.1 Extend TileCache eviction to preserve viewport tiles
    - Modify `evict_if_over_budget` to accept current viewport tile coords
    - Skip eviction of any tile whose TileCoord overlaps the viewport at the active pyramid level
    - If budget is exceeded but all remaining tiles are viewport tiles, allow over-budget
    - _Requirements: 1.5_

  - [x]* 15.2 Write property test for viewport-aware eviction (Property 2)
    - **Property 2: Viewport-aware eviction preserves visible tiles**
    - Test with random viewport + cache entries exceeding budget
    - Verify viewport tiles remain after eviction; total usage ≤ budget (or minimum for viewport)
    - **Validates: Requirements 1.5**

- [x] 16. Final checkpoint - Full integration verification
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The existing `engine-tiles` crate (TileCache, Scheduler, GenerationTracker, Pyramid) is already built and tested — tasks connect it to production
- Backend tasks (1–8) can be validated with `cargo test` and `cargo build` without the frontend
- Frontend tasks (9–13) depend on the backend `tile://` protocol being functional
- Layer management commands (task 14) already exist in `engine-project::commands` but need Tauri IPC exposure

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "2.1"] },
    { "id": 1, "tasks": ["1.2", "1.3", "2.2"] },
    { "id": 2, "tasks": ["1.4", "2.3", "2.4"] },
    { "id": 3, "tasks": ["4.1", "7.1"] },
    { "id": 4, "tasks": ["4.2", "4.3", "4.4"] },
    { "id": 5, "tasks": ["5.1", "6.1"] },
    { "id": 6, "tasks": ["5.2", "5.3", "5.4", "6.2", "6.3"] },
    { "id": 7, "tasks": ["9.1", "14.1", "15.1"] },
    { "id": 8, "tasks": ["9.2", "9.3", "15.2"] },
    { "id": 9, "tasks": ["9.4", "10.1", "12.1"] },
    { "id": 10, "tasks": ["10.2", "10.3", "10.4", "10.5", "12.2"] },
    { "id": 11, "tasks": ["12.3", "12.4", "12.5", "12.6"] },
    { "id": 12, "tasks": ["13.1", "13.2"] },
    { "id": 13, "tasks": ["13.3", "13.4", "13.5", "13.6"] }
  ]
}
```
