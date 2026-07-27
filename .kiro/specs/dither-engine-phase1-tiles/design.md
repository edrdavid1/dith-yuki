# Design: Phase 1 — Tile Engine Implementation

## Feature Overview

Implement the core tile caching and pyramid downsampling system for the Dither image processing engine. This is the foundational layer that enables:
- Efficient memory usage for large images
- Parallel processing via tile batching
- Responsive UI (instant feedback at coarse pyramid levels)
- Scalable rendering architecture

## Architecture Reference

This design follows the specification in `/tile-engine-architecture.md` (§1–6). Key concepts:
- **TileKey**: Stable identifier (layer, coordinate, stage)
- **TileCache**: LRU eviction with `dirty` marking (not deletion)
- **Pyramid**: Lazy downsampling (1:2, 1:4, etc.)
- **GenerationTracker**: Per-layer versioning for selective invalidation
- **Scheduler**: Priority queue with work-stealing parallelism

## Core Components

### 1. Addressing Types (§1 of tile-engine-architecture.md)

```rust
// types.rs
pub type LayerId = u32;
pub type MipLevel = u8;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileCoord {
    pub level: MipLevel,
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TileKey {
    pub layer: LayerId,
    pub coord: TileCoord,
    pub stage: CacheStage,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum CacheStage {
    Raw,        // Original pixels, no filters
    Processed,  // After layer filters
    Composite,  // After blending with layers below
}

pub const TILE_SIZE: u32 = 256;
pub const HALO: u32 = 2;  // Overlap for error diffusion
```

### 2. Pixel Tile (§2.1)

```rust
// tile.rs
pub struct PixelTile {
    pub data: Box<[f32]>,  // (TILE_SIZE + 2*HALO)^2 * 4 elements
}

impl PixelTile {
    pub fn new() -> Self {
        let size = (TILE_SIZE + 2 * HALO) as usize;
        Self {
            data: vec![0.0; size * size * 4].into_boxed_slice(),
        }
    }

    pub fn at(&self, x: u32, y: u32, channel: u32) -> f32 {
        let size = (TILE_SIZE + 2 * HALO) as usize;
        let idx = ((y * (size as u32) + x) * 4 + channel) as usize;
        self.data[idx]
    }

    pub fn set(&mut self, x: u32, y: u32, channel: u32, value: f32) {
        let size = (TILE_SIZE + 2 * HALO) as usize;
        let idx = ((y * (size as u32) + x) * 4 + channel) as usize;
        self.data[idx] = value;
    }
}
```

### 3. TileCache with LRU (§3)

```rust
// cache.rs
pub struct CacheEntry {
    pub tile: Arc<PixelTile>,
    pub generation: u64,
    pub last_touched: Instant,
    pub dirty: AtomicBool,
}

pub struct TileCache {
    entries: DashMap<TileKey, CacheEntry>,
    lru_queue: SegQueue<TileKey>,
    budget_bytes: AtomicUsize,
    used_bytes: AtomicUsize,
}

impl TileCache {
    pub fn new(budget_bytes: usize) -> Self {
        Self {
            entries: DashMap::new(),
            lru_queue: SegQueue::new(),
            budget_bytes: AtomicUsize::new(budget_bytes),
            used_bytes: AtomicUsize::new(0),
        }
    }

    pub fn get_or_insert(&self, key: TileKey, tile: Arc<PixelTile>) -> Arc<PixelTile> {
        if let Some(entry) = self.entries.get(&key) {
            entry.value().tile.clone()
        } else {
            self.entries.insert(key, CacheEntry {
                tile: tile.clone(),
                generation: 0,
                last_touched: Instant::now(),
                dirty: AtomicBool::new(false),
            });
            self.used_bytes.fetch_add(std::mem::size_of::<PixelTile>(), Ordering::Relaxed);
            self.lru_queue.push(key);
            tile
        }
    }

    pub fn mark_dirty(&self, key: TileKey) {
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.dirty.store(true, Ordering::Release);
        }
    }

    pub fn evict_if_over_budget(&self) {
        let used = self.used_bytes.load(Ordering::Relaxed);
        let budget = self.budget_bytes.load(Ordering::Relaxed);
        if used > budget {
            // Eviction logic: LRU removal from least-recently-used tiles
            while let Some(key) = self.lru_queue.pop() {
                if self.entries.remove(&key).is_some() {
                    let freed = std::mem::size_of::<PixelTile>();
                    self.used_bytes.fetch_sub(freed, Ordering::Relaxed);
                    if self.used_bytes.load(Ordering::Relaxed) <= budget {
                        break;
                    }
                }
            }
        }
    }
}
```

### 4. Invalidation (§3.3)

```rust
// invalidation.rs
pub enum InvalidationEvent {
    LayerRawChanged { layer: LayerId, coords: Vec<TileCoord> },
    LayerFilterChanged { layer: LayerId },
    LayerPropsChanged { layer: LayerId },
    MaskChanged { layer: LayerId, coords: Vec<TileCoord> },
}

pub fn invalidate(cache: &TileCache, _doc: &Document, event: InvalidationEvent) {
    match event {
        InvalidationEvent::LayerRawChanged { layer, coords } => {
            for coord in coords {
                cache.mark_dirty(TileKey { layer, coord, stage: CacheStage::Raw });
                cache.mark_dirty(TileKey { layer, coord, stage: CacheStage::Processed });
                // Cascade to all composite tiles that depend on this layer
                cascade_composite_invalidation(cache, layer, coord);
            }
        }
        InvalidationEvent::LayerFilterChanged { layer } => {
            // Mark all Processed tailesfor this layer as dirty
            // (iterate through cache and find matching keys)
            cascade_composite_invalidation_for_layer(cache, layer);
        }
        InvalidationEvent::LayerPropsChanged { layer } => {
            // Only composite tiles need recomputation
            cascade_composite_invalidation_for_layer(cache, layer);
        }
        InvalidationEvent::MaskChanged { layer, coords } => {
            for coord in coords {
                cache.mark_dirty(TileKey { layer, coord, stage: CacheStage::Processed });
                cascade_composite_invalidation(cache, layer, coord);
            }
        }
    }
}
```

### 5. Pyramid Downsampling (§2.2)

```rust
// pyramid.rs
pub fn downsample_tile(parent: &PixelTile) -> PixelTile {
    let mut child = PixelTile::new();
    let h = TILE_SIZE;
    let w = TILE_SIZE;

    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let p00 = parent.at(x * 2, y * 2, c);
                let p10 = parent.at(x * 2 + 1, y * 2, c);
                let p01 = parent.at(x * 2, y * 2 + 1, c);
                let p11 = parent.at(x * 2 + 1, y * 2 + 1, c);
                let avg = (p00 + p10 + p01 + p11) * 0.25;
                child.set(x + HALO, y + HALO, c, avg);
            }
        }
    }
    child
}
```

### 6. GenerationTracker (§5.1)

```rust
// generation.rs
pub struct GenerationTracker {
    pub document_gen: AtomicU64,
    pub layer_gen: DashMap<LayerId, u64>,
}

impl GenerationTracker {
    pub fn new() -> Self {
        Self {
            document_gen: AtomicU64::new(0),
            layer_gen: DashMap::new(),
        }
    }

    pub fn increment_document_gen(&self) -> u64 {
        self.document_gen.fetch_add(1, Ordering::Release)
    }

    pub fn increment_layer_gen(&self, layer: LayerId) -> u64 {
        let mut entry = self.layer_gen.entry(layer).or_insert(0);
        *entry += 1;
        *entry
    }

    pub fn get_layer_gen(&self, layer: LayerId) -> u64 {
        self.layer_gen.get(&layer).map(|e| *e).unwrap_or(0)
    }
}
```

### 7. Scheduler (§5.2, §5.3)

```rust
// scheduler.rs
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Priority {
    Immediate,        // Coarse pyramid level
    ViewportCenter,   // High priority visible tiles
    ViewportEdge,     // Lower priority edge tiles
    Prefetch,         // Out-of-viewport prefetch
}

pub struct RecomputeTask {
    pub key: TileKey,
    pub generation: u64,
    pub layer_generation: u64,
    pub priority: Priority,
}

pub struct Scheduler {
    immediate_queue: SegQueue<RecomputeTask>,
    viewport_center_queue: SegQueue<RecomputeTask>,
    viewport_edge_queue: SegQueue<RecomputeTask>,
    prefetch_queue: SegQueue<RecomputeTask>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            immediate_queue: SegQueue::new(),
            viewport_center_queue: SegQueue::new(),
            viewport_edge_queue: SegQueue::new(),
            prefetch_queue: SegQueue::new(),
        }
    }

    pub fn enqueue(&self, task: RecomputeTask) {
        match task.priority {
            Priority::Immediate => self.immediate_queue.push(task),
            Priority::ViewportCenter => self.viewport_center_queue.push(task),
            Priority::ViewportEdge => self.viewport_edge_queue.push(task),
            Priority::Prefetch => self.prefetch_queue.push(task),
        }
    }

    pub fn dequeue(&self) -> Option<RecomputeTask> {
        self.immediate_queue
            .pop()
            .or_else(|| self.viewport_center_queue.pop())
            .or_else(|| self.viewport_edge_queue.pop())
            .or_else(|| self.prefetch_queue.pop())
    }
}
```

## Module Structure

```
crates/engine-tiles/src/
├── lib.rs              # Module exports
├── types.rs            # TileKey, TileCoord, CacheStage, LayerId
├── tile.rs             # PixelTile implementation
├── cache.rs            # TileCache with LRU
├── invalidation.rs     # Invalidation logic
├── pyramid.rs          # Downsampling
├── generation.rs       # GenerationTracker
├── scheduler.rs        # Scheduler + Priority queue
├── tests/
│   ├── cache_tests.rs  # LRU eviction, dirty marking
│   ├── pyramid_tests.rs # Downsampling correctness
│   └── generation_tests.rs # Generation tracking
└── benches/
    ├── pyramid_bench.rs # Downsample throughput
    └── cache_bench.rs   # Cache access latency
```

## Correctness Properties

Property-based tests will verify:

1. **Cache Coherence**: Reading a tile after marking dirty returns stale value until recomputed
2. **Generation Isolation**: Incrementing layer_gen doesn't affect other layer_gen counters
3. **Downsampling Accuracy**: Box-filtered downsampling produces expected RGBA values
4. **LRU Eviction**: Least-recently-used tiles are evicted first
5. **Dirty Propagation**: Marking a Raw tile dirty cascades to Processed and Composite

## Testing Strategy

### Unit Tests (Phase 1a)
- TileCoord/TileKey construction and hashing
- PixelTile creation and access
- Cache insertion/retrieval
- Dirty marking (no deletion)
- Generation counter semantics

### Integration Tests (Phase 1b)
- Cache with pyramid: insert at level 0, verify level 1 can downsample
- Invalidation cascade: mark Raw dirty, verify Processed and Composite marked dirty
- Scheduler priority: high-priority tasks dequeue before low-priority

### Benchmarks (Phase 1c)
- Downsample throughput: tiles/second (target: 5ms/256×256)
- Cache access latency: operations/nanosecond
- LRU eviction overhead: time to evict N tiles

## Deliverables

By end of Phase 1:
- ✅ `TileKey`, `TileCoord`, `CacheStage` types
- ✅ `PixelTile` with RGBA access
- ✅ `TileCache` with LRU eviction and dirty marking
- ✅ Pyramid downsampling (1:2 box filter)
- ✅ `GenerationTracker` with per-layer versioning
- ✅ `Scheduler` with 4-tier priority queue
- ✅ Invalidation logic (cascade behavior)
- ✅ Unit + integration tests
- ✅ Criterion benchmarks
- ✅ All passing, 0 warnings

## Non-Functional Requirements

| Requirement | Target | Rationale |
|-------------|--------|-----------|
| Downsample latency | ≤5ms per 256×256 tile | NFR from ТЗ; enables "instant feedback" |
| Cache lookup | O(1) average | DashMap concurrent HashMap |
| Memory efficiency | <100MB for 5000×5000 + 10-layer pyramid | LRU eviction + sparse representation |
| Concurrency | No blocking on reads | Lock-free DashMap + SegQueue |

## Known Limitations

- **Boundary halo**: Border state may differ from sequential processing (architecture decision)
- **No scratch disk**: Evicted tiles are lost (Phase 0 limitation; added in Phase 6)
- **Simple LRU**: Approximation only; not perfect LRU due to SegQueue non-ordering
- **No per-tile compression**: All tiles stored as uncompressed f32 (Phase 3+ optimization)

## Open Questions for Phase 2

- Document model API (how Document passes layer info to tile engine)
- Filter application interface (how filters trigger tile recomputation)
- Mask handling in tile computation (interaction with Processed stage)

---

**Next**: Create requirements.md from this design, then tasks.md
