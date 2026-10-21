# Step A — H4 stale tile after param change (diagnostics only)

**Branch:** `diagnose/h4-stale-gen-z9`  
**Date:** 2026-10-21  
**No production fetch/draw behavior changes** (trace + Stress Z9 + harness sim only).

## Hypothesis (as stated)

After dither param changes, tile (often `0/0/0`) stays on a previous iteration; pan/zoom heals.

Proposed mechanism:

1. `tile-ready` / `ed-progress` wake is **skipped while a wake fetch is `inflight`**
2. A **previous generation** 200 is accepted as final because FE only checks **`tileRev`**, not server gen (**H4**)
3. **`tileRev` does not increase** on filter change

## Instrumentation added

| Event | Where | Fields |
| --- | --- | --- |
| `wake_skipped_inflight` | `TileCanvas` (tile-ready + ed-progress/safety coalesce) | tile, `cause`, `pending_reason`, `tileRev` |
| `tile_drawn` | `TileCanvas` | `gen`, `details.source` (`fresh`/`stale`/`pyramid`/`lod_fallback`), `tileRev` |
| `tile_ready_emitted` | `pending_tiles` (+ existing worker path) | **real `doc_gen` / `gen`** (was wrongly `doc_id`) |
| Worker decode | `tileWorker` under `trace` | `stale` / `pyramid` on `tile-decoded` |
| Stress **Z9** | `viewportStressRunner` + panel | 8 param pulses × 50 ms (~400 ms), then quiet |
| Harness `z9_rapid_param_composite_gen` | `tile_gen_trace_harness` | BE quiet + FE-gate **simulation** of skip |

## Runs

### Release (`cargo test -p dither --release z9_rapid_param… --ignored`)

```
[z9] profile=release quiet=true live=8 c00=8 lag=0 wake_skip=8 wake_fetch=1
```

`events_z9_release.jsonl` / `z9_release.md`

### Dev (same test, debug build)

```
[z9] profile=dev quiet=true live=8 c00=8 lag=0 wake_skip=8 wake_fetch=1
```

Same pattern (FE gate sim + BE cache catch-up).

### FE unit model

`h4WakeRaceChain.test.ts` — stuck chain for `0/0/0`:

```
tile_ready_emitted@41
tile_ready_emitted@42
wake_skipped_inflight@42
tile_drawn@41
```

## Event chain for stuck tile `0/0/0` (release harness, FE-gate sim)

| seq | event | gen | note |
| ---: | --- | ---: | --- |
| 368 | `tile_ready_emitted` | 0 | pre-burst quiet |
| 394 | `z9_burst_begin` | — | 8×50 ms |
| 451 | `fetch_tile_posted` | 0 | first wake → **inflight** |
| 500 | `tile_ready_emitted` | 1 | newer Composite |
| 524 | **`wake_skipped_inflight`** | 1 | gate holds |
| … | pulses 1…7 | 2…8 | each new ready **skipped** |
| 1869 | `wake_skipped_inflight` | 8 | last skip |
| 1931 | `z9_burst_end` | — | fetched=1 skipped=8 |
| 1932 | `z9_result` | — | BE `c00=8` matches `live=8` |

## Verdict

| Claim | Result |
| --- | --- |
| Wake skipped due to inflight | **Confirmed** (code path + Z9 sim: 8 skips / 1 fetch; FE `decideEventDrivenWake`) |
| Previous gen treated as final (H4) | **Confirmed as mechanism** — decode accepts on `tileRev` only; model shows draw@41 after skip@42. Live GUI Z9 in Stress panel should emit the same `wake_skipped_inflight` + `tile_drawn` source/gen. |
| `tileRev` does not bump on filter | **Refuted** — `document-changed` / `filter_updated` increments `tileRevRef` and clears event-driven pending. Stuck tiles are not explained by a frozen rev; they are explained by **rev-matched decode of an older `X-Tile-Generation` after a missed wake**. |
| BE cache stuck after quiet | **Refuted in harness** — after silence, Composite(0,0) matches live gen (`lag=0`). Pan/zoom healing is consistent with FE re-request, not BE never computing. |

## Step B (do not start until you confirm)

Optional `X-Tile-Gen` / `X-Tile-Quality` (additive headers only); per-tile FE gen; `needsRefetch` if wake missed / response gen &lt; current; never draw outdated over newer; safety-timer gen/quality check; `STALE_GEN_AT_QUIET`; unit + Z9 → 0 violations.

## Author GUI check (optional)

`DITHER_TRACE=1` → Stress → **Z9 rapid params (H4)** → inspect jsonl for `wake_skipped_inflight` on the stuck tile and `tile_drawn.details.source` / `gen`.
