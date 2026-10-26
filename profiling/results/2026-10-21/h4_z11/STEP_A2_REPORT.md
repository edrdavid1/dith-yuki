# Step A2 — Palette change / x=0 sticky tiles (diagnose only)

**Date:** 2026-10-21  
**No production fix** — path audit, gen inventory, Z11 harness/stress, Probe UI, strip-discard reschedule check.

## Verdict

Palette color updates take a **separate path** from filter param updates: they bump `palette.revision`, mark Processed+Composite dirty, and `schedule_dirty_viewport_tiles`, but **do not** raise `doc_gen` / `layer_gen`, **do not** emit `DocumentChanged`, and **do not** call `request_preview_refresh`.  
`X-Tile-Gen` / FE `maxKnownGen` track **doc_gen only** (header is `doc_gen.layer_gen`; FE parses left of `.`). **Palette revision and soft-proof are not in gen** → pixels can change (or fail to refresh) while FE gen gates and the safety timer still treat the tile as current — matching “safety doesn’t fix / FE considers tile relevant”.

---

## 1) Palette path vs filter param path

### Palette color (author sticky path)

| Step | Location |
| --- | --- |
| UI | `SwatchGrid.tsx` → `updatePaletteColor` (`frontend/src/shared/ipc/palettes.ts:170`) on picker confirm; Color Lab draft/Apply is separate |
| Command | `PaletteService::update_palette_color` `src-tauri/src/services/palette_service.rs:717` |
| Mutation | `palette.colors[i]=…`; `palette.revision += 1` only — **no** `doc.increment_generation()` (`:754–760`) |
| Invalidate | `invalidate_palette_changed` (`:179–197`) → `InvalidationEvent::LayerFilterChanged` for layers referencing palette (`invalidation.rs:96–98` marks **all Processed** for layer + **all Composite** coords) |
| Schedule | `schedule_dirty_viewport_tiles` only (`:196`) — **not** `request_preview_refresh` |
| Emit | **No** `DocumentChanged` / `filter_updated` |
| Cache evict | **No** `palette_cache` / `palette_lut_cache` evict on color update (evict is on **delete** `:1011–1012`); caches key by revision and rebuild on mismatch |

`replace_palette` / `modify_palette` **does** call `doc.increment_generation()` (`palette_service.rs:474`) — different from color drag.

### Filter param (Z9/Z10 path)

| Step | Location |
| --- | --- |
| Mutation | `doc.increment_generation()` `filter_service.rs:1334` |
| Layer rev | `increment_layer_gen` `:1351` |
| Trace | `gen_bumped` `:1364` |
| Refresh | `request_preview_refresh` `:1378` (may clear ED residuals) |
| Emit | `emit_document_changed_with_gen(..., "filter_updated", …, live_gen)` `:1405–1410` |
| FE | `onDocumentChanged` bumps `tileRev`, clears event-driven pending (`TileCanvas.tsx:1722–1730`) |

### Compare

| | Filter param | Palette color |
| --- | --- | --- |
| `doc_gen` | raised | **unchanged** |
| `layer_gen` | raised | **unchanged** |
| `DocumentChanged` | yes + gen | **no** |
| Dirty stages | via refresh/invalidate | `LayerFilterChanged` (Processed+Composite) |
| FE `tileRev` | bumps | **stays** (unless other events) |
| Safety / maxKnown | can see gen lag | **no gen lag** even if pixels wrong |

---

## 2) What is in `X-Tile-Gen` / `maxKnownGen`?

| Signal | Contents |
| --- | --- |
| Header `X-Tile-Gen` | `"{doc_gen}.{layer_gen}"` `main.rs:838–840`, comment `:634` |
| Header `X-Tile-Generation` | `doc_gen` only |
| Soft proof | Separate serve hash (`proof_cfg_hash`); **not** in `X-Tile-Gen` |
| Palette revision | **Not** in either header |
| FE parse | `parseTileGenHeader` / worker: `split('.')[0]` → **doc_gen only** (`eventDrivenWake.ts:89–94`, `tileWorker.ts:148–151`) |
| `maxKnownGen` | Ready docs gens from tile-ready / fresh 200 — doc_gen only |

**Yes — tile pixels can change (palette) while `X-Tile-Gen` stays identical.**  
Safety (`TileCanvas.tsx:685–697`) only refetches on `genLag` / stale quality / pending — **not** on palette revision. That is exactly “FE considers tile relevant”.

---

## 3) Z11 before/after (harness + stress)

### Added

- Harness: `z11_palette_{single,drag,series}_12mp_*` — ≥12 MPix FS **with bound palette**, pulses mirror `update_palette_color` (rev++ / LayerFilterChanged / schedule_dirty, no gen bump).
- GUI Stress: **Z11a / Z11b / Z11c** + `changePalette` via `updatePaletteColor`.
- Artifacts: `profiling/results/2026-10-21/h4_z11/`.

### Matrix (quiet after palette activity)

| Run | quiet | doc_gen | layer_gen | palette_rev | STALE_GEN_AT_QUIET | pixel_unchanged (vis) | unchanged x=0 | strip_discard |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| single warm release | yes | 0→0 | 0→0 | 1→2 | **0** | 19/20 | 3/4 | 0 |
| series warm release | yes | 0→0 | 0→0 | 1→9 | **0** | 19/20 | 3/4 | 0 |
| drag warm release | yes | 0→0 | 0→0 | 1→94 | **0** | 19/20 | 3/4 | 0 |
| drag cold release | yes | 0→0 | 0→0 | 1→94 | **0** | 20/20 | 4/4 | 0 |
| drag warm **dev** | yes | 0→0 | 0→0 | 1→94 | **0** | 19–20/20 | 3–4/4 | 0 |

### Column breakdown (single warm release)

All columns `gen_stale=0`. `pixel_unchanged` ≈ full column (x=0: 3/4). Gen-based STALE cannot see palette stickiness.

### After `palette_color_pulse` (single, fresh jsonl)

- `strip_insert` ×4 (viewport rows), `composite_*` ×20, `tile_ready_emitted` ×20  
- **Processed fingerprints mostly unchanged** despite strip insert + full palette RGB↔CMY swap — BE did schedule/reinsert, but content often matched prior hash (open BE follow-up: palette application / LUT / residual vs hash sensitivity).  
- **`strip_discarded_by_doc_gen` = 0** (expected: doc_gen stable on palette path).

GUI Z11: run under `DITHER_TRACE=1` with palette bound; then **Probe visible tiles**.

---

## 4) Probe visible tiles (DITHER_TRACE)

- Button + **Ctrl/Cmd+Shift+P** on Stress panel  
- IPC `tile_gen_trace_cache_probe` now returns Composite **and** Processed (gen/dirty/fingerprint), `doc_gen`, `layer_gen`, `palette_revisions`, and flags that gen excludes palette/soft_proof  
- Trace events: `visible_tile_probe` / `visible_tile_probe_summary`  
- Heuristic: BE `fresh` at live `doc_gen` + FE gen already current → **fe_likely** (gen-blind); BE dirty/missing → **be_likely**

Author stuck tile: use Probe while column 0 looks stale — if BE Composite/Processed `fresh` + fingerprints moved but FE bitmap old → FE; if BE fingerprint/dirty still old → BE.

---

## 5) `strip_discarded_by_doc_gen` reschedule?

**Not guaranteed.**

Discard branch (`tile_pipeline.rs:546–575`): when `now_gen != compute_gen` after strip compute:

- Emits `strip_discarded_by_doc_gen`
- Returns an in-memory Arc for the **claiming** coord only  
- **Does not** `insert_fresh_gen` for the strip  
- **Does not** call `reschedule_if_insert_rejected`  
- **Does not** call `wake_ed_frontier_after_strip_batch`  

`reschedule_if_insert_rejected` (`:850–873`) only runs on the **insert** path (`:517`). Mid-strip gen advance returns `InvalidState` (`:428–431`) — also no strip-wide re-place.

**Tests:** `ed_strip_gate_mid_cancel_releases_waiters` (`:1806–1868`) checks waiters exit / gate release — **not** that owner + waiters are re-enqueued after discard. **No test** asserts re-placing after `strip_discarded_by_doc_gen`.

Palette A2 path does not bump doc_gen, so this discard mode is **orthogonal** to palette stickiness (Z11 saw 0 discards). It remains relevant to Z10 param storms.

---

## Implications for the author bug

1. **Gen inventory gap:** palette (and soft-proof) changes are invisible to FE `maxKnownGen` / safety → explains “safety doesn’t fix”.  
2. **No `DocumentChanged`:** no `tileRev` bump on color update → no bulk FE invalidate (unlike filter_updated). Recovery relies on `tile-ready` refetch at **same** gen (should still paint) or pan/zoom (`tileRev++`).  
3. **Z11:** STALE_GEN_AT_QUIET always 0 under palette-only; strip discard not involved; strip ownership still x=0.  
4. **Next fix candidates (not done here):** include palette revision (or content epoch) in serve gen / FE compare; emit document-changed or dedicated palette-changed wake; on strip discard, re-schedule owner+strip waiters; call `request_preview_refresh` (residual clear) from palette invalidate.

**Do not fix in A2** — stop here.
