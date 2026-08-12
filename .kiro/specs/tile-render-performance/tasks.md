# Implementation Plan: Tile Render Performance

## Overview

This plan implements six categories of performance optimizations to the tile rendering pipeline: SIMD-accelerated pixel processing, bulk memory copies, LUT pre-computation, parallel multi-layer processing, pyramid-level rendering, and worker wake efficiency. Each optimization is verified immediately after implementation via property-based tests comparing against snapshot reference implementations.

The implementation language is Rust. The `wide` crate provides portable SIMD, `proptest` handles property-based testing, and `criterion` provides benchmarking. `rayon` (already a dependency) handles parallel processing.

## Tasks

- [x] 1. Wave 0 — Prerequisites and reference snapshots
  - [x] 1.1 Add `wide` and `proptest` dev-dependencies to `crates/engine-project/Cargo.toml`
    - Add `wide = "0.7"` to `[dependencies]` (needed at runtime for SIMD)
    - Add `proptest = "1"` to `[dev-dependencies]`
    - Add `criterion = { version = "0.5", features = ["html_reports"] }` to `[dev-dependencies]`
    - Add `[[bench]]` entries for `compositor_bench` and `filter_bench`
    - _Requirements: 1.1, 1.2, 1.3, 1.4_

  - [x] 1.2 Add `proptest` dev-dependency to `src-tauri/Cargo.toml` if not present, and add `wide` dependency to `engine-project`
    - Verify `proptest = "1.4"` already exists in `src-tauri` dev-dependencies (it does)
    - Add `rayon = "1.7"` to `engine-project` dependencies (needed for parallel filter apply)
    - _Requirements: 4.1_

  - [x] 1.3 Create reference snapshot functions for correctness comparison
    - In `crates/engine-project/src/compositor.rs`: create `#[cfg(test)] pub fn reference_blend_tile(...)` as a copy of current `blend_tile`
    - In `crates/engine-project/src/filters/levels.rs`: create `#[cfg(test)] pub fn reference_apply_to_tile(...)` as a copy of current `apply_to_tile`
    - In `crates/engine-project/src/filters/curves.rs`: create `#[cfg(test)] pub fn reference_apply_to_tile(...)` as a copy of current `apply_to_tile`
    - In `crates/engine-project/src/filters/dither.rs`: create `#[cfg(test)] pub fn reference_apply_floyd_steinberg(...)` as a copy of current `apply_floyd_steinberg`
    - In `src-tauri/src/tile_protocol.rs`: create `#[cfg(test)] pub fn reference_f32_tile_to_rgba8(...)` as a copy of current `f32_tile_to_rgba8`
    - In `src-tauri/src/tile_pipeline.rs`: create `#[cfg(test)] pub fn reference_copy_tile(...)` as a copy of current `copy_tile`
    - _Requirements: 8.1, 8.3_

  - [x] 1.4 Set up Criterion benchmark infrastructure in `crates/engine-project/benches/`
    - Create `crates/engine-project/benches/compositor_bench.rs` with `single_tile_no_filter` and `composite_5_layers` benchmarks (placeholder bodies)
    - Create `crates/engine-project/benches/filter_bench.rs` with `single_tile_levels` benchmark (placeholder body)
    - Create `crates/engine-tiles/benches/pipeline_bench.rs` with `viewport_20_tiles_5_layers` benchmark (placeholder body)
    - Add `[[bench]] name = "pipeline_bench" harness = false` to `crates/engine-tiles/Cargo.toml`
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

- [x] 2. Checkpoint — Verify project compiles with new dependencies
  - Ensure all tests pass, ask the user if questions arise.

- [x] 3. Wave 1 — Bulk copy and flat dither error map
  - [x] 3.1 Replace triple-nested copy loops with `copy_from_slice` in `src-tauri/src/tile_pipeline.rs`
    - Replace `copy_tile` function body: `dst.data.copy_from_slice(&src.data)` instead of triple-nested loop
    - _Requirements: 2.1, 2.2_

  - [x] 3.2 Replace triple-nested copy loops with `copy_from_slice` in `crates/engine-project/src/filters/apply.rs`
    - In `apply_filter_to_tile`: replace the initial source-to-result copy loop with `result.data.copy_from_slice(&tile.data)`
    - In `apply_single_filter` Placeholder arm: replace loop with `result.data.copy_from_slice(&tile.data)`
    - _Requirements: 2.2_

  - [x] 3.3 Replace triple-nested copy loops with `copy_from_slice` in `crates/engine-project/src/compositor.rs`
    - In `get_processed_tile`: already uses `copy_from_slice` — verify and leave as-is
    - In `apply_layer_mask` (3 clone sites): already uses `copy_from_slice` — verify and leave as-is
    - _Requirements: 2.3_

  - [x] 3.4 Implement flat dither error map in `crates/engine-project/src/filters/dither.rs`
    - Replace `vec![vec![[0.0; 4]; 260]; 260]` with `vec![0.0f32; 260 * 260 * 4]`
    - Use index arithmetic `(y * 260 + x) * 4 + c` for all error map accesses
    - Keep the reference implementation as `#[cfg(test)] fn reference_apply_floyd_steinberg` (from 1.3)
    - _Requirements: 7.1, 7.2_

  - [x]* 3.5 Write property test for bulk copy equivalence
    - **Property 5: Bulk Copy Equivalence**
    - Create `crates/engine-project/tests/copy_equivalence.rs`
    - Generate random PixelTile data via proptest, verify `copy_from_slice` produces bitwise-identical tile to reference triple-loop copy
    - **Validates: Requirements 2.1, 2.2, 2.3**

  - [x]* 3.6 Write property test for flat dither map equivalence
    - **Property 10: Flat Dither Map Equivalence**
    - Create `crates/engine-project/tests/dither_flat_map.rs`
    - Generate random PixelTile + random color_depth (1-8) via proptest
    - Compare flat-array Floyd-Steinberg output to reference vec-of-vec implementation, assert pixel-identical
    - **Validates: Requirements 7.1, 7.2**

- [x] 4. Checkpoint — Verify bulk copy and flat dither
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Wave 2 — LUT pre-computation
  - [x] 5.1 Add LUT field and `rebuild_lut` / `lut_lookup` methods to `LevelsFilter`
    - Add `lut: Box<[f32; 4096]>` field to `LevelsFilter` struct (or use `Vec<f32>` with len 4096)
    - Implement `rebuild_lut(&mut self)`: iterate 0..4096, compute `apply_to_value(i/4095.0)`, store
    - Implement `lut_lookup(&self, x: f32) -> f32`: clamp, index, linear interpolation between adjacent entries
    - Call `rebuild_lut()` from `new()` and add a `with_params(...)` constructor that rebuilds
    - _Requirements: 3.1, 3.3_

  - [x] 5.2 Add LUT field and `rebuild_lut` / `lut_lookup` methods to `CurvesFilter`
    - Add `lut: Box<[f32; 4096]>` field to `CurvesFilter` struct
    - Implement `rebuild_lut(&mut self)`: iterate 0..4096, compute `evaluate(i/4095.0)`, store
    - Implement `lut_lookup(&self, x: f32) -> f32`: clamp, index, linear interpolation
    - Call `rebuild_lut()` from `new()` and after `add_point()`
    - _Requirements: 3.2, 3.3_

  - [x] 5.3 Update `LevelsFilter::apply_to_tile` to use `lut_lookup` instead of `apply_to_value`
    - Replace `self.apply_to_value(val)` call in the per-pixel loop with `self.lut_lookup(val)`
    - Keep `apply_to_value` as a public method (used by rebuild_lut and tests)
    - _Requirements: 3.3_

  - [x] 5.4 Update `CurvesFilter::apply_to_tile` to use `lut_lookup` instead of `evaluate`
    - Replace `self.evaluate(val)` call in the per-pixel loop with `self.lut_lookup(val)`
    - Keep `evaluate` as a public method (used by rebuild_lut and tests)
    - _Requirements: 3.3_

  - [x]* 5.5 Write property test for LUT accuracy bound
    - **Property 6: LUT Accuracy Bound**
    - Create `crates/engine-project/tests/lut_accuracy.rs`
    - Generate random Levels params (input_black < input_white, gamma in 0.1..10.0) and random f32 values in [0,1]
    - Assert `|lut_lookup(x) - apply_to_value(x)| <= 1.0/65536.0`
    - Similarly for Curves: random control points, verify `|lut_lookup(x) - evaluate(x)| <= 1.0/65536.0`
    - **Validates: Requirements 3.4**

  - [x]* 5.6 Write property test for LUT Curves RGBA8 equivalence
    - **Property 3: LUT Curves Equivalence**
    - Create `crates/engine-project/tests/lut_curves_equivalence.rs`
    - Generate random PixelTile + random CurvesFilter (2-5 control points in [0,1])
    - Apply via LUT path and via analytical path, convert both to RGBA8, assert identical bytes
    - **Validates: Requirements 1.3, 1.5, 3.3**

- [x] 6. Checkpoint — Verify LUT pre-computation
  - Ensure all tests pass, ask the user if questions arise.

- [x] 7. Wave 3 — Worker wake and parallel processing
  - [x] 7.1 Implement `WorkerWake` struct with Condvar in `src-tauri/src/worker.rs`
    - Add `WorkerWake` struct with `Mutex<bool>` + `Condvar`
    - Implement `new()`, `notify_one()`, `wait()` as per design
    - _Requirements: 6.1, 6.2, 6.3_

  - [x] 7.2 Integrate `WorkerWake` into `AppState` and worker loop
    - Add `worker_wake: WorkerWake` field to `AppState` in `src-tauri/src/commands.rs`
    - In `tile_worker_loop`: replace `thread::park_timeout(1ms)` with `state.worker_wake.wait()`
    - In `Scheduler::enqueue` (or the call site that enqueues): call `state.worker_wake.notify_one()`
    - _Requirements: 6.1, 6.2, 6.3_

  - [x] 7.3 Implement parallel `ensure_processed_tiles_fresh` in `src-tauri/src/tile_pipeline.rs`
    - Collect all visible leaf layers needing recomputation into a Vec
    - If count <= 1: compute inline (no overhead)
    - If count > 1: use `rayon::scope` to compute Processed tiles in parallel
    - Synchronize before returning (rayon::scope blocks until all spawns finish)
    - _Requirements: 4.1, 4.2, 4.3_

  - [x]* 7.4 Write property test for parallel composite equivalence
    - **Property 7: Parallel Composite Equivalence**
    - Create `src-tauri/tests/parallel_composite.rs`
    - Generate random document with 2-5 layers, random tile data, random filter configs
    - Run sequential `ensure_processed_tiles_fresh` + `composite_tile` vs parallel version
    - Assert byte-identical Composite tile output
    - **Validates: Requirements 4.1, 4.2**

- [x] 8. Checkpoint — Verify worker wake and parallelism
  - Ensure all tests pass, ask the user if questions arise.

- [x] 9. Wave 4 — SIMD acceleration
  - [x] 9.1 Create SIMD module at `crates/engine-project/src/simd.rs`
    - Add `pub mod simd;` to `crates/engine-project/src/lib.rs`
    - Implement `blend_row_simd(dst, src, mode, opacity)` using `wide::f32x4`
    - Implement scalar fallback `blend_row_scalar(dst, src, mode, opacity)` with identical signature
    - Process rows of 256 pixels (1024 f32s) in chunks of 4 (one pixel = 4 channels)
    - _Requirements: 1.1, 1.5_

  - [x] 9.2 Implement SIMD `f32_to_rgba8_row_simd` in `crates/engine-project/src/simd.rs`
    - Clamp [0,1], multiply by 255.0, add 0.5, truncate to u8
    - Process 4 f32 values → 4 u8 values per iteration using `wide::f32x4`
    - Implement scalar fallback `f32_to_rgba8_row_scalar` with identical signature
    - _Requirements: 1.4, 1.5_

  - [x] 9.3 Implement SIMD `levels_row_simd` in `crates/engine-project/src/simd.rs`
    - Read f32 values, compute LUT index, perform linear interpolation between adjacent entries
    - Uses the LUT from Wave 2's `LevelsFilter`
    - Implement scalar fallback `levels_row_scalar` with identical signature
    - _Requirements: 1.2, 1.5_

  - [x] 9.4 Integrate SIMD `blend_row_simd` into `compositor.rs` `blend_tile` function
    - Replace per-pixel loop with row-based processing: for each row in main region, call `blend_row_simd`
    - Extract row slices from `dst.data` and `src.data` using index arithmetic
    - _Requirements: 1.1_

  - [x] 9.5 Integrate SIMD `f32_to_rgba8_row_simd` into `tile_protocol.rs` `f32_tile_to_rgba8`
    - Replace per-pixel loop with row-based processing: for each row, call `f32_to_rgba8_row_simd`
    - Output directly into pre-allocated `Vec<u8>` buffer
    - _Requirements: 1.4_

  - [x] 9.6 Integrate SIMD `levels_row_simd` into `LevelsFilter::apply_to_tile`
    - Replace per-pixel `lut_lookup` calls with row-based SIMD LUT application
    - Process RGB channels with SIMD, copy alpha unchanged
    - _Requirements: 1.2_

  - [x]* 9.7 Write property test for SIMD blend equivalence
    - **Property 1: SIMD Blend Equivalence**
    - Create `crates/engine-project/tests/simd_equivalence.rs`
    - Generate two random PixelTiles, random BlendMode, random opacity in [0.0, 1.0]
    - Assert `blend_row_simd` output == `reference_blend_tile` output (byte-identical f32)
    - **Validates: Requirements 1.1, 1.5**

  - [x]* 9.8 Write property test for SIMD levels equivalence
    - **Property 2: SIMD Levels Equivalence**
    - In `crates/engine-project/tests/simd_equivalence.rs`
    - Generate random PixelTile + random LevelsFilter params
    - Assert SIMD levels output == reference levels output (byte-identical f32)
    - **Validates: Requirements 1.2, 1.5**

  - [x]* 9.9 Write property test for SIMD f32-to-RGBA8 equivalence
    - **Property 4: SIMD f32-to-RGBA8 Equivalence**
    - In `crates/engine-project/tests/simd_equivalence.rs`
    - Generate random PixelTile (including values outside [0,1] for clamping)
    - Assert SIMD f32_to_rgba8 output == reference scalar output (byte-identical u8)
    - **Validates: Requirements 1.4, 1.5**

- [x] 10. Checkpoint — Verify SIMD acceleration
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Wave 5 — Pyramid levels
  - [x] 11.1 Enable `computePyramidLevel` on frontend in `frontend/src/components/TileCanvas.tsx`
    - Remove the forced `return 0` override in `computePyramidLevel`
    - Implement: `if (zoom >= 1.0) return 0; const level = Math.max(0, Math.floor(Math.log2(1.0 / zoom))); const maxLevel = Math.floor(Math.log2(Math.max(docWidth, docHeight) / 256)); return Math.min(level, maxLevel);`
    - _Requirements: 5.1_

  - [x] 11.2 Implement on-demand pyramid tile generation in `crates/engine-tiles/src/pyramid.rs`
    - Implement `generate_pyramid_tile(level, coord, cache)` function
    - For level N, fetch 4 child tiles at level N-1 from cache
    - Downsample each child using 2×2 box filter (average 4 pixels → 1 pixel)
    - Return `None` if any source tile is missing from cache
    - _Requirements: 5.2_

  - [x] 11.3 Integrate pyramid tile serving into tile pipeline and protocol handler
    - In `compute_composite_tile` (or a new `compute_pyramid_tile`): when `key.coord.level > 0`, call `generate_pyramid_tile`
    - Cache the generated pyramid tile at the correct `TileKey` with level > 0
    - Ensure the tile protocol handler serves pyramid tiles (level field already parsed)
    - _Requirements: 5.2, 5.3, 5.4_

  - [x]* 11.4 Write property test for pyramid level formula
    - **Property 8: Pyramid Level Formula**
    - Create `crates/engine-tiles/tests/pyramid_properties.rs`
    - Generate random zoom values (0.01..2.0), verify computed level matches `max(0, floor(log2(1/z)))` for z < 1 and 0 for z >= 1
    - **Validates: Requirements 5.1**

  - [x]* 11.5 Write property test for box filter downsample correctness
    - **Property 9: Box Filter Downsample Correctness**
    - In `crates/engine-tiles/tests/pyramid_properties.rs`
    - Generate random parent tile, downsample, verify each output pixel == mean of 2×2 input neighborhood
    - **Validates: Requirements 5.2**

- [x] 12. Checkpoint — Verify pyramid levels
  - Ensure all tests pass, ask the user if questions arise.

- [x] 13. Wave 6 — Verification and benchmarks
  - [x] 13.1 Write end-to-end pipeline preservation property test
    - **Property 11: End-to-End Pipeline RGBA8 Preservation**
    - Create `src-tauri/tests/pipeline_preservation.rs`
    - Generate random document state (1-5 layers, random filters, random blend modes/opacities)
    - Run full optimized pipeline (filters → composite → RGBA8) and compare against reference sequential pipeline
    - Assert byte-identical RGBA8 output
    - **Validates: Requirements 8.1, 8.3**

  - [x] 13.2 Populate Criterion benchmark bodies with real workloads
    - `compositor_bench.rs`: benchmark `blend_tile` with 1 layer (no filter) and 5 layers
    - `filter_bench.rs`: benchmark `LevelsFilter::apply_to_tile` with LUT path
    - `pipeline_bench.rs`: benchmark 20-tile × 5-layer viewport refresh
    - Add `filter_param_to_first_tile` benchmark measuring parameter-change-to-RGBA8 latency
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_

  - [x] 13.3 Run full regression test suite and verify all existing tests pass
    - Run `cargo test --workspace` to confirm no regressions
    - Run `cargo test -p engine-project` for filter and compositor tests
    - Run `cargo test -p engine-tiles` for cache and scheduler tests
    - _Requirements: 8.4, 8.5_

  - [x] 13.4 Run Criterion benchmarks and document results
    - Run `cargo bench -p engine-project`
    - Run `cargo bench -p engine-tiles`
    - Verify performance targets: single tile < 1ms, levels tile < 3ms, 5-layer composite < 5ms, 20-tile viewport < 100ms, param change < 50ms
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_

- [x] 14. Final checkpoint — All tests pass, benchmarks meet targets
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation after each wave
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The `wide` crate provides portable SIMD across x86_64 (SSE2/AVX2) and aarch64 (NEON) without nightly Rust
- Reference functions are `#[cfg(test)]` only — zero production code overhead
- `rayon` is already a dependency of `engine-tiles`; adding it to `engine-project` enables parallel filter application

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2", "1.3", "1.4"] },
    { "id": 1, "tasks": ["3.1", "3.2", "3.3", "3.4"] },
    { "id": 2, "tasks": ["3.5", "3.6", "5.1", "5.2"] },
    { "id": 3, "tasks": ["5.3", "5.4"] },
    { "id": 4, "tasks": ["5.5", "5.6", "7.1", "7.3"] },
    { "id": 5, "tasks": ["7.2", "7.4", "9.1", "9.2", "9.3"] },
    { "id": 6, "tasks": ["9.4", "9.5", "9.6"] },
    { "id": 7, "tasks": ["9.7", "9.8", "9.9", "11.1", "11.2"] },
    { "id": 8, "tasks": ["11.3", "11.4", "11.5"] },
    { "id": 9, "tasks": ["13.1", "13.2", "13.3"] },
    { "id": 10, "tasks": ["13.4"] }
  ]
}
```
