# Phase 2 — Remaining Work (Optional Enhancements)

**Status**: Phase 2 core is complete. Below are optional items for enhancement before Phase 3.

---

## Optional Enhancements

### 1. Fix Clippy Warnings in engine-project

**Current**: 3 clippy warnings (pre-existing from Task 1–2):

```
⚠️ LayerPropsPatch Default impl can be derived
⚠️ Collapsible else-if block in reorder_layer
⚠️ Use &mut [LayerNode] instead of &mut Vec<LayerNode>
```

**Impact**: None (code works correctly, warnings are style-only)

**Effort**: ~5 minutes to fix

---

### 2. Add Filter Commands to Tauri API

**What**: Extend `/crates/app/src/commands.rs` with filter manipulation:

```rust
#[tauri::command]
pub fn add_filter(
    layer_id: u32,
    kind: String,
    params: serde_json::Value,
    state: State<AppState>,
) -> Result<FilterInstanceIdResponse, String>;

#[tauri::command]
pub fn remove_filter(layer_id: u32, filter_id: String, state: State<AppState>) -> Result<(), String>;

#[tauri::command]
pub fn update_filter_params(
    layer_id: u32,
    filter_id: String,
    params: serde_json::Value,
    state: State<AppState>,
) -> Result<(), String>;
```

**Prerequisite**: Need to implement filter mutation functions in `/crates/engine-project/src/commands.rs` first (add_filter, remove_filter, update_filter_params)

**Effort**: ~2 hours (filter functions + Tauri bindings + tests)

**Blocking**: Phase 3 (filter algorithms) can proceed without this; it's optional API extension

---

### 3. File I/O Commands

**What**: Implement open_document, save_document commands

**Current**: Stubbed (returns error)

**Implementation**:
- `open_document(path: String)` → loads JSON, reconstructs Document
- `save_document(path: String)` → serializes Document to JSON

**Effort**: ~1 hour (uses existing DTO serialization)

**Blocking**: Phase 6 (project format) can be done independently

---

### 4. Extend Unit Tests

**What**: Add more targeted tests for edge cases:

```
- Remove layer from nested group
- Reorder layer across parent boundaries
- Modify group properties (not just leaf layers)
- Concurrent mutations (stress test)
- Large document (1000+ layers)
```

**Current Coverage**: 40 unit tests (sufficient for MVP)

**Effort**: ~1 hour for 5 additional tests

---

### 5. Performance Benchmarks

**What**: Add criterion benchmarks for Tauri command latency:

```rust
criterion::black_box! {
    bench_add_layer,
    bench_set_layer_props,
    bench_reorder_layer,
    bench_get_document_snapshot,
}
```

**Effort**: ~1 hour

**Why**: Establish baseline for Phase 3+ optimization

---

### 6. Documentation

**What**: Add inline documentation (already done) + examples

**Optional**:
- README.md for Phase 2 API
- Example Tauri IPC calls (TypeScript/JavaScript)
- Architecture diagrams in docs/

**Effort**: ~1 hour

---

## Recommended Priority

### Must-Have for Phase 3
- ✅ Phase 2 core complete

### Nice-to-Have Before Phase 3
- Optional: Fix clippy warnings
- Optional: Add filter commands (can be done during Phase 3)

### Post-Phase 3 (Phase 4+)
- File I/O (Phase 6)
- Undo/redo (Phase 4)
- Performance optimization (after Phase 3 algorithms)

---

## Summary

Phase 2 is **feature-complete** for the document model. The optional items above are enhancements that don't block Phase 3. Recommend proceeding to **Phase 3 (Filter Algorithms)** while optionally addressing these items in parallel or as Post-Phase 3 polish.

