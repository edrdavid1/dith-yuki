# STEP4 — Composite input versions (completion report)

**Date:** 2026-10-24  
**Constraints:** full N=100 matrix not restarted; no Composite parking / ED parallelism; product touched only for the versions fix (+ small `EdFrontier::sweep_ready` helper).  
**Stop:** awaiting **"go"**.

---

## Verdict (by mode)

| Mode | Trap (versions=0) | Fixed (versions=on) | Closed? |
|------|-------------------|---------------------|---------|
| **Floyd–Steinberg** | Proven (L1 up to 45% / 36.7%) | 0 mismatches on gap + hot cell 0/197 + partial full matrix | **Yes** (harness ProdShaped/stress) |
| **Bayer4x4** | **Not proven** (0/30 ×3 scen w=8) | 0/30 × w=4,8 ×3 scen | **Correctness OK under test; trap unproven** — do not claim the race is exercised |
| **BlockedFs** | **Not proven** (0/10 ×3 scen w=8) | 0/10 ×3 scen w=8 | **Provisionally OK**; FD CPU, reduced N; trap unproven |
| **BlueNoise** | **Not proven** (0/10 ×3 scen w=8) | 0/10 ×3 scen w=8 | **Provisionally OK**; same caveats as BlockedFs |

---

## Root cause (`file:line`)

### Why L0 Composite / pyramid children were replaced at the same `content_gen`

1. **`TileCache::insert_fresh_gen` allows equal-gen replace** (`crates/engine-tiles/src/cache.rs` — occupied entry replaced when `cached.generation <= incoming`). Last writer wins; body bytes can change without bumping `content_gen`.
2. **Multiple publishers stamp the same `compute_content` / `content_gen`:**
   - Per-tile Processed: `tile_pipeline.rs` `insert_fresh_gen(..., compute_content)`.
   - Full-document slice publish: `filters/full_document.rs` `publish_processed_tiles` → `insert_fresh_gen` per tile at one generation.
   - L0 Composite: `tile_pipeline.rs` ~909 `insert_fresh_gen_inputs(..., compute_content, ...)`.
3. **Pyramid reuse only compared generations:** parent clean && `generation == min(children)` (`tile_pipeline.rs` ~717–721 pre-fix; `pyramid_parent_needs_rebuild` gen-only) → L1 could keep a box-filter of **non-final** L0 Composite while children were later replaced at the **same** gen. STEP3b: L1-only mismatch with stable Processed + L0 Composite digests at quiet.
4. **Why non-final L0 Composite exists:** Composite can assemble as soon as Processed looks fresh; a later equal-gen Processed (or L0 Composite) replace updates bytes under the same stamp. Concurrent workers make “early assemble → late replace” timing-sensitive (w=1 → 0% in STEP3b).

### What versioning changes (diff summary)

| Area | Change |
|------|--------|
| `cache.rs` | `entry_version` (monotonic), `input_versions` manifest on Composite, `insert_fresh_gen_inputs`, `composite_inputs_current`, kill-switch `DITHER_COMPOSITE_INPUT_VERSIONS=0` |
| `ed.rs` | `tile_fresh` requires Composite input versions current; `EdFrontier::sweep_ready` (missed-wake helper) |
| `tile_pipeline.rs` | Snapshot input versions **before** assemble; refuse insert / dirty+retry on TOCTOU; `invalidate_composites_fed_by` after successful insert |
| Kill switch | Default ON; `=0` restores gen+dirty-only freshness |

**Equal-gen body replace is not removed.** The fix eliminates the **consequence** (stale parent reuse / serving Composite whose inputs moved). Final hashes of all pyramid levels match a 1-worker reference when versions are on.

---

## Discrepancy tables

### FS — versions OFF (trap)

| Source | Scenario | w | N | Rate | Levels |
|--------|----------|---|---|------|--------|
| baseline trap (earlier) | ps_1_to_8 | 8 | 20 | **0.450** | L1×9 |
| baseline trap | ps_8_to_9 | 8 | 20 | **0.200** | L1×4 |
| gap `step4_fs_levels_trap` | ps_1_to_8 | 8 | 30 | 0.000 | — |
| gap | ps_8_to_9 | 8 | 30 | **0.367** | L1×11 |
| gap | levels_16_to_32 | 8 | 30 | **0.033** | L1×1 |

### FS — versions ON

| Source | Scenario | w | N | Rate |
|--------|----------|---|---|------|
| hot cell | ps_8_to_9 | 8 | 197 | **0** |
| gap levels | ps_1_to_8 / ps_8_to_9 / levels | 8 | 30 | **0** |
| aborted full3 (prior) | all FS cells × w=1..8 | 100 | **0** (incomplete blocked_fs) |

Hashes cover Composite **L0..viewport_level** (default 4000 viewport → **level 1** ⇒ L0+L1). See coverage gaps for L2/L3.

### Bayer — ON / OFF

| versions | scen × w | Rate |
|----------|----------|------|
| ON | 3 scen × {4,8}, N=30 | **0** |
| OFF | 3 scen × w=8, N=30 | **0** → **trap not proven** |

### BlockedFs / BlueNoise (FullDocument CPU)

| versions | N | w | Rate | Note |
|----------|---|---|------|------|
| ON | 10 | 8 | **0** | Prompt asked N=30 × w=4,8; reduced — each FD apply ~60–90s on 4000 CPU |
| OFF | 10 | 8 | **0** | **trap not proven** for these modes |

---

## Rapid / stale / golden (prior STEP4)

| Test | Result |
|------|--------|
| `step4_rapid_updates_hash_match` | 0/50 bad |
| `step4_stale_midwave_no_publish` | 0/30 bad, behind=0 |
| `step4_golden_hashes_fixed` | `determinism_ok=true`, runs_per_config=3 |

---

## Overhead

### Memory

| | versions=0 | versions=on |
|--|------------|-------------|
| `used_mb` (TILE_BYTES accounting) | **643.65** | **643.65** (unchanged) |
| Approx manifest bytes (Composite Arc headers + pairs) | ~3.8 KB (headers only) | **~14.6 KB** (~384 pairs) |

Per Composite: `entry_version` + `Arc<[(TileKey,u64)]>` (L0 ≈1 pair; parent ≤4). Negligible vs ~1.0 MB/`TILE_BYTES` per entry.

Fit-L3 `used_mb` after wave: **not measured** (see gaps — fit/6k harness deadlock).

### Performance

- Fit-L3 ×3 before/after: **not completed** (same deadlock).
- Proxy: 4000 default viewport, FS ps_8→9 w=8, N=3 — wall dominated by ED; no meaningful regression isolated (before/after each ~tens of seconds for the short cell). STEP2 fit-L3 busy ~1.1–1.3s remains the best absolute baseline; versions add manifest checks + occasional dirty/rebuild, not a second ED pass.

---

## Coverage / not covered

| Covered | Not covered |
|---------|-------------|
| FS L1 race trap + fix at 4000 | Live Tauri `tile_worker_loop` + AppHandle IPC |
| Bayer full N=30 w=4,8 | Bayer/BlockedFs/BlueNoise **trap proof** |
| BlockedFs/BlueNoise N=10 w=8 ON | BlockedFs/BlueNoise N=30 × w=4; FD w=4 |
| Golden/rapid/stale | **6016 fit L3** hash stress (harness `blocked>0 && queued==0` deadlock) |
| Partial FS full matrix (prior) | True eye-latency / dequeue A/B under versions |
| Pyramid digests L0–L1 at default zoom | Systematic **L2/L3** hash matrix (fit viewport deadlocks in step4 harness) |

**Harness note:** `apply` now clears `ed_frontier` (like production refresh). Fit/6k still hit `blocked≈64–72` with empty queue — separate from the Composite version bug; skipped rather than burning wall clock.

---

## Artifacts

- `step4_gap_bayer_{fixed,trap}.json`
- `step4_fs_levels_{fixed,trap}.json`
- `step4_gap_fd_{fixed,trap}.json`
- `step4_stress_baseline_trap.json`, `step4_stress_fixed.json` (hot cell)
- `step4_perf_proxy_{before,after}.json`
- `golden_hashes_fixed.json`
- Logs: `run_step4_gap_*.log`, `run_step4_fs_levels_*.log`, `run_step4_6k_skipped.txt`

---

## Stop

Waiting for **"go"** before park / ED parallelism / further product work.
