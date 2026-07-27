# Phase 2 Specification: Document Model & Filter Application

## What Phase 2 Covers

Phase 2 integrates the Phase 1 tile engine with the application layer by implementing:

1. **Document Model** — the core data structure containing layers, groups, masks, and filters
2. **Layer Hierarchy** — recursive tree structure with groups and lazy traversal
3. **Mask System** — alpha-modulating external raster layers
4. **Filter Pipeline** — filter instances with parameters and enable/disable toggling
5. **Invalidation Integration** — marking tiles dirty when document changes
6. **Tauri API Commands** — 7 endpoints for document/layer/filter manipulation

## Key Files Created

All files are in `.kiro/specs/dither-engine-phase2-document-filters/`:

- **requirements.md** — 8 detailed requirements (Document model, Layer hierarchy, Masks, Filters, DocumentHandle, Invalidation, Tauri commands, DTOs, Error handling)
- **design.md** — Architecture decisions, concurrency model, module structure, design rationale
- **tasks.md** — 11 implementation tasks organized in 7 dependency waves, with acceptance criteria

## Highlights

### Architecture
- **New crate**: `engine-project` for document/layer/filter data structures
- **DocumentHandle**: Uses `arc-swap` for lock-free concurrent reads from worker threads
- **Stable IDs**: Layers and filters identified by stable UUID, not indices
- **Lazy traversal**: Tree walking via iterators, no allocations
- **Two-level generations**: Document + per-layer, enabling selective invalidation

### Key Decisions
1. **Dirty marking** (not deletion) — tiles stay in cache while stale, showing instant feedback
2. **Shallow cloning** on mutation — Document cloned (metadata only, ~100 μs), not pixels
3. **Invalidation cascade** — Layer filter change → mark Processed + Composite; props change → only Composite
4. **Filter stack per layer** — filters apply in sequence, with `requires_full_row` escape hatch for non-tiled effects
5. **Masks as layers** — External MaskStorage references another layer, no special cache stage

### Modules (Phase 2)
- `types.rs` — DocumentId, LayerId, FilterInstanceId, BlendMode, etc.
- `document.rs` — Document, DocumentHandle, GenerationTracker
- `layer.rs` — Layer, LayerGroup, LayerNode, tree traversal
- `mask.rs` — MaskRef, MaskStorage, apply_mask function
- `filter.rs` — FilterInstance, FilterKind, FilterParams, apply_filter_to_tile
- `error.rs` — EngineError enum, Display impl
- `dto.rs` — DocumentSnapshotDto, LayerNodeDto, serialization
- `lib.rs` — Public API, re-exports

### Tauri Commands (7 endpoints)
- `new_document(width, height) -> DocumentId`
- `get_document_snapshot(doc_id) -> DocumentSnapshotDto`
- `add_layer(doc_id, kind, parent_group, index) -> LayerId`
- `set_layer_props(doc_id, layer_id, patch) -> Ok()`
- `add_filter(doc_id, layer_id, kind, index) -> FilterInstanceId`
- `update_filter_params(doc_id, layer_id, filter_id, params) -> Ok()`
- (Placeholder for `open_document`, `remove_layer`, `reorder_layer`, etc. — full list in requirements §6)

### Testing
- 8+ unit tests (document creation, tree walk, masks, filters, error serialization, DTO round-trip)
- 3+ integration tests (invalidation cascade, group hierarchy, concurrent access)
- All Phase 1 tests remain passing (no regressions)

## Success Criteria

- [ ] `engine-project` crate compiles, zero clippy warnings
- [ ] All 8 modules implemented
- [ ] 11 tasks completed
- [ ] 8+ unit tests passing
- [ ] 3+ integration tests passing
- [ ] All 7 Tauri commands callable
- [ ] Documentation generated

## How to Proceed

Phase 2 is ready to implement. You have three options:

1. **Start immediately** — Begin with Task 1 (create crate + types)
2. **Review design first** — Read `design.md` for architecture rationale and concurrency model
3. **Adjust scope** — If you want to change any requirements or adjust the task breakdown, let me know

The spec follows the same design-first, requirements, then tasks structure as Phase 1. Each task has clear acceptance criteria and dependencies are organized in waves for parallel execution.

## Relationship to Phase 1

Phase 2 **uses** Phase 1 but **does not modify** it:
- Phase 1 deliverables (TileCache, scheduler, pyramid) remain unchanged
- Phase 2 extends `InvalidationEvent` enum in Phase 1 (backward compatible)
- Phase 1 tests remain green

## Next Phases

- **Phase 3**: Implement filter algorithms (Curves, Dither, LUT3D, Glitch effects)
- **Phase 4**: Undo/redo history
- **Phase 5**: Color pipeline (profiles, conversions, rendering)
- **Phase 6**: Project file format and disk storage

---

Spec location: `.kiro/specs/dither-engine-phase2-document-filters/`

Ready to execute?
