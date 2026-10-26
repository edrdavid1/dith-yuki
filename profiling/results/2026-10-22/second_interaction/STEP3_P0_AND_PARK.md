# STEP3: P0 + Composite parking — STOPPED at Step0 golden determinism

**Date:** 2026-10-22  
**Baseline git commit (working tree dirty; HEAD):** `fd1c09a316c388d126db8133a5e44b3ae3e20170`  
**Verdict:** Step0 golden hashes are **not deterministic** on unchanged product code. Per the prompt (“If hashes fluctuate … STOP and report”), **Step2 park was not implemented**. Step1 P0 overlay is done and kept.

---

## What changed (`file` summary)

### Step0 harness (measurement only)
- `src-tauri/src/second_interaction_harness.rs` — `golden_hashes_baseline_step0`, `fit_l3_metrics_baseline_step0`, blake3 digests, `golden_diagnose_composite_nondeterminism`.
- Harness fix: hash only tiles with `tile_ready_emit_gen == content_gen`; require `all_fresh_at_quiet` (quiet alone can leave dirty bodies that `get_entry` still serves).
- `src-tauri/Cargo.toml` — `blake3` for the harness.
- Artifacts: `run_golden_full.log`, `run_golden_quick2.log`, `run_golden_diag.log`, `run_golden_diag2.log`, `golden_hashes_baseline.json` (quick-only; **not** a valid full baseline).

### Step1 P0 (kept)
- `frontend/src/features/preview/previewBusyUi.ts` — `shouldShowRenderingOverlay` / `shouldShowRecomputingChip` (`fullDocumentBusy && !hadPreviewFrame` vs chip).
- `frontend/src/features/preview/__tests__/previewBusyUi.test.ts` — open→add→update #2/#3: no overlay once a frame exists; early edit still overlays.
- `frontend/src/features/preview/TileCanvas.tsx` — `onFirstFrameDrawn`; fire on first blit (direct + LOD fallback); reset notify ref on doc identity layout clear.
- `frontend/src/components/PreviewWindow.tsx` — wire callback; reset `hadPreviewFrame` on `docId`; keep `busy:false` as secondary latch.

### Step2 park
- **Not started** (blocked by Step0).

---

## Step0: nondeterminism (blocking)

### Symptom
Across ×3 re-applies of the same dither config on one session, **Processed L0 blake3 is stable**, but **visible Composite** (and, when flaking, **L0 Composite under the visible pyramid**) changes on the **first** quiet after some `pixel_size` transitions, then run2/run3 match.

Example from full golden (`run_golden_full.log`, after harness “fresh-only” fix):

| size | mode | ps | lv | run1 comp | run2=run3 comp |
|------|------|----|----|-----------|----------------|
| 4000×3000 | floyd_steinberg | 8 | 16 | `c937b8e3…` | `9c404b6f…` |
| 4000×3000 | floyd_steinberg | 9 | 16 | `f924c30a…` | `f10ad5b5…` |
| 4000×3000 | floyd_steinberg | 8 | 32 | `a4749abb…` | `928e91bd…` |

`complete=true`, `comp_stale=0`, `proc_stale=0` on all of these — so this is **not** the harness hashing dirty leftovers.

### Isolated diagnostic (`golden_diagnose_composite_nondeterminism`)
- Cold `ps=8` ×3: **stable** (proc + L1 comp + L0 comp).
- `ps=1` → `ps=8` ×3: **intermittent** — diag2 `after_ps1` run1 flaked:

| | proc (stable) | L0 Composite | L1 Composite (visible) |
|--|---------------|--------------|-------------------------|
| run1 (bad) | `b47c6efd…` | `244a8d83…` | `ec44e2a1…` |
| run2/3 (good) | `b47c6efd…` | `83bed340…` | `9c404b6f…` |

So the divergence is already at **L0 Composite** while **final Processed L0** matches the good run. Wrong Composite is still stamped at `content_gen` (`tile_ready` / `complete` pass).

### Likely source (not fixed; out of STEP3 park scope)
`insert_fresh_gen` **replaces equal generation**. A Composite can be published at `content_gen` from Processed (or other inputs) that are later **replaced at the same gen** with the final bytes. Quiet + freshness checks then see a consistent gen stamp and wrong pixels. Pyramid L1 inherits the bad L0 Composite. Race is timing-sensitive (hence cold/matrix sometimes green, sustained transitions flake).

Related code: `TileCache::insert_fresh_gen` (equal-gen replace), L0 `compute_composite_tile` insert at `compute_content`, L1 parent reuse when `generation == child_stamp`.

### Consequence
Bit-exact golden comparison for park on/off is **impossible** until this is fixed or the golden protocol is redefined (e.g. only digest after a proven single-publish-per-gen barrier). **Stopped before Step2.**

---

## Step1 P0: overlay vs chip

| Case | Expected | Implementation |
|------|----------|----------------|
| First open / before any blit | Overlay while busy | `hadPreviewFrame=false` until first blit or `busy:false` |
| After first blit / kept frame (`tile_rev_reload_keep_frame`) | Chip only on later busy waves | `onFirstFrameDrawn` on first canvas blit |
| Update #2 / #3 | No overlay, chip if busy | Same latch |
| Document change | Overlay can show again | Reset on `docId` + TileCanvas notify ref reset |
| Edit before first frame | Overlay OK | Latch still false |
| Backend busy / `add_filter` | Unchanged | No backend busy changes |

**Test:** `npx vitest run src/features/preview/__tests__/previewBusyUi.test.ts` — pass.

**Live WebView:** not measured in this step (harness-only).

---

## Step2 / Step3 / Step4

| Item | Status |
|------|--------|
| Park Composite in `worker.rs` + `DITHER_PARK_COMPOSITE` | **Not done** |
| Hang / stale / rapid-update park tests | **Not done** |
| Hash verify park on/off vs baseline | **Blocked** (no stable baseline) |
| Post-change fit L3 metrics ×3 | **Not done** |

### Reference metrics (STEP2, pre-STEP3 product change)
From `fit_l3_step2_release.json` / churn A/B (6016×4352 fit L3, CPU) — update #2 order of magnitude:

| | first_fresh_ms | busy_hold_ms | ed_done_ms | requeue_inr | dequeue |
|--|----------------|--------------|------------|-------------|---------|
| fit_l3 update2 (one run) | ~862 | ~1291 | ~1316 | ~1192 | ~5901 |
| churn A update2 | ~837 | ~1216 | ~1236 | ~1081 | ~5185 |

No STEP3 before/after table (park not shipped).

---

## Hash verification table

**Full mode×config×match table:** not produced — determinism failed before a baseline was accepted.

Quick (`DITHER_GOLDEN_QUICK=1`): FS ps=8 lv=16 at 4000 and fit 6k wrote `golden_hashes_baseline.json` with `determinism_ok=true` but `runs_per_config=1` (does not satisfy ×3).

---

## What could not be verified
- Stable golden across all modes/configs/sizes (blocked).
- Park accuracy / kill-switch hash parity (not implemented).
- Live WebView overlay timing.
- Fit L3 metrics ×3 under a formal Step0 baseline script (deferred with park).

---

## Stop
Do **not** start ED parallelism / wavefront occupancy. Next work (separate from this prompt): fix equal-gen Composite vs Processed publish races (or harden golden to reject multi-insert gens), re-run Step0 ×3 until stable, then resume Step2 park.
