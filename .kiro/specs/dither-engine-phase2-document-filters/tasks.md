# Implementation Plan: Phase 2 — Document Model & Filter Application

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
      "tasks": ["2", "3", "4"],
      "dependsOn": ["1"]
    },
    {
      "wave": 3,
      "tasks": ["5", "6"],
      "dependsOn": ["2", "3", "4"]
    },
    {
      "wave": 4,
      "tasks": ["7"],
      "dependsOn": ["1", "2", "3", "4", "5", "6"]
    },
    {
      "wave": 5,
      "tasks": ["8", "9"],
      "dependsOn": ["1", "2", "3", "4", "5", "6", "7"]
    },
    {
      "wave": 6,
      "tasks": ["10"],
      "dependsOn": ["1", "2", "3", "4", "5", "6", "7", "8", "9"]
    },
    {
      "wave": 7,
      "tasks": ["11"],
      "dependsOn": ["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]
    }
  ]
}
```

## Overview

Implement the Document model, layer hierarchy, mask system, filter pipeline, and Tauri API commands. This phase bridges Phase 1 tile engine with UI/application logic, enabling document manipulation and filter-based rendering.

**Phase 2 Scope**:
- Document & DocumentHandle (thread-safe access)
- Layer & LayerGroup hierarchy with lazy tree traversal
- Mask system (alpha modulation)
- Filter instance model (parameters, enable/disable)
- Invalidation for layer structure changes
- 7 Tauri command endpoints (document, layer, filter operations)
- DTOs for serialization
- Error handling

**Success Criteria**:
- New crate `engine-project` compiles without warnings
- All 7 modules (document, layer, mask, filter, error, dto, and lib) implemented
- 8+ unit tests passing
- 3+ integration tests passing
- All Tauri commands callable (mocked for now, no file I/O)
- Zero clippy warnings: `cargo clippy -p engine-project -- -D warnings`
- Documentation: `cargo doc -p engine-project`

---

## Tasks

- [ ] 1. Create `engine-project` crate and types module. Create `/crates/engine-project/` structure with `Cargo.toml`, `src/lib.rs`, `src/types.rs`. Define core types: `DocumentId`, `LayerId`, `FilterInstanceId`, `MipLevel`, `TileBounds`, `BlendMode` (15+ variants: Normal, Multiply, Screen, Overlay, Darken, Lighten, ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion, Plus 4 placeholders). All types implement Clone, Copy (where applicable), PartialEq, Eq, Hash, Debug, Serialize/Deserialize. Reference: design.md §1, requirements.md §1–3. Dependencies: serde, serde_json, derivative (if needed). Acceptance: `cargo build -p engine-project` succeeds, types compile and re-export from lib.rs. Depends on: (none)

- [ ] 2. Implement Document & DocumentHandle. File `/crates/engine-project/src/document.rs` with struct `Document { id, width, height, color_profile, root: Vec<LayerNode>, palettes, revision, generations }` and `DocumentHandle` wrapping `ArcSwap<Document>`. Methods: `new(width, height) -> Document`, `snapshot() -> Arc<Document>` (lock-free), `mutate(f)` (clone, mutate, store). Implement `GenerationTracker` (two-level: document_gen, layer_gen). Acceptance: Document snapshots are consistent, DocumentHandle mutations atomic, multiple concurrent snapshots work without blocking. Reference: design.md §8, requirements.md §1.1. Depends on: Task 1

- [ ] 3. Implement Layer & LayerNode hierarchy. File `/crates/engine-project/src/layer.rs` with enum `LayerNode { Leaf(Layer), Group(LayerGroup) }`, struct `Layer` (id, name, kind, blend_mode, opacity, visible, offset, mask, filters, bounds_l0), struct `LayerGroup` (id, name, blend_mode, opacity, visible, mask, children). Implement tree traversal function `walk_bottom_to_top(nodes: &[LayerNode]) -> impl Iterator<Item = LayerRef>`. Acceptance: Traversal iterates all layers in correct bottom-to-top order, no allocations (lazy), groups emit GroupStart/GroupEnd markers. Reference: design.md §5, requirements.md §1.2–1.5. Depends on: Task 1, Task 2

- [ ] 4. Implement Mask system. File `/crates/engine-project/src/mask.rs` with struct `MaskRef { storage, enabled, inverted }`, enum `MaskStorage { External(LayerId), EmbeddedVector(Vec<Stroke>) }`. Implement function `apply_mask(tile: &PixelTile, mask: Option<&MaskRef>, coord: TileCoord) -> PixelTile` (multiply alpha, handle invert). Acceptance: Mask disabled → tile unchanged, inverted mask flips alpha, alpha multiplication preserves tile channel data. Reference: design.md §7, requirements.md §2. Depends on: Task 1

- [ ] 5. Implement Filter model & application. File `/crates/engine-project/src/filter.rs` with struct `FilterInstance { id, kind, params, enabled, requires_full_row }`, enum `FilterKind { Curves, Levels, Placeholder }`, enum `FilterParams { Curves, Levels, Placeholder }`. Implement function `apply_filter_to_tile(tile: &PixelTile, filter: &FilterInstance, stage: CacheStage) -> PixelTile`. Acceptance: Disabled filter returns tile unchanged, placeholder filters serialize/deserialize, requires_full_row triggers panic if applied to tiled context (catches misuse). Reference: design.md §6, requirements.md §3. Depends on: Task 1

- [ ] 6. Implement invalidation integration. File `/crates/engine-tiles/src/invalidation.rs` (extend Phase 1): Add `InvalidationEvent` variants for document changes (LayerFilterChanged, LayerMaskChanged, LayerPropsChanged, LayerStructureChanged). Extend `invalidate()` function to handle new events. Implement `cascade_composite()` to mark Composite tiles dirty for all layers above changed layer. Acceptance: Changing layer props marks only Composite dirty (not Processed), filter change marks Processed + Composite, cascade logic correct (verified by tests). Reference: design.md §4, requirements.md §5. Depends on: Task 2, Task 3

- [ ] 7. Error handling module. File `/crates/engine-project/src/error.rs` with enum `EngineError` (LayerNotFound, DocumentNotFound, FilterNotFound, InvalidLayerKind, InvalidFilterParams, IoError, etc.). Implement Display + From traits for standard Rust errors. Acceptance: All errors serializable to JSON (via serde), impl From<IoError>, impl Display. Reference: requirements.md §8. Depends on: Task 1

- [ ] 8. Implement DTOs & serialization. File `/crates/engine-project/src/dto.rs` with struct `DocumentSnapshotDto`, `LayerNodeDto`, `FilterInstanceDto`. Implement conversion functions `document_to_dto(&Document) -> DocumentSnapshotDto`, `to_layer_node_dto()`. Acceptance: Round-trip serialization works (to JSON and back), thumbnail_url computed as tile:// URL. Reference: design.md §3, requirements.md §7. Depends on: Task 2, Task 3, Task 5

- [ ] 9. Tauri command registration. File `/crates/app/src/commands.rs` (new) with 7 commands: `open_document`, `new_document`, `get_document_snapshot` (§6.1), `add_layer`, `set_layer_props`, `reorder_layer` (§6.2), `add_filter`, `update_filter_params` (§6.3). Each command must: (a) acquire DocumentHandle from app state, (b) validate input, (c) call invalidation/scheduler, (d) return Ok(dto) or Err(EngineError). Emit `EngineEvent::DocumentStateChanged { revision }` after mutation. Acceptance: All commands type-check, instantiate with default test data, return valid DTOs. Note: File I/O (open_document) stubbed for now. Reference: design.md §8, requirements.md §6. Depends on: Task 2, Task 3, Task 5, Task 6, Task 7, Task 8

- [ ] 10. Unit tests. Write tests in respective modules: `document.rs` (2 tests: creation, revision increment), `layer.rs` (2 tests: tree walk order, group nesting), `mask.rs` (1 test: mask enable/disable/invert), `filter.rs` (1 test: filter enable/disable), `error.rs` (1 test: error serialization), `dto.rs` (1 test: round-trip DTO). Target 8+ tests total. All tests pass `cargo test -p engine-project`. Reference: requirements.md Acceptance Criteria. Depends on: Task 1–9

- [ ] 11. Integration tests & verification. File `/crates/engine-project/tests/integration_test.rs` with 3+ tests: **Test 1** (Document mutation → invalidation): Create document, add layers, change filter params, verify invalidation event fired and scheduler queued tasks. **Test 2** (Layer hierarchy + groups): Create document with groups, change group opacity, verify Composite cascade. **Test 3** (DocumentHandle concurrent reads): Spawn 3 threads reading snapshots while UI thread mutates, verify no panics and consistency. Build & test: `cargo build -p engine-project`, `cargo test -p engine-project`, `cargo clippy -p engine-project -- -D warnings`, `cargo doc -p engine-project`. All tests pass, zero clippy warnings, docs generate. Create `/PHASE_2_SUCCESS_REPORT.md` with summary (deliverables, test counts, architecture notes, next steps). Acceptance: All builds clean, all tests pass, zero warnings, Phase 1 tests still pass. Depends on: Task 1–10

---

## Notes

**Phase 2 Architecture**:
- New crate: `/crates/engine-project/src/` with 6+ modules (document, layer, mask, filter, error, dto, lib)
- Extends Phase 1: Minimal changes to `engine-tiles/src/invalidation.rs`, no changes to existing Phase 1 tests
- Tauri integration in `/crates/app/src/commands.rs` (new file)

**Dependencies** (add to Cargo.toml):
- `serde`, `serde_json`: Serialization
- `arc-swap`: Lock-free document snapshots
- `arc` (or `crossbeam-epoch`): Optional, if using more advanced atomic structures (for now, just `arc-swap` + DashMap from Phase 1)
- Phase 1 exports: Use `engine-tiles` crate types (TileKey, CacheStage, etc.)

**Key Design Decisions**:
1. Stable IDs over indices (LayerId, FilterInstanceId)
2. `arc-swap` for lock-free reads, structural clone on write
3. Lazy tree traversal (no Vec allocation per walk)
4. Dirty marking, not deletion (Phase 1 + Phase 2 together)
5. Two-level generation tracking (document + per-layer)
6. Masks as alpha-modulating External layers
7. Filters as stack per layer, with requires_full_row escape hatch

**Known Limitations**:
1. File I/O stubbed (Phase 6 adds project format)
2. Vector masks placeholder only (Phase 3+ adds rasterization)
3. Undo/redo not implemented (Phase 4)
4. Blend modes: 15 defined, remainder as placeholders (phase 5+ adds rendering)

**Integration Points**:
- Phase 1 (tiles): TileCache, scheduler, pyramid downsampling — Phase 2 uses but doesn't modify
- Phase 3 (filters): Implement actual filter algorithms (Curves, Dither, LUT3D) — Phase 2 just validates/stores parameters
- Phase 5 (color): Color profile references, RGB/Lab/CMYK conversions — Phase 2 stores ColorProfileRef as opaque
- Phase 6 (project format): Serialize/deserialize Document + tile cache to disk — Phase 2 ready for it

**Next Phase (Phase 3)**:
- Implement actual filter algorithms (Curves apply, Dither apply, Lut3D apply, Glitch, etc.)
- Add `requires_full_row` handling for row-based glitches
- Integrate filters into tile generation pipeline (composite_tile calls apply_filter)
- Benchmark filter apply performance per tile

**References**:
- tile-engine-architecture.md: Phase 1 tile engine (§5–6 on scheduler, composition)
- tauri-api-document-model.md: Full API specification and data model
- design.md: Architectural decisions and rationale
- requirements.md: Detailed acceptance criteria
