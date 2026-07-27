# Phase 3 Requirements: Filter Algorithms & Integration

## Overview

Phase 3 implements actual filter algorithms and integrates them into the tile generation pipeline. This phase realizes the FilterInstance model from Phase 2 by providing working implementations of Curves, Levels, Dither, and Glitch effects.

**Context**: Phase 2 defines FilterInstance structure and Tauri commands. Phase 3 implements the `apply_filter_to_tile()` function with real algorithms that transform pixel data.

---

## Requirement 1: Curves Filter

**Requirement 1.1**: Implement `CurvesFilter` struct:
- `curve: Vec<(f32, f32)>` — control points (input, output) in range [0, 1]
- `channel: CurveChannel` — which channel to apply (Red, Green, Blue, Luminance, All)
- Methods:
  - `new(channel: CurveChannel) -> Self` — default linear curve
  - `add_point(input: f32, output: f32)` — add/update control point
  - `evaluate(x: f32) -> f32` — lookup output for input (cubic interpolation between points)

**Requirement 1.2**: Implement curve evaluation:
- Sort control points by input value (ascending)
- For input value x, find bracketing points (p0, p1)
- Use Catmull-Rom or cubic spline interpolation for smooth curves
- Clamp output to [0, 1]
- Performance: ~1-5 μs per pixel (acceptable for tiled rendering)

**Requirement 1.3**: Implement application to PixelTile:
- If channel is Luminance: convert RGBA → Lab, apply curve to L, convert back
- If channel is All: apply curve to each RGB channel independently
- If channel is specific (R/G/B): apply only to that channel
- Output: new PixelTile with transformed pixels

**Requirement 1.4**: Test cases:
- Linear curve (identity) returns unchanged tile
- Inverse curve (flip black/white) correctly inverts
- S-curve (contrast boost) increases contrast
- Custom point curve evaluates correctly

---

## Requirement 2: Levels Filter

**Requirement 2.1**: Implement `LevelsFilter` struct:
- `input_black: f32` (default 0.0)
- `input_white: f32` (default 1.0)
- `gamma: f32` (default 1.0)
- `output_black: f32` (default 0.0)
- `output_white: f32` (default 1.0)
- Methods:
  - `new() -> Self` — default (no-op)
  - `apply_to_value(pixel: f32) -> f32` — apply levels transformation

**Requirement 2.2**: Implement levels transformation:
```
1. Remap input [input_black, input_white] → [0, 1]
   remapped = (pixel - input_black) / (input_white - input_black)
   remapped = clamp(remapped, 0, 1)

2. Apply gamma correction
   gamma_corrected = remapped ^ (1 / gamma)

3. Remap output [0, 1] → [output_black, output_white]
   output = output_black + gamma_corrected * (output_white - output_black)
```

**Requirement 2.3**: Application to PixelTile:
- Apply to each RGB channel independently (or to Luminance if specified)
- Per-pixel transformation: ~1 μs per pixel
- Output: new PixelTile

**Requirement 2.4**: Test cases:
- Default levels (no-op) returns unchanged
- Input range [0.2, 0.8] correctly remaps
- Gamma 2.0 brightens; gamma 0.5 darkens
- Output range clamps correctly

---

## Requirement 3: Dither Algorithms

**Requirement 3.1**: Implement `DitherFilter` struct:
- `algorithm: DitherAlgorithm` — Floyd-Steinberg, Ordered (Bayer), Threshold
- `color_depth: u8` — target bits per channel (1-8, default 4)
- Methods:
  - `new(algo: DitherAlgorithm) -> Self`
  - `dither_tile(tile: &PixelTile, offset: TileCoord) -> PixelTile`

**Requirement 3.2**: Floyd-Steinberg dithering:
- Error diffusion algorithm
- Per-pixel: compute error from quantization
- Distribute error to neighbors (right: 7/16, below-left: 3/16, below: 5/16, below-right: 1/16)
- Performance: ~50 μs per tile (slower due to neighbor dependencies, but acceptable)

**Requirement 3.3**: Ordered (Bayer) dithering:
- Pre-computed Bayer matrix (4×4 or 8×8)
- Threshold each pixel against matrix value at (x % size, y % size)
- Much faster (~5 μs per tile) but less smooth than Floyd-Steinberg
- Use when dithering large areas or performance-critical paths

**Requirement 3.4**: Threshold dithering:
- Simplest: compare each pixel to threshold (0.5 by default)
- Output: pure black or white
- Useful for binary output or extreme dithering

**Requirement 3.5**: Test cases:
- Floyd-Steinberg produces smooth gradients with error distribution
- Ordered dithering shows Bayer pattern (expected, not wrong)
- Threshold produces clean black/white edges
- Different `color_depth` values produce correct quantization levels

---

## Requirement 4: Glitch Effects

**Requirement 4.1**: Implement `GlitchFilter` struct:
- `glitch_type: GlitchType` — PixelSort, RGBShift, BlockDisplace
- `intensity: f32` — strength of effect (0.0-1.0)
- `seed: u64` — random seed for reproducibility
- Methods:
  - `new(glitch_type: GlitchType, intensity: f32) -> Self`
  - `apply_to_tile(tile: &PixelTile, coord: TileCoord) -> PixelTile`

**Requirement 4.2**: Pixel Sorting glitch:
- Mark pixels to sort based on threshold (uses FilterInstance.requires_full_row = true)
- Sort pixels horizontally by brightness or color distance
- Non-tiled (requires full row), handled separately in tile generation
- Placeholder: can defer to Phase 3+ if complexity too high

**Requirement 4.3**: RGB Shift glitch:
- Shift R, G, B channels by small random offsets
- Offset depends on pixel position and deterministic PRNG
- Creates chromatic aberration effect
- Tiled (no row dependencies)
- Performance: ~10 μs per tile

**Requirement 4.4**: Block Displacement glitch:
- Divide tile into blocks, randomly displace each block
- Block size: 8×8 or 16×16 (configurable)
- Displacement: random offset in pixels (within tile bounds)
- Tiled
- Performance: ~20 μs per tile

**Requirement 4.5**: Test cases:
- RGB shift produces red/cyan fringes
- Block displacement creates recognizable displaced blocks
- Seed ensures reproducible results
- Intensity 0.0 returns unchanged; 1.0 maximum effect

---

## Requirement 5: Filter Pipeline Integration

**Requirement 5.1**: Integrate filters into tile generation:
- Extend `engine-tiles` tile generation to call filter application
- Hook point: after Raw pixel generation, before Processed stage
- Apply filter stack in order: each filter input = previous filter output
- Early exit if filter disabled

**Requirement 5.2**: Handle `requires_full_row` filters:
- Pixel Sort and some advanced glitches require full row context
- Separate pipeline: generate full layer row/column, apply filter, slice back into tiles
- Mark affected tiles as needing this path during invalidation

**Requirement 5.3**: Performance targets:
- Per-tile latency (256×256 tile):
  - Curves: <5 μs
  - Levels: <5 μs
  - Dither (Floyd-Steinberg): ~50 μs
  - Dither (Ordered): ~5 μs
  - RGB Shift: ~10 μs
  - Block Displace: ~20 μs
- Total pipeline: <100 μs per tile (acceptable for 30 fps, 16 tiles per frame)

**Requirement 5.4**: Integration with Phase 2 invalidation:
- Filter param change → mark Processed + Composite dirty
- New filter added → mark for generation with correct stage
- Disabled filter → skip in pipeline (no-op, tile passes through)

---

## Requirement 6: Tauri Commands for Filters

**Requirement 6.1**: Implement filter Tauri commands:
- `add_filter(doc_id: DocumentId, layer_id: LayerId, kind: String, params: JsonValue) -> Result<FilterInstanceId, Error>`
  - kind: "curves", "levels", "dither", "glitch"
  - params: algorithm-specific JSON (e.g., {"channel": "luminance"} for curves)
- `update_filter_params(doc_id, layer_id, filter_id, params) -> Result<(), Error>`
  - Update params, trigger invalidation, return immediately
- `remove_filter(doc_id, layer_id, filter_id) -> Result<(), Error>`
  - Delete filter from stack
- `set_filter_enabled(doc_id, layer_id, filter_id, enabled) -> Result<(), Error>`
  - Toggle without deletion

**Requirement 6.2**: All commands must:
- Validate filter ID exists and belongs to layer
- Call invalidation with `FilterChanged` event
- Increment layer generation
- Post recompute tasks to scheduler
- Return Ok(()) immediately

---

## Requirement 7: Error Handling

**Requirement 7.1**: Extend `EngineError` enum:
- `InvalidCurvePoint { reason: String }` — control point out of range
- `InvalidDitherAlgorithm { reason: String }` — unsupported algorithm
- `InvalidGlitchType { reason: String }` — unknown glitch type
- `FilterApplicationFailed { reason: String }` — runtime error in filter

**Requirement 7.2**: All filter code must:
- Validate inputs (curve points in [0,1], intensity in [0,1], etc.)
- Return Result types for fallible operations
- Never panic in filter code (catch and return error)

---

## Requirement 8: Testing

**Requirement 8.1**: Unit tests for each filter:
- Curves: linear, inverse, s-curve, custom points
- Levels: identity, remapping, gamma, output range
- Dither: Floyd-Steinberg correctness, ordered pattern, threshold
- Glitch: RGB shift produces shift, block displacement, reproducibility

**Requirement 8.2**: Integration tests:
- Apply filter to document via Tauri command
- Verify invalidation fired
- Verify scheduler queued tasks
- Render tile and verify output pixels changed

**Requirement 8.3**: Performance benchmarks:
- Measure per-tile latency for each filter
- Measure full pipeline (all filters in stack) latency
- Verify <100 μs per tile target

**Requirement 8.4**: Test coverage:
- 8+ unit tests per filter algorithm (32+ total)
- 3+ integration tests (document + filter + rendering)
- All Phase 1 & 2 tests still pass (no regressions)

---

## Requirement 9: Documentation

**Requirement 9.1**: Document filter algorithms:
- README or module-level doc explaining each filter
- Examples of before/after
- Performance characteristics
- Algorithm references (papers/implementations for curves, dithering)

**Requirement 9.2**: API documentation:
- Public function signatures with doc comments
- Example code for creating and applying filters
- Error handling patterns

---

## Success Criteria

1. **All 4 filter algorithms implemented and working**:
   - Curves (tone adjustment)
   - Levels (histogram adjustment)
   - Dither (color reduction via error diffusion or ordered)
   - Glitch (RGB shift + block displacement)

2. **Integration complete**:
   - Filters integrate into tile generation pipeline
   - Tauri commands callable and working
   - Invalidation correctly triggers re-renders

3. **Performance acceptable**:
   - Per-tile latency <100 μs (target met or close)
   - Full application end-to-end (document → filter → tile) <200 μs

4. **Tests passing**:
   - 32+ unit tests (8+ per algorithm)
   - 3+ integration tests
   - All Phase 1 & 2 tests still pass
   - 0 clippy warnings in new code

5. **No regressions**:
   - Phase 1 tile engine still works (51 tests pass)
   - Phase 2 document model still works (46 tests pass)
   - Total test count: 120+ tests passing

---

## Known Limitations & Future Work

### Phase 3 Scope
- Curves: basic spline interpolation (not advanced control point UI)
- Dither: Floyd-Steinberg and Ordered only (not threshold initially)
- Glitch: RGB shift and block displacement (pixel sort deferred)
- All filters: non-parametric UI (JSON params only, no sliders yet)

### Phase 4+
- Advanced filter UI (sliders, curve editor in frontend)
- Additional algorithms (color correction, blur, etc.)
- GPU acceleration (compute shaders for large filters)
- Filter presets and favorites (UI feature)

---

## References

- Phase 2 spec: Document model, FilterInstance structure
- Phase 1 spec: Tile generation, cache invalidation
- Algorithm references:
  - Curves: Catmull-Rom spline interpolation
  - Dither: Floyd-Steinberg (Jarvis-Judice-Ninke variant), Bayer matrices
  - Glitch: Creative coding references

