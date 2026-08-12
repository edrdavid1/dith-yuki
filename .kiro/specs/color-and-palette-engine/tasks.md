# Implementation Plan: Color and Palette Engine

## Overview

This plan implements the `engine-color` crate (Oklab conversions, KD-tree, palette management, threshold maps), the `engine-io` sandbox module, and the palette-aware filter pipeline in `engine-project`. Tasks are ordered so foundational leaf crates are built first, then integrated upward through the dependency graph. Each task builds incrementally on prior work with no orphaned code.

## Tasks

- [x] 1. Set up engine-color crate structure and dependencies
  - [x] 1.1 Configure engine-color Cargo.toml and module skeleton
    - Add dependencies: `engine-core`, `engine-io`, `serde`, `thiserror`, `dashmap`, `png` (for threshold maps)
    - Replace placeholder `pub mod todo` in `lib.rs` with `pub mod oklab`, `pub mod kdtree`, `pub mod palette`, `pub mod palette_cache`, `pub mod threshold_map`
    - Create empty module files: `oklab.rs`, `kdtree.rs`, `palette/mod.rs`, `palette_cache.rs`, `threshold_map.rs`
    - Add `proptest` to `[dev-dependencies]`
    - _Requirements: 12.1, 12.2, 12.5, 12.7_

  - [x] 1.2 Configure engine-io sandbox module
    - Add `thiserror` and `dirs` (home directory lookup) to engine-io `Cargo.toml`
    - Replace placeholder `pub mod todo` in `engine-io/src/lib.rs` with `pub mod sandbox`
    - Create `engine-io/src/sandbox.rs` with `SandboxError` enum and `resolve_user_path` function stub
    - Add `proptest` to `[dev-dependencies]`
    - _Requirements: 12.3, 12.6_

- [x] 2. Implement Oklab color space conversions
  - [x] 2.1 Implement `linear_to_oklab` and `oklab_to_linear` in `engine-color/src/oklab.rs`
    - Define `LinRgb` and `Oklab` structs with Copy/Clone/Debug/PartialEq
    - Implement forward conversion: NaN/Inf sanitization → clamp [0,1] → LMS matrix → cube root → Oklab matrix
    - Implement inverse conversion: NaN/Inf sanitization → inverse Oklab matrix → cube → inverse LMS matrix → clamp [0,1]
    - Implement `oklab_dist_sq` squared Euclidean distance function
    - Include source-level comments about sRGB/Rec.709 primaries assumption
    - Add unit tests for known reference colors (black, white, pure red/green/blue, mid-gray) and NaN/Inf handling
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5, 1.6_

  - [ ]* 2.2 Write property test for Oklab round-trip
    - **Property 1: Oklab Round-Trip**
    - Generate random f32 triplets in [0.0, 1.0], verify `oklab_to_linear(linear_to_oklab(rgb))` within 1e-5 tolerance
    - **Validates: Requirements 1.1, 1.2, 1.3**

- [x] 3. Implement sandbox path validation
  - [x] 3.1 Implement `resolve_user_path` in `engine-io/src/sandbox.rs`
    - Define `SandboxError` enum with variants: `BadExtension`, `OutsideHome`, `NotFound`, `NoHome`
    - Implement extension check (case-insensitive ASCII comparison)
    - Implement path canonicalization and home directory containment check
    - Use `dirs::home_dir()` for platform home directory lookup
    - Add unit tests for valid paths, `..` escape, wrong extension, non-existent file, no home directory scenarios
    - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5, 10.6, 10.7_

  - [ ]* 3.2 Write property test for sandbox validation
    - **Property 17: Sandbox Validation Correctness**
    - Generate random path strings with valid/invalid extensions and locations, verify extension check and home containment invariants
    - **Validates: Requirements 10.2, 10.4, 10.5**

- [x] 4. Checkpoint - Ensure engine-color and engine-io compile
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Implement KD-tree module
  - [x] 5.1 Implement KD-tree construction and nearest-neighbor search in `engine-color/src/kdtree.rs`
    - Define `KdTree` struct with `KdNode` enum (Leaf/Split)
    - Implement `KdTree::build(colors: &[Oklab]) -> Option<Self>` with recursive median split
    - Implement `KdTree::nearest(query: Oklab) -> usize` with pruning and tie-breaking (lowest index)
    - Add unit tests: empty palette → None, single color → always 0, equidistant tie-breaking
    - _Requirements: 6.5, 6.6, 6.7_

  - [ ]* 5.2 Write property test for KD-tree nearest vs brute-force
    - **Property 9: KD-Tree Nearest Matches Brute-Force**
    - Generate random palette (2–100 Oklab colors) and random query point, verify KD-tree result matches linear scan
    - **Validates: Requirements 6.5, 6.6**

- [x] 6. Implement palette module (core struct, sRGB conversion, import/export)
  - [x] 6.1 Implement Palette struct, LinearColor, sRGB conversion functions, and PaletteError
    - Create `engine-color/src/palette/mod.rs` with `Palette`, `LinearColor`, `PaletteId`, `PaletteFormat` types
    - Implement `srgb_to_linear(u8) -> f32` and `linear_to_srgb(f32) -> u8` with proper gamma curves
    - Define `PaletteError` enum with all variants from design
    - Add `pub mod formats` and `pub mod generate` submodule declarations
    - _Requirements: 2.1, 3.3, 4.2_

  - [x] 6.2 Implement palette format parsers (ASE, ACO, GPL, PAL, CSV, JSON)
    - Create `engine-color/src/palette/formats/mod.rs` as dispatcher
    - Implement `ase.rs`: binary big-endian parser for Adobe Swatch Exchange
    - Implement `aco.rs`: binary parser for Adobe Color (version 1/2)
    - Implement `gpl.rs`: text parser for GIMP Palette format
    - Implement `pal.rs`: RIFF container parser for Microsoft Palette
    - Implement `csv_json.rs`: text parsers for CSV and JSON array formats
    - Each parser returns `Vec<(u8, u8, u8)>` or descriptive error with offset/line number
    - Validate 1–65536 entry count per format
    - _Requirements: 3.1, 3.2, 3.4, 3.5, 3.6_

  - [x] 6.3 Implement palette format exporters (pretty-printers)
    - Add export function to each format module that takes `&[LinearColor]` → sRGB u8 → format bytes
    - GPL export includes palette name in header (truncated to 256 chars)
    - JSON export produces array of `{"r": N, "g": N, "b": N}` objects
    - Validate non-empty palette before export
    - Integrate sandbox validation for import paths via `engine_io::sandbox::resolve_user_path`
    - Implement top-level `import_palette` and `export_palette` API functions
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 4.6, 3.7, 10.8_

  - [ ]* 6.4 Write property test for export/import round-trip
    - **Property 6: Palette Format Export/Import Round-Trip**
    - Generate random palettes (1–100 colors), export to each format, re-import, verify ≤1 per-channel difference in u8 space
    - **Validates: Requirements 3.1, 3.3, 3.7, 3.8, 4.1**

  - [ ]* 6.5 Write property test for export clamping
    - **Property 7: Export Clamping Produces Valid sRGB**
    - Generate random f32 values including out-of-range, verify `linear_to_srgb` always returns 0–255 without panic
    - **Validates: Requirements 4.2**

- [x] 7. Implement palette generation (MedianCut, KMeans)
  - [x] 7.1 Implement palette generation in `engine-color/src/palette/generate.rs`
    - Implement `PaletteGenMethod` enum with `MedianCut` and `KMeans` variants
    - Implement `generate_palette(pixels: impl Iterator<Item = LinearColor>, target_count: u16, method: PaletteGenMethod) -> Result<Vec<LinearColor>, PaletteError>`
    - MedianCut: recursive bounding box split along longest axis at median, return bin means
    - KMeans: k-means++ initialization, iterate until max centroid movement < 1e-4 or 50 iterations
    - Skip fully transparent pixels, return error for empty input
    - If fewer unique colors than target, return only the unique colors found
    - Add unit tests: 2 colors from red+blue, convergence test, empty input error
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.6, 5.7_

  - [ ]* 7.2 Write property test for palette generation count bound
    - **Property 8: Palette Generation Count Bound**
    - Generate random pixel sets and target counts (2–256), verify output has ≤ min(target, unique_input_colors) and ≥ 1 color
    - **Validates: Requirements 5.1, 5.7**

- [x] 8. Implement PaletteKdCache
  - [x] 8.1 Implement `PaletteKdCache` in `engine-color/src/palette_cache.rs`
    - Define `PaletteKdCache` struct wrapping `DashMap<PaletteId, (u64, Arc<KdTree>)>`
    - Implement `new()`, `get_or_build(&self, palette: &Palette) -> Result<Arc<KdTree>, PaletteError>`, and `evict(&self, palette_id: PaletteId)`
    - Cache hit: return existing Arc when revision matches
    - Cache miss: convert palette colors to Oklab, build KdTree, insert, return Arc
    - Empty palette → return PaletteError::Empty
    - Last-writer-wins on concurrent builds (DashMap insert semantics)
    - Add unit tests: cache hit, cache miss, revision mismatch triggers rebuild, eviction
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.7, 6.8, 6.9_

  - [ ]* 8.2 Write property test for cache revision consistency
    - **Property 10: PaletteKdCache Revision Consistency**
    - Generate palette, verify cache returns correct tree for current revision, mutate palette, verify cache rebuilds
    - **Validates: Requirements 6.2, 6.3**

- [x] 9. Implement threshold map loading and caching
  - [x] 9.1 Implement `ThresholdMap` and `ThresholdMapCache` in `engine-color/src/threshold_map.rs`
    - Define `ThresholdMap` struct with `data: Vec<f32>`, `width: u32`, `height: u32`
    - Implement `ThresholdMap::sample(global_x, global_y) -> f32` with modulo wrapping
    - Define `ThresholdMapError` enum with all variants from design
    - Define `ThresholdMapCache` with `DashMap` keyed by `(PathBuf, SystemTime)`, max 64 entries with LRU eviction
    - Implement `get_or_load(path: &Path) -> Result<Arc<ThresholdMap>, ThresholdMapError>`
    - Validate via sandbox, load PNG, verify grayscale (1-bit/8-bit), verify dimensions ≤ 4096×4096, normalize to [0,1]
    - Add unit tests: non-grayscale rejection, oversized rejection, sample modulo correctness
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5, 9.6, 9.7_

  - [ ]* 9.2 Write property test for threshold map sampling
    - **Property 16: Threshold Map Sampling Correctness**
    - Generate random ThresholdMap dimensions and data, random coordinates, verify `sample(x, y) == data[(y % H) * W + (x % W)]`
    - **Validates: Requirements 9.3**

- [x] 10. Checkpoint - Ensure engine-color crate tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 11. Update Document model for full Palette entities
  - [x] 11.1 Extend Document with Palette management methods
    - Add `engine-color` dependency to `engine-project/Cargo.toml`
    - Change `Document.palettes` from `Vec<PaletteId>` to `Vec<Palette>` (using `engine_color::palette::Palette`)
    - Implement `Document::add_palette(name, colors) -> PaletteId` with unique ID assignment and revision=1
    - Implement `Document::modify_palette(id, colors) -> Result<(), EngineError>` with revision increment
    - Implement `Document::remove_palette(id) -> Result<(), EngineError>` with referential integrity check
    - Implement `Document::get_palette(id) -> Option<&Palette>`
    - Update `Serialize`/`Deserialize` implementations to include full palette data
    - Add `PaletteNotFound` and `PaletteInUse` variants to `EngineError`
    - Add unit tests for add/modify/remove lifecycle and referential integrity
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 2.6_

  - [ ]* 11.2 Write property test for palette ID uniqueness
    - **Property 2: Palette ID Uniqueness**
    - Generate random sequences of `add_palette` calls, verify all returned PaletteIds are unique and revisions start at 1
    - **Validates: Requirements 2.2**

  - [ ]* 11.3 Write property test for palette revision monotonicity
    - **Property 3: Palette Revision Monotonicity**
    - Generate random palette modifications, verify revision strictly increases after each modification
    - **Validates: Requirements 2.3**

  - [ ]* 11.4 Write property test for palette removal referential integrity
    - **Property 4: Palette Removal Referential Integrity**
    - Generate documents with random palettes and filter references, verify removal succeeds iff no references exist
    - **Validates: Requirements 2.4, 2.5**

  - [ ]* 11.5 Write property test for document palette serialization round-trip
    - **Property 5: Document Palette Serialization Round-Trip**
    - Generate documents with random palettes, serialize to JSON, deserialize, verify identical palette fields
    - **Validates: Requirements 2.6**

- [x] 12. Implement expanded Dither filter
  - [x] 12.1 Refactor FilterKind and FilterParams for Dither/PaletteQuantize separation
    - Add `PaletteQuantize` variant to `FilterKind` enum
    - Define `DitherMode` enum (Bayer, ThresholdMap, ErrorDiffusion) and `DiffusionKernel` enum
    - Update `FilterParams::Dither` to use new `DitherMode` and `color_depth` fields (replacing old `DitherAlgorithm`)
    - Add `FilterParams::PaletteQuantize { palette_id, diffusion }` variant
    - Update `FilterInstance::validate()` for new parameter structures
    - Update `FilterKind::Display` implementation
    - Fix any compile errors in existing code referencing old `DitherAlgorithm` enum
    - _Requirements: 11.1, 11.2, 11.3, 11.7_

  - [x] 12.2 Implement expanded Dither filter with all modes in `engine-project/src/filters/dither.rs`
    - Implement `DitherFilter::apply(tile, coord, mode, color_depth, threshold_cache) -> Result<PixelTile, EngineError>`
    - Bayer mode: precomputed 2×2, 4×4, 8×8 normalized matrices, global coordinate addressing
    - ThresholdMap mode: load via `ThresholdMapCache`, sample with global coords modulo map size
    - ErrorDiffusion mode: Floyd-Steinberg, Atkinson, JJN, Stucki kernels with left-to-right top-to-bottom scan
    - All modes: quantize to N levels per channel, preserve alpha, use global pixel coordinates for seamless tiling
    - Add unit tests for each mode and edge cases (color_depth validation)
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8, 7.9_

  - [ ]* 12.3 Write property test for dither seamless tiling
    - **Property 11: Dither Seamless Tiling**
    - Generate adjacent tile pairs with Bayer/ThresholdMap mode, verify threshold continuity at shared boundary via global coordinates
    - **Validates: Requirements 7.2, 7.9**

  - [ ]* 12.4 Write property test for dither output level membership
    - **Property 12: Dither Output Level Membership**
    - Generate random tiles with color_depth 1–8, verify all output channel values are valid quantization levels
    - **Validates: Requirements 7.4**

  - [ ]* 12.5 Write property test for alpha preservation
    - **Property 13: Alpha Preservation**
    - Generate random tiles with varying alpha, apply Dither, verify alpha channel unchanged
    - **Validates: Requirements 7.5, 8.7**

  - [ ]* 12.6 Write property test for dither determinism
    - **Property 14: Dither Determinism**
    - Generate random tile + params, apply twice, verify bitwise identical output
    - **Validates: Requirements 7.6**

- [x] 13. Implement PaletteQuantize filter
  - [x] 13.1 Implement `PaletteQuantizeFilter` in `engine-project/src/filters/palette_quantize.rs`
    - Create new file `palette_quantize.rs` in filters directory
    - Implement `PaletteQuantizeFilter::apply(tile, coord, palette, kdtree, diffusion) -> Result<PixelTile, EngineError>`
    - Nearest-color mode: convert pixel to Oklab → KD-tree lookup → write palette color in linear RGB
    - Error diffusion mode: Oklab error buffer, distribute via kernel, clamp L∈[0,1] a∈[-0.5,0.5] b∈[-0.5,0.5]
    - Preserve alpha unmodified
    - Every output pixel RGB must exactly match a palette entry (membership invariant)
    - Add to `filters/mod.rs` module declarations and re-exports
    - Add unit tests: nearest-only quantization, error diffusion, empty palette error, palette membership check
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5, 8.6, 8.7, 8.8, 8.9_

  - [ ]* 13.2 Write property test for PaletteQuantize output membership
    - **Property 15: PaletteQuantize Output Membership**
    - Generate random tiles and random palettes (2–50 colors), apply PaletteQuantize, verify every output pixel matches a palette color
    - **Validates: Requirements 8.2, 8.3, 8.8**

- [x] 14. Update filter dispatcher and wire everything together
  - [x] 14.1 Update `filters/apply.rs` dispatcher for new filter kinds
    - Update `apply_filter_to_tile` signature to accept `&PaletteKdCache`, `&ThresholdMapCache`, and `&Document` parameters
    - Add dispatch arm for `FilterParams::Dither` → call `DitherFilter::apply`
    - Add dispatch arm for `FilterParams::PaletteQuantize` → lookup palette from document, get KD-tree from cache, call `PaletteQuantizeFilter::apply`
    - Handle missing palette → return error, tile unchanged
    - Update existing Curves/Levels/Glitch dispatch arms to work with new signature
    - Update `filters/mod.rs` to re-export new types (`PaletteQuantizeFilter`, `DitherMode`, `DiffusionKernel`)
    - _Requirements: 11.4, 11.5, 11.6_

  - [x] 14.2 Add palette generation integration to engine-project
    - Implement a utility function that accepts a layer ID, target count, and method, iterates the layer's tiles to extract non-transparent pixels, calls `engine_color::palette::generate_palette`, and stores the result in the Document
    - Name format: `"{layer_name}_{method}"` truncated to 64 characters
    - Error if layer not found or has no non-transparent pixels
    - _Requirements: 5.1, 5.2, 5.5, 5.6_

- [x] 15. Checkpoint - Ensure full project compiles and unit tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 16. Integration tests
  - [ ]* 16.1 Write integration tests in `engine-project/tests/palette_integration.rs`
    - Test full pipeline: create document → add palette → create layer with PaletteQuantize filter → apply filter → verify output
    - Test multi-threaded PaletteKdCache usage (N threads, no panics)
    - Test invalidation: modify palette → verify cache rebuilds tree on next access
    - Test palette generation → export → re-import round-trip
    - _Requirements: 6.4, 6.8, 6.9, 8.5_

- [x] 17. Final checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation
- Property tests validate universal correctness properties from the design document
- Unit tests validate specific examples and edge cases
- The `engine-color` crate is built first as a leaf dependency, then wired into `engine-project`
- Existing `DitherAlgorithm` and `DitherFilter` in `dither.rs` will be refactored in task 12.1/12.2 to support the expanded modes

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1.1", "1.2"] },
    { "id": 1, "tasks": ["2.1", "3.1"] },
    { "id": 2, "tasks": ["2.2", "3.2", "5.1"] },
    { "id": 3, "tasks": ["5.2", "6.1"] },
    { "id": 4, "tasks": ["6.2", "6.3", "7.1", "9.1"] },
    { "id": 5, "tasks": ["6.4", "6.5", "7.2", "8.1", "9.2"] },
    { "id": 6, "tasks": ["8.2", "11.1"] },
    { "id": 7, "tasks": ["11.2", "11.3", "11.4", "11.5", "12.1"] },
    { "id": 8, "tasks": ["12.2", "13.1"] },
    { "id": 9, "tasks": ["12.3", "12.4", "12.5", "12.6", "13.2"] },
    { "id": 10, "tasks": ["14.1", "14.2"] },
    { "id": 11, "tasks": ["16.1"] }
  ]
}
```
