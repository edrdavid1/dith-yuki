# Phase 2 Design: Document Model & Filter Application

## Architecture Overview

Phase 2 implements the application-level data model (Document, Layers, Filters) that drives the Phase 1 tile engine. The design maintains several key principles:

1. **Stable IDs, not indices**: Layers and filters use stable UUIDs (`LayerId`, `FilterInstanceId`), not Vec indices, so reordering/removal doesn't invalidate references.
2. **Lock-free reads from workers**: Document snapshots via `arc-swap` allow worker threads to read a consistent state without blocking on writes.
3. **Invalidation by dirty marking**: Tiles stay in cache marked dirty, not deleted, enabling mgnevnaya обратная связь (instant feedback).
4. **Lazy evaluation of hierarchy**: Tree traversal uses iterators, not pre-flattened lists, to avoid allocations.
5. **Two-level generation tracking**: Document + per-layer, allowing selective invalidation of unrelated work.

---

## Module Structure

Phase 2 spans three Rust crates:

### `engine-project` (new crate)
Implements Document, Layer, LayerGroup, mask system, filters, and tree traversal. Contains no tile rendering logic, only data structures and invalidation orchestration.

**Modules**:
- `lib.rs` — public API, re-exports
- `document.rs` — `Document`, `DocumentHandle`, `DocumentId`
- `layer.rs` — `Layer`, `LayerNode`, `LayerGroup`, `LayerKind`, tree traversal
- `mask.rs` — `MaskRef`, `MaskStorage`, `apply_mask()` function
- `filter.rs` — `FilterInstance`, `FilterKind`, `FilterParams`, `apply_filter_to_tile()`
- `error.rs` — `EngineError` enum
- `dto.rs` — `DocumentSnapshotDto`, `LayerNodeDto`, conversion functions

### `engine-tiles` (existing crate, extended)
- Add `InvalidationEvent` enum variants for document structure changes (§5.1)
- Extend `invalidate()` function to handle new events

### `app` (Tauri backend, existing)
- Register new `#[tauri::command]` functions for document/layer/filter operations (§6.1–6.4)
- Integrate `DocumentHandle` state into Tauri app state
- Emit `EngineEvent::DocumentStateChanged` after mutations

---

## Design Decisions

### 1. Why `arc-swap` for DocumentHandle?

Alternative considered: `RwLock<Document>` (standard Rust solution).

**Problem with RwLock**: Workers reading from scheduler would wait for writes from UI thread. On every slider drag, UI thread acquires `RwLock::write()`, updates Document, and releases. If a worker is in the middle of a read, it must wait. At 30 fps with 5–10 ms per frame budget, even a 1 ms write-stall is noticeable.

**arc-swap solution**: 
- `load_full()` is atomic, lock-free, always succeeds in nanoseconds.
- `store()` creates new Arc, swaps pointer, old Arc is dropped when no readers hold it.
- Workers see consistent snapshots; mutations don't block.
- Trade-off: Document is cloned on each mutation (but shallow — only metadata, not pixels).

For 50 layers × 10 filters = ~1500 items to clone: ~100 μs, acceptable for a UI operation.

### 2. Why Stable IDs Over Indices?

Scenario: User drags Layer 3 to position 1 (reorder). If layer identity was "index 3", now it's "index 1". Any task in the scheduler with "layer 3" is now wrong — it refers to what's now layer 4, or references are nonsensical.

**Solution**: Each layer has permanent `id: LayerId`, immutable across reorders. Reorder changes Vec order, not IDs. Scheduler tasks carry stable layer IDs, unaffected by reordering.

**Cost**: Map lookups (`HashMap<LayerId, &Layer>`) during traversal. Mitigated by:
- Traversal is breadth-first depth-first, not random access.
- IDs are only used when referencing a specific layer (e.g., "layer 5 changed filter params").

### 3. Lazy Tree Traversal with `walk_bottom_to_top()` Iterator

Alternative: Flatten entire tree into `Vec<LayerRef>` each time composite runs.

**Problem**: Vector allocation + iteration overhead, repeated during each tile composition. With 50+ layers, this adds microseconds per tile.

**Solution**: Lazy iterator emitting `LayerRef::Leaf`, `LayerRef::GroupStart`, `LayerRef::GroupEnd` as it walks the tree. Consumer (composite function) handles grouping logic (recurse on children when seeing GroupStart).

```rust
pub enum LayerRef<'a> {
    Leaf(&'a Layer),
    GroupStart(&'a LayerGroup),
    GroupEnd(&'a LayerGroup),
}

pub fn walk_bottom_to_top<'a>(nodes: &'a [LayerNode]) 
    -> impl Iterator<Item = LayerRef<'a>> 
{
    // Yields Leaf, then GroupStart → children → GroupEnd for each node
}
```

### 4. Invalidation Cascade by Stage Dependency

Phase 1 defined `CacheStage: Raw | Processed | Composite`. Phase 2 enriches with layer context:

| Event | Affected Tiles | Cascade |
|-------|---|---|
| Raw pixels change | Raw(layer, coords) | Mark Processed(layer, coords) dirty → Composite(coords) for all above |
| Filter params change | Processed(layer, *) | Mark all Processed(layer) dirty → cascade Composite |
| Layer props change (opacity) | — | Mark Composite(layer, *) dirty (Processed unchanged) |
| Mask change | Processed(layer, coords) | Mark Processed(layer, coords) dirty → cascade Composite |
| Layer add/remove | — | Mark Composite(*) dirty (layer order changed) |

Key insight: `LayerPropsChanged` (opacity, visibility) does NOT invalidate Processed — only Composite. This is the hottest path (slider drag), and Processed is expensive to recompute.

### 5. Two-Level Generation Tracking

Phase 1 GenerationTracker: `document_gen: AtomicU64`, `layer_gen: DashMap<LayerId, u64>`.

During task execution, check task generation against relevant level:
- `Processed(layer)` → check `layer_gen[layer]`
- `Composite(layer)` → check `max(layer_gen) for all layers`

Allows: Changing Layer A's filter doesn't cancel work on Layer B's tiles. Saves CPU on multi-layer documents.

### 6. Filter Application Model

Filters are organized into a stack per layer. Application order:

```
Raw pixels
  ↓ (apply Raw-stage filters, e.g., local adjustments not needing context)
Processed pixels (after mask)
  ↓ (apply Processed-stage filters, e.g., dithering, color reduction)
Ready for composition
  ↓ (apply blend_mode + opacity to lower layers)
Composite
```

**Exception**: `FilterInstance.requires_full_row = true` filters (e.g., pixel-sorting glitch) are **not tiled**. They process the entire layer row/column in one pass before entering the tiled pipeline. Handled separately in tile generation (not in `apply_filter_to_tile()` — will panic to catch misuse).

### 7. Mask System Design

A mask is a grayscale (alpha channel) that modulates the layer's alpha. Stored as a separate raster layer (which can have its own filters, but masked layers don't render on canvas — only used as alpha source).

```rust
pub enum MaskStorage {
    External(LayerId),           // mask is another layer
    EmbeddedVector(Vec<Stroke>), // vector (placeholder, future work)
}

fn apply_mask(tile: &PixelTile, mask: Option<&MaskRef>, coord: TileCoord) -> PixelTile {
    if mask.is_none() || !mask.enabled { return tile.clone(); }
    
    let mask_tile = cache.get_or_schedule(TileKey { 
        layer: mask.storage.as_layer_id(),
        coord,
        stage: Processed  // mask itself goes through Processed (filters, etc.)
    });
    
    let inverted = mask.inverted;
    // Multiply alpha: tile.a *= mask.a (or 1 - mask.a if inverted)
    // ... implementation in apply_mask() function
}
```

Masks have no separate Composite stage — they're just Processed rasters, referenced by their owning layer.

### 8. DocumentHandle Workflow

UI thread (Tauri command):
```rust
#[tauri::command]
fn update_filter_params(...) -> Result<(), EngineError> {
    let doc_handle = /* from app state */;
    
    doc_handle.mutate(|doc| {
        let layer = doc.find_layer_mut(layer_id)?;
        let filter = layer.find_filter_mut(filter_id)?;
        filter.params = new_params;
        doc.revision += 1;
        GenerationTracker::increment_layer_gen(layer_id);
    });
    
    // Invalidate, schedule, return immediately
    invalidate(cache, &doc_handle.snapshot(), InvalidationEvent::LayerFilterChanged { layer: layer_id });
    Ok(())
}
```

Worker thread (scheduler):
```rust
let doc = doc_handle.snapshot(); // O(1) atomic load, never blocks
// Process tile using consistent doc snapshot
composite_tile(&doc, coord, cache);
```

### 9. Error Handling Strategy

All public functions return `Result<T, EngineError>`. No unwrap in library code (only in tests/examples). Errors are:
- Validation errors: Invalid layer kind, out-of-range parameters
- Not-found errors: Layer/filter/document doesn't exist
- IO errors: Disk read failures

Tauri commands catch errors and return `Result<T, EngineError>` over invoke (Tauri serializes Err variant as structured JSON for TypeScript).

---

## Concurrency & Memory Model

### Reading Document
- Multiple worker threads call `doc_handle.snapshot()` → each gets `Arc<Document>`
- All snapshots are immutable (`Arc<T>` is shared read-only reference)
- Snapshot is guaranteed consistent (frozen at one generation)
- No allocations during read, O(1)

### Writing Document
- UI thread (Tauri async runtime) calls `doc_handle.mutate(|doc| { ... })`
- Closure receives `&mut Document`
- Document cloned into new heap allocation (`Arc::new(clone)`)
- New Arc stored atomically, old Arc dropped when no threads hold it
- Next `snapshot()` sees new version

### Tile Cache
- Separate `Arc<TileCache>` in app state, never cloned
- All threads share same DashMap cache by reference
- Thread-safe by design (DashMap, SegQueue)

### No Data Races
- Document snapshots are immutable, no mutation after snapshot
- TileCache is designed thread-safe (from Phase 1)
- No shared mutable state except ArcSwap and DashMap (both safe by design)

---

## Testing Strategy

### Unit Tests (in each module)
- `layer.rs`: Tree traversal order, walk_bottom_to_top correctness
- `mask.rs`: Mask application (enable/disable, invert logic)
- `filter.rs`: Filter parameter validation, enabled flag behavior
- `document.rs`: Document creation, layer add/remove, revision increment
- `dto.rs`: Serialization round-trip (serde)

### Integration Tests
- **Test 1**: Document mutation → invalidation cascade
  - Create document, add two layers, change filter on layer 1 → verify Composite marked dirty for both, Processed only for layer 1
  - Check scheduler has tasks queued for affected tiles
  
- **Test 2**: Layer hierarchy with groups
  - Create group, add layers to group, change group opacity → verify Composite cascade includes group + all children
  
- **Test 3**: DocumentHandle concurrent access
  - Spawn 3 worker threads reading snapshot while UI thread mutates document
  - Verify no panics, workers complete with consistent state
  - Measure latency (snapshot should be <1 μs)

### Property-Based Tests (future, if added)
- Generate random document mutations, verify invalidation is sound (all affected tiles marked, no unaffected tiles marked)

---

## Migration from Phase 1

Phase 2 adds new crate `engine-project` but does not modify Phase 1 (`engine-tiles`). Minimal changes to `engine-tiles`:

1. New `InvalidationEvent` variants in `invalidation.rs`
2. Extend `invalidate()` function to handle them
3. Add per-layer generation checking in scheduler

All Phase 1 tests remain green.

---

## Open Questions for Phase 3+

1. **Full-row filters**: How to integrate `requires_full_row` filters into tile generation? (Likely: separate non-tiled pass before entering pyramid cache)
2. **Vector masks**: Placeholder for now; Phase 3+ adds rasterization of vector strokes
3. **Group recursion efficiency**: Is cloning Document on each mutation acceptable for 100+ layer documents? (Likely yes, but benchmark to confirm)
4. **Undo/redo storage**: Store full Document snapshots or diffs? (Deferred to Phase 4)

---

## References
- tile-engine-architecture.md: Phase 1 tile engine design
- tauri-api-document-model.md: API contracts and DTO structures (Phase 2 realizes this blueprint)
- Phase 1 tasks: Deliverables from Phase 1 that Phase 2 builds on

