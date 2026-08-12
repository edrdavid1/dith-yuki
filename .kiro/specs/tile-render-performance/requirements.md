# Requirements Document

## Introduction

Performance optimization of the existing tile rendering pipeline in Dither Yuki 2. The current pipeline exhibits rendering latency far above acceptable thresholds due to scalar pixel processing, excessive data copying, lack of pre-computation for expensive filter operations, sequential multi-layer processing, and disabled pyramid-level rendering. This specification defines requirements for eliminating these bottlenecks while preserving pixel-identical output and maintaining all existing API contracts.

## Glossary

- **Tile_Pipeline**: The end-to-end system that computes Raw, Processed, and Composite tiles for display, encompassing filters, compositor, protocol handler, and worker pool.
- **Compositor**: The module (`compositor.rs`) that blends visible layers bottom-to-top using Porter-Duff compositing and blend modes to produce Composite-stage tiles.
- **Filter_Engine**: The subsystem (`filters/apply.rs`, `filters/curves.rs`, `filters/levels.rs`, `filters/dither.rs`) that transforms Raw tiles into Processed tiles by applying filter stacks.
- **Protocol_Handler**: The tile:// custom protocol handler (`tile_protocol.rs`) that converts f32 PixelTile data to RGBA8 byte buffers for frontend consumption.
- **Worker_Pool**: The set of background threads (`worker.rs`) that dequeue RecomputeTask items from the Scheduler and execute tile computations.
- **PixelTile**: A 260×260×4-channel f32 data structure (~1.03 MB) representing a single tile with 2px halo region.
- **LUT**: Lookup Table — a pre-computed array mapping discretized input values to output values, replacing per-pixel mathematical evaluation.
- **SIMD**: Single Instruction, Multiple Data — CPU vector instructions (SSE2/AVX2/NEON) that process 4–8 f32 values simultaneously.
- **Pyramid_Level**: A mipmap tier where level N represents the image downsampled by factor 2^N, enabling reduced-resolution rendering when zoomed out.
- **Tile_Copy**: The operation of duplicating PixelTile data from one buffer to another, currently implemented via triple-nested at()/set() loops.
- **Condvar**: A condition variable synchronization primitive that allows threads to sleep until signaled, avoiding polling-based wake patterns.

## Requirements

### Requirement 1: SIMD-Accelerated Pixel Processing

**User Story:** As a user adjusting filters or viewing composited layers, I want pixel operations to complete within milliseconds, so that parameter changes feel instantaneous.

#### Acceptance Criteria

1. WHEN a Composite tile is computed, THE Compositor SHALL process pixel blending operations using SIMD intrinsics that operate on 4 or more f32 values per instruction.
2. WHEN a Levels filter is applied to a tile, THE Filter_Engine SHALL process pixel channel values using SIMD intrinsics that operate on 4 or more f32 values per instruction.
3. WHEN a Curves filter is applied to a tile, THE Filter_Engine SHALL process pixel channel values using SIMD intrinsics or LUT-based batch operations that operate on 4 or more pixels per iteration.
4. WHEN the Protocol_Handler converts a tile to RGBA8, THE Protocol_Handler SHALL convert f32 values to u8 using SIMD intrinsics that process 4 or more values per instruction.
5. THE Tile_Pipeline SHALL provide a portable fallback implementation for platforms without SIMD support, producing identical output to the SIMD path.

### Requirement 2: Bulk Tile Data Copy

**User Story:** As a user loading images or triggering filter recomputation, I want tile copying to use memory-bandwidth-optimal operations, so that data transfer overhead is minimized.

#### Acceptance Criteria

1. WHEN a PixelTile is copied, THE Tile_Pipeline SHALL use a single bulk memory copy operation (such as `copy_from_slice`) on the entire `data` buffer instead of per-element at()/set() loops.
2. WHEN the `apply_filter_to_tile` function copies a source tile to a result buffer, THE Filter_Engine SHALL use a single bulk memory copy operation instead of per-element iteration.
3. WHEN the Compositor copies a tile from cache into a working buffer, THE Compositor SHALL use a single bulk memory copy operation instead of per-element iteration.

### Requirement 3: LUT Pre-computation for Curves and Levels Filters

**User Story:** As a user dragging a Curves or Levels slider, I want each tile to render without repeated expensive math per pixel, so that interactive feedback is smooth.

#### Acceptance Criteria

1. WHEN a Levels filter is constructed or its parameters change, THE Filter_Engine SHALL pre-compute a LUT of size 4096 entries or greater mapping discretized input values to output values.
2. WHEN a Curves filter is constructed or its control points change, THE Filter_Engine SHALL pre-compute a LUT of size 4096 entries or greater mapping discretized input values to Catmull-Rom interpolated output values.
3. WHEN a LUT-based filter is applied to a tile, THE Filter_Engine SHALL retrieve output values via LUT index lookup with linear interpolation between adjacent entries, instead of evaluating powf() or Catmull-Rom per pixel.
4. THE Filter_Engine SHALL produce output values within ±1/65536 (approximately 1.5e-5) of the analytically computed values for all input values in the range [0.0, 1.0].

### Requirement 4: Parallel Multi-Layer Processed Tile Computation

**User Story:** As a user working with multiple layers, I want all layer filter computations to run concurrently, so that composite tile latency scales sub-linearly with layer count.

#### Acceptance Criteria

1. WHEN a Composite tile is computed and multiple visible layers require fresh Processed tiles, THE Tile_Pipeline SHALL compute Processed tiles for independent layers concurrently using available worker threads.
2. WHEN Processed tiles are computed in parallel, THE Tile_Pipeline SHALL synchronize completion of all required Processed tiles before beginning composite blending.
3. THE Tile_Pipeline SHALL limit concurrency to the number of available worker threads to avoid thread contention overhead.

### Requirement 5: Pyramid Level Rendering

**User Story:** As a user zoomed out to view the full image, I want the renderer to use reduced-resolution tiles, so that fewer pixels are processed and viewport refresh is fast.

#### Acceptance Criteria

1. WHEN the viewport zoom level is below 1.0, THE Tile_Pipeline SHALL compute and serve tiles at the appropriate pyramid level (level = max(0, floor(log2(1.0 / zoom)))).
2. WHEN a pyramid-level tile at level N is requested and not present in cache, THE Tile_Pipeline SHALL generate the pyramid tile by downsampling level 0 tiles using a 2×2 box filter per reduction step.
3. WHEN pyramid tiles are generated, THE Tile_Pipeline SHALL cache generated pyramid tiles in the TileCache at the corresponding TileKey with the correct level value.
4. WHEN the viewport zoom changes from below 1.0 to 1.0 or above, THE Tile_Pipeline SHALL transition to serving level 0 tiles without visual discontinuity.

### Requirement 6: Efficient Worker Thread Wake Mechanism

**User Story:** As a user making rapid parameter changes, I want worker threads to wake immediately when new tasks arrive, so that latency between scheduling and execution is minimized.

#### Acceptance Criteria

1. WHEN a task is enqueued into the Scheduler, THE Worker_Pool SHALL wake at least one sleeping worker thread within 10 microseconds using a signaling primitive (such as a Condvar or semaphore).
2. WHILE no tasks are available in the Scheduler, THE Worker_Pool SHALL put idle worker threads to sleep without polling, consuming no CPU cycles.
3. WHEN a worker thread completes a task and the Scheduler is empty, THE Worker_Pool SHALL put the worker thread to sleep using the signaling primitive instead of a fixed-duration sleep.

### Requirement 7: Cache-Friendly Dither Error Map

**User Story:** As a user applying Floyd-Steinberg dithering, I want the error diffusion computation to be memory-efficient, so that dither filter performance matches other filters.

#### Acceptance Criteria

1. WHEN Floyd-Steinberg dithering is applied, THE Filter_Engine SHALL store the error diffusion map in a single contiguous flat array with row-major layout instead of a vector of vectors.
2. WHEN Floyd-Steinberg error values are accessed, THE Filter_Engine SHALL use direct index arithmetic (row * width + column) to access error values without pointer indirection.

### Requirement 8: Correctness Preservation

**User Story:** As a user, I want all performance optimizations to produce visually identical output to the current implementation, so that image quality is never compromised by optimization.

#### Acceptance Criteria

1. THE Tile_Pipeline SHALL produce pixel-identical RGBA8 output bytes for all tile coordinates, layer configurations, and filter parameter combinations compared to the pre-optimization implementation.
2. THE Tile_Pipeline SHALL preserve the f32 intermediate precision pipeline without truncating to lower precision at any intermediate stage.
3. FOR ALL valid PixelTile inputs and filter parameter combinations, applying a filter and then comparing the output to the pre-optimization reference implementation SHALL yield per-channel differences of zero in the final RGBA8 output (round-trip correctness property).
4. THE Tile_Pipeline SHALL maintain all existing public API signatures and IPC protocol contracts unchanged.
5. THE Tile_Pipeline SHALL pass all existing unit tests and integration tests without modification.

### Requirement 9: Performance Targets

**User Story:** As a user, I want tile rendering to meet specific latency targets, so that the application feels responsive during interactive editing.

#### Acceptance Criteria

1. WHEN a single tile with one layer and no filters is rendered, THE Tile_Pipeline SHALL complete the computation in less than 1 millisecond on the reference hardware.
2. WHEN a single tile with one layer and a Levels filter is rendered, THE Tile_Pipeline SHALL complete the computation in less than 3 milliseconds on the reference hardware.
3. WHEN a single Composite tile with 5 visible layers is rendered, THE Tile_Pipeline SHALL complete the computation in less than 5 milliseconds on the reference hardware.
4. WHEN a full viewport refresh of 20 tiles across 5 layers is triggered, THE Tile_Pipeline SHALL complete all tile computations in less than 100 milliseconds on the reference hardware.
5. WHEN a filter parameter is changed, THE Tile_Pipeline SHALL produce the first visible updated tile within 50 milliseconds on the reference hardware.
