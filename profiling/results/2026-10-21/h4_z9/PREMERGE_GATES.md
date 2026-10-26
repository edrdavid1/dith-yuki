# Pre-merge gates (H4 Step B follow-ups)

**Branch:** `diagnose/h4-stale-gen-z9`  
**Base:** `origin/main` (already contains #66 / H8; #65 closed as subsumed)  
**Date:** 2026-10-21

## Code changes (this pass)

1. **Slow BE / `refetchDone` trap:** collapsed retry that still returns stale emits `wake_retry_returned_stale` and stays pending; safety timer clears `refetchDone` and re-fetches (no pan/zoom required).
2. **Z9 K≥4 + shifted:** watches first 4 L0 tiles in request order; `z9_rapid_param_shifted_viewport` (UL ≠ 0/0).
3. **maxKnownGen ready-only:** no bump from filter-apply `doc_gen`, ed-progress target, or 202 expected gen; only tile-ready / fresh 200.
4. **CI sequential:** PR CI runs Z9 smoke tests one-at-a-time with `--test-threads=1`; nightly runs ×50.

## Harness results (release)

| Test | Result |
| --- | --- |
| `z9_rapid_param_composite_gen` | ok — watched=4 ul=0/0/0 stale=0 |
| `z9_rapid_param_shifted_viewport` | ok — ul=0/2/1 watched=4 |
| `z9_slow_be_12mp_stale_retry` | ok — wall≈2.2s, stale_retry_ev=4, max_tile_fetch=3, quiet, lag=0 |

## Release-prompt gates (local)

| Gate | Result |
| --- | --- |
| `cargo fmt --all -- --check` | ok |
| `cargo clippy … -- -D warnings` (CI package set) | ok |
| `npm test` (frontend) | ok — 568 |
| `cargo test --workspace` | one flake `graph_gpu_matches_cpu_for_pixel_sizes` under parallel load; green alone |
| `cargo test -p dither -- --test-threads=1` | ok — 298 passed |
| Z9 smoke (composite / shifted / slow12mp) sequential | ok |
| `z3_cold_small_composite_holes_zero` | ok |

**Status:** GREEN for merge of H4 Step B follow-ups (uncommitted on branch; GUI Z1–Z7 / Z_INJECT left to author).

## Author

GUI Z1–Z7 / Z_INJECT — **author**.
