# Second interaction — measurement report (stop before implementation)

**Date:** 2026-10-22  
**Constraint:** measurements only; no product/architecture change.  
**Harness:** `src-tauri/src/second_interaction_harness.rs` (ignored test)

## 4) Where “Rendering…” comes from (file:line)

| Piece | Location | Condition |
|-------|----------|-----------|
| Label text | `frontend/src/components/PreviewWindow.tsx:325-328` | `fullDocumentBusy && !hadPreviewFrame` |
| Corner chip (no label) | `PreviewWindow.tsx:330-336` | `fullDocumentBusy && hadPreviewFrame` |
| `hadPreviewFrame` init | `PreviewWindow.tsx:88` | `false` |
| Reset on doc change | `PreviewWindow.tsx:110-113` | `setHadPreviewFrame(false)` on `docId` |
| Set true | `PreviewWindow.tsx:122-124` | when `full-document-busy` payload `busy: false` |
| Event listen | `frontend/src/shared/ipc/events.ts:109-121` | `onFullDocumentBusy` → `full-document-busy` |
| BE arm busy | `src-tauri/src/commands/mod.rs:277-281` | `begin_full_document_busy` when refcount 0→1 |
| BE emit | `commands/mod.rs:300-331` | `emit_full_document_busy` |
| Called from refresh | `commands/mod.rs:605-607` | `run_preview_refresh` always begins busy |
| Clear when wave idle | `commands/mod.rs:666-674` | `on_preview_task_finished` → `clear_full_document_busy` |
| **#1 add path (no busy)** | `filter_service.rs:516-524` | `invalidate` + `schedule_dirty_viewport_tiles` only |
| **#2+ update path (busy)** | `filter_service.rs:1378-1390` | `request_preview_refresh(..., clear_residuals)` |

### Busy-cycle difference (FE)

| Cycle | Trigger | `hadPreviewFrame` | UI |
|-------|---------|-------------------|-----|
| After open / before any busy | — | `false` | — |
| Interaction **#1** (`add_filter`) | schedule only | stays `false` | **no** Rendering… |
| Interaction **#2** (`update_filter`) | `begin_full_document_busy` | still `false` | **Rendering…** + dim overlay |
| End of #2 wave | `busy: false` | → `true` | — |
| Interaction **#3+** | busy again | `true` | tiny corner chip only |

There is **no separate time threshold** for the label — only the boolean pair above. Busy duration = entire preview wave idle (queue + inflight + pending refresh), not first tile paint.

## 1–3) Measurements so far

### Harness design

- Starts **filter-less** Raw doc (unlike Z9/Z11 which pre-install dither).
- **#1** mirrors `add_filter` (no busy).
- **#2–#8** mirror `update_filter` via `request_preview_refresh`.
- Simulates FE `hadPreviewFrame` for `fe_would_show_rendering`.
- Wall = mutate + schedule/refresh + wait_silence (viewport quiet).

### Release 1920×1080 (`DITHER_JOURNAL=0`, earlier run)

| Finding | Evidence |
|---------|----------|
| Rendering? **only on #2** | every scenario |
| Wall #1 ≈ #2 ≈ #3… (~130–160 ms) | FS sequences |

### Release 4000×3000 (`matrix_release.md`, `DITHER_JOURNAL=0`, gpu_path=auto)

| Scenario | #1 wall | #2 wall | #3 wall | Rendering only #2? | Notes |
|----------|--------:|--------:|--------:|:------------------:|-------|
| `fs_same_ps8` | 252 | 249 | 232 | yes | **Sequence alone: no compute spike** |
| `fs_ps_alt` (1→8→9…) | 184 | 277 | 225 | yes | #2 ≈ later; param noise |
| `fs_levels_alt` | 371 | 344 | 293 | yes | #1 actually slowest |
| `bayer_ps_alt` | **184** | **533** | **836** | yes | **#1 fast, updates 3–4× slower** |
| `blocked_fs_ps_alt` | **40820** | 889 | 673 | yes | #1=ps1 CPU FD; #2+=ps8; `pipeline=n/a` |
| `blue_noise_ps_alt` | **62927** | 1750 | 1255 | yes | same pattern as blocked_fs |

Interpretation:

1. **Rendering… is 100% path/UI** — only the first `request_preview_refresh` after an `add_filter` that never armed busy.
2. **FS tiled:** sequence number does **not** uniquely slow #2 when params match (`fs_same_ps8`). Perceived #2 slowness on FS is dominated by busy overlay + wait-for-wave-idle vs progressive #1.
3. **Bayer:** real compute regression on updates vs add (investigate Path B / over-invalidate) — closest harness match to “#1 fast, later slower”.
4. **BlockedFs/BlueNoise:** opposite of user FS story when #1 is ps=1: first FD apply is the expensive cold/CPU pass; later ps=8 edits are cheaper. GPU path was **CPU** here (`pipeline=n/a` — selftest/GPU family not taken in this harness process).

### A/B / remaining

- A/B `DITHER_GPU_PATH=cpu` + journal off: in flight / see `run_ab_cpu_4k.log`
- Harness never uses `with_document_undo` or journal writers — undo/journal cannot explain harness walls
- Dev profile + true 6k (6016×4000) optional follow-up
- Live UI still needed for tile:// / FE decode stage split (harness skips Tauri tile-ready IPC)

## 5) Cold-start / warm-up candidates (code inventory)

Created once (or rarely), then reused — **independent of slider values** unless noted:

| Artifact | When first created | Approx cost | Param-sensitive? | Warm-up candidate? |
|----------|-------------------|-------------|------------------|--------------------|
| wgpu `GpuContext` / adapter | app start or first GPU touch | seconds cold; process-lifetime | No | Yes — already at startup when GPU present |
| `blocked_fs` / `blue_noise` compute pipelines | first GPU apply (`pipeline=new`) | tens–hundreds ms compile; then `cached` | No (algo-fixed) | **Yes** — `warm_blocked_fs_pipelines()` already exists (`blocked_fs.rs:557-578`); `gpu_selftest_host` can call it |
| GPU selftest Pass | startup | one small parity pass | No | Yes — gate for GPU path |
| Path B resident pipelines (Bayer/…) | first Path B frame | compile + VRAM slots | Algo family | Yes if Path B opt-in |
| `PaletteLutCache` / KD | first palette nearest | LUT build ~palette size | Palette content | Warm default palette on open |
| `FullDocumentCache` slot | first FD ensure | full RGBA f32 ≈ W×H×16 B | Params+gen | Warming = full render (expensive); low priority |
| `ErrorResidualsStore` / strip gates | first ED pass | per-tile residuals | Cleared every dither refresh | No — cleared on each update |
| `BlockRepresentativeCache` | first ps>1 mega-pixel | scans Raw | ps | Partial — rebuild on ps change |
| TileCache Raw | open image | ~1.03 MB × tiles | No | Already on open |
| FE `hadPreviewFrame` | first **busy** cycle end | n/a | Path asymmetry | **Fix candidate:** set true after first successful paint / arm busy on add — not a warm-up |

### Open-time warm-up benefit (estimate)

| Action | Benefit | Memory | When | Cancel |
|--------|---------|--------|------|--------|
| Keep `warm_blocked_fs_pipelines` after selftest | Removes `pipeline=new` from first BlockedFs/BlueNoise **edit** | Pipeline objects in VRAM (small vs atlas) | After selftest Pass | Skip if `DITHER_GPU_PATH=cpu` / no adapter |
| Ensure selftest finished before first user FD edit | Avoids CPU fallback then GPU switch | — | Startup | Already gated |
| Pre-build LUT for document default palette | Faster first palette dither | 64³ LUT ~1 MB order | After open + palette known | Evict with palette |
| Arm busy on `add_filter` **or** set `hadPreviewFrame` on first tile-ready | Fixes misleading Rendering… on #2 | 0 | FE one-liner / BE parity | N/A (correctness/UX) |
| Do **not** pre-run full 6K ED | Would burn CPU/RAM for unused params | Huge | — | — |

**Priority recommendation (for later implementation):**  
1) FE/BE busy parity on add (UX truth)  
2) Pipeline warm already partially present — verify it runs before first user FD gesture  
3) Default palette LUT  
4) Never warm full-document ED/FS for open

## How to re-run

```bash
# Quick FS (1080p)
DITHER_SECOND_IX_QUICK=1 DITHER_JOURNAL=0 DITHER_TRACE=1 \
  cargo test -p dither --release second_interaction_matrix -- --ignored --nocapture --test-threads=1

# Full 4k matrix
DITHER_SECOND_IX_SIZE=4k DITHER_JOURNAL=0 DITHER_TRACE=1 \
  cargo test -p dither --release second_interaction_matrix -- --ignored --nocapture --test-threads=1

# A/B CPU
DITHER_JOURNAL=0 DITHER_GPU_PATH=cpu DITHER_SECOND_IX_SIZE=4k \
  cargo test -p dither --release second_interaction_ab_cpu_no_journal -- --ignored --nocapture --test-threads=1
```

Artifacts: `profiling/results/2026-10-22/second_interaction/`
