# STEP1 — Fresh pixel vs warm state (#2 vs #3+)

Measurement-only. No product behavior change.  
Source prompt: `.local-doc/What actually happens.md`

## 1. Metric fix (harness)

`second_interaction_harness.rs` now records:

| field | meaning |
|-------|---------|
| `first_visible_ms` | `tile_fresh` — **may be stale pixels** (labeled in report) |
| `first_fresh_vis_ms` | first visible Composite with `tile_ready_emit_gen == content_gen` |
| `all_fresh_vis_ms` | all visible Composite stamp `content_gen` |
| `cache_before` | residuals / tile entries / epoch before the wave |

Live-trace helper: `analyze_fresh_pixel_waves.py` (fresh = `tile_ready` with `gen/body_gen/content_gen == schedule content_gen`; `source:"unknown"` draws = stale).

## 2. Live capture status

**Existing** `trace_viewport/events.jsonl` does **not** contain the requested 6K sequence (add → #2…#6 with quiet between). It has:

- zoom=2.0 updates call 1–4 (partial viewport, ~81–96 queue delta)
- one 6K fit-view update (call=1, ps=5, delta=411)

### Hand scenario (run this next)

```bash
# Terminal A — release app with trace
DITHER_TRACE=1 DITHER_JOURNAL=0 DITHER_GPU_PATH=cpu npm run tauri:dev:release
# or your usual release launch; ensure events land under profiling/results/.../events.jsonl
```

In the UI, **fit-view a ~6K image**, pause until quiet (no Rendering / no spinner), then **exactly**:

1. **Add** Floyd–Steinberg, pixel_size=8, levels=16 → wait quiet  
2. **#2** ps 8→9 → wait quiet  
3. **#3** ps 9→8 → wait quiet  
4. **#4** ps 8→9 → wait quiet  
5. **#5** levels 16→32 → wait quiet  
6. **#6** change palette → wait quiet  

Then:

```bash
python3 profiling/results/2026-10-22/second_interaction/analyze_fresh_pixel_waves.py \
  path/to/events.jsonl
```

Copy the wave table into this file under §4.

## 3. Code audit (read-only)

### C — Does visible L3 need the full ED L0 prefix?

**Yes at fit-view on 6K.**

Call chain:

1. Viewport schedules **Composite** at visible L3 (`commands/mod.rs` ~427–449, `layer: 0`).
2. Also `schedule_ed_for_viewport` (`tile_pipeline.rs:1132–1163`): for each visible L>0 coord, expands to L0 footprint  
   `max_x/y = ((coord+1)<<level)-1`, then `schedule_ed_prefix_closure` over `ed_prefix_coords(0, max_x, max_y)`.
3. For fit bbox `(0,0)–(2,2)` at L3, corner `(2,2)` → L0 `x1=y1=23` (shift 3) → near-full-doc prefix; live: **`total_l0_processed=408`**.
4. `compute_composite_tile` for L>0 (`tile_pipeline.rs:655–706`) needs **4 Composite children** at L−1; recurses to L0. At L0 it calls `ensure_processed_tiles_fresh` → ED prefix + `ed_ready` or `EdPrefixPending`.
5. Serve pending: `waiting_children` / `missing_dep` L2 Composite (live ~50 ms after busy) until pyramid children exist.

**L3 (0,0) alone:** children L2 `(0|1,0|1)` → … → **64 L0** Composite footprints; ED also pulls the causal L0 prefix up to that corner via `ed_prefix_coords`. At fit-view the **union over all 9 L3 tiles ≈ whole-image ED**.

### Clear on update vs add

| | `add_filter` (`filter_service.rs:447–456` + schedule, **no busy**) | `update_filter` → `request_preview_refresh` + `run_preview_refresh` (`commands/mod.rs:586–620`) |
|--|--|--|
| `error_residuals` | `evict_layer` | `evict_layer` if `clear_residuals` |
| `block_representatives` | `clear_dithered()` | `evict_layer(doc, layer)` |
| strip gates | `clear_strip_gates()` | **not cleared** in `run_preview_refresh` |
| `full_document` | `invalidate_layer` | `invalidate_layer` if clear |
| tile cache | `LayerFilterChanged` invalidate | same |
| busy | **no** | `begin_full_document_busy` |
| ED pass note | via schedule only | `note_ed_pass_start` after schedule |

`layer_needs_dither_cache_reset` (`commands/mod.rs:501–517`): **true whenever any enabled Dither/Ascii is on the layer** — same for #2 and #3+. So **clear_residuals does not differ between #2 and #3** for FS param edits.

Implication: **`error_residuals` / block reps / FD are reset every FS update.** Warm “ED residual reuse” across #2→#3 is **not** expected from this path. Remaining warm candidates: CPU/OS caches, strip-gate map (survives refresh), rayon/pool, atlas placeholders (FE only).

### Levels / palette

Param/`levels` change still hits dither-enabled → `clear_residuals=true` → full ED invalidate. Palette may use a different service path (`palette_service` also `evict_document` residuals) — still not a “repaint-only” path for FS. **Expect full ED recompute**, not Composite-only.

### D — 7513 L0 dequeues (6K wave)

From live 6K busy window:

| signal | value |
|--------|------:|
| `worker_dequeue` | 8630 |
| by level | L0:7513 L1:808 L2:237 L3:72 |
| by stage | Processed:4768 Composite:3862 |
| unique (level,x,y,stage) keys | 963 |
| extras (repeats) | **7667** |
| `worker_requeue` | **2274**, reason **`input_not_ready`** |
| `task_discarded_stale` | **0** |

So the inflation is **re-dequeue / requeue while ED deps incomplete**, not `task_is_stale` discards. Composite L0 starts fire as Processed becomes ready; parents retry via `retry_pyramid_parent` / `EdPrefixPending` park (`worker.rs` Composite branch).

## 4. Interaction × metrics (available data)

### Live zoom×2 (not 6K fit) — every update similar

| wave | call | busy_ms | first_fresh_ready_ms | enq | delta | notes |
|-----:|-----:|--------:|---------------------:|----:|------:|-------|
| 1 | 1 | 613 | **480** | 9 | 81 | Rendering candidate |
| 2 | 2 | 508 | **403** | 9 | 96 | |
| 3 | 3 | 494 | **392** | 9 | 57 | |
| 4 | 4 | 499 | **394** | 9 | 85 | |

→ At zoom 2, **#3+ is not faster** on fresh pixels (~400 ms each). Supports branch: cost paid every time; #2 differs mainly by overlay.

### Live 6K fit — single update only

| metric | value |
|--------|------:|
| busy | 3161 ms |
| first_fresh_ready (`content_gen=4`) | **2316 ms** |
| first `tile_drawn` | 201 ms **stale** (`source:unknown`, no gen) |
| schedule enq / delta | 9 / **411** |
| ed_pass | 408 L0, **3128 ms** |

### Harness 6k CPU `fs_ps_alt` (L2 zoom≈0.20, not fit L3)

`DITHER_SECOND_IX_SIZE=6k DITHER_GPU_PATH=cpu DITHER_SECOND_IX_ONLY=fs_ps_alt DITHER_SECOND_IX_MAX_IX=3`

| ix | kind | first_vis (maybe stale) | **first_fresh_vis_ms** | queued | rendering? | residuals before→after |
|---:|------|------------------------:|----------------------:|-------:|------------|------------------------|
| 1 | add ps=1 | 224 | **224** | 408 | false | 0→96 |
| 2 | update ps=8 | 507 | **507** | 272 | true | 96→96 |
| 3 | update ps=9 | 608 | **608** | 408 | false | 96→96 |

`all_fresh_vis_ms` never within 180 s (ignore `wall_ms≈180s`).  
**#3 is not faster than #2** on first fresh pixel in harness.

## 5. Answers A–D (current evidence)

**A. Is ~2.3 s / ~3 s ED paid every update, or only #2?**  
- Zoom×2 live: **every update** pays ~0.4 s fresh / ~0.5 s busy.  
- Harness 6k L2: #2 **507 ms**, #3 **608 ms** fresh — **not only #2**.  
- 6K fit live: only **one** update → still need hand capture for fit-view #3+. Code clears ED state every update → **predict full ED every time** at fit too.

**B. If #3+ faster, what warm state?**  
- Evidence so far: **#3+ is not faster** (zoom2 live + harness).  
- Residuals/block-reps/FD cleared each FS update; residuals return to ~96 after each wave.

**C. L3 vs full ED L0?**  
- Yes at fit-view — see §3 + live delta≈411≈408.

**D. 7513 L0 dequeues?**  
- Repeated Processed+Composite work + **2274 `input_not_ready` requeues**; **0** stale discards — see §3.

## 6. Branch verdict (provisional)

Until 6K #2…#6 exists: **lean “ED cost every update; #2 unique in overlay”** (matches zoom×2 + clear semantics).

If hand capture shows #3+ also ~2–3 s fresh: fix candidates (no impl):

1. **ED-prefix cache / skip when only levels-or-palette** if pixels unchanged for ED topology — risk: wrong FS if residuals required.  
2. **Busy until first fresh visible, not full ED idle** — risk: inconsistent tiles while ED continues.  
3. **Coarse-level dither when zoomed out** — risk: preview≠export.  
4. **Priority: visible L0 footprint before far ED** — may already partially exist; measure after capture.

If #3+ much faster: name surviving state (strip gates / something else) and whether open-time warm can prime it.

## 7. Not measured / why

| item | why |
|------|-----|
| 6K #2…#6 live table | no multi-update 6K session in current `events.jsonl` |
| `hadPreviewFrame` / overlay vs chip in trace | not emitted as events; infer from busy + ix only |
| `block_representatives` dithered flag / `full_document` valid bit in trace | not in events; harness `cache_before` only proxies residuals/entries |
| `first_fresh_drawn` with gen==cg | FE `tile_drawn` often omits gen even after fresh decode |
| harness `first_fresh_vis_ms` on 6k | run in progress / see log |

## 8. Stop

Awaiting **go** before any product fix. Next action for you: hand scenario §2, then re-run `analyze_fresh_pixel_waves.py`.
