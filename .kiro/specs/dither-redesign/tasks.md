# Implementation Plan: Dither Redesign

## Overview

This plan implements the redesigned dithering system for Dither Yuki 2, replacing the legacy `DitherFilter` (color_depth 1–8 bits) with a rich parameter model (mode, levels, threshold_scale, pixel_size, color_mode, palette_id). The implementation follows a bottom-up approach: parameter model → ordered dithering → error diffusion with cross-tile propagation → palette integration → pipeline wiring → backward compatibility.

## Tasks

- [x] 1. Define new parameter model and validation
  - [x] 1.1 Create `DitherModeV2` enum in `crates/engine-project/src/filter.rs`
    - Add variants: `Bayer2x2`, `Bayer4x4`, `Bayer8x8`, `CustomPng { path: String }`, `FloydSteinberg`, `Atkinson`
    - Derive `Debug`, `Clone`, `Serialize`, `Deserialize`
    - Use `#[serde(rename_all = "snake_case")]` for JSON format
    - _Requirements: 1.1, 1.8_

  - [x] 1.2 Create `DitherColorMode` enum in `crates/engine-project/src/filter.rs`
    - Add variants: `Rgb`, `Grayscale`
    - Derive `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Serialize`, `Deserialize`
    - _Requirements: 1.5_

  - [x] 1.3 Create `DitherParamsV2` struct in `crates/engine-project/src/filter.rs`
    - Fields: `mode: DitherModeV2`, `levels: u16`, `threshold_scale: f32`, `pixel_size: u8`, `color_mode: DitherColorMode`, `palette_id: Option<PaletteId>`
    - Derive `Debug`, `Clone`, `Serialize`, `Deserialize`
    - Set defaults: `threshold_scale = 1.0`, `pixel_size = 1`, `color_mode = Rgb`, `palette_id = None`
    - _Requirements: 1.2, 1.3, 1.4, 1.5, 1.6, 1.7_

  - [x] 1.4 Implement `DitherParamsV2::validate()` method
    - Range checks: levels [2, 256], threshold_scale [0.1, 4.0], pixel_size [1, 32]
    - Non-empty path check for `CustomPng` mode
    - Return `EngineError::InvalidFilterParams` with descriptive message for violations
    - _Requirements: 1.8, 1.9, 1.10, 1.11_

  - [x] 1.5 Add `DitherV2(DitherParamsV2)` variant to `FilterParams` enum
    - Update `FilterInstance::validate()` to handle new variant by delegating to `DitherParamsV2::validate()`
    - _Requirements: 9.1_

  - [x] 1.6 [PBT] Write property test for parameter validation completeness
    - Generate arbitrary `DitherParamsV2` with proptest, verify valid params pass and out-of-range params fail
    - File: `crates/engine-project/tests/dither_validation_props.rs`
    - **Property 1: Parameter Validation Completeness**
    - **Validates: Requirement 1 (AC 1-11)**

- [x] 2. Serialization and round-trip
  - [x] 2.1 Implement Serde serialization for DitherModeV2
    - `CustomPng` serializes as `{"custom_png": {"path": "..."}}`
    - Other variants serialize as snake_case strings: `"bayer_2x2"`, `"floyd_steinberg"`, etc.
    - _Requirements: 11.1_

  - [x] 2.2 [PBT] Write property test for serde round-trip
    - Generate arbitrary valid `DitherParamsV2`, serialize to JSON, deserialize, verify equality
    - File: `crates/engine-project/tests/dither_serde_props.rs`
    - **Property 11: Serialization Round-Trip**
    - **Validates: Requirement 11 (AC 1-2)**

- [x] 3. Ordered dithering engine
  - [x] 3.1 Create `crates/engine-project/src/filters/dither_ordered.rs` module
    - Implement `apply_ordered(tile, coord, params, threshold_cache, palette_cache, document) -> Result<PixelTile, EngineError>`
    - Implement `get_threshold()` helper dispatching to Bayer 2x2/4x4/8x8 matrices using global coords and `rem_euclid`
    - Implement `get_threshold()` for CustomPng via `ThresholdMapCache::get_or_load()` + `ThresholdMap::sample()`
    - Apply threshold_scale: `offset = (threshold - 0.5) * threshold_scale`
    - Preserve alpha channel unchanged
    - _Requirements: 2.1, 2.2, 2.5, 2.6, 5.3, 8.1, 8.2, 8.3_

  - [x] 3.2 Implement pixel_size block logic in ordered dithering
    - Snap to block representative via `block_gx = (gx / ps) * ps`, `block_gy = (gy / ps) * ps`
    - Compute dithered color once per block using block's top-left global coordinate
    - Fill all pixels in the block with the same color
    - Align blocks to global coordinates for cross-tile consistency
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [x] 3.3 Implement color mode handling in ordered dithering
    - RGB mode: quantize R, G, B channels independently with same levels and threshold
    - Grayscale mode: convert to luminance `L = 0.2126*R + 0.7152*G + 0.0722*B`, dither single channel, write result to R=G=B
    - _Requirements: 5.1, 5.2_

  - [x] 3.4 Implement quantization dispatch in ordered dithering
    - When `palette_id` is set: apply threshold offset to pixel, then find nearest palette color via KD-tree in Oklab space
    - When `palette_id` is null: uniform quantization `round(value * (levels-1)) / (levels-1)` with offset applied
    - Clamp all output values to [0.0, 1.0]
    - _Requirements: 6.1, 6.2, 6.3, 6.5, 7.1, 7.2, 7.3, 7.4_

  - [x] 3.5 [PBT] Write property test for seamless tiling
    - Process uniform tile at two adjacent coordinates, verify global pixel produces same output regardless of tile boundary
    - File: `crates/engine-project/tests/dither_ordered_props.rs`
    - **Property 2: Ordered Dithering Seamless Tiling**
    - **Validates: Requirement 2 (AC 1-3)**

  - [x] 3.6 [PBT] Write property test for alpha preservation (ordered modes)
    - Generate random tiles, apply ordered dithering, verify alpha channel bitwise identical
    - File: `crates/engine-project/tests/dither_alpha_props.rs`
    - **Property 7: Alpha Preservation Invariant**
    - **Validates: Requirement 5 (AC 3)**

- [x] 4. Error diffusion engine with cross-tile propagation
  - [x] 4.1 Create `crates/engine-project/src/filters/dither_residuals.rs` module
    - Implement `ErrorResiduals` struct with `right: Vec<f32>` (256×2×3) and `bottom: Vec<f32>` (2×256×3)
    - Implement `ErrorResidualsStore` using DashMap keyed by `(u32, TileCoord)`
    - Implement methods: `get_left()`, `get_top()`, `store()`, `clear()`
    - _Requirements: 3.3, 3.4, 10.4_

  - [x] 4.2 Create `crates/engine-project/src/filters/dither_diffusion.rs` module
    - Implement `apply_error_diffusion(tile, coord, params, residuals_store, layer_id, palette_cache, document) -> Result<PixelTile, EngineError>`
    - Sequential scan left-to-right, top-to-bottom
    - Floyd-Steinberg kernel: (1,0) 7/16, (-1,1) 3/16, (0,1) 5/16, (1,1) 1/16
    - Atkinson kernel: 6 neighbors each 1/8
    - _Requirements: 3.1, 3.2_

  - [x] 4.3 Implement cross-tile error propagation
    - On start: seed error buffer from left neighbor's right residuals and top neighbor's bottom residuals
    - On finish: extract right-edge (2 cols) and bottom-edge (2 rows) residuals into `ErrorResiduals` struct, store in `ErrorResidualsStore`
    - _Requirements: 3.3, 3.4, 3.5_

  - [x] 4.4 Implement pixel_size blocking and color mode in error diffusion
    - Same block alignment logic as ordered dithering (global coordinate snapping)
    - RGB mode: independent channel error diffusion
    - Grayscale mode: luminance conversion, single-channel diffusion, write R=G=B
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 5.1, 5.2_

  - [x] 4.5 Implement palette quantization in error diffusion
    - Compute error in Oklab space between adjusted pixel and nearest palette color
    - Distribute Oklab error to neighbors via kernel weights
    - When palette_id is null: uniform quantization with error distribution
    - _Requirements: 6.4, 6.5_

  - [x] 4.6 Write integration test for cross-tile error diffusion
    - Create 2×2 tile grid, process in row-major order with residual propagation
    - Verify output matches single-block processing of same image
    - File: `crates/engine-project/tests/dither_cross_tile_test.rs`
    - _Requirements: 3.5, 3.6_

- [x] 5. Property tests for dithering invariants
  - [x] 5.1 [PBT] Write property test for pixel block uniformity
    - Generate random tiles with pixel_size 2–32, verify all pixels within each block have identical RGB
    - File: `crates/engine-project/tests/dither_pixel_size_props.rs`
    - **Property 5: Pixel Block Uniformity**
    - **Validates: Requirement 4 (AC 1-3)**

  - [x] 5.2 [PBT] Write property test for block alignment across tiles
    - Generate block spanning tile boundary, verify same color in both tiles
    - File: `crates/engine-project/tests/dither_pixel_size_props.rs`
    - **Property 6: Block Alignment Across Tiles**
    - **Validates: Requirement 4 (AC 4)**

  - [x] 5.3 [PBT] Write property test for grayscale output uniformity
    - Generate random tiles with color_mode Grayscale, verify R=G=B for all output pixels
    - File: `crates/engine-project/tests/dither_color_mode_props.rs`
    - **Property 8: Grayscale Output Uniformity**
    - **Validates: Requirement 5 (AC 2)**

  - [x] 5.4 [PBT] Write property test for palette membership invariant
    - Generate random tiles and palettes, apply dither with palette_id set, verify all output pixels match palette entries
    - File: `crates/engine-project/tests/dither_palette_props.rs`
    - **Property 9: Palette Membership Invariant**
    - **Validates: Requirement 6 (AC 1-4)**

  - [x] 5.5 [PBT] Write property test for uniform quantization level validity
    - Generate random tiles with palette_id null, verify output channel values are members of `{k/(L-1) : k=0..L-1}`
    - File: `crates/engine-project/tests/dither_levels_props.rs`
    - **Property 10: Uniform Quantization Level Validity**
    - **Validates: Requirement 7 (AC 1-4)**

  - [x] 5.6 [PBT] Write property test for determinism
    - Apply dither twice with same inputs, verify byte-identical output
    - File: `crates/engine-project/tests/dither_determinism_props.rs`
    - **Property 13: Determinism**
    - **Validates: Requirement 9 (AC 5)**

- [x] 6. Filter pipeline integration
  - [x] 6.1 Add `DitherV2` dispatch case to `apply_single_filter()` in `crates/engine-project/src/filters/apply.rs`
    - Route ordered modes to `apply_ordered()`
    - Route error diffusion modes to `apply_error_diffusion()`
    - Pass `ErrorResidualsStore` reference through call chain
    - _Requirements: 9.1, 9.2, 9.3_

  - [x] 6.2 Add `ErrorResidualsStore` to `AppState` in `src-tauri/src/commands.rs`
    - Initialize in app setup alongside other state
    - Clear on document mutation / filter parameter change
    - _Requirements: 9.4, 10.4_

  - [x] 6.3 Set `requires_full_row = true` for error diffusion filter instances
    - When mode is FloydSteinberg or Atkinson, mark filter as requiring sequential row processing
    - Scheduler respects this flag for tile ordering
    - _Requirements: 10.1, 10.2, 10.3_

  - [x] 6.4 Update `add_filter` / `update_filter` IPC commands
    - Accept DitherV2 parameters from frontend
    - Validate via `DitherParamsV2::validate()` before applying
    - On parameter change: clear ErrorResidualsStore for affected layer, invalidate Processed tiles
    - _Requirements: 9.1, 9.2, 12.1_

  - [x] 6.5 Register new modules in `crates/engine-project/src/filters/mod.rs`
    - Add `pub mod dither_ordered`, `pub mod dither_diffusion`, `pub mod dither_residuals`
    - Re-export public types
    - _Requirements: 9.1_

- [x] 7. Backward compatibility
  - [x] 7.1 Implement `From<(DitherMode, u8)> for DitherParamsV2` conversion
    - Map `color_depth` to `levels = 2^color_depth`
    - Map `Bayer{2}` → `Bayer2x2`, `Bayer{4}` → `Bayer4x4`, `Bayer{8}` → `Bayer8x8`
    - Map `ErrorDiffusion{FloydSteinberg}` → `FloydSteinberg`, `{Atkinson}` → `Atkinson`
    - Map `ThresholdMap{path}` → `CustomPng{path}`
    - Set defaults: `threshold_scale=1.0`, `pixel_size=1`, `color_mode=Rgb`, `palette_id=None`
    - _Requirements: 12.1, 12.2, 12.3, 12.4_

  - [x] 7.2 Update filter dispatcher to auto-migrate legacy `FilterParams::Dither`
    - On encounter: convert via `DitherParamsV2::from()`, dispatch as DitherV2
    - No data loss — legacy projects continue to render correctly
    - _Requirements: 12.1, 12.2, 12.3, 12.4_

  - [x] 7.3 [PBT] Write property test for legacy migration correctness
    - Generate all valid legacy (DitherMode, color_depth 1-8) combinations, verify conversion produces valid DitherParamsV2 with `levels = 2^color_depth`
    - File: `crates/engine-project/tests/dither_compat_props.rs`
    - **Property 12: Legacy Migration Correctness**
    - **Validates: Requirement 12 (AC 1-4)**

  - [x] 7.4 Write unit tests for specific legacy conversions
    - Verify: Bayer{2} → Bayer2x2 with levels=4 (2^2)
    - Verify: FloydSteinberg → FloydSteinberg with levels=16 (2^4)
    - Verify: ThresholdMap{"/path/to/map.png"} → CustomPng{"/path/to/map.png"}
    - _Requirements: 12.2, 12.3, 12.4_

- [x] 8. Frontend types update
  - [x] 8.1 Update TypeScript types in `frontend/src/types/` for DitherV2 parameters
    - Add `DitherModeV2` type: `"bayer_2x2" | "bayer_4x4" | "bayer_8x8" | { custom_png: { path: string } } | "floyd_steinberg" | "atkinson"`
    - Add `DitherColorMode` type: `"rgb" | "grayscale"`
    - Add `DitherParamsV2` interface with all fields
    - _Requirements: 9.1_

  - [x] 8.2 Update filter panel UI component for DitherV2 parameters
    - Add controls for: mode selector, levels slider (2-256), threshold_scale slider (0.1-4.0), pixel_size slider (1-32), color_mode toggle, palette selector
    - Wire to `update_filter` IPC with DitherV2 params
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

- [x] 9. Final integration and verification
  - [x] 9.1 Run full test suite (`cargo test --workspace`) and verify all tests pass
    - Existing tests must continue passing (no regressions)
    - New property tests and integration tests must pass
    - _Requirements: 9.5, 12.1_

  - [x] 9.2 Write end-to-end integration test
    - Create document → load image → add DitherV2 filter → verify processed tiles are correct
    - Test both ordered and error diffusion paths
    - Test palette-constrained and uniform quantization
    - File: `crates/engine-project/tests/dither_v2_integration.rs`
    - _Requirements: 9.1, 9.2, 9.3, 9.4_

## Notes

- Tasks marked [PBT] are property-based tests using `proptest` (already a dev-dependency)
- The legacy `FilterParams::Dither` variant is preserved for deserialization but auto-migrates at apply time
- `ErrorResidualsStore` uses DashMap for lock-free concurrent access
- Ordered dithering remains fully parallelizable — no cross-tile dependencies
- Error diffusion requires row-major processing order, signaled via `requires_full_row = true`
- The existing `PaletteKdCache` and `ThresholdMapCache` are reused without modification

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2", "1.3"] },
    { "id": 1, "tasks": ["1.4", "1.5"] },
    { "id": 2, "tasks": ["1.6", "2.1"] },
    { "id": 3, "tasks": ["2.2", "3.1"] },
    { "id": 4, "tasks": ["3.2", "3.3", "3.4", "4.1"] },
    { "id": 5, "tasks": ["3.5", "3.6", "4.2", "4.3"] },
    { "id": 6, "tasks": ["4.4", "4.5", "4.6"] },
    { "id": 7, "tasks": ["5.1", "5.2", "5.3", "5.4", "5.5", "5.6"] },
    { "id": 8, "tasks": ["6.1", "6.2", "6.3", "6.4", "6.5"] },
    { "id": 9, "tasks": ["7.1", "7.2"] },
    { "id": 10, "tasks": ["7.3", "7.4", "8.1", "8.2"] },
    { "id": 11, "tasks": ["9.1", "9.2"] }
  ]
}
```
