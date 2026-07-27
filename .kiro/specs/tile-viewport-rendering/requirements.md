# Requirements Document

## Introduction

This feature migrates the Dither Yuki 2 image editor from a full-image render pipeline (`render_preview` → PNG → base64 → `<img>`) to a viewport-driven tile rendering architecture. The migration connects the existing `engine-tiles` infrastructure (TileCache, pyramid, scheduler, generation tracking) to the production render path, replaces the frontend `<img>` element with a tile-based `<canvas>`, adds zoom/pan controls, and enables multi-layer compositing with 12 blend modes.

The three primary goals are:
1. 50–100ms preview response latency from parameter change to visible update
2. Zoom and pan support via tile pyramid levels
3. Multi-layer compositing approaching professional-grade blending

## Glossary

- **Tile_Renderer**: The backend subsystem responsible for computing, caching, and serving tile pixel data through the `tile://` custom protocol
- **Tile_Canvas**: The frontend `<canvas>`-based component that fetches and draws visible tiles, replacing the current `<img>`-based PreviewCanvas
- **Viewport_Controller**: The frontend subsystem that tracks zoom level, pan offset, and visible tile coordinates, and communicates viewport state to the backend
- **Compositor**: The backend subsystem that blends multiple layers bottom-to-top using blend modes, opacity, and masks to produce Composite-stage tiles
- **Layer_Panel**: The frontend UI component that displays the layer tree and allows reordering, visibility toggling, and property editing
- **TileCache**: The existing concurrent DashMap-based cache in `engine-tiles` that stores PixelTile entries keyed by TileKey (layer, coord, stage)
- **Tile_Protocol**: The Tauri custom protocol handler registered at `tile://` that serves encoded tile data to the frontend
- **Pyramid**: The multi-level mipmap structure where level 0 is full resolution and each subsequent level is 2× downsampled via box filter
- **Viewport**: The rectangular region of the document currently visible on screen, defined by zoom level, pan offset, and canvas dimensions
- **CacheStage**: The three lifecycle stages of a tile: Raw (source pixels), Processed (after filters/masks), Composite (after blending with layers below)
- **Generation_Tracker**: The existing versioning system with per-document and per-layer counters used to detect and discard stale recomputation tasks

## Requirements

### Requirement 1: TileCache as Single Source of Truth

**User Story:** As a developer, I want TileCache to be the sole storage for pixel data, so that the render pipeline has one consistent source of truth without redundant state.

#### Acceptance Criteria

1. WHEN an image is loaded, THE Tile_Renderer SHALL decompose the image into Raw-stage PixelTile entries at pyramid level 0, tiling the image in 256×256 pixel blocks left-to-right top-to-bottom, and store each tile in TileCache with a TileKey whose stage is CacheStage::Raw
2. WHEN an image's dimensions are not evenly divisible by 256, THE Tile_Renderer SHALL produce edge tiles that are still 256×256 PixelTiles with the unused region zero-filled (transparent black)
3. THE Tile_Renderer SHALL NOT read pixel data from `AppState.image_data: Mutex<Option<ImageData>>`; all pixel reads SHALL resolve exclusively through TileCache lookups by TileKey
4. THE Tile_Renderer SHALL store tiles at all three CacheStage values (Raw, Processed, Composite) in TileCache using the existing TileKey addressing scheme of (layer: LayerId, coord: TileCoord, stage: CacheStage)
5. WHEN TileCache memory usage exceeds the 256 MB budget, THE Tile_Renderer SHALL evict least-recently-used entries while preserving any tile whose TileCoord overlaps the current Viewport pixel bounds at the active pyramid level
6. IF image decomposition into tiles fails due to an unsupported format or decode error, THEN THE Tile_Renderer SHALL leave TileCache unchanged for that layer and return an error indicating the cause of failure

### Requirement 2: Tile Protocol Handler

**User Story:** As a frontend developer, I want to fetch individual tiles by URL, so that the canvas can request only visible tiles and receive them independently.

#### Acceptance Criteria

1. THE Tile_Protocol SHALL register a Tauri custom protocol at the `tile://` scheme during application startup
2. WHEN the Tile_Protocol receives a request for a tile at URL path `tile://doc/{doc_id}/layer/{layer_id}/stage/{stage}/l/{level}/{x}/{y}` and the tile is present in the cache and not marked dirty, THE Tile_Protocol SHALL return the tile pixel data as exactly 262,144 bytes of raw RGBA8 (4 bytes per pixel, 256×256 pixels, row-major order) with a 200 status code and a Content-Type header of `application/octet-stream`
3. WHEN the Tile_Protocol receives a request for a tile that is not present in the cache or is marked dirty, THE Tile_Protocol SHALL schedule an Immediate-priority recomputation task via the Scheduler and return a 202 status code with an empty body
4. WHEN a pending tile has been recomputed, THE Tile_Renderer SHALL emit a `tile-ready` Tauri event whose payload contains the `doc_id`, `layer_id`, `stage`, `level`, `x`, and `y` fields identifying the completed tile
5. IF the Tile_Protocol receives a request with a tile coordinate that exceeds the tile grid bounds for the given level, or references a nonexistent layer_id, THEN THE Tile_Protocol SHALL return a 404 status code with a body containing an error message indicating which parameter was invalid
6. IF the Tile_Protocol receives a request with a doc_id that does not match any loaded document, THEN THE Tile_Protocol SHALL return a 404 status code with a body containing an error message indicating the document was not found
7. IF the Tile_Protocol receives a request with a malformed URL path that does not match the expected `tile://doc/{doc_id}/layer/{layer_id}/stage/{stage}/l/{level}/{x}/{y}` structure, THEN THE Tile_Protocol SHALL return a 400 status code with a body containing an error message indicating the URL format is invalid

### Requirement 3: Viewport-Driven Rendering

**User Story:** As a user, I want only the visible portion of my image to be rendered, so that editing remains responsive regardless of total document size.

#### Acceptance Criteria

1. WHEN the frontend calls `set_viewport` with zoom, x, y, width, and height parameters, THE Tile_Renderer SHALL compute the set of tile coordinates visible at the corresponding pyramid level by dividing the viewport rectangle (in document pixels) by the tile size (256) and clamping to grid bounds
2. THE Tile_Renderer SHALL compute the pyramid level as `max(0, floor(log2(1.0 / zoom)))` for the given zoom factor, clamped to the maximum available pyramid level for the document
3. WHEN `set_viewport` is called, THE Tile_Renderer SHALL schedule recomputation tasks only for tiles that intersect the visible rectangle and are either missing from cache or marked dirty; tiles that are already cached and not dirty SHALL NOT be recomputed
4. THE Tile_Renderer SHALL assign ViewportCenter priority to tiles whose center point is within the inner 50% of the viewport area, and ViewportEdge priority to tiles whose center point is in the outer 50% of the viewport area
5. THE Tile_Renderer SHALL assign Prefetch priority to tiles in the one-tile-wide ring adjacent to but outside the current viewport, scheduling them at lower priority than ViewportCenter and ViewportEdge tiles
6. WHEN `set_viewport` is called with a viewport that has changed from the previous call, THE Tile_Renderer SHALL cancel any pending recomputation tasks for tiles that are no longer in the viewport or its prefetch ring

### Requirement 4: Lazy Per-Tile Filter Application

**User Story:** As a user, I want filters to be applied only to visible tiles on demand, so that parameter changes produce visible feedback within 50–100ms.

#### Acceptance Criteria

1. WHEN a Processed-stage tile is requested and the corresponding Raw-stage tile exists in TileCache, THE Tile_Renderer SHALL apply the layer's filter stack using `apply_filter_to_tile` to produce the Processed tile and store it in TileCache at the Processed stage
2. THE Tile_Renderer SHALL apply filters to individual 256×256 tiles using the existing `apply_filter_to_tile` function, reading the 2px halo region from neighboring Raw-stage tiles in TileCache to maintain error diffusion correctness at tile boundaries
3. WHEN `update_filter` is called, THE Tile_Renderer SHALL invalidate Processed-stage and Composite-stage tiles for the affected layer using Generation_Tracker, without invalidating Raw-stage tiles
4. WHEN `update_filter` is called, THE Tile_Renderer SHALL NOT trigger a full-image re-render; only tiles whose coordinates fall within the current viewport bounds SHALL be scheduled for recomputation, and a `tile-ready` event SHALL be emitted for each recomputed tile
5. THE Tile_Renderer SHALL process a single 256×256 tile with one filter in 5ms or less per CPU core, enabling full viewport updates within 100ms for viewports containing up to 20 tiles (approximately 1280×1024 pixels at full resolution)
6. IF a Processed-stage tile is requested and the corresponding Raw-stage tile does not exist in TileCache, THEN THE Tile_Renderer SHALL not produce the Processed tile and SHALL schedule the Raw-stage tile for loading before retrying the filter application

### Requirement 5: Tile-Based Canvas Frontend

**User Story:** As a user, I want a smooth, responsive canvas that displays tiles as they become ready, so that I see incremental updates rather than waiting for the entire image.

#### Acceptance Criteria

1. THE Tile_Canvas SHALL replace the existing PreviewCanvas `<img>` element with an HTML5 `<canvas>` element that draws individual tile ImageBitmaps at their correct viewport positions based on the tile's (level, x, y) coordinates, the current zoom factor, and pan offset
2. WHEN the Viewport changes (zoom, pan, or resize), THE Tile_Canvas SHALL compute the set of visible tile coordinates, cancel any in-flight requests for tiles no longer visible, and request each newly-visible tile from the Tile_Protocol
3. WHEN a `tile-ready` event is received, THE Tile_Canvas SHALL fetch the updated tile from Tile_Protocol and redraw only the affected canvas region without full-canvas invalidation
4. THE Tile_Canvas SHALL use a Web Worker to fetch tile data from `tile://` URLs and decode the raw RGBA8 bytes (262,144 bytes per 256×256 tile) into ImageBitmap objects, ensuring the main thread frame time does not exceed 16ms (60fps)
5. WHILE tiles are pending (202 response), THE Tile_Canvas SHALL display the nearest available lower-resolution pyramid tile scaled to fill the corresponding position; IF no pyramid tile is available, THEN THE Tile_Canvas SHALL display a neutral gray placeholder at that position
6. IF a tile fetch fails with a non-recoverable error (status other than 200 or 202, or a decode failure), THEN THE Tile_Canvas SHALL display an error indicator at the affected tile position and retry the fetch up to 2 additional times with a 500ms delay between attempts

### Requirement 6: Zoom Support

**User Story:** As a user, I want to zoom in and out of my image with smooth transitions, so that I can inspect pixel-level detail or view the entire composition.

#### Acceptance Criteria

1. WHEN the user scrolls the mouse wheel over the canvas, THE Viewport_Controller SHALL adjust the zoom level by a multiplicative factor (2× per scroll detent) centered on the cursor position, preserving the document point under the cursor at the same screen position
2. THE Viewport_Controller SHALL clamp zoom levels to the range 1% (0.01) to 6400% (64.0) of the original image size; any zoom operation that would exceed these bounds SHALL be clamped to the nearest limit
3. WHILE zoom is at 100% (1.0), THE Tile_Canvas SHALL display tiles from pyramid level 0 with no interpolation, providing pixel-perfect accuracy matching the full-resolution export
4. WHEN zoom is less than 100%, THE Viewport_Controller SHALL select the pyramid level using `max(0, floor(log2(1.0 / zoom)))` clamped to the maximum available level, and THE Tile_Canvas SHALL draw tiles from that level scaled to fill the correct viewport area
5. WHEN zoom is greater than 100%, THE Viewport_Controller SHALL use pyramid level 0 tiles and THE Tile_Canvas SHALL scale them up using nearest-neighbor interpolation to preserve pixel edges
6. THE Viewport_Controller SHALL provide a zoom indicator displaying the current zoom percentage (rounded to the nearest integer) and allow the user to type an exact zoom value (validated to be within 1%–6400%) or select from presets (Fit, 25%, 50%, 100%, 200%, 400%)
7. WHEN the user selects the "Fit" zoom preset, THE Viewport_Controller SHALL calculate the zoom factor as `min(canvasWidth / docWidth, canvasHeight / docHeight)` and center the document within the canvas

### Requirement 7: Pan Support

**User Story:** As a user, I want to pan across my image by dragging, so that I can navigate to any region of a large document while zoomed in.

#### Acceptance Criteria

1. WHEN the user presses and holds the middle mouse button, or holds the Space key and then presses the left mouse button, THE Viewport_Controller SHALL enter pan mode and change the cursor to a grab/hand icon; WHEN the user releases the initiating mouse button, THE Viewport_Controller SHALL exit pan mode and restore the previous cursor
2. WHILE the Viewport_Controller is in pan mode and the user drags, THE Viewport_Controller SHALL update the pan offset by the drag delta divided by the current zoom factor (delta_doc = delta_screen / zoom), translating the viewport in document coordinates
3. WHEN the user pans, THE Tile_Canvas SHALL request tiles for newly-visible regions and discard tiles that have moved entirely outside the visible area from the canvas draw list
4. THE Viewport_Controller SHALL constrain panning so that the viewport center cannot move more than 50% of the viewport's width beyond the document bounds horizontally, nor more than 50% of the viewport's height beyond the document bounds vertically
5. WHILE the user is panning, THE Tile_Canvas SHALL reposition already-fetched tiles within one animation frame (16ms or less) without waiting for new tile data

### Requirement 8: Multi-Layer Compositing

**User Story:** As a user, I want to see all visible layers blended together in real time, so that I can work with complex multi-layer compositions.

#### Acceptance Criteria

1. WHEN a Composite-stage tile is requested, THE Compositor SHALL blend all visible layers bottom-to-top at the requested tile coordinate using each layer's blend mode and opacity, operating in linear f32 RGBA color space, treating any layer whose bounds do not cover the requested tile coordinate as fully transparent (RGBA 0,0,0,0)
2. THE Compositor SHALL support all 12 existing BlendMode variants: Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, and Exclusion
3. WHEN a layer's visibility, opacity, or blend mode changes, THE Compositor SHALL invalidate Composite-stage tiles for that layer and all layers above it in document-order traversal across the entire layer tree regardless of group nesting
4. WHEN a layer has a MaskRef with enabled set to true, THE Compositor SHALL multiply the layer's alpha channel by the mask's luminance value at each pixel (or by 1.0 minus the mask luminance if MaskRef.inverted is true) before blending with layers below
5. THE Compositor SHALL process layer groups by compositing visible children within the group bottom-to-top first, then blending the group result into the parent composite using the group's own blend mode and opacity
6. IF no visible layers contribute content at a requested tile coordinate, THEN THE Compositor SHALL return a fully transparent tile (RGBA 0,0,0,0)
7. WHEN a layer or group has visible set to false, THE Compositor SHALL skip that layer or group and all its descendants entirely during composite blending

### Requirement 9: Layer Panel UI

**User Story:** As a user, I want a layer panel showing all layers as a tree with drag-and-drop reordering, so that I can organize and control my composition.

#### Acceptance Criteria

1. THE Layer_Panel SHALL display all layers and groups as a tree structure reflecting the document's `root: Vec<LayerNode>` hierarchy in bottom-to-top visual order (topmost layer at the top of the panel), showing each layer's name, and indenting children of Group nodes by one level
2. THE Layer_Panel SHALL display a thumbnail of 32×32 pixels for each layer, rendered from the highest pyramid level tile of that layer via the `tile://` protocol
3. WHEN the user drags a layer to a new position within the tree, THE Layer_Panel SHALL display a drop-position indicator at the target location and, on drop, invoke `reorder_layer` with the layer's ID, the target parent group (or null for root), and the target index to update the document tree and invalidate affected Composite-stage tiles
4. THE Layer_Panel SHALL provide per-layer controls for: a visibility toggle (eye icon), an opacity slider with range 0.0–1.0 in steps of 0.01 displaying the current value as a percentage (0–100%) with no decimal places, a blend mode dropdown listing the 12 BlendMode variants (Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion), and an editable layer name field accepting 1 to 64 characters
5. WHEN the user clicks the add-layer button, THE Layer_Panel SHALL invoke `add_layer` to create a new empty Raster layer inserted directly above the currently selected layer within the same parent; IF no layer is selected, the new layer SHALL be inserted at the top of the root list
6. IF a Tauri_IPC command invoked by the Layer_Panel (`reorder_layer`, `add_layer`, or `set_layer_props`) returns an error, THEN THE Layer_Panel SHALL display an error message indicating the failed operation and revert the panel UI to its state prior to the attempted action
7. WHEN the user edits a layer name and confirms (by pressing Enter or moving focus away), THE Layer_Panel SHALL invoke `set_layer_props` with the new name trimmed of leading and trailing whitespace; IF the trimmed name is empty, THEN THE Layer_Panel SHALL reject the edit and retain the previous name

### Requirement 10: Invalidation and Event Pipeline

**User Story:** As a developer, I want a clear invalidation pipeline from parameter mutation to frontend update, so that stale data is never displayed and updates propagate efficiently.

#### Acceptance Criteria

1. WHEN a filter parameter changes, THE Tile_Renderer SHALL increment the layer generation in Generation_Tracker, mark all Processed-stage tiles for that layer as dirty, and mark all Composite-stage tiles for that layer and all layers above it as dirty
2. WHEN a layer property (visibility, opacity, blend mode) changes, THE Tile_Renderer SHALL mark Composite-stage tiles dirty for the changed layer and all layers above it in the layer ordering, leaving Raw-stage and Processed-stage tiles unchanged
3. WHEN a layer is added, removed, or reordered, THE Tile_Renderer SHALL mark all Composite-stage tiles dirty for every layer in the document
4. WHEN a dirty tile within the current viewport is recomputed, THE Tile_Renderer SHALL emit a `tile-ready` event containing the tile's layer ID, tile coordinate, and cache stage within 100ms of the originating mutation for viewports containing 40 or fewer visible tiles
5. THE Tile_Renderer SHALL discard recomputation tasks whose document generation value or layer generation value does not match the current Generation_Tracker values, preventing stale computations from overwriting newer results
6. WHEN a `tile-ready` event is emitted, THE Tile_Renderer SHALL include sufficient tile identity information (layer ID, TileCoord, CacheStage) in the event payload for the frontend to determine which screen region to repaint

### Requirement 11: Performance Constraints

**User Story:** As a user, I want responsive editing even on large documents, so that the application remains usable with 8192×8192 images and multiple layers.

#### Acceptance Criteria

1. WHEN a filter parameter is changed on a document up to 8192×8192 pixels with a viewport showing up to 40 tiles and a filter stack of 4 or fewer filters, THE Tile_Renderer SHALL produce at least one updated visible tile within 100ms of the parameter change
2. THE Tile_Renderer SHALL apply a single filter pass to one 256×256 tile in 5ms or less per CPU core, measured as worst-case across all supported filter types (curves, levels, dither, glitch)
3. WHILE the user is scrolling or panning, THE Tile_Canvas SHALL maintain a frame rate of 60fps (no frame exceeding 16.67ms) at the 95th percentile by performing all tile fetching and decoding off the main thread
4. THE Tile_Renderer SHALL support documents up to 8192×8192 pixels (32×32 = 1024 tiles at level 0) without the TileCache exceeding the 256 MB budget for the viewport working set, defined as up to 40 visible tiles across 3 CacheStage values and all active layers
5. WHILE the user is actively adjusting a filter slider, THE Tile_Renderer SHALL prioritize viewport-visible tiles over off-screen tiles and SHALL produce the first updated viewport tile within 25ms of the parameter change, enabling the user to see progressive updates before all tiles are complete
