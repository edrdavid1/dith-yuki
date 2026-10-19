# Journal H9 Step 7 — failure tests + after measurements

**Branch:** `perf/journal-h9`  
**Date:** 2026-10-19  
**Machine:** Apple M3 (same as Step 1 baseline)  
**Build:** `cargo test -p dither --release` (no `DITHER_TRACE`)

Step 6 (thumbnail-on-save) is deferred pending separate confirmation.

## §6 invariant tests (green)

| # | Invariant | Coverage |
| --- | --- | --- |
| 1 | No corruption at write phases | `journal::fail_tests::*` failpoints: before blob rename, after blob / before state, after state / before meta |
| 2 | State refs only on-disk blobs | write order + assemble hash check |
| 3 | One blob per `(layer_id, raw_rev)` | `second_write_same_raw_rev_does_not_rewrite_blob` |
| 4 | No composite/thumbnail in journal path | write path uses `build_journal_state` only (no `save_project_to_bytes`) |
| 5 | Single-flight | runtime `try_begin_write` / coalesce (Step 3) |
| 6–7 | Recovery equivalence | `recovery_equivalence_document_and_raw_rev`, assemble round-trip |
| 8 | Old format + unknown `.state` version | legacy claim path; `rejects_newer_version` in engine-project |
| 9 | Fuzz `.state` | existing `fuzz_journal_state` |
| 10 | Soft discard / signals flush | soft-discard reuse test; signal path unchanged |
| 11 | No tile-pipeline locks on state write | state path does not take `ed_frontier` / strip gate |

Corrupt recover restores files and surfaces a dialog error (Steps 4–5).

```text
cargo test -p dither journal::   # 16 passed (debug)
```

## After measurements (release harness)

```text
cargo test -p dither --release journal_baseline_sizes_release \
  -- --ignored --nocapture --test-threads=1
```

| Case | first_ms (blob+state) | rewrite_ms (state only) | state B | blobs MiB | total MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| 4mp_1L | 134 | **9.2** | 1890 | 9.2 | 9.2 |
| 12mp_1L | 153 | **7.9** | 1892 | 27.6 | 27.6 |
| 25mp_1L | 285 | **8.2** | 1892 | 57.5 | 57.5 |
| 4mp_2L | 126 | **9.1** | 3538 | 18.4 | 18.4 |

### vs Step 1 baseline (full dyproj)

| Case | Before wall ms | After first / rewrite | Before zip MiB | After total MiB |
| --- | ---: | --- | ---: | ---: |
| 4mp_1L | 316–357 | 134 / **9** | 19.9 | 9.2 |
| 12mp_1L | 864–1034 | 153 / **8** | 53.6 | 27.6 |
| 25mp_1L | 1882–2037 | 285 / **8** | 109–118 | 57.5 |

- **State rewrite** is ~8–9 ms and **does not grow with pixel size** (acceptance §7.2).
- **Sustained disk**: after the one-time blob, debounce refreshes only ~2 KiB state → ≪1 MiB/min vs ~2 GiB/min baseline.
- **Concurrent peak:** 1 (single-flight).

## Pre-merge follow-up

See [`premerge.md`](./premerge.md): 25 MPix restore **997 ms**, kill -9 **200/200**, Raw/Processed equivalence, clean-doc guard, delete/undo blob reuse. Windows build still author/CI.

## Still author-owned

- Z3 TIME_TO_FRESH A/B with `DITHER_JOURNAL=0` (≤2% delta).
- Interactive pan/zoom feel; core ≤5% during interactive work.
- Full Z1–Z7 / ED golden regression suite after merge.
- Windows `cargo test` matrix.

## Residual risks

- First-open blob encode still O(pixels) on cold journal (background thread, but competes briefly).
- Soft-discarded journals kept until Discard / clean exit (by design).
- Step 6 thumbnail path deferred to a third PR after merge.
