# Requirements: Phase 1 — Tile Engine

## Feature Overview

Implement the tile caching and pyramid downsampling system that forms the core of the Dither rendering engine. This enables efficient processing of large images through:
- Spatially partitioned tile-based storage
- Lazy pyramid generation
- LRU cache eviction with "dirty" marking (instant feedback without deletion)
- Parallel tile processing via work-stealing scheduler

## User Story

As a rendering engine, I want a tile-based cache with pyramid downsampling so that I can efficiently process large images, provide instant user feedback at coarse zoom levels, and scale to multiple processor cores.

## Acceptance Criteria

### Core Data Types (Requirement 1)

1. **TileKey**: Uniquely identifies a cached tile
   - Contains: `layer: LayerId`, `coord: TileCoord`, `stage: CacheStage`
   - Implements: `Clone, Copy, PartialEq, Eq, Hash, Debug`
   - Is hashable and usable as DashMap key

2. **TileCoord**: 3D coordinate in tile space
   - Contains: `level: MipLevel, x: u32, y: u32`
   - Implements: `Clone, Copy, PartialEq, Eq, Hash, Debug`
   - Level 0 = full resolution, Level 1 = 1:2 downsampled, etc.

3. **CacheStage**: Lifecycle stage of tile data
   - Variants: `Raw` (original pixels), `Processed` (after filters), `Composite` (after blending)
   - Prevents mixing incompatible data types

4. **PixelTile**: Container for pixel data
   - Stores: f32 RGBA in row-major layout
   - Size: (256 + 2×HALO)² pixels × 4 channels
   - Methods: `new()`, `at(x, y, channel)`, `set(x, y, channel, value)`

### Cache Behavior (Requirement 2)

1. **LRU Storage**: `TileCache` stores tiles by `TileKey`
   - Lookup is O(1) average case
   - Insert evicts least-recently-used tile when budget exceeded
   - Budget is configurable (bytes)

2. **Dirty Marking, Not Deletion**: When tile must be recomputed
   - Tile remains in cache with `dirty = true`
   - `generation` field holds stale value
   - On read, stale tile returned until recomputation finishes
   - New version inserted atomically (replaces slot)
   - *No visual artifacts*: stale data is better than missing data

3. **Concurrent Access**: Multiple threads read/write safely
   - DashMap for lock-free reads
   - `AtomicBool` for dirty flag
   - Timestamp tracking for LRU ordering

### Invalidation Semantics (Requirement 3)

1. **Selective Invalidation**: Only affected tiles marked dirty
   - `LayerRawChanged(layer, coords)` → mark Raw + Processed + cascade Composite
   - `LayerFilterChanged(layer)` → mark all Processed for layer + cascade Composite
   - `LayerPropsChanged(layer)` → cascade Composite only (opacity/blend/visibility)
   - `MaskChanged(layer, coords)` → mark Processed + cascade Composite

2. **Cascade Behavior**: Composite tiles cascade correctly
   - Marking Raw of layer L dirty → mark Composite for layer L and all layers ≥L
   - Reason: Composite tiles depend on all layers below

3. **Generation Tracking**: Two-level versioning
   - `document_gen`: Global counter (increments on any change)
   - `layer_gen[layer]`: Per-layer counter (increments on layer-specific change)
   - Task carries both values; checked at execution (discard if stale)

### Pyramid System (Requirement 4)

1. **Lazy Downsampling**: Coarse levels computed on demand
   - Level N = 1:2 box-filtered downsample of Level N-1
   - Stored as `Raw` tile of same layer at higher level
   - Coexists with Level 0 in same cache under different `TileCoord`

2. **Downsample Quality**: Box filter (simple average)
   - Each output pixel = average of 2×2 input pixels
   - Applied per channel (RGBA)
   - Deterministic (reproducible)

3. **Performance**: Downsample ≤5ms per 256×256 tile
   - Target: measure via criterion benchmarks
   - If exceeded: investigate parallelization

### Scheduler System (Requirement 5)

1. **Priority Queue**: 4-tier priority with SegQueue
   - `Immediate`: Coarse pyramid level of current viewport
   - `ViewportCenter`: Highest priority visible tiles
   - `ViewportEdge`: Lower priority visible tiles
   - `Prefetch`: Out-of-viewport tiles for smooth panning

2. **Work-Stealing**: Rayon thread pool processes in priority order
   - High-priority tasks dequeued before low-priority
   - Each worker thread tries Immediate → ViewportCenter → ... → Prefetch

3. **Task Abandonment**: Stale tasks discarded without recomputation
   - Before execution: check `task.generation == current_generation`
   - If mismatch: skip recomputation (user changed parameters)

### Testing Requirements (Requirement 6)

1. **Unit Tests**: Core functionality
   - TileKey construction and hashing
   - PixelTile creation and access
   - Cache insertion/get
   - Dirty marking
   - Generation counters
   - Each module has ≥1 test

2. **Integration Tests**: Multi-component interaction
   - Cache + Pyramid: insert Level 0, verify Level 1 downsamples correctly
   - Invalidation + Cascade: mark Raw dirty, verify Composite marked dirty
   - Scheduler: high-priority task dequeues before low-priority

3. **Benchmarks**: Performance verification via criterion
   - Downsample throughput (tiles/second)
   - Cache access latency (op/ns)
   - LRU eviction overhead

### Performance Targets (Requirement 7)

| Target | Threshold | Rationale |
|--------|-----------|-----------|
| Downsample latency | ≤5ms per 256×256 | NFR: "instant feedback" |
| Cache lookup | O(1) average | DashMap efficiency |
| Memory per tile | ~1.08 MB | (256+4)² × 4 channels × 4 bytes |
| Cache budget | <100 MB for NFR scenario | 5000×5000 image + 10-layer pyramid |

## Glossary

- **Tile**: 256×256 pixel block (constant TILE_SIZE)
- **Halo**: 2-pixel overlap for error diffusion filters (HALO = 2)
- **MipLevel**: Pyramid level (0 = full resolution, 1 = 1:2 downsampled, etc.)
- **Generation**: Version counter for invalidation tracking
- **Dirty**: Flag marking tile as stale (requires recomputation)
- **LRU**: Least-Recently-Used eviction policy
- **Cascade**: Marking dependent tiles dirty after source tile changes
- **Priority**: Task urgency in scheduler queue

## Non-Functional Requirements

| Requirement | Specification |
|-------------|---------------|
| **Concurrency** | No blocking on tile reads; multiple threads safe |
| **Memory efficiency** | Sparse storage; unused tiles not allocated |
| **Determinism** | Downsampling produces identical results each run |
| **Latency** | P99 tile computation < 10ms (target: 5ms) |
| **Throughput** | ≥1000 tile operations/second in thread pool |

## Out of Scope (Phase 1)

- Document model API (Phase 2)
- Filter implementation (Phase 2+)
- Scratch disk eviction (Phase 6)
- Tile compression (Phase 3+)
- GPU acceleration (Phase 7+)
- Mask processing details (Phase 2)

---

## Success Criteria Summary

- [x] All data types defined (TileKey, TileCoord, PixelTile, etc.)
- [x] TileCache with LRU eviction working
- [x] Dirty marking (no deletion) implemented
- [x] Pyramid downsampling implemented
- [x] GenerationTracker with per-layer versioning
- [x] Scheduler with priority queue
- [x] Invalidation cascade logic
- [x] Unit tests passing (≥6)
- [x] Integration tests passing (≥3)
- [x] Benchmarks showing downsample ≤5ms
- [x] All code compiling without warnings
- [x] Documentation complete
