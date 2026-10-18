# Z3 cold ≥12 MPix — TIME_TO_FRESH / holes

## Method

| Item | Value |
| --- | --- |
| Doc | **≥12 MPix** (harness 4000×3000; GUI live doc) |
| Mode | Floyd–Steinberg (CPU) |
| Cold cache | Evict **Processed + Composite**, keep **Raw** (`cache_cold_start`) |
| Visible set | **bottom-right** (`z3_viewport_corner`) |
| Motion | zoom during recompute (Z3) |
| After quiet | FE `TIME_TO_FRESH` + `VISIBLE_HOLES`; harness `time_to_fresh_ms` + `composite_holes` |

## Before / after table

### Backend harness (production-shaped worker)

| | release fresh | release holes | dev fresh | dev holes |
| --- | ---: | ---: | ---: | ---: |
| **After** | **538.5 ms** | **0** | **909.2 ms** | **0** |
| **Before** | **never fresh (timeout)** | **12** | **never fresh (timeout)** | **12** |

Before: L0 may finish (~268–752 ms) while visible Composite stays at 12 holes (L2 ED park + pyramid spin). Do not treat L0 ms as TIME_TO_FRESH.

### GUI FE checker (Stress Z3)

| | TIME_TO_FRESH (ms) | VISIBLE_HOLES |
| --- | ---: | ---: |
| **After** (3 series) | 20738, 17934, 17052 (median **17934**) | **0** |
| **Before** (`560eb15` + Step1/4 diag cherry-pick) | **never fresh (timeout)** / holes — *run Stress Z3 in* `/tmp/dither-z3-before` | expect **>0** or timeout |

Worktree ready: `/tmp/dither-z3-before` @ `560eb15` + `8bb7b7e` + `4ea08c3` (no hole fixes). Harness there: holes=12 after L0.

## GUI After: TIME_TO_FRESH 17–21 s breakdown

`TIME_TO_FRESH = performance.now() − freshnessT0` **after** quiet checks. Mid-zoom sleeps **before** `freshnessT0` are **excluded**.

| Component | Est. (median series ~17934 ms) |
| --- | ---: |
| Post-T0 jitter `sleep` | ~40–120 ms |
| Work until last visible 200 | **~14.4 s** |
| Quiet tail (last 200 → summary) | **~3.5 s** |
| — `waitForSilence` settle | 1500 ms |
| — `waitForFeViewportStable` | ~1500 ms |
| — flush + second FE stable | ~50 + ~400 ms |
| **Sum** | **≈17.9 s** |

Series tails: quiet ≈ 3634 / 3543 / 3320 ms; work ≈ 17104 / 14391 / 13732 ms.

### 202→200 by wake source (After, correlated from `events.jsonl`)

Direct `tile-ready` handler historically omitted `wake` in `fetch_tile_posted` (fixed: now `wake: tile_ready` under `DITHER_TRACE`). Attribution via `tile_ready_received(accepted)` → fetch → 200:

| Series (ttf) | tile_ready | ed_progress | safety_timer | other |
| --- | ---: | ---: | ---: | ---: |
| 20738 ms | **89.6%** | 0% | **0%** | 10.4% |
| 17934 ms | **100%** | 0% | **0%** | 0% |
| 17052 ms | **72.7%** | 0% | **0%** | 27.3% |

**safety_timer share = 0% (<5%)** — no FE tile-ready delivery bug. Rejected `tile_ready` were `level_mismatch` (child L1 while viewport L2); viewport-level Composites accepted and woke fetches.

Example visible L2 delays (series 17934, ms, all `tile_ready`): `2/4/4` 9892, `2/5/4` 9889, `2/3/4` 9809, … down to `2/1/2` 537.

## Why harness ~539 ms ≪ full ED L0 ~1655 ms

Harness clock starts at **final** zoom after mid-pass; L0 Processed largely warm. Full cold L0: `ed_l0_baseline.md` → **1655 ms** release / **3202 ms** dev.

## composite_holes=12 root cause (fixed)

1. `schedule_ed_for_viewport` used viewport level → L2 Processed parked forever (no L2 Raw).
2. `retry_pyramid_parent` immediate requeue → pending spin starved L0 Composite.
3. Supporting: L0 park deps + `block_on` lost-wake.

After: holes **0**; pending spin ~177 (was ~320k).

## Safety timer (≤1 per tile)

One global `setInterval` (1500 ms); per-tile `inflight` + `lastSafetyAt`.

## Regression tests

| Test | Role |
| --- | --- |
| `z3_cold_small_composite_holes_zero` | CI (not ignored): 748×1024, `assert holes==0`, pending spin `<1000` |
| `schedule_ed_for_viewport_l2_enqueues_l0_only` | unit: L2→L0 ED schedule |
| `z3_cold_12mp_time_to_fresh` | ignored soak @ 12 MPix |

## GUI before procedure

```text
cd /tmp/dither-z3-before
DITHER_TRACE=1 DITHER_JOURNAL=0 npm run tauri:dev:release
# Open ≥12 MPix, CPU FS, Stress → Z3
# Expect holes > 0 or TIME_TO_FRESH timeout; paste into Before row above
```
