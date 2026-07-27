# Phase 2 Requirements: Document Model & Filter Application

## Overview

Phase 2 integrates the tile engine (Phase 1) with the application layer, implementing the Document model, layer hierarchy, mask system, and filter application pipeline. This phase bridges UI/document structure with low-level tile rendering.

**Context**: Phase 1 completed the tile caching, pyramid, and scheduler. Phase 2 defines the data structures and rendering pipeline that use those primitives. Phase 3+ will add specific filters (dither, curves, LUT3D, etc.).

---

## Requirement 1: Document & Layer Hierarchy Model

**Requirement 1.1**: Implement `Document` struct containing:
- `id: DocumentId` (unique identifier, stable across saves)
- `width: u32, height: u32` (canvas dimensions)
- `color_profile: ColorProfileRef` (reference to color space; placeholder for now, detail in Phase 5)
- `root: Vec<LayerNode>` (top-level list of layers/groups)
- `palettes: Vec<PaletteId>` (list of color palettes used in document)
- `revision: u64` (incremented on any structural change, for undo/redo tracking)
- `generations: GenerationTracker` (two-level: document + per-layer, from Phase 1)

**Requirement 1.2**: Implement `LayerNode` enum to support recursive tree structure:
- `Leaf(Layer)` — single raster or adjustment layer
- `Group(LayerGroup)` — container with children, owns blend_mode/opacity/mask

**Requirement 1.3**: Implement `Layer` struct for leaf nodes:
- `id: LayerId` (stable unique ID within document, not index)
- `name: String` (display name in UI)
- `kind: LayerKind` — `Raster` (stores pixels) or `Adjustment` (applies filters to layers below)
- `blend_mode: BlendMode` (15+ modes: Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion, and placeholders for 4+ more)
- `opacity: f32` (0.0–1.0)
- `visible: bool` (if false, skip in composition)
- `offset: (i32, i32)` (pixel offset within canvas)
- `mask: Option<MaskRef>` (optional alpha mask)
- `filters: Vec<FilterInstance>` (stack of filters applied to layer)
- `bounds_l0: TileBounds` (layer's extent in tiles at MipLevel 0, to avoid querying empty tiles)

**Requirement 1.4**: Implement `LayerGroup` struct:
- `id: LayerId` (group also has TileKey-addressable cache entries for Composite)
- `name: String`
- `blend_mode: BlendMode`
- `opacity: f32`
- `visible: bool`
- `mask: Option<MaskRef>`
- `children: Vec<LayerNode>` (bottom-to-top order, like root)

**Requirement 1.5**: Implement tree traversal iterator `walk_bottom_to_top(nodes: &[LayerNode]) -> impl Iterator<Item = LayerRef>`:
- Emits `LayerRef::Leaf(layer)` for each raster/adjustment layer
- Emits `LayerRef::GroupStart(group)` before children, `LayerRef::GroupEnd(group)` after
- Depth-first, bottom-to-top order (leaves first)
- Lazy (does not allocate a flat list each call)

---

## Requirement 2: Mask System

**Requirement 2.1**: Implement `MaskRef` struct:
- `storage: MaskStorage` — reference to mask raster data (separate TileKey namespace, no stage enum)
- `enabled: bool` — toggle mask on/off without deletion
- `inverted: bool` — invert mask logic (white becomes black)

**Requirement 2.2**: Implement `MaskStorage` enum:
- `External(LayerId)` — mask is a separate raster layer (not rendered on its own, referenced as mask)
- `EmbeddedVector(Vec<Stroke>)` — vector strokes (future; for now placeholder/validation only)

**Requirement 2.3**: Mask application function `apply_mask(tile: &PixelTile, mask: Option<&MaskRef>, coord: TileCoord) -> PixelTile`:
- If mask is None or disabled, return tile unchanged
- If enabled, load mask tile at same coord, multiply alpha channels (premultiply convention)
- If inverted, use `1.0 - mask_alpha`
- Return masked tile (does not mutate input)

---

## Requirement 3: Filter Pipeline & Instance Model

**Requirement 3.1**: Implement `FilterInstance` struct:
- `id: FilterInstanceId` — stable UUID for the filter instance (not index in Vec)
- `kind: FilterKind` — which filter to apply (enum, variants added incrementally)
- `params: FilterParams` — filter-specific parameters (enum with variants per FilterKind)
- `enabled: bool` — skip filter if false (without removal)
- `requires_full_row: bool` — if true, filter processes entire row/layer (not tiled)

**Requirement 3.2**: Implement `FilterKind` enum (Phase 2 placeholder):
- `Curves` — tone curve adjustment (detail in Phase 5 color pipeline)
- `Levels` — input/output range adjustment
- `Placeholder` — catch-all for future filters, validates deserialization

**Requirement 3.3**: Implement `FilterParams` enum:
- `Curves { curve: Vec<(f32, f32)> }` — control points (parameterized by channel: R, G, B, or luminance)
- `Levels { input_black: f32, input_white: f32, output_black: f32, output_white: f32 }`
- `Placeholder(String)` — for future filters

**Requirement 3.4**: Filter application function `apply_filter_to_tile(tile: &PixelTile, filter: &FilterInstance, stage: CacheStage) -> PixelTile`:
- If not enabled, return tile unchanged
- For `Raw` stage: apply to source pixels directly
- For `Processed` stage: apply after mask/adjustment composition
- For `Composite` stage: skip (filters apply pre-composition)
- Panic if `requires_full_row` is true (not handled in tiled context; caller must handle separately)

---

## Requirement 4: Document Handle & Concurrent Access

**Requirement 4.1**: Implement `DocumentHandle` struct wrapping thread-safe access to `Document`:
- Uses `arc-swap` crate for lock-free reads from worker threads
- `snapshot() -> Arc<Document>` — O(1) atomic load, no blocking
- `mutate(f: impl FnOnce(&mut Document))` — structural clone, apply mutation, atomic store
- Ensures workers see consistent document snapshots without waiting for writes

**Requirement 4.2**: Validation:
- Snapshot returns consistent state (all reads during one snapshot reflect one moment)
- Multiple concurrent snapshots from different threads do not block each other
- Mutations do not block renders in progress (different snapshots)

---

## Requirement 5: Invalidation Integration

**Requirement 5.1**: Extend `InvalidationEvent` (from Phase 1) to support document structure:
- `LayerRawChanged { layer: LayerId, coords: Vec<TileCoord> }` — pixel data changed (Phase 1)
- `LayerFilterChanged { layer: LayerId }` — filter params or filter stack changed
- `LayerMaskChanged { layer: LayerId, coords: Vec<TileCoord> }` — mask data changed
- `LayerPropsChanged { layer: LayerId }` — opacity/blend_mode/visibility/offset changed
- `LayerStructureChanged { added: Vec<LayerId>, removed: Vec<LayerId> }` — add/remove/reorder layers

**Requirement 5.2**: Implement invalidation logic for layer structure changes:
- Add layer → increment `layer_gen[new_layer]`, mark Composite dirty for all above
- Remove layer → mark Composite dirty for all remaining layers
- Reorder layers → mark Composite dirty for all layers (order affects composition result)
- Group create/remove/reorder → equivalent to layer changes (group has same Composite cache)

**Requirement 5.3**: Integration with Phase 1 TileCache:
- Invalidation calls mark_dirty on TileKey entries without deleting them
- Cascade logic correct (Processed → Composite → ancestors)
- Filter change on adjustment layer → invalidate Processed of adjustment + Composite above

---

## Requirement 6: Tauri Command Definitions

**Requirement 6.1**: Document commands:
- `open_document(path: String) -> Result<DocumentSnapshotDto, EngineError>` — load from disk
- `new_document(width: u32, height: u32) -> Result<DocumentId, EngineError>` — create blank
- `get_document_snapshot(doc_id: DocumentId) -> Result<DocumentSnapshotDto, EngineError>` — metadata + layer tree

**Requirement 6.2**: Layer commands:
- `add_layer(doc_id: DocumentId, kind: LayerKindDto, parent_group: Option<LayerId>, index: usize) -> Result<LayerId, EngineError>`
- `remove_layer(doc_id: DocumentId, layer_id: LayerId) -> Result<(), EngineError>`
- `duplicate_layer(doc_id: DocumentId, layer_id: LayerId) -> Result<LayerId, EngineError>`
- `reorder_layer(doc_id: DocumentId, layer_id: LayerId, new_parent: Option<LayerId>, new_index: usize) -> Result<(), EngineError>`
- `set_layer_props(doc_id: DocumentId, layer_id: LayerId, patch: LayerPropsPatch) -> Result<(), EngineError>`
  - `patch` fields: `opacity: Option<f32>`, `blend_mode: Option<BlendMode>`, `visible: Option<bool>`, `offset: Option<(i32, i32)>`, `name: Option<String>`

**Requirement 6.3**: Filter commands:
- `add_filter(doc_id: DocumentId, layer_id: LayerId, kind: FilterKindDto, index: usize) -> Result<FilterInstanceId, EngineError>`
- `update_filter_params(doc_id: DocumentId, layer_id: LayerId, filter_id: FilterInstanceId, params: FilterParamsDto) -> Result<(), EngineError>` — **synchronous** reply after params applied, tiles updated asynchronously
- `reorder_filter(doc_id: DocumentId, layer_id: LayerId, filter_id: FilterInstanceId, new_index: usize) -> Result<(), EngineError>`
- `remove_filter(doc_id: DocumentId, layer_id: LayerId, filter_id: FilterInstanceId) -> Result<(), EngineError>`
- `set_filter_enabled(doc_id: DocumentId, layer_id: LayerId, filter_id: FilterInstanceId, enabled: bool) -> Result<(), EngineError>`

**Requirement 6.4**: All layer/filter commands must:
- Increment appropriate `generation` (document or layer-specific)
- Call invalidation logic to mark affected tiles dirty
- Post recompute tasks to scheduler with correct priorities
- Return Ok(()) immediately (not wait for tile computation)
- Emit `DocumentStateChanged` event with new revision

---

## Requirement 7: DTOs for Serialization

**Requirement 7.1**: Implement `DocumentSnapshotDto` (what fends receives over invoke):
```rust
pub struct DocumentSnapshotDto {
    pub id: DocumentId,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub layers: Vec<LayerNodeDto>,
    pub palettes: Vec<PaletteId>,
}

pub struct LayerNodeDto {
    pub id: LayerId,
    pub parent_group: Option<LayerId>,
    pub kind: &'static str, // "raster", "adjustment", "group"
    pub name: String,
    pub blend_mode: String,
    pub opacity: f32,
    pub visible: bool,
    pub offset: (i32, i32),
    pub has_mask: bool,
    pub filters: Vec<FilterInstanceDto>,
    pub thumbnail_url: String, // tile://doc/{id}/layer/{layer_id}/stage/composite/l/{max_level}/0/0
}

pub struct FilterInstanceDto {
    pub id: FilterInstanceId,
    pub kind: String,
    pub params: serde_json::Value, // generic JSON for forward compatibility
    pub enabled: bool,
}
```

**Requirement 7.2**: Implement DTOs for command parameters (mirror DTO structures above for input validation).

---

## Requirement 8: Error Handling

**Requirement 8.1**: Implement `EngineError` enum covering:
- `LayerNotFound { layer_id: LayerId }`
- `DocumentNotFound { doc_id: DocumentId }`
- `FilterNotFound { filter_id: FilterInstanceId }`
- `InvalidLayerKind { reason: String }`
- `InvalidFilterParams { reason: String }`
- `IoError { reason: String }`
- Other TBD based on implementation

**Requirement 8.2**: All commands return `Result<T, EngineError>` and propagate errors without panicking.

---

## Acceptance Criteria

1. **Document model**: All structs compile, serialize/deserialize correctly
2. **Tree traversal**: `walk_bottom_to_top()` iterates all layers in correct order for composite stacks
3. **Invalidation**: Changing layer props marks correct tiles dirty; cascade logic verified by tests
4. **Tauri integration**: All 6.1–6.4 commands registered and callable from TypeScript client (mocked for now)
5. **Thread safety**: DocumentHandle concurrent reads/writes verified by clippy + miri (if applicable)
6. **Tests**: 8+ unit tests (layer hierarchy, invalidation cascade, mask application, filter enable/disable), 3+ integration tests (document mutations + scheduler interactions)
7. **Compilation**: `cargo build -p engine-project`, `cargo clippy -p engine-project -- -D warnings`, zero warnings
8. **Documentation**: `cargo doc -p engine-project`, public API documented

---

## Success Criteria

- All requirements 1–8 implemented and passing tests
- Phase 1 tile engine tests still passing (no regressions)
- Document mutations correctly trigger invalidation and scheduler updates
- No panics, all error paths return Result
- Architecture compatible with Phase 3 (filter implementation)

