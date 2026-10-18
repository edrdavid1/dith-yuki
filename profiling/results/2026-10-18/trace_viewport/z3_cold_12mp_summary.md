# Z3 cold ≥12 MPix — TIME_TO_FRESH / holes

## Method

- Doc: **4000×3000 (12 MPix)**, Floyd–Steinberg, cold Processed/Composite eviction before apply.
- Sequence: zoom-in → apply mid-recompute → zoom steps → final zoom `0.25` + full L0 ED prefix schedule.
- Harness: `z3_cold_12mp_time_to_fresh` (backend; no Tauri `tile-ready` IPC).
- `time_to_fresh_ms`: wall until L0 Processed `done == total`.
- `composite_holes`: visible Composite tiles still missing when L0 completes (harness Composite park/wake still incomplete vs production worker — treat as diagnostic, not FE hole count).

## After Step 1 (this branch)

| profile | visible | composite_holes | l0 | time_to_fresh_ms |
| --- | ---: | ---: | --- | ---: |
| release | 12 | 12 | 192/192 | 446.8 |
| dev | 12 | 12 | 192/192 | 752.0 |

## Before Step 1

Backend harness lives on this branch only. **Before** comparison requires either:

1. Live GUI Z3 with `DITHER_TRACE=1` on commit `560eb15` / `c8024ed` (H8-only), same 12 MPix doc, cold cache — read `TIME_TO_FRESH` / `VISIBLE_HOLES` from the stress panel summary; or
2. Cherry-pick `z3_cold_12mp_time_to_fresh` onto a worktree at `560eb15` and re-run.

FE Step 4 adds `TIME_TO_FRESH` / `VISIBLE_HOLES` to `runQuietThenCheck` for live Z3.

## Live FE checklist

```text
DITHER_TRACE=1 DITHER_JOURNAL=0
# open ≥12 MPix doc, CPU Floyd–Steinberg, cold zoom Z3 in Stress panel
```
