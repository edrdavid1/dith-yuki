# Step B — H4 stale-gen fix (needsRefetch + X-Tile-Gen/Quality)

**Branch:** `diagnose/h4-stale-gen-z9`  
**Date:** 2026-10-21  
**Base:** Step A @ `f6922ac` (H4 confirmed)

## Before / after — Z9 (8×50 ms param pulses)

| Metric | Step A (before) | Step B (after) |
| --- | ---: | ---: |
| FE-gate sim on inflight wake | `wake_skipped_inflight` (drop) | `wake_needs_refetch` (coalesce) |
| Fetches per tile / 8 signals | 1 | **2** (1 + ≤1 collapsed retry) |
| Skips / needs-refetch marks | skip=**8** | needs≈**7** (release avg 6.96) |
| Collapsed retries | 0 (never retried) | **≤1** (avg 1.0) |
| BE Composite(0,0) lag after quiet | 0 | 0 |
| Soft “violations” (skip-as-final) | **8** per series (H4) | **0** |
| Z9 ×50 release | n/a | **fail=0**, max_retries=1 |
| Z9 ×50 dev | n/a | **fail=0**, max_retries=1 |

Single release smoke: `fetch=2 needs=7 retry=1 lag=0 quiet=true`  
(`z9_stepb_release_single.log`, `events_z9_release.jsonl`)

## Affected locations

| Area | Change |
| --- | --- |
| `src-tauri/src/main.rs` `handle_tile_request` | Additive `X-Tile-Gen` (`doc_gen.layer_gen`) + `X-Tile-Quality` (`fresh`/`stale`/`upsample`) on **200 and 202**; existing headers unchanged |
| `src-tauri/src/commands/mod.rs` | Optional `doc_gen` on `document-changed`; `emit_document_changed_with_gen` |
| `src-tauri/src/services/filter_service.rs` | `filter_updated` emits live `doc_gen` |
| `frontend/.../eventDrivenWake.ts` | `needs_refetch` (not skip); `shouldRetryForMaxGen`; `shouldDiscardStaleGen`; `parseTileGenHeader` |
| `frontend/.../TileCanvas.tsx` | Per-tile gen + `maxKnownGen`; inflight → needsRefetch; collapsed ≤1 retry; discard lower gen after higher; safety timer scans **visible** by gen/quality (≤1 safety wake cadence per tile) |
| `frontend/.../tileWorker.ts` | Always read `X-Tile-Gen` / `X-Tile-Quality` (not trace-gated); no extra fetches when `DITHER_TRACE` off |
| `frontend/.../viewportTraceChecker.ts` | `STALE_GEN_AT_QUIET` after silence |
| `frontend/.../viewportStressRunner.ts` | **Z9** constant scenario; quiet snapshot includes `tileGens` / `maxKnownGen` |
| `src-tauri/.../tile_gen_trace_harness.rs` | Z9 sim uses needs_refetch + ≤1 retry; `z9_rapid_param_50_series` |

Inflight skip sites fixed: **tile-ready**, **ed-progress** (via `refetchEventDrivenPending`), **safety_timer**.

## Tests

| Test | Result |
| --- | --- |
| FE `h4WakeRaceChain` / `eventDrivenWake` / `staleGenAtQuiet` | green |
| `z9_rapid_param_composite_gen` release | ok |
| `z9_rapid_param_50_series` release ×50 | **fail=0** |
| `z9_rapid_param_50_series` dev ×50 | **fail=0** |
| `z3_cold_small_composite_holes_zero` release | ok |
| Z1–Z7 / Z_INJECT (Stress panel) | checker wires `STALE_GEN_AT_QUIET`; full GUI matrix not re-run in this Step B harness pass |

## Residual risks

1. **`STALE_GEN_AT_QUIET`** compares displayed gen to session `maxKnownGen` — if filter apply raises max via `doc_gen` and quiet is declared before safety/collapsed retry lands, GUI Z9 could still flag (harness BE lag stays 0).
2. **202 `X-Tile-Quality=fresh`** means *target* quality once ready, not bytes on the wire (empty body).
3. **Collapsed retry is one-shot** (`refetchDone`) — a second gen advance during that retry still depends on the next wake / safety timer.
4. Concurrent harness processes can false-fail `quiet_ok` (observed when two Z9×50 runs overlapped); run matrix single-process.
5. Pyramid path stores quality as `upsample`; LOD blit fallback may still label `pyramid` in draw traces — safety treats both as stale quality.

## Constraints honored

- No file-format changes; existing response headers retained; new headers additive only.
- Trace events gated on `DITHER_TRACE` / `tileGenTraceEnabled()`; header reads do not issue extra requests when tracing is off.
