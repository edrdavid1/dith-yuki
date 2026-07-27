# Implementation Plan: Phase 3 — Filter Algorithms & Integration

## Task Dependency Graph

```json
{
  "waves": [
    {
      "wave": 1,
      "tasks": ["1", "2"]
    },
    {
      "wave": 2,
      "tasks": ["3", "4"],
      "dependsOn": ["1", "2"]
    },
    {
      "wave": 3,
      "tasks": ["5"],
      "dependsOn": ["1", "2", "3", "4"]
    },
    {
      "wave": 4,
      "tasks": ["6", "7"],
      "dependsOn": ["1", "2", "3", "4", "5"]
    },
    {
      "wave": 5,
      "tasks": ["8"],
      "dependsOn": ["1", "2", "3", "4", "5", "6", "7"]
    }
  ]
}
```

---

## Overview

Implement actual filter algorithms (Curves, Levels, Dither, Glitch) and integrate them into the tile generation pipeline. This phase realizes the FilterInstance model from Phase 2 with working pixel transformations.

**Phase 3 Scope**:
- Curves filter (tone curve adjustment)
- Levels filter (histogram adjustment)
- Dither algorithms (Floyd-Steinberg, Ordered/Bayer)
- Glitch effects (RGB Shift, Block Displacement)
- Integration into tile generation
- Tauri commands for filter manipulation
- Comprehensive testing

**Success Criteria**:
- All 4 filter algorithms implemented and working
- 32+ unit tests passing (8+ per algorithm)
- 3+ integration tests (document → filter → rendering)
- All Phase 1 & 2 tests still pass (no regressions)
- Per-tile latency <100 μs (performance acceptable)
- 0 clippy warnings in new code
- Tauri commands callable and working

---

## Tasks

### Wave 1: Foundation

- [ ] **1. Create filters module structure** (1 hour)
  - Create `/crates/engine-project/src/filters/` submodule
  - Files: `mod.rs`, `curves.rs`, `levels.rs`, `dither.rs`, `glitch.rs`, `apply.rs`
  - Update `/crates/engine-project/src/lib.rs` to export filters submodule
  - Stub all modules with trait definitions
  - Acceptance: Code compiles, all modules empty but exported
  - Reference: design.md §1–2

- [ ] **2. Implement Curves filter** (2 hours)
  - File: `/crates/engine-project/src/filters/curves.rs`
  - Struct: `CurvesFilter { curve: Vec<(f32, f32)>, channel: CurveChannel }`
  - Methods:
    - `new(channel) -> Self` — default linear curve
    - `add_point(input, output)` — add/update control point
    - `evaluate(x) -> f32` — Catmull-Rom interpolation between points
    - `apply_to_tile(tile: &PixelTile) -> Result<PixelTile, Error>`
  - Unit tests (6 tests):
    - Linear curve returns unchanged
    - Inverse curve flips black/white
    - S-curve boosts contrast
    - Custom multi-point curve
    - Edge cases (all 0, all 1)
    - Clamping
  - Acceptance: 6 tests pass, <5 μs per pixel, curves compile
  - Reference: requirements.md §1, design.md §2

### Wave 2: Core Filters

- [ ] **3. Implement Levels filter** (1.5 hours)
  - File: `/crates/engine-project/src/filters/levels.rs`
  - Struct: `LevelsFilter { input_black, input_white, gamma, output_black, output_white }`
  - Methods:
    - `new() -> Self` — default (no-op)
    - `apply_to_value(pixel: f32) -> f32` — levels transform (remap + gamma)
    - `apply_to_tile(tile: &PixelTile) -> Result<PixelTile, Error>`
  - Unit tests (6 tests):
    - Identity (no-op)
    - Input remapping [0.2, 0.8] → [0, 1]
    - Gamma 2.0 (brighten)
    - Gamma 0.5 (darken)
    - Output range remapping
    - Clamping
  - Acceptance: 6 tests pass, <5 μs per pixel
  - Reference: requirements.md §2, design.md §2

- [ ] **4. Implement Dither filters** (3 hours)
  - File: `/crates/engine-project/src/filters/dither.rs`
  - Enum: `DitherAlgorithm { FloydSteinberg, Ordered, Threshold }`
  - Struct: `DitherFilter { algorithm, color_depth }`
  - Methods:
    - `new(algo: DitherAlgorithm, color_depth: u8) -> Self`
    - `dither_floydsteinberg(tile: &PixelTile, color_depth) -> PixelTile` — error diffusion
    - `dither_ordered(tile: &PixelTile, color_depth, offset: TileCoord) -> PixelTile` — Bayer matrix
    - `dither_threshold(tile: &PixelTile, threshold) -> PixelTile` — binary
    - `apply_to_tile(tile: &PixelTile, coord: TileCoord) -> Result<PixelTile, Error>`
  - Unit tests (10 tests):
    - Floyd-Steinberg: error distribution correctness
    - Ordered: pattern matches Bayer matrix
    - Threshold: binary output
    - color_depth 1-8 quantization levels
    - Reproducibility (same input = same output)
    - Deterministic PRNG
  - Acceptance: 10 tests pass, Floyd-Steinberg ~50 μs, Ordered ~5 μs
  - Reference: requirements.md §3, design.md §2

### Wave 3: Advanced Effects

- [ ] **5. Implement Glitch effects** (2 hours)
  - File: `/crates/engine-project/src/filters/glitch.rs`
  - Enum: `GlitchType { RGBShift, BlockDisplace }`
  - Struct: `GlitchFilter { glitch_type, intensity, seed }`
  - Methods:
    - `new(glitch_type: GlitchType, intensity: f32, seed: u64) -> Self`
    - `apply_rgb_shift(tile: &PixelTile, intensity, seed, coord) -> PixelTile`
    - `apply_block_displace(tile: &PixelTile, intensity, seed, coord) -> PixelTile`
    - `apply_to_tile(tile: &PixelTile, coord: TileCoord) -> Result<PixelTile, Error>`
  - Unit tests (8 tests):
    - RGB shift produces chromatic aberration
    - Block displacement creates recognizable blocks
    - Intensity 0.0 = no-op
    - Intensity 1.0 = maximum effect
    - Reproducibility (seed ensures same output)
    - Deterministic PRNG XorShift64
    - Different coords produce different offsets
    - Offset clamping (stays within tile bounds)
  - Acceptance: 8 tests pass, ~10 μs for RGB shift, ~20 μs for block displacement
  - Reference: requirements.md §4, design.md §2

### Wave 4: Integration

- [ ] **6. Implement filter application dispatcher** (1.5 hours)
  - File: `/crates/engine-project/src/filters/apply.rs`
  - Function: `apply_filter_to_tile(tile: &PixelTile, filter: &FilterInstance, cache: &TileCache, coord: TileCoord) -> Result<PixelTile, Error>`
    - Pattern match on FilterKind (Curves, Levels, Dither, Glitch)
    - Parse FilterParams JSON into algorithm-specific structs
    - Call appropriate filter apply function
    - Handle errors gracefully (log, return unchanged tile)
  - Function: `apply_filter_stack(tile: &PixelTile, layer: &Layer, coord: TileCoord, cache: &TileCache) -> Result<PixelTile, Error>`
    - Iterate layer.filters in order
    - Apply each filter sequentially (output of i = input to i+1)
    - Skip disabled filters
    - Return final transformed tile
  - Unit tests (3 tests):
    - Single filter in stack
    - Multiple filters applied in order
    - Disabled filter skipped
  - Acceptance: 3 tests pass, filter dispatcher works
  - Reference: design.md §5

- [ ] **7. Integrate filters into tile generation** (2 hours)
  - File: `/crates/engine-tiles/src/generation.rs` (extend existing)
  - Hook point: After raw pixel generation, before Processed stage
  - Call `engine_project::filters::apply::apply_filter_stack()` in tile generation
  - Pass layer, tile, cache, coord
  - Handle Result (on error, log and return unchanged tile)
  - Update Phase 1 tests to pass FilterStack calls (or mock if needed)
  - Acceptance: Tile generation calls filter stack, Phase 1 tests still pass
  - Reference: design.md §6

### Wave 5: Polish & Verification

- [ ] **8. Integration tests, benchmarks, and verification** (2 hours)
  - File: `/crates/engine-project/tests/integration_test.rs` (extend)
  - Test 1: Add Curves filter via Tauri command, render tile, verify pixels changed
  - Test 2: Multiple filters in stack, verify order correct
  - Test 3: Disable filter, verify tile unchanged
  - Benchmarks:
    - Measure per-tile latency for each filter (using `std::time::Instant`)
    - Verify <100 μs total
  - Run full test suite:
    - `cargo test --all` — all 102+ tests pass
    - `cargo clippy --all -- -D warnings` — 0 warnings in new code
    - No Phase 1/2 regressions
  - Create `/PHASE_3_SUCCESS_REPORT.md` with deliverables, test counts, performance numbers
  - Acceptance: All tests pass, benchmarks documented, no regressions
  - Reference: requirements.md §8

---

## Acceptance Criteria (All Must Pass)

✅ **Implementation**:
- All 4 filter algorithms working (Curves, Levels, Dither, Glitch)
- Filters integrated into tile generation pipeline
- Tauri commands for filter manipulation (add, update, remove, enable/disable)

✅ **Testing**:
- 32+ unit tests (8+ per algorithm)
- 3+ integration tests (document → filter → rendering)
- All Phase 1 tests still pass (51 tests)
- All Phase 2 tests still pass (46 tests)
- **Total: 120+ tests passing**

✅ **Performance**:
- Per-tile latency <100 μs (measured)
- Curves <5 μs, Levels <5 μs, Dither ~50 μs, Glitch ~20 μs

✅ **Code Quality**:
- 0 compiler errors
- 0 clippy warnings (new code)
- All code formatted (`cargo fmt`)

✅ **Documentation**:
- Inline code docs (doc comments)
- PHASE_3_SUCCESS_REPORT.md with summary
- Algorithm references (papers/implementations)

---

## Notes

**Incremental approach**: Start with Curves (simplest), then Levels, then Dither (more complex), then Glitch (creative). Each iteration builds on previous, allows testing as you go.

**Performance-first mindset**: Use `std::time::Instant` in tests to catch performance regressions early. Measure per-pixel and per-tile latency.

**Error handling**: Never panic in filter code. Return Result, log errors, return unchanged tile on failure (best-effort rendering).

**Determinism**: All outputs must be reproducible. Curves/Levels are deterministic by design. Dither/Glitch use seeded PRNG (seed from FilterInstance + tile coord).

**References**:
- Phase 2 spec: FilterInstance model, apply_filter_to_tile() signature
- Phase 1 spec: Tile generation, cache invalidation
- Algorithm papers linked in design.md

