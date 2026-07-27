# Phase 3 Design: Filter Algorithms & Integration

## Architecture Overview

Phase 3 implements the actual filter processing logic that transforms pixels. The design focuses on:

1. **Per-filter algorithm modules**: Each filter (Curves, Levels, Dither, Glitch) is a separate module with clear responsibility
2. **Pipeline integration**: Filters are applied in sequence during tile generation (Raw → Processed pipeline)
3. **Performance optimization**: Algorithms tuned for per-tile processing (~256×256 pixels)
4. **Deterministic output**: Same input + seed produces same output (reproducible glitches)
5. **Graceful error handling**: Invalid params return errors, never panic

---

## Module Structure

### New Crate: `engine-filters` (alternative: extend engine-project)

**Option A**: New crate `/crates/engine-filters/` (recommended for Phase 3)
- Focused responsibility: only filter algorithms
- Independent testing and benchmarking
- Can be used by other projects

**Option B**: Extend `/crates/engine-project/src/` with submodules
- Simpler (fewer crates)
- Tighter integration with FilterInstance

**Recommendation**: **Option A** (separate crate for clarity, but can defer to later if Phase 3 gets complex)

**For now, extend engine-project with submodules**:

```
crates/engine-project/src/
├── filters/                    (NEW submodule)
│   ├── mod.rs                  — exports
│   ├── curves.rs               — CurvesFilter implementation
│   ├── levels.rs               — LevelsFilter implementation
│   ├── dither.rs               — DitherFilter implementation
│   ├── glitch.rs               — GlitchFilter implementation
│   └── apply.rs                — apply_filter_to_tile() dispatcher
├── filter.rs                   (UPDATED from Task 1)
│   ├── FilterInstance (existing)
│   ├── FilterKind (updated: add algorithm details)
│   ├── FilterParams (updated: add Curves, Levels, etc.)
│   └── apply_filter_to_tile() → calls filters/apply.rs
```

---

## Design Decisions

### 1. Why Per-Filter Modules?

Each filter has distinct algorithm, parameters, and testing needs. Separating into modules:
- Makes code organization clear
- Enables parallel work (one person per filter)
- Simplifies testing (unit test each filter independently)
- Allows easy addition of new filters later

### 2. Curve Interpolation: Catmull-Rom vs Bezier

**Catmull-Rom chosen** because:
- Control points are on the curve (WYSIWYG for user)
- No need for "handles" like Bezier
- Smooth and continuous
- Faster to compute (4-point polynomial)

**Implementation**:
```rust
fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    let a0 = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let a1 = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let a2 = -0.5 * p0 + 0.5 * p2;
    let a3 = p1;
    a0 * t3 + a1 * t2 + a2 * t + a3
}
```

### 3. Dither: Floyd-Steinberg Performance

Floyd-Steinberg is O(pixels) but has neighbor dependencies (can't parallelize easily). For a 256×256 tile:
- 65,536 pixels × ~20 operations per pixel = ~1.3M operations
- On modern CPU: ~50-100 μs (acceptable)

**Trade-off**: Slower than Ordered dithering, but better quality. User can choose algorithm.

### 4. Glitch Reproducibility

All glitch effects use deterministic PRNG seeded by:
- `seed` parameter (from FilterInstance)
- Tile coordinate (TileCoord)
- This ensures: same tile rendered twice = same output

**Implementation**:
```rust
let prng_state = seed ^ (coord.level as u64) ^ (coord.x as u64) ^ (coord.y as u64);
let mut rng = XorShift64::new(prng_state);
// rng.next() produces deterministic random values
```

### 5. Filter Stack Application Order

Filters applied in Vec order (0 → n-1). Output of filter i = input to filter i+1.

**Why this order?**
- User expects: first filter in list applies first (intuitive)
- Some filters may want to build on previous transforms (e.g., dither after curves)
- If user wants different order, they reorder in document (UI feature, Phase 4+)

### 6. Integration Point: Where Filters Run

**Tile generation pipeline**:
```
Raw stage
  ↓ [Raw pixels from raster layer]
Filters applied (our Phase 3 code)
  ↓ [Processed pixels]
Processed stage
  ↓ [Mask applied, if any]
Ready for composition
  ↓ [Blend with layers below]
Composite stage
```

**Hook point**: In `/crates/engine-tiles/src/generation.rs`, in the tile generation function:
- After raw pixel generation
- Before returning Processed tile
- Call `engine_project::filters::apply::apply_filter_stack(tile, layer, &document, cache)`

### 7. Error Handling in Filters

All filter functions return `Result<PixelTile, EngineError>`:
- Invalid params → Err
- Runtime errors (OOM, etc.) → Err
- Never panic (catch unwrap, log, return error)

If filter fails during tile generation:
- Log error (info/warn level)
- Return tile unchanged (best-effort)
- Don't block rendering

### 8. Performance Optimization Strategies

**Curves**:
- Pre-compute lookup table (256 entries) if curve updated
- Evaluate via LUT (1 lookup + interpolation = ~1 ns per pixel)

**Dither (Floyd-Steinberg)**:
- Use single-pass algorithm (no separate error map allocation)
- Quantize on-the-fly while diffusing

**Dither (Ordered)**:
- Use pre-computed Bayer matrices (4×4 or 8×8, stored as static const)
- Modulo arithmetic for matrix indexing (no branches)

**Glitch**:
- RGB Shift: simple offset, no allocation
- Block Displace: small PRNG state, minimal work

---

## Thread Safety & Concurrency

**Filter application is side-effect-free**:
- Input: `&PixelTile` (immutable), parameters (immutable)
- Output: new `PixelTile` (no mutation of input)
- No shared state, no locks needed

Worker threads can safely apply filters to different tiles concurrently (each thread has its own PixelTile).

---

## Testing Strategy

### Unit Tests (per-filter module)

**curves.rs**:
- Linear curve (identity)
- Inverse curve (1-x)
- S-curve (sigmoidal contrast boost)
- Custom multi-point curves
- Edge cases (all 0, all 1)

**levels.rs**:
- Identity (no remapping)
- Input remapping [0.2, 0.8] → [0, 1]
- Gamma correction (γ=2.0 brightens, γ=0.5 darkens)
- Output range remapping
- Clamping

**dither.rs**:
- Floyd-Steinberg: verify error distribution
- Ordered: verify pattern matches Bayer matrix
- Threshold: binary output
- Color depth: quantization levels correct

**glitch.rs**:
- RGB shift: verify channel offsets
- Block displacement: verify block movement
- Reproducibility: same seed + coord = same output

### Integration Tests

**filters/integration_test.rs**:
1. **Document + filter + rendering**:
   - Create document with layer
   - Add Curves filter via Tauri command
   - Verify tile regenerated with filter applied
   - Check output pixels are different from unfiltered

2. **Filter stack**:
   - Add 2 filters to same layer
   - Verify output = filter2(filter1(input))

3. **Performance**:
   - Measure per-tile latency for each filter
   - Verify <100 μs target (or document why slower)

### Regression Tests

- All Phase 1 tests still pass (51 tests)
- All Phase 2 tests still pass (46 tests)
- No changes to Phase 1/2 APIs

---

## Incremental Implementation Plan

### Iteration 1: Curves Filter
- Implement CurvesFilter struct + evaluate()
- Add apply_curves_to_tile()
- Unit tests
- Benchmark

### Iteration 2: Levels Filter
- Implement LevelsFilter + apply_levels_to_tile()
- Unit tests

### Iteration 3: Dither Filters
- Implement Floyd-Steinberg
- Implement Ordered (Bayer)
- Unit tests + benchmarks

### Iteration 4: Glitch Effects
- Implement RGB Shift
- Implement Block Displacement
- Unit tests

### Iteration 5: Integration
- Hook filters into tile generation
- Tauri commands (add_filter, update_filter_params, etc.)
- Integration tests
- Full end-to-end test: document → filter → rendering

### Iteration 6: Polish
- Benchmarks and performance tuning
- Additional tests (edge cases, stress tests)
- Documentation

---

## Performance Targets

| Filter | Per-Tile Latency | Status |
|--------|-----------------|--------|
| Curves | <5 μs | Target |
| Levels | <5 μs | Target |
| Dither (Floyd-Steinberg) | ~50 μs | Target (50-100 μs acceptable) |
| Dither (Ordered) | ~5 μs | Target |
| RGB Shift | ~10 μs | Target |
| Block Displacement | ~20 μs | Target |
| **Total (all filters)** | **<100 μs** | Target |

**Measurement**: Use `std::time::Instant` for microbenchmarks, `criterion` for detailed benchmarks.

---

## Integration with Phase 2 Document Model

**Phase 2 structures already support Phase 3**:
- FilterInstance carries algorithm + params (generic JSON)
- apply_filter_to_tile() function signature ready
- Invalidation events (LayerFilterChanged) already trigger cache updates

**Phase 3 extends Phase 2**:
- Implement actual `apply_filter_to_tile()`
- Parse FilterParams JSON into algorithm-specific structs
- Call filter-specific functions
- Return transformed tile

**No changes to Phase 2 API** (backward compatible).

---

## Extensibility for Phase 4+

**Future filters can be added by**:
1. Create new module (e.g., `blur.rs`)
2. Implement filter struct + apply function
3. Add to FilterKind enum
4. Add to apply_filter_stack() dispatcher
5. Add unit + integration tests

**No changes to core architecture needed**.

---

## References

- Phase 2 spec: FilterInstance model, apply_filter_to_tile() signature
- Phase 1 spec: Tile generation pipeline, cache invalidation
- Algorithm references:
  - Catmull-Rom: https://en.wikipedia.org/wiki/Centripetal_Catmull%E2%80%93Rom_spline
  - Floyd-Steinberg: https://en.wikipedia.org/wiki/Floyd%E2%80%93Steinberg_dithering
  - Bayer matrices: https://en.wikipedia.org/wiki/Ordered_dithering

