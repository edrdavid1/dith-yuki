# B1 pre-merge gates

**Branch:** `perf/cpu-fs-ed-pass`  
**No B2–B4.**

## 1. Gate deadlock / non-blocking wait

### Behavior
- Strip gate is `try_claim` + short `wait_timeout` slices (~10 ms), then **`EngineError::EdStripBusy`**.
- Worker re-parks on **sibling** Processed tiles in the row (never self).
- Owner `Drop` notifies Condvar; gates cleared on invalidate / doc evict.

### Tests (release)
| Test | Result |
| --- | --- |
| `ed_strip_gate_workers_1_2_8_single_and_multi_row` | ok |
| `ed_strip_gate_mid_cancel_releases_waiters` | ok |
| `ed_strip_owner_one_calc_per_ty` | ok |
| `fs_ed_l0_5984x4172` | strip_runs/line=**1.00**, wall≈0.73 s |

## 2. Bitwise Processed vs sequential strip-once (pre-B1 result)

Pre-B1 recomputed the same strip N×/row; **output equals one strip/row** when deterministic. Golden = sequential `compute(min_x, ty)` per row; concurrent 4–8 worker drain must match bit-for-bit.

| Case | Result |
| --- | --- |
| 300×200 FS (not multiple of 256) | ok |
| 520×400 FS `pixel_size=2` | ok |
| Levels → Atkinson → Adjust stack | ok |
| All 13 ED kernels @ 512×320 | ok |

Test: `b1_bitwise_vs_sequential_non_multiple_and_stacks_and_kernels`.

Also: `ed_wavefront_matches_row_major_reference` ok.

## 3. ed-progress / partial display granularity

This branch has no FE `ed-progress` emitter yet (lives on h8/split work). Proxy: sample `done = count(fresh L0 Processed)` during drain of 2048×1536 (8×6 tiles).

| Check | Result |
| --- | --- |
| Monotonic `done` | ok |
| Jump ≤ row width (`tx_n`) | ok |
| Reaches `total` | ok |

Test: `b1_ed_progress_row_granularity_large_doc`.  
Row publish jumps by up to one full ty (e.g. +8), which is correct for strip ownership and does not break a done/total indicator.

## Reproduce

```bash
cargo test -p dither --release ed_strip_gate_ -- --test-threads=1
cargo test -p dither --release ed_strip_owner_one_calc -- --test-threads=1
cargo test -p dither --release b1_bitwise -- --test-threads=1
cargo test -p dither --release b1_ed_progress -- --test-threads=1
cargo test -p dither --release ed_wavefront -- --test-threads=1
DITHER_FS_PROF=1 cargo test -p dither --release fs_ed_l0_5984x4172 -- --ignored --nocapture --test-threads=1
```
