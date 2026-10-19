# Journal H9 Step 1 — baseline (current full-dyproj journal)

**Branch:** `perf/journal-h9` (from `fix/h8-composite-ed-park` + cpu-fs-ed-pass WIP)  
**Date:** 2026-10-19  
**Machine:** Apple M3, 8 cores, arm64  
**Build:** `cargo test -p dither --release` (no `DITHER_TRACE` / `DITHER_FS_PROF`)  
**Harness:** `journal_baseline_harness` (`--ignored --test-threads=1`)

## Setup confirmed

| Item | Status |
| --- | --- |
| Base | `fix/h8-composite-ed-park` @ `b26575f` + cpu-fs-ed-pass strip-owner / B1 gates |
| `DITHER_JOURNAL=0` | Present (`journal::enabled`, `main.rs` setup skip) — from h8 |
| Section 2 facts | Still hold on this branch (full dyproj write, no single-flight, tokio sync write) |

## Instrumentation (behavior-preserving)

- `journal/stats.rs` — atomics: writes by source (debounce / heartbeat / soft_discard / signals / direct), IncompleteRaw skips, kill-switch noops, concurrent peak, write ms samples
- Call sites pass `JournalWriteSource` via `write_journal_for_doc_from`
- Trace log line under `DITHER_TRACE=1` only

## Write cost (release, 1× warm call after Raw fill)

| Case | Size | Layers | Wall ms | Zip MiB | Notes |
| --- | ---: | ---: | ---: | ---: | --- |
| 4mp_1L | 2000×2000 | 1 | **339** | 19.9 | scales with pixels |
| 12mp_1L | 4000×3000 | 1 | **902** | 53.6 | |
| 25mp_1L | 5984×4172 | 1 | **1882** | 108.8 | ~25 MPix |
| 4mp_2L | 2000×2000 | 2 | **490** | 29.1 | +2nd layer PNG + composite |

**Cap:** `JOURNAL_MAX_BYTES` = 512 MiB (none of these hit it).

**Interpretation:** wall time grows roughly with pixel area (~0.08 ms/kPix @ 25 MPix). Matches the known cost model (processed composite rebuild + PNG encode of composite/layers). This is the baseline the v2 redesign must beat (target: state write ≤20 ms, independent of pixel size).

## Concurrency

```
journal_baseline_concurrent_peak → debounce=1 heartbeat=1 peak=2
```

Confirms **no single-flight**: overlapping Debounce + Heartbeat writes reach `concurrent_peak >= 2` (matches dual identical stacks in the sample profile).

## IncompleteRaw skip

```
journal_baseline_incomplete_raw_skip → skips=1
```

When Raw tiles are missing, `write_journal_for_doc` returns `Ok` after counting `incomplete_raw_skips` (silent insurance gap — v2 `journal_blob_pending`).

**When in practice:** import/open races debounce (3 s) before all Raw tiles are in `TileCache`; or eviction under memory pressure mid-session. Soft discard / signal flush can hit the same path.

## Call sources (code map)

| Source | Site |
| --- | --- |
| Debounce | `runtime.rs` after 3 s sleep |
| Heartbeat | `runtime.rs` `flush_stale` |
| SoftDiscard | `commands.rs` `prepare_soft_discard_sync` |
| Signals | `commands.rs` `flush_all_dirty_journals` |
| Direct | harness / tests / `write_journal_for_doc` default |

Per-`doc_id` counts: not histogrammed yet (single-doc harness); live multi-tab dump via `DITHER_TRACE=1` stats JSON on each write.

## TIME_TO_FRESH Z3 (journal on vs `DITHER_JOURNAL=0`)

**Not measured in this harness session** — Z3 TIME_TO_FRESH is GUI / `tile_gen_trace_harness` driven; journal only contends when a dirty debounce/heartbeat fires during the pass.

| Ready | Detail |
| --- | --- |
| Kill switch | `DITHER_JOURNAL=0 npm run tauri:dev:release` |
| Prior Z3 (h8, journal typically on) | GUI median TIME_TO_FRESH ~17.9 s @ 12 MPix (`profiling/results/2026-10-18/trace_viewport/z3_cold_12mp_summary.md`); backend harness ~539 ms |
| Next | Run Stress Z3 ×3 with journal default and ×3 with `DITHER_JOURNAL=0`; compare median ≤2% (acceptance §7.1) |

## §9 open questions (partial answers)

1. **Import bytes before TileCache?** Yes — `install_raster_document` / `import_raster_layer` take `&[f32]` RGBA and call `decompose_image_to_tiles_at_generation` into the cache (`document_service.rs:191–214`, `254–274`). Authoritative buffer exists only for the duration of that call; after that, Raw tiles in `TileCache` are the source. Journal blob encode can use that buffer **during** import, or `assemble_layer_png` later.
2. **`raw_rev` / undo:** not implemented yet (Step 2).
3. **Open without composite/thumbnail?** `open_project_from_bytes` only requires `document.json`, `layers/*`, threshold assets (`project.rs:249–298`). SecureZip **allowlists** composite/thumbnail but does not require them (`secure_zip.rs:414–435`). Still need an explicit open test of a zip without those entries in Step 2/4.
4. **Windows 0600/0700 / dir fsync:** `atomic_write` on Unix does parent `sync_all` after rename (`atomic_write.rs:47–51`). Windows uses `MoveFileExW` with `MOVEFILE_WRITE_THROUGH`; **no** explicit ACL 0600/0700 today — Step 3 must add.

## How to reproduce

```bash
cargo test -p dither --release journal_baseline_ \
  -- --ignored --nocapture --test-threads=1
```

## Residual risks / next

- Step 1 GUI Z3 A/B still open (kill switch ready).
- Foundation ED-pass + h8 merge compile fixes (`block_on` 3-arg) are on this branch; commit with Step 1.
- Step 2: `.state` writer without pixels + `raw_rev`.
