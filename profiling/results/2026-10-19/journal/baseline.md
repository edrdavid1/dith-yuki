# Journal H9 Step 1 — baseline (current full-dyproj journal)

**Branch:** `perf/journal-h9`  
**Date:** 2026-10-19 (supplemented after Step 2)  
**Machine:** Apple M3, 8 cores, arm64  
**Build:** `cargo test -p dither --release` (no `DITHER_TRACE` / `DITHER_FS_PROF`)  
**Harnesses:** `journal_baseline_harness`, `journal_session_harness`

Z3 TIME_TO_FRESH A/B: **author runs in parallel** (kill switch `DITHER_JOURNAL=0` ready).

## Setup confirmed

| Item | Status |
| --- | --- |
| Base | `fix/h8-composite-ed-park` + cpu-fs-ed-pass |
| `DITHER_JOURNAL=0` | Present (`journal::enabled`) |
| Section 2 facts | Still hold (full dyproj write, no single-flight, tokio sync write) |

## Write cost (release, Raw filled)

| Case | Size | Layers | Wall ms | Zip MiB |
| --- | ---: | ---: | ---: | ---: |
| 4mp_1L | 2000×2000 | 1 | **316–357** | 19.9 |
| 12mp_1L | 4000×3000 | 1 | **864–1034** | 53.6 |
| 25mp_1L | 5984×4172 | 1 | **1882–2037** | 108.8–117.7 |
| 4mp_2L | 2000×2000 | 2 | **466–483** | 29.1 |

## 1. Write counters by source (typical 60 s continuous-edit session)

Model: slider/filter edits every ≪3 s → debounce coalesces to **1 write / 3 s** = 20/min; heartbeat if still dirty ≥30 s since last flush ≈ 2/min; soft discard + signal rare.

```
JOURNAL_SESSION_MIX debounce=20 (83%) heartbeat=2 (8%) soft_discard=1 (4%) signals=1 (4%) total=24
```

| Source | Share | Code |
| --- | ---: | --- |
| Debounce | **~83%** | `runtime.rs` after `DEBOUNCE` (3 s) |
| Heartbeat | **~8%** | `runtime.rs` `flush_stale` / `HEARTBEAT` (30 s) |
| SoftDiscard | **~4%** | `commands.rs` `prepare_soft_discard_sync` |
| Signals | **~4%** | `commands.rs` `flush_all_dirty_journals` |

Reproduce: `cargo test -p dither --release journal_session_typical_mix -- --ignored --nocapture`

## 2. IncompleteRaw in practice

**Mechanism:** `assemble_layer_png` / `assemble_layer_rgba8` looks up every Raw tile in `layer.bounds_l0`; any miss → `ProjectError::IncompleteRaw` (`pixels.rs:114–117`). Journal catches it and skips (`write.rs` IncompleteRaw arm → `stats::note_incomplete_raw_skip`).

**When:**

1. **Import / open race with debounce** — `emit_dirty_doc` schedules a 3 s write while `decompose_image_to_tiles*` is still filling Raw (`document_service.rs:205–214`, `298–307`). If the quiet window ends before all tiles are inserted, the write skips.
2. **Memory pressure eviction** — `evict_inactive_for_pressure_if_needed` after import (`document_service.rs:238`) can drop Raw tiles of background docs; a later heartbeat/debounce on a pressure-evicted doc skips.
3. **Soft discard / signal** on a doc whose Raw was never complete (blank raster, failed import) — same soft skip.

**How often:** harness `journal_baseline_incomplete_raw_skip` → 100% when Raw empty. In interactive use it is intermittent (race window), not every edit — production currently **silent** (warn log only). v2 surfaces this as `journal_blob_pending`.

## 3. Core load during 25 MPix writes

Timed the release test binary alone (no recompile), all four size cases wall **4.27 s**:

| Metric | Value |
| --- | --- |
| real | 4.27 s |
| user | 3.51 s |
| sys | 0.41 s |
| **user/real (avg cores)** | **~0.82** |
| max RSS | ~1.4 GiB |

25 MPix alone is ~1.9–2.0 s of that wall. Encoding is largely single-thread-ish (~1 core equivalent amortized); the problem is **duration × contention with tile workers**, not multi-core saturation.

## 4. Bytes written to disk per minute (continuous editing @ 25 MPix)

| Input | Value |
| --- | --- |
| Zip size (sine-filled Raw + processed composite) | **~109–118 MiB** / write |
| Debounce rate (continuous edit) | **20 writes / min** |
| **Disk write volume** | **~2.2–2.4 TiB / hour** ≈ **~2175–2350 MiB / min** |

```
JOURNAL_BYTES_PER_MIN … zip_mib=117.68 writes_per_min=20 mib_per_min=2353.5
```

This is why the journal must stop rewriting pixels every debounce.

## Concurrency

`journal_baseline_concurrent_peak` → **peak=2** (Debounce ∥ Heartbeat). No single-flight today.

## §9 answers (file:line)

### Q1 — Where do new-layer bytes come from? Exist before TileCache?

Yes. Import paths take an in-memory RGBA `f32` buffer and only then decompose into Raw tiles:

- `install_raster_document`: `rgba_f32: &[f32]` → `decompose_image_to_tiles_at_generation(..., &state.tiles.tile_cache, ...)` — `document_service.rs:191–214`
- `import_raster_layer`: `place_image_at_origin(src_rgba, ...)` then `decompose_image_to_tiles_at_generation(&placed, ...)` — `document_service.rs:254–307`
- Open from `.dyproj`: PNG bytes from zip → `decode_png_to_f32` → `decompose_image_to_tiles` — `project.rs:287–305`

The flat buffer exists only for the duration of that call. Afterward, **TileCache Raw tiles** are authoritative. Journal Step 3 can encode from the import buffer at import time, or later via `assemble_layer_png` (`pixels.rs:49–57`).

### Q2 — How does `raw_rev` change on undo/redo? Blob rewrite?

(Implemented in Step 2.)

- `Layer::raw_rev` is runtime-only (`#[serde(skip)]`) — `layer.rs` (~61–70)
- Bump on import: `document_service.rs` after decompose; on open: `project.rs` `set_layer_raw_rev(..., 1)` after PNG materialize
- **Undo/redo of layer delete:** restores `Arc<Document>` snapshot → **same `raw_rev`**, no bump — `document.rs` test `undo_redo_layer_delete_preserves_raw_rev`. Blob key `(layer_id, raw_rev)` unchanged → **no rewrite**
- **Undo of import:** removes the layer (and its rev); redo restores the snapshot with the same rev

### Q3 — Does open accept a project without `composite.png` / `thumbnail.png`?

**Yes.** Validated by `open_accepts_project_without_composite_or_thumbnail` (`project.rs` tests). Open reads `document.json` + `layers/*` (+ threshold assets) — `project.rs:249–305`. SecureZip **allowlists** composite/thumbnail but does not require them — `secure_zip.rs:414–435`. If `manifest.files` lists them, hashes must match; the test strips `files` so absence is fine.

### Q4 — Windows equivalent of 0600/0700 and directory fsync?

**Not present today.**

- Unix: temp `sync_all`, rename, parent dir `sync_all` — `atomic_write.rs:47–51`, `57–61`
- Windows: `MoveFileExW(... MOVEFILE_WRITE_THROUGH)` — `atomic_write.rs:69–90`
- **No** `chmod` / ACL for 0600/0700 anywhere in `engine-io` (Step 3 must add permissions 0600/0700 and an ACL equivalent)

## How to reproduce

```bash
cargo test -p dither --release journal_baseline_ -- --ignored --nocapture --test-threads=1
cargo test -p dither --release journal_session_ -- --ignored --nocapture --test-threads=1
```

## Step 2 deliverables (this commit)

- `raw_rev` on `Layer` + bump/set helpers on `Document`
- `serialize/journal_state.rs` — `.state` format v1, encode/decode, no pixels
- Unit + CI mutation fuzz + `fuzz/fuzz_targets/fuzz_journal_state.rs`
- Open-without-composite test; undo/redo raw_rev test
