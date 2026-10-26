# STEP3b — L0/L1 Composite nondeterminism after pixel_size change (measurement-only)

**Date:** 2026-10-23  
**Constraints honored:** no product edits to `worker.rs` / `tile_pipeline.rs` / `tile_cache` / `ed`; harness + report only; no Composite parking; P0 from STEP3 untouched.  
**Stopped:** 6016×4352 fit-L3 frequency matrix cancelled by request after ~1.4h wall; 4000×3000 N=30 complete.

Artifacts: `step3b_l1_frequency.json`, `run_step3b_l1_cells.log`, `run_golden_diag3.log`, `step3b_io_proof.json`, harness `src-tauri/src/step3b_composite_nondeterminism_harness.rs`.

---

## Verdict

1. **Product bug (race), not harness-only.** Same L1-only mismatch rates on **ProdShaped** (mirrors `tile_worker_loop`) and **Harness** workers.  
2. **Persistent (b), not transient (a).** On every caught mismatch, wait 0.5s + 2s + reschedule visibles left the bad hash unchanged (`persist_b` = mismatch count; `heal_a` = 0).  
3. **Root cause (strong proof for the common case):** visible **L1 Composite** can disagree with cold reference while **Processed L0 and L0 Composite digests match** the reference. L1 is a box-filter of L0 Composite children (`tile_pipeline.rs:652–722`, `pyramid.rs:320+`). Therefore L1 was published from **non-final L0 Composite**, then children were replaced at the **same generation** (`insert_fresh_gen` equal-gen replace) while the parent stayed clean with `generation == child_stamp` and was **reused without rebuild** (`tile_pipeline.rs:716–721`, `pyramid_parent_needs_rebuild` only compares gens — `pyramid.rs:116–119`).  
4. **Equal-gen Processed → wrong L0 Composite** (STEP3 hypothesis) is **rarer** (~7% at w=8) and was **not** proven via before/after Processed IO flips in this pass (`equal_gen_proc_flip` stayed 0 in `step3b_io_proof.json`); still consistent with the same stamp/replace model one level down.

---

## Q1 — Production worker vs harness?

### Does it reproduce in a production-*shaped* worker?
**Yes.** ProdShaped w=8: L1-only rate **26.7%** (8/30), same as Harness w=8.

True `tile_worker_loop` needs Tauri `AppHandle` (emit `tile-ready`); tests use `prod_shaped_worker_loop` in `step3b_composite_nondeterminism_harness.rs` mirroring park / `EdPrefixPending` / `delayed_requeue` / drop L>0 ED / post-compute stale check, without IPC.

### Diffs: `second_interaction` harness worker vs `worker.rs`

| Topic | Production `worker.rs` | `second_interaction_harness::harness_worker_loop` |
|-------|------------------------|---------------------------------------------------|
| Entry | `tile_worker_loop` ~122 | ~488 |
| Drop L>0 ED Processed | yes ~211–222 | **no** (only parks when `level==0` ~560–572) |
| Empty-deps Composite | `delayed_requeue` ~322–356 | **immediate** `enqueue_or_bump` ~612–615 |
| Trace / inject_spin | yes ~274–314 | no |
| Post-compute stale | re-check before emit ~400–433 | compute already inserted; wake on Ok ~632–635 |
| `tile-ready` IPC | yes | no |
| Early park experiment | no | optional `DITHER_EXPERIMENT_PARK_COMPOSITE` ~526–551 |
| Counters | diag/trace | harness counters |

Apply-path note (not worker): harness golden `apply_dither_and_quiet` **clears strip gates** (`second_interaction_harness.rs` ~1605); production `run_preview_refresh` **does not** (`commands/mod.rs:576–620`, STEP1 §3). STEP3b cells used ClearGates; ProdRefresh same-session smoke (N=10) showed stable L0 hashes — strip-gate survival not required for the L1 flake.

---

## Q2 — Converge after quiet?

| Wait | Re-request visibles | Result on mismatch |
|------|---------------------|--------------------|
| 0.5 s, 2 s (+ 5 s in earlier IO probe) | `schedule_dirty_viewport_tiles` | **Hash unchanged** |

Classification: **(b) persistently wrong** until a new invalidation/mark-dirty. Clean L1 with matching child stamp is skipped by `composite_needs_compute` / reuse path, so schedule alone does not rebuild.

---

## Q3 — What input did the wrong Composite see?

**Common failure (L1-only):** at quiet, `processed_l0` and `l0_composite` == cold reference, `visible_composite` ≠ reference.  
Example (`run_golden_diag3.log`):

```
matrix_ps8 run=1 proc=b47c6efd… l0comp=83bed340… comp=11d2643d…   # L1 bad
matrix_ps8 run=2 proc=b47c6efd… l0comp=83bed340… comp=9c404b6f…   # good
```

Proof: final L0 Composite digest matches the good run, so **final** L0 inputs are correct; bad L1 cannot be a pure function of those finals → L1 sampled **earlier** L0 Composite (or incomplete children) and was not rebuilt.

Harness IO around L0 `compute_composite_tile` (`DITHER_STEP3B_COMPOSITE_IO=1`): 20× ps1→8 attempts recorded **0** `equal_gen_proc_flip` (`step3b_io_proof.json`). So “Composite read non-final **Processed**” is **not proven** in that sample; the dominant proven path is **non-final L0 Composite → stale L1 parent**.

---

## Q4 — Where does non-final data come from?

| Mechanism | Role |
|-----------|------|
| `insert_fresh_gen` equal-gen replace | `cache.rs:532–545` — last writer wins at same `generation`; older stamp cannot overwrite newer (`>` rejects) |
| L0 Composite insert | `tile_pipeline.rs:851–855` stamps `compute_content` |
| L1 parent insert | `tile_pipeline.rs:742–746` stamps `child_stamp = min(children)` |
| L1 reuse | `tile_pipeline.rs:716–721` if clean && `generation == child_stamp` |
| `pyramid_parent_needs_rebuild` | `pyramid.rs:116–119` — **gen inequality only**, not byte identity |
| FD / per-tile Processed publish | multiple `insert_fresh_gen` paths (`tile_pipeline.rs:241`, `:609`, full_document) can replace equal gen |
| Strip gates | **not** cleared in `run_preview_refresh`; cleared in add/harness ClearGates. Not necessary for L1 flake (seen with ClearGates) |
| `get_processed_tile` Raw fallback | `compositor.rs:83–107` — possible undithered Composite if Processed missing; not required to explain L1-only case |

---

## Q5 — Why pixel_size / minimal repro set?

| Scenario | Evidence |
|----------|----------|
| ps 1→8 | Primary N=30 matrix; rates above |
| ps 8→9 | `run_golden_diag3`: L1 run1 ≠ run2/3, L0comp stable |
| levels 16→32 | Not separately frequency-tested (cancelled with 6k) |
| mode change / same-params recompute | Formal `step3b_scenario_minimal_set` **not run** (stopped before) |
| Cold ps=8 only (no prior transition) | Usually stable; flake tied to **param-change waves** under concurrency |

Minimal repro: **multi-worker (≥2) + filter param change that rebuilds Composite pyramid + hash visible L1 vs cold reference.**

---

## Q6 — Who stamps equal gen?

`TileCache::insert_fresh_gen` (`crates/engine-tiles/src/cache.rs:532–558`):

- If cached `generation >` incoming → **reject** (false); keep newer.  
- If cached `generation <=` incoming → **replace** tile + gen, clear dirty.  
- Equal gen → **last insert wins** (test `insert_fresh_gen_replaces_equal_generation` ~1055).  
- Older result **cannot** overwrite a newer gen; at equal gen, a **later** compute **can** overwrite an earlier one (including after a parent already box-filtered the earlier body).

---

## Q7 — Frequency (N=30), workers — **4000×3000 only**

Scenario: Floyd–Steinberg, transition toward ps=8 lv=16 after ps=1; reuse one worker pool per cell; cold reference from ps=8×2 on that pool. Release, `DITHER_GPU_PATH=cpu`.

| Worker kind | workers | N ok | L0 mismatch rate | L1-only rate | Converge |
|-------------|---------|------|------------------|--------------|----------|
| Harness | 1 | 30 | 0% | **0%** | — |
| Harness | 2 | 30 | 0% | **16.7%** | b (5 persist) |
| Harness | 4 | 30 | 0% | **20.0%** | b (6) |
| Harness | 8 | 30 | 6.7% | **26.7%** | b (10) |
| ProdShaped | 1 | 30 | 0% | **0%** | — |
| ProdShaped | 2 | 30 | 0% | **6.7%** | b (2) |
| ProdShaped | 4 | 30 | 0% | **16.7%** | b (5) |
| ProdShaped | 8 | 30 | 6.7% | **26.7%** | b (10) |

**1 worker → no discrepancies ⇒ race** (multi-threaded publish / pyramid race).

6016×4352: **not measured** (run aborted).

---

## Fix candidates (estimates, no implementation)

1. **Invariant: do not reuse L1 parent when any child body changed at equal gen** — content hash / child revision counter; or always rebuild parent when a child `insert_fresh_gen` succeeds at equal gen (~M, touches pyramid wake).  
2. **Forbid equal-gen body replace** unless dirty+bump content_gen / sub-generation (~M–L, cache API + callers).  
3. **Stamp Composite with input fingerprint** (blake3 of Processed/L0 children); emit/serve only if fingerprint matches current inputs (~L).  
4. **Golden protocol:** digest L0 Processed + L0 Composite only, or require w=1 for goldens — measurement workaround, does not fix preview.

---

## Not measured / why

- **6016×4352 fit L3 N=30** — stopped by request (too slow; prior all-in-one runs SIGKILL’d from setup thrash).  
- **Formal scenario matrix** (levels/mode/same-params × N) — not started after stop.  
- **Live `tile_worker_loop` + AppHandle** — no Tauri test runtime; ProdShaped is the proxy.  
- **Per-tile input blake3 proving L0 Composite read non-final Processed** — IO flag recorded 0 flips in 20 attempts; dominant bug is L1 vs L0.  
- **Strip-gate-only A/B at N=30** — only smoke N=10.

---

## Stop

Waiting for **"go"** before any product fix or STEP3 park resume.
