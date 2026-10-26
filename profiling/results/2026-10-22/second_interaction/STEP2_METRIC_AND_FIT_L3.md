# STEP2 — Metric verification, fit-view L3 harness, requeue churn

Measurement-only. Product paths untouched (except harness worker / env-gated experiment).  
Prompt: `.local-doc/What actually happens.md` §STEP2.

## Task 1 — Metric resolution

### How the two clocks are computed

| Metric | Code | Predicate |
|--------|------|-----------|
| `first_visible_ms` | `second_interaction_harness.rs` `wait_first_visible_fresh` / `visible_fresh_counts` → `engine_tiles::tile_fresh` | `!dirty && generation >= content_epoch` (`ed.rs:57-64`) |
| `first_fresh_vis_ms` | `wait_first_fresh_visible` / `wave_until_quiet` → `tile_ready_emit_gen == content_gen` | `tile_fresh` then return `entry.generation` (`ed.rs:72-77`); require `== content_gen` |

Harness syncs `content_epoch = content_gen` after mutate (`sync_content_epoch`).

### Why they coincided in STEP1 (224/224, …)

**(a) primarily, with a weak equivalence (b).**

- Harness has **no FE `tileMap` placeholder** (`TileCanvas` `tile_rev_reload_keep_frame`). First cache-fresh Composite **is** the new stamp.
- With `content_epoch == content_gen`, first `tile_fresh` almost always has `generation == content_gen` (L0 stamps `compute_content`; pyramid parents stamp `min(children)` at that gen). So `tile_fresh` and `emit_gen == content_gen` fire on the same poll.

They are **not** identical predicates (`>= epoch` vs `== content_gen`), but under harness sync + normal stamping they collapse.

### Why `all_fresh` failed while `quiet_ok` (STEP1)

**Harness bug (fixed in STEP2), not a product “tiles never stamp” finding.**

Old `harness_worker_loop` did `let _ = compute_composite_tile(...)` and **dropped `EdPrefixPending`** without requeue/park. Scheduler went idle (`quiet_ok`) while most visible Composites never completed → `visible_fresh_at_end` 1/24.

Production `worker.rs:271-393` parks/requeues on `EdPrefixPending` (`reason=input_not_ready`).

**After fix:** fit-L3 runs show `all_fresh_at_quiet=true` for updates. Residual `quiet_ok=false` can still appear from a short `wait_silence(200ms)` flake vs frontier; composites are stamped.

### Before / after (harness only)

| | STEP1 (broken worker) | STEP2 (ED-aware worker) |
|--|--|--|
| `all_fresh` @ quiet | false (1/24) | **true** (fit L3) |
| `first_fresh ≈ first_vis` | yes | yes (expected; no FE stale) |

---

## Task 2 — Fit-view L3 on 6K

### Viewport / size match

| | Live `trace_viewport/events.jsonl` | Harness (after STEP2) |
|--|--|--|
| Doc | ~6K (408 L0 ⇒ ≈24×17 @256) | **`6016×4352`** (was 6016×4000 → only 384 L0 / 6 L3 tiles) |
| Level / zoom | L3, zoom≈0.1198 | L3, zoom≈0.1198, canvas 717×725 |
| `visible` | 9, bbox (0,0)–(2,2) | **9**, bbox (0,0)–(2,2) |
| `ed` L0 total | 408 | **408** |
| `#1` / first-update `enqueued_delta` | 411 (update) | add **398**; updates **68–403** (varies) |

Artifact: `fit_l3_step2_release.json`, log `run_fit_l3_step2c.log`.

### Interaction × metrics (harness fit L3)

| ix | kind | first_fresh_ms | first_vis_ms | busy_hold_ms | ed_done_ms | ed_l0 | delta | requeue_inr | dequeue | dequeue_l0 | all_fresh | rendering? |
|---:|------|---------------:|-------------:|-------------:|-----------:|------:|------:|------------:|--------:|-----------:|-----------|------------|
| 1 | add ps=8 | **615** | 615 | — | 930 | 408 | 398 | 844 | 4623 | 4036 | true | false |
| 2 | upd ps=9 | **862** | 862 | **1291** | 1316 | 408 | 216 | 1192 | 5901 | 5208 | true | **true** |
| 3 | upd ps=8 | **979** | 979 | **1364** | 1389 | 408 | 89 | 1121 | 5431 | 4741 | true | true* |
| 4 | upd ps=9 | **930** | 930 | **1387** | 1424 | 408 | 122 | 1355 | 5910 | 5168 | true | true* |
| 5 | levels 32 | **924** | 924 | **1328** | 1354 | 408 | 68 | 1236 | 5838 | 5134 | true | true* |
| 6 | palette | **907** | — | **1441** | 1441 | 408 | 403 | 1244 | 5621 | 4907 | true | true* |

\* `fe_would_show_rendering` stayed true when `quiet_ok` flake prevented `hadPreviewFrame` latch in an earlier revision; product overlay still only unique on first busy cycle with `!hadPreviewFrame`.

### Live (single update only — no #3+ in that capture)

| | busy | first_fresh_ready | ed_ms | delta |
|--|-----:|------------------:|------:|------:|
| 6K fit update | 3161 | 2316 | 3128 | 411 |

Harness is **faster** (~0.9 s fresh / ~1.3 s busy) under the same 408 L0 budget — no FE/GPU contention; deltas on updates often &lt;411.

### Question A (fit-view)

**~ED / fresh cost is paid on every update (#2…#6), not only #2.**  
`first_fresh` stays ~0.86–0.98 s; `busy_hold` ~1.3–1.4 s; `ed_total_l0=408` every wave. #3+ is not faster.

---

## Task 3 — Requeue churn A/B

Hypothesis: `input_not_ready` requeues steal cores from ED.

- **A (default):** harness worker mirrors production park/requeue on `EdPrefixPending` (`DITHER_EXPERIMENT_PARK_COMPOSITE` unset).
- **B:** Composite with pending deps parked on `ed_frontier` **before** `compute_composite_tile` (`DITHER_EXPERIMENT_PARK_COMPOSITE=1`).

Artifact: `churn_ab_release.json` / `run_churn_ab.log`. Metric = update #2 after add ps=8→9.

| run | first_fresh_ms | busy_ms | ed_done_ms | requeue_inr | park_early | dequeue |
|-----|---------------:|--------:|-----------:|------------:|-----------:|--------:|
| A1 | 837 | 1216 | 1236 | 1081 | 0 | 5185 |
| A2 | 806 | 1104 | 1121 | 1187 | 0 | 5566 |
| A3 | 737 | 1160 | 1174 | 1277 | 0 | 5623 |
| B1 | 703 | 1016 | 979 | **0** | 9 | 1999 |
| B2 | 679 | 1020 | 969 | **0** | 9 | 1925 |
| B3 | 717 | 1066 | 1016 | **0** | 9 | 2063 |

| | A mean (spread) | B mean (spread) | B−A |
|--|----------------:|----------------:|----:|
| first_fresh | 794 (100) | 700 (38) | **−94 ms** |
| busy | 1160 (112) | 1034 (50) | **−126 ms** |
| ed_done | 1177 (115) | 988 (47) | **−189 ms** |

**Verdict:** B removes Composite requeue churn (dequeue ~5.5k→2.0k) and saves **~0.1–0.2 s** of ~1.2 s harness wave (~10–15%). **Not** the bulk of live ~3 s / harness ED wall. Churn is real but secondary to ED compute itself.

---

## Task 4 — What clears busy (read-only)

1. `run_preview_refresh` (`commands/mod.rs:607`) calls `begin_full_document_busy`.
2. If schedule finds nothing to do (`!preview_pass_busy`), it immediately `end_full_document_busy` (`:642-646`).
3. Otherwise each finished worker task calls `on_preview_task_finished` (`:666-674`): when `queued==0 && inflight==0`, **`clear_full_document_busy`** (swap to 0) + drain coalesced refresh.

So busy clears on **scheduler+inflight idle**, which in practice is **after ED L0 + pyramid Composite catch up**, not at first fresh visible tile.

**Can we know “first fresh visible” when the user sees it?**  
In principle yes (serve/`tile_ready` with `body_gen==content_gen`, or FE gen map) — that event fires **before** busy clear (live: 2316 ms ready vs 3161 ms busy; harness: ~860 ms fresh vs ~1290 ms busy). Busy is a **wave-idle** signal, not first-paint.

---

## Verdict

1. **ED / fresh cost every update**, not only #2 (fit L3 harness #2–#6: fresh ~0.86–0.98 s, busy ~1.3–1.4 s, 408 L0 each).
2. **Churn contributes ~100–200 ms** of ~1.2 s when Composite is parked early; **not** the main ~3 s live / ~1.3 s ED cost.
3. **Metric:** explained; STEP1 `all_fresh` failure was harness dropping `EdPrefixPending` (fixed); `first_fresh≈first_vis` expected without FE placeholders.

## Not measured / caveats

| Item | Why |
|------|-----|
| Live #2…#6 fit table | Still no multi-update 6K live `events.jsonl` |
| Harness vs live absolute ms | Harness ~2–3× faster; no WebView/GPU/IO |
| `quiet_ok` flake | Short silence check vs frontier; use `all_fresh_at_quiet` |
| Unique-key / repeat breakdown in harness counters | Have totals; not per-key histogram |
| Bit-identical “flag off vs pre-experiment product worker” | Harness was previously drop-on-error; A now matches product semantics, not the broken STEP1 harness |

## Fix candidates (no implementation)

| Candidate | Fixes | Risk |
|-----------|-------|------|
| Clear busy on first fresh visible (keep ED bg) | Overlay/#2 feel | Inconsistent tiles; need FE gen gate |
| Skip full ED when only levels/palette if residuals reusable | #5/#6 cost | Wrong FS if topology needs reset |
| Coarse preview dither when L≥2 | Fit-view latency | Preview≠export |
| Reduce Composite spin (already parks) | Requeue CPU | Little gain if ED is the wall |

---

**Stop — awaiting go.** Churn A/B log path: `profiling/results/2026-10-22/second_interaction/run_churn_ab.log`.
