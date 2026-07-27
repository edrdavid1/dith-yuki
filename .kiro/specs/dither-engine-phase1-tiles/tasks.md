# Implementation Plan: Phase 1 — Tile Engine

## Task Dependency Graph

```json
{
  "waves": [
    {
      "wave": 1,
      "tasks": ["1"]
    },
    {
      "wave": 2,
      "tasks": ["2", "3", "5"],
      "dependsOn": ["1"]
    },
    {
      "wave": 3,
      "tasks": ["4", "6"],
      "dependsOn": ["2", "3", "5"]
    },
    {
      "wave": 4,
      "tasks": ["7"],
      "dependsOn": ["1", "2", "3", "4", "5", "6"]
    },
    {
      "wave": 5,
      "tasks": ["8"],
      "dependsOn": ["7"]
    },
    {
      "wave": 6,
      "tasks": ["9"],
      "dependsOn": ["1", "2", "3", "4", "5", "6", "7"]
    },
    {
      "wave": 7,
      "tasks": ["10"],
      "dependsOn": ["1", "2", "3", "4", "5", "6", "7", "8", "9"]
    }
  ]
}
```

## Overview

Implement the core tile caching and pyramid downsampling system for the Dither image processing engine. This foundational layer enables efficient memory usage, parallel processing, responsive UI feedback, and scalable rendering.

**Phase 1 Scope**:
- Core types (TileKey, TileCoord, CacheStage)
- PixelTile struct with RGBA storage
- TileCache with LRU eviction and dirty marking
- Pyramid downsampling (1:2 box filter)
- GenerationTracker for versioning
- Scheduler with 4-tier priority queue
- Invalidation with cascade logic
- Unit + integration tests
- Criterion benchmarks

**Success Criteria**:
- All 7 modules implemented and compiled without warnings
- 6+ unit tests passing
- 3+ integration tests passing
- 2+ benchmarks showing ≤5ms downsample latency
- All tests passing: `cargo test -p engine-tiles`
- Zero clippy warnings: `cargo clippy -p engine-tiles -- -D warnings`
- Documentation generated: `cargo doc -p engine-tiles`

---

## Tasks

- [x] 1. Define Core Types: Create type definitions for TileCoord, TileKey, CacheStage, LayerId, MipLevel, and constants. File `/crates/engine-tiles/src/types.rs` created with all types compiling and publicly exported. Acceptance: All types implement Clone, Copy, PartialEq, Eq, Hash, Debug. Reference: tile-engine-architecture.md §1. Depends on: (none)

- [x] 2. Implement PixelTile: Create PixelTile struct with RGBA pixel storage and access methods. File `/crates/engine-tiles/src/tile.rs` with size calculation (TILE_SIZE + 2*HALO)² × 4 channels. Methods: new(), at(x, y, channel), set(x, y, channel, value). Memory layout is row-major, contiguous. Acceptance: new() allocates (260)² × 4 = 270,400 f32 elements. Reference: tile-engine-architecture.md §2.1. Depends on: Task 1

- [x] 3. Implement TileCache with LRU Eviction: Create TileCache with concurrent access, dirty marking, and LRU eviction. File `/crates/engine-tiles/src/cache.rs` with dependencies: dashmap, crossbeam. Thread-safe DashMap and SegQueue. Methods: new(budget_bytes), get_or_insert(key, tile), mark_dirty(key), evict_if_over_budget(). Acceptance: Dirty marking without deletion, LRU eviction when over budget. Reference: tile-engine-architecture.md §3. Depends on: Task 1, Task 2

- [x] 4. Implement Pyramid Downsampling: Create downsampling function for lazy pyramid generation. File `/crates/engine-tiles/src/pyramid.rs` with function downsample_tile(parent) -> PixelTile. 1:2 box filter: output pixel = (p00 + p10 + p01 + p11) * 0.25. Output size (260)² × 4. All 4 RGBA channels processed. Acceptance: Correct averaging, test uniform color → same. Reference: tile-engine-architecture.md §2.2. Depends on: Task 1, Task 2, Task 3

- [x] 5. Implement GenerationTracker: Create per-layer versioning for selective invalidation. File `/crates/engine-tiles/src/generation.rs` with struct GenerationTracker { document_gen: AtomicU64, layer_gen: DashMap }. Methods: new(), increment_document_gen() -> u64, increment_layer_gen(layer) -> u64, get_layer_gen(layer) -> u64. Atomic increments (monotonic). Acceptance: Document and per-layer generations independent. Reference: tile-engine-architecture.md §5.1. Depends on: Task 1

- [x] 6. Implement Scheduler and Invalidation: Create priority scheduler and invalidation logic. Files `/crates/engine-tiles/src/scheduler.rs` and `invalidation.rs`. Scheduler: 4-tier priority enum (Immediate, ViewportCenter, ViewportEdge, Prefetch), struct RecomputeTask, methods enqueue() and dequeue(). Invalidation: enum InvalidationEvent (4 variants), function invalidate(cache, event), cascade logic marks Composite tiles. Acceptance: Priority order dequeue, cascade marks correct tiles. Reference: tile-engine-architecture.md §5.2, §5.3, §3.3. Depends on: Task 1, Task 3, Task 5

- [x] 7. Unit Tests: Write tests for all 6 modules. File tests in types.rs, tile.rs, cache.rs, pyramid.rs, generation.rs, scheduler.rs/invalidation.rs. At least 1 test per module (6+ total). Coverage: TileCoord hashable, CacheStage variants, PixelTile size/access, cache operations, pyramid downsampling, generation independence, scheduler priority, invalidation cascade. Acceptance: All tests pass, `cargo test -p engine-tiles`. Reference: Requirement 6. Depends on: Task 1–6

- [x] 8. Integration Tests: Write tests for multi-component workflows. File `/crates/engine-tiles/tests/integration_test.rs` with 3+ tests. Test 1: Cache + Pyramid (insert Level 0, downsample to Level 1). Test 2: Invalidation Cascade (insert Raw/Processed/Composite, mark Raw, verify all marked). Test 3: Scheduler Priority (enqueue different priorities, dequeue in order). Acceptance: All tests pass `cargo test -p engine-tiles --test integration_test`. Reference: Requirement 6. Depends on: Task 1–7

- [x] 9. Benchmarks: Create criterion benchmarks for performance. Files `/crates/engine-tiles/benches/pyramid_bench.rs` and `cache_bench.rs`. Add criterion dev-dependency and [[bench]] entries (harness=false). Benchmark 1: downsample_tile throughput, target ≤5ms per 256×256. Benchmark 2: cache get_or_insert and mark_dirty latency. Acceptance: `cargo bench -p engine-tiles` generates HTML reports. Reference: Requirement 7. Depends on: Task 1–7

- [x] 10. Verification and Checkpoint: Build, test, lint, and document Phase 1 completion. Run: `cargo build -p engine-tiles`, `cargo test -p engine-tiles`, `cargo clippy -p engine-tiles -- -D warnings`, `cargo doc -p engine-tiles`. Create `/PHASE_1_SUCCESS_REPORT.md` with summary of deliverables, test counts, performance results, dependencies, known limitations, next steps. Acceptance: All builds clean, all tests pass, zero clippy warnings, docs build. Depends on: Task 1–9

---

## Notes

**Phase 1 Architecture**:
- All modules in `/crates/engine-tiles/src/`: types, tile, cache, pyramid, generation, scheduler, invalidation
- Concurrent access via DashMap (lock-free reads) and SegQueue (atomic operations)
- No File I/O: all data in memory (Phase 6 adds disk eviction)
- Simple LRU: SegQueue-based approximation (good enough for now)

**Dependencies**:
- dashmap 5.5+: Concurrent HashMap for cache storage
- crossbeam 0.8+: SegQueue for lock-free priority queues
- criterion 0.5+: Benchmarking framework (dev-only)

**Key Concepts**:
- Dirty marking (not deletion): Stale tiles stay in cache marked dirty until recomputed
- Cascade invalidation: Marking Raw dirty → Processed → Composite (layer + above)
- Priority queue: Immediate → ViewportCenter → ViewportEdge → Prefetch dequeue order
- Generation tracking: Two-level (document + per-layer) for selective recomputation

**Known Limitations**:
1. Simple LRU: SegQueue doesn't maintain perfect LRU order (approximation)
2. No disk eviction: Evicted tiles are lost (added in Phase 6)
3. No compression: All tiles stored as uncompressed f32 (Phase 3+)
4. Boundary halo: Border state may differ from sequential (architecture decision)

**Next Phase (Phase 2)**:
- Integrate Document model (layers, properties)
- Implement filter application and mask handling
- Add Tauri commands to trigger tile recomputation
- Design Processed stage computation pipeline

**References**:
- tile-engine-architecture.md: Full specification (§1–6)
- design.md: Phase 1 design document
- requirements.md: Phase 1 acceptance criteria

