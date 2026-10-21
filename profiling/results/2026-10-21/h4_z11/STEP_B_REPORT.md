# Step B — `content_gen` + palette content-changed (Z11)

**Branch:** `diagnose/h4-stale-gen-z9`  
**Date:** 2026-10-21  
**Depends on:** Step A2 (palette sticky: no `doc_gen` / no `DocumentChanged`; FE gen-blind)

## Verdict

Z11 fingerprint gate is **green** after Step B. Palette-only edits advance `content_gen`, stamp `X-Tile-Gen` third segment, emit lightweight `content-changed`, and FE safety/wake compares against that epoch. Strip discard by `doc_gen` now reschedules owner + wakes waiters.

## Before / after — Z11 (visible tiles fingerprint ≠ BE)

| Variant | Before (A2 / pre-fix) | After (Step B) |
| --- | ---: | ---: |
| Single warm release | **19 / 20** `STALE_CONTENT_AT_QUIET` (fp unchanged; `doc_gen` 0→0) | **0 / 20** |
| Drag ~60 Hz × 1.5 s release | red (A2 gen-blind) | **0** unchanged; `content_gen`→93 |
| Series 8×50 ms release | red | **0** unchanged; `content_gen`→8 |
| Z11 ×50 release | n/a | **pass=50 fail=0** |
| Z11 ×50 dev | n/a | **pass=50 fail=0** |
| Z9 rapid param smoke | — | ok (no regression) |
| Z10 continuous slider smoke | — | ok (no regression) |
| `ed_strip_discard_reschedule_enqueues_owner_row` | red until reschedule wired | **ok** |

Harness note: early Z11 reds also hit a soft-alpha fill (`data.fill(v)` → `dither_alpha` binary→0 RGB). Gate uses opaque α + near-black→near-white palette so fingerprints measure real palette application.

## Requests per tile during palette drag

| Path | Behavior |
| --- | --- |
| FE (`content-changed`) | **No `tileRev++`**. Coalesce: ≤1 in-flight fetch + ≤1 collapsed `needs_refetch` follow-up per visible tile (`TileCanvas` `onContentChanged` + `decideEventDrivenWake`). |
| BE schedule (harness drag, 93 pulses, 20 visible) | `schedule_enter`=94, `tile_enqueued`=1910 (~1 dirty enqueue/tile/pulse), `tile_ready_emitted`=149 (~7.5 ready/visible over the drag — coalesced by strip ownership). `strip_discarded_by_doc_gen`=45 all followed by `strip_discard_rescheduled`. |

## Affected locations

| Area | Change |
| --- | --- |
| `crates/engine-tiles/src/generation.rs` | `content_gen` atomic; `increment_document_gen` also bumps it |
| `crates/engine-project/src/document.rs` | `bump_content_gen()` |
| `src-tauri/.../palette_service.rs` | Color add/update/remove: `bump_content_gen` + `content-changed {gen}` (no `tileRev` storm); residual/LUT/KD evict |
| `src-tauri/.../commands/proof.rs` | Soft-proof: `bump_content_gen` + `content-changed` |
| Filter / structural paths | Existing `increment_generation()` → `content_gen` advances |
| `src-tauri/.../tile_pipeline.rs` | Insert stamp = `content_gen`; strip discard → `reschedule_ed_strip_after_discard` + waiter wake; unit test |
| `src-tauri/.../tile_serve.rs` / `main.rs` | `X-Tile-Gen` = `doc.layer.content` (additive); `X-Tile-Quality` stale if dirty or gen behind `content_gen` |
| `pending_tiles.rs` / `worker.rs` | `tile-ready` carries `content_gen` |
| `frontend/.../eventDrivenWake.ts` | `parseTileGenHeader` prefers last segment |
| `frontend/.../TileCanvas.tsx` | `maxKnownGen` from ready / fresh-200 / `content-changed`; coalesce refetch |
| `frontend/.../tileWorker.ts` | Always parse gen/quality headers (no extra requests when trace off) |
| `frontend/.../viewportTraceChecker.ts` | `STALE_CONTENT_AT_QUIET` (fingerprint / content stamp) |
| `src-tauri/.../tile_gen_trace_harness.rs` | Z11 single/drag/series gates |

## Constraints

- File format unchanged; headers additive only.
- Tracing gated on `DITHER_TRACE` / `tileGenTraceEnabled()`; no extra network when tracing is off.

## Residual risks

1. **Rapid palette drag** still races ED strips (`strip_discard` under content_gen churn); reschedule path recovers, but CPU cost scales with drag Hz × strip width.
2. **Collapsed FE retry is one-shot** per wake cycle — a second content bump during that retry relies on the next `content-changed` / safety timer.
3. **202 responses** still advertise target quality; FE must not treat stale quality as final (unchanged contract).
4. Z11 harness is BE/cache/fingerprint; full GUI Z11 matrix not re-run in this pass (checker wired).
5. Paths that mutate pixels without `increment_generation` / `bump_content_gen` / `bump_layer_raw_rev` would still be gen-blind (none known in preview hot path).
