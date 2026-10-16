# Release → pixels profiling summary

Date: 2026-10-16  
Machine: Apple M3 (Metal), macOS  
Harness: `dither_profile_harness_main_grid` (release), `DITHER_PROFILE_QUICK=1` (5 warm samples; sizes 748×1024 and 1920×1080)  
Full UI chain T0–T23: instrumentation landed; end-to-end gesture JSONL not collected in this session (requires app + slider). Apply-path marks T7–T15 exercised via harness.

## Total apply time by scenario (median wall_apply)

| Mode | Size | Levels | GPU med (ms) | CPU med (ms) | Speedup |
|---|---|---:|---:|---:|---:|
| blocked_fs | 748×1024 | 16 | 218 | 2895 | 13.3× |
| blocked_fs | 748×1024 | 256 | 222 | 2880 | 13.0× |
| blocked_fs | 1920×1080 | 16 | 528 | 7594 | 14.4× |
| blocked_fs | 1920×1080 | 256 | 517 | 7560 | 14.6× |
| blue_noise | 748×1024 | 16 | 177 | 3847 | 21.7× |
| blue_noise | 748×1024 | 256 | 151 | 3839 | 25.4× |
| blue_noise | 1920×1080 | 16 | 339 | 10314 | 30.4× |
| blue_noise | 1920×1080 | 256 | 338 | 10309 | 30.5× |

## Three largest stages by share (GPU apply, levels=16)

### 748×1024 (~0.77 MP)

| Rank | Stage | blocked_fs med | share | blue_noise med | share |
|---:|---|---:|---:|---:|---:|
| 1 | dispatch_wall | 207 ms | ~95% | 143 ms | ~81% |
| 2 | upload + readback | ~3 ms | ~1–2% | ~2 ms | ~1–2% |
| 3 | host f32↔u8 | ~2 ms | ~1% | ~1 ms | ~1% |

*UI waits not in this table:* debounce ≤150 ms + commit ≤100 ms ≈ ≤250 ms — comparable to whole GPU apply at this size.

### 1920×1080 (~2.1 MP)

| Rank | Stage | blocked_fs med | share | blue_noise med | share |
|---:|---|---:|---:|---:|---:|
| 1 | dispatch_wall | 512 ms | ~97% | 308 ms | ~91% |
| 2 | upload + readback | ~7 ms | ~1% | ~3 ms | ~1% |
| 3 | host f32↔u8 | ~3 ms | ~1% | ~2 ms | ~1% |

### 4000×3000 (GPU host-conversion micro only)

- `in_f32_to_u8` 6.4 ms, `out_u8_to_f32` 5.4 ms (rayon parallel path inside apply)
- Full 12 MP apply grid not completed in this session (time budget)

## Stage detail (GPU) — see also `../harness-20742/summary_harness.md`

Dominant phase is **dispatch_wall** (submit + poll wait). GPU timestamp-query for true kernel time: **not measured** (`TIMESTAMP_QUERY` not requested on the blocked_fs device).

`prepare_ms` stayed **0.00** after `warm_blocked_fs_pipelines()` — pipeline reuse (`pipeline=cached`).

## Hypothesis answers

| ID | Verdict | Numbers |
|---|---|---|
| H1 | **Confirmed on small images; mixed on 1080p** | Debounce+commit ≤250 ms. GPU apply 748²≈150–220 ms → waits ≥ apply. At 1080p blocked_fs ≈528 ms → shader > waits. |
| H2 | **Refuted (GPU)** | Host f32↔u8 + upload + readback ≈5–10 ms vs dispatch 120–510 ms. |
| H3 | **Mostly refuted at these sizes** | blocked_fs 0.77→2.1 MP (~2.7× pixels) → 218→528 ms (~2.4×). Scales with pixels; fixed WG cost does not dominate alone. |
| H4 | **Confirmed** | blocked_fs 1080p levels 16 vs 256: 528 vs 517 ms (noise). |
| H5 | **Refuted after warm-up** | `prepare_ms=0`, warm pipelines reused. First call can pay `pipeline=new` (not in warm medians). |
| H6 | **Refuted** | Selftest is session/gate state (`gpu_path_allowed` was false until harness set Pass). Not re-run inside each `apply_gpu_dither_rgba`. |
| H7 | **Refuted at ≤1080p** | Readback ~2–4 ms ≪ dispatch. 12 MP apply not fully profiled; host conversions there still ~12 ms total. |

## Anomalies

1. First harness attempt with `DITHER_GPU_PATH=gpu` fell back to **CPU** until device warm + `SelftestState::Pass` — release path needs Pass (or debug/`cfg(test)` ForceGpu bypass after a real GPU success).
2. **blue_noise CPU ≫ blocked_fs CPU**, but **blue_noise GPU < blocked_fs GPU** (opposite ranking).
3. `dispatch_wall` is wall-clock around submit/poll, **not** GPU timestamp-query (T12 not measured).
4. Full T0–T23 gesture chain (debounce, IPC, tile-ready, commit, rAF) not numerically joined in this run.

## Environment

- CPU/GPU: Apple M3, Metal  
- `max_storage_buffer_binding_size` reported by blocked_fs probe  
- wgpu via `engine-gpu` cached blocked_fs device  
- OS: darwin 25.x  

## How to reproduce

```bash
# GPU quick grid
DITHER_PROFILE=1 DITHER_GPU_PATH=gpu DITHER_PROFILE_QUICK=1 \
  cargo test -p dither --release dither_profile_harness_main_grid \
  -- --ignored --nocapture --test-threads=1

# CPU quick grid
DITHER_PROFILE=1 DITHER_GPU_PATH=cpu DITHER_PROFILE_QUICK=1 \
  cargo test -p dither --release dither_profile_harness_main_grid \
  -- --ignored --nocapture --test-threads=1

# Full UI marks: release app with DITHER_PROFILE=1, drag a dither slider;
# JSONL → profiling/results/<date>/marks.jsonl
```

## Proposed next step (do not implement here)

1. Enable optional `TIMESTAMP_QUERY` on the blocked_fs/blue_noise device when the adapter supports it, so T12 is real kernel time (today dispatch_wall mixes queue wait).  
2. Run one instrumented UI gesture series (T0–T23) at 748 and 1080 to quantify debounce+commit+tile delivery vs apply — expected: on small images, **cut coalesce / commit** or paint-through without waiting full 100 ms; on large images, **shader/strip scheduling** first.  
3. Investigate blue_noise CPU regression vs GPU (why CPU is slower than blocked_fs while GPU is faster).
