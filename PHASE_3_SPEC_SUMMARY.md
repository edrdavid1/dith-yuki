# Phase 3 Specification Summary — Filter Algorithms & Integration

**Date**: July 27, 2026  
**Status**: ✅ **SPEC READY — Ready for Implementation**

---

## What is Phase 3?

Phase 3 implements **actual pixel-transforming filters** that were stubbed in Phase 2. The FilterInstance model is ready; now we implement the algorithms that make filters work.

---

## The 4 Core Filters

### 1. **Curves Filter** 🎯 (Start Here)
- **What**: Tone curve adjustment (brighten, darken, increase contrast)
- **Algorithm**: Catmull-Rom spline interpolation
- **Per-Pixel Latency**: <5 μs
- **Example**: S-curve for contrast boost
- **Task Duration**: ~2 hours

### 2. **Levels Filter** ✨
- **What**: Histogram adjustment (remap input/output range, gamma)
- **Algorithm**: Simple math: remap input → apply gamma → remap output
- **Per-Pixel Latency**: <5 μs
- **Example**: Recover shadows/highlights
- **Task Duration**: ~1.5 hours

### 3. **Dither Filters** 🎨
- **What**: Color reduction with visual quality (reduce colors, maintain perception)
- **Algorithms**:
  - Floyd-Steinberg: Error diffusion (high quality, ~50 μs)
  - Ordered (Bayer): Pattern-based (fast, ~5 μs)
  - Threshold: Binary (extreme, <1 μs)
- **Use Case**: Retro graphics, color reduction effects
- **Task Duration**: ~3 hours

### 4. **Glitch Effects** 💔
- **What**: Creative/destructive effects (chromatic aberration, displacement)
- **Algorithms**:
  - RGB Shift: Separate R/G/B channels
  - Block Displacement: Shuffle tile blocks
- **Per-Pixel Latency**: ~10-20 μs
- **Use Case**: Artistic effects, data corruption simulation
- **Task Duration**: ~2 hours

---

## Implementation Structure

### New Code Location

```
crates/engine-project/src/filters/
├── mod.rs          — module exports
├── curves.rs       — CurvesFilter + apply_curves_to_tile()
├── levels.rs       — LevelsFilter + apply_levels_to_tile()
├── dither.rs       — DitherFilter + apply_dither_to_tile()
├── glitch.rs       — GlitchFilter + apply_glitch_to_tile()
└── apply.rs        — apply_filter_to_tile() dispatcher
```

### Hook into Phase 1 Tile Generation

```rust
// In engine-tiles/src/generation.rs (tile generation function)
let raw_tile = generate_raw_pixel_tile(...);

// NEW: Apply filter stack (Phase 3)
let filtered_tile = apply_filter_stack(&raw_tile, layer, coord, cache)?;

// Continue with rest of pipeline
let processed_tile = apply_mask(&filtered_tile, layer.mask)?;
// ... etc
```

---

## Testing Strategy

### Unit Tests (32+ total)
- **Curves**: 6 tests (linear, inverse, s-curve, custom, edge cases, clamping)
- **Levels**: 6 tests (identity, remapping, gamma, output, clamping, ...)
- **Dither**: 10 tests (Floyd-Steinberg, ordered, threshold, quantization, reproducibility)
- **Glitch**: 8 tests (RGB shift, block displacement, intensity, seed, clamping)
- **Dispatcher**: 3 tests (single filter, multiple, disabled)

### Integration Tests (3+ total)
- Document + Curves filter + tile rendering
- Multiple filters in stack
- Filter enable/disable toggle

### Performance Benchmarks
- Measure per-tile latency for each filter
- Target: <100 μs per tile (all filters combined)
- Use `std::time::Instant` or `criterion` crate

---

## Success Metrics

```
✅ Implementation: All 4 filters working
✅ Tests: 120+ tests passing (32 Phase 3 + 88 Phase 1/2)
✅ Performance: <100 μs per tile
✅ Quality: 0 clippy warnings, 0 regressions
```

---

## Timeline

- **Task 1-2** (Wave 1): Foundation + Curves (~3 hours)
- **Task 3-4** (Wave 2): Levels + Dither (~4.5 hours)
- **Task 5** (Wave 3): Glitch (~2 hours)
- **Task 6-7** (Wave 4): Integration (~3.5 hours)
- **Task 8** (Wave 5): Testing + Verification (~2 hours)
- **Total**: ~15 hours implementation

---

## What Comes After Phase 3?

### Phase 4: Undo/Redo
- History stack for document mutations
- Command replay

### Phase 5: Color Pipeline
- Color profile management
- RGB/Lab/CMYK conversions
- Professional color workflows

### Phase 6: Project Format
- Save/load documents to disk
- Layer flattening, export formats

---

## Files Ready for Review

1. **requirements.md** — Detailed acceptance criteria
2. **design.md** — Architecture & algorithm details
3. **tasks.md** — Task-by-task implementation plan

---

## Next Action

👉 **Start implementing Phase 3 by reading**:
1. `/crates/engine-project/src/filters/` (will be created)
2. `design.md` for algorithm pseudocode
3. `requirements.md` for test cases

**Or start coding directly** by creating the filter modules and implementing Curves filter first.

---

**Status**: ✅ **PHASE 3 SPEC COMPLETE — Ready to Build**

