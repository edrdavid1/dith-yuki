# Journal H9 — pre-merge checklist

**Branch:** `perf/journal-h9`  
**Date:** 2026-10-19  
**Machine:** Apple M3 (author Z3 A/B + full regression still pending)

## Results

| # | Check | Result |
| --- | --- | --- |
| 1 | 25 MPix restore (assemble+open → recalculation can start) | **997 ms** (≤2 s) |
| 2 | Raw pixel equality + Processed tile hashes (import + `.dyproj`) | **pass** (`journal::premerge_tests::equivalence_*`) |
| 3 | Real `kill -9` loop ≥200 random delays | **200/200** old-or-new valid state |
| 4 | Clean doc writes nothing; first dirty creates blobs | **pass** (+ guard in `write_journal_for_doc_from`) |
| 5 | Windows / atomic replace / perms | see below |
| 6 | delete → write → undo → write: blob not re-encoded | **pass** |

### Restore timing (release)

```text
cargo test -p dither --release journal_premerge_restore_time_25mp \
  -- --ignored --nocapture --test-threads=1
```

```
JOURNAL_RESTORE_25MP assemble_ms=278.0 open_ms=719.3 total_ms=997.2
  width=5984 height=4172 raw_tiles=408
```

### Kill -9 loop

```text
JOURNAL_KILL_ITERS=200 cargo test -p dither --release journal_premerge_kill9_crash_loop \
  -- --ignored --nocapture --test-threads=1
```

Parent seeds a valid journal, spawns the same test binary as a child writer, `kill -9` after 1–80 ms, then `startup_cleanup` + decode + assemble. All 200 iterations recovered.

### Windows / durability (item 5)

**This pre-merge run was macOS-only.** Windows CI / a Windows host should run:

```text
cargo test -p dither journal::
cargo test -p engine-io atomic_write
```

On-disk contracts (already implemented, covered by unit tests on Unix + `engine-io` on both):

| Concern | Implementation |
| --- | --- |
| Atomic file replace | `engine_io::atomic_write`: temp → `fsync` → `rename` (POSIX) / `MoveFileExW(MOVEFILE_REPLACE_EXISTING \| WRITE_THROUGH)` (Windows) |
| Blob write | `journal::fsutil::write_blob_atomic`: temp → `sync_all` → rename (+ dir `fsync` on Unix) |
| Permissions | Unix **0600** files / **0700** dirs (`atomic_write_private` / `ensure_dir_private`). Windows: ACL tightening deferred; durability via `WRITE_THROUGH` |
| Directory fsync | Unix only after rename; Windows relies on `MOVEFILE_WRITE_THROUGH` |

Unit coverage: `journal::premerge_tests::atomic_write_private_sets_unix_permissions`, `blob_atomic_replace_is_durable_rename`, `engine_io::atomic_write` tests.

Mark as **verified on macOS**; Windows build/tests still required before calling the platform matrix complete.

## Step 6

Thumbnail-on-save remains a **third PR after merge**, only when the author confirms.

## Author-owned

- Z3 TIME_TO_FRESH A/B (`DITHER_JOURNAL=0`)
- Full Z1–Z7 / ED golden / save-export regression
