# Kernel slowdown diagnosis

**Date:** 2026-10-16  
**Spec:** `.local-doc/Diagnose-Kernel-Slowdown.md` (`kernel_slowdown_diag_v1`)  
**Depends on:** `docs/diagnostics/gpu-timestamp-2026-10-16.md`  
**Commit at measure:** `d8c1249`  
**Raw lines:** `profiling/results/2026-10-16/kernel_slowdown_lines.log`

This report produces numbers and hypothesis verdicts only. **No fix is proposed.**

---

## 3.1 Environment

| Item | Value | Cite |
|---|---|---|
| Adapter | Apple M3 (Metal) | `[diag/kernel_slowdown] adapter=Apple M3 (Metal) …` |
| `TIMESTAMP_QUERY` | **true** | same line: `timestamp_query=true` |
| Limits | `max_wg_x=1024` `max_inv=1024` `wg_storage=32768` | same line |
| Host | darwin aarch64 | user machine |
| Env | `DITHER_PROFILE=1` | command below |
| Scratch harness | local `engine-gpu` example (deleted after measure; not committed) | — |

```bash
DITHER_PROFILE=1 cargo run -p engine-gpu --example kernel_slowdown_diag --release
```

Scratch baselines for Exp B–E used a **16-entry** linear palette (`palette_len=16`), not the harness RGB cube (see §3.4). Kernel times are median of 3 timestamp-query samples unless noted.

---

## 3.2 Results table

| Exp | Baseline ms | Variant ms | Ratio (base/var) | Hypothesis verdict |
|---|---:|---:|---:|---|
| A: trivial passthrough | (n/a) | **0.280** | — | **H-A refuted** (≤10 ms) |
| B: blue_noise no palette | **5.868** | **0.115** | **50.967** | **H-B confirmed** (>5×) |
| C: blocked_fs no barriers | **4.828** | **5.164** | **0.935** | **H-C refuted** (<1.5× speedup) |
| D: coalesced read | **4.828** | **5.972** | **0.808** | **H-D refuted** (no speedup) |
| E: occupancy probe | see below | see below | ≤1.13× | **H-E refuted** (no config >2×) |

### Exp A — log

```text
[diag/kernel_slowdown] exp=A name=pass_through size=1920x1080 kernel_ms=0.280
```

Decision: ≤10 ms → hardware fine; continue.

### Exp B — log

```text
[diag/kernel_slowdown] exp=B_baseline name=blue_noise size=1920x1080 kernel_ms=5.868
[diag/kernel_slowdown] exp=B name=blue_noise_nopalette size=1920x1080 kernel_ms=0.115 baseline=5.868 ratio=50.967
```

Decision: drop >5× → palette lookup dominates on this path.

### Exp C — log

```text
[diag/kernel_slowdown] exp=C_baseline name=blocked_fs size=1920x1080 kernel_ms=4.828
[diag/kernel_slowdown] exp=C name=blocked_fs_nobar size=1920x1080 kernel_ms=5.164 baseline=4.828 ratio=0.935
```

Decision: drop <1.5× (variant was slightly slower) → barriers not dominant.

### Exp D — log

```text
[diag/kernel_slowdown] exp=D name=blocked_fs_coalesced size=1920x1080 kernel_ms=5.972 baseline=4.828 ratio=0.808
```

Decision: no speedup → coalescing hypothesis refuted at this size.

### Exp E — log

```text
[diag/kernel_slowdown] exp=E name=bn_wg64 size=1920x1080 kernel_ms=2.404
[diag/kernel_slowdown] exp=E name=bn_wg256 size=1920x1080 kernel_ms=2.689
[diag/kernel_slowdown] exp=E name=bn_wg1024 size=1920x1080 kernel_ms=2.589
[diag/kernel_slowdown] exp=E name=bfs_wg64 size=1920x1080 kernel_ms=4.828
[diag/kernel_slowdown] exp=E name=bfs_wg128 size=1920x1080 kernel_ms=5.436
```

Per-workgroup occupancy metrics: **unavailable** (wgpu does not expose resident-workgroup counts on this adapter). Probe used workgroup size / configuration only.

Decision: no configuration reduced time by >2× (largest relative change among BN sizes is 2.689/2.404 ≈ 1.12× in the wrong direction for “larger is faster”).

---

## 3.3 Dominant cause

**H-B is confirmed.** Removing the palette nearest-color loop from `blue_noise` cut kernel time by **~51×** (5.868 → 0.115 ms) under otherwise identical dispatch. **H-A, H-C, H-D, and H-E are refuted** on this adapter at 1920×1080.

The §0 absolute times (blue_noise ~338 ms, blocked_fs ~551 ms) are **not** reproduced by the scratch 16-entry palette path (~6 ms / ~5 ms). With a 16-entry palette, production `apply_*_gpu_opts` warm calls in the same session were also single-digit ms (e.g. `mode=blue_noise` `kernel_sum_ms=10.961`, `mode=blocked_fs` `kernel_sum_ms=7.138` in the same log). That aligns with the “physics” single-digit expectation once palette work is small. The large harness times from `gpu-timestamp-2026-10-16.md` therefore sit on a **different palette workload** than the scratch 16-entry baselines (see §3.4) — still consistent with H-B as the dominant cost when nearest-palette is live.

No other §1 hypothesis is confirmed. No interaction term among confirmed hypotheses (only one confirmed).

---

## 3.4 What remains unknown

- **Palette cardinality of the §0 harness path vs scratch:** `DitherParamsV2::default()` uses `color_mode: Rgb`, and `levels_palette(levels, Rgb)` builds an **n³ cube** capped at `MAX_SYNTHETIC_CUBE_LEVELS=16` → **4096** entries for `levels=16`. Scratch Exp B–E used `palette_len=16`. Experiments were **not** re-run with a 4096-entry palette; the absolute 338/551 ms numbers were therefore not re-matched in A–E.
- Whether H-C / H-D / H-E ratios change under a 4096-entry palette (**not tested**; each experiment ran alone on the 16-entry path).
- Whether `nearest_*` is called once or twice per pixel in each production shader, and how that scales with `palette_len` (**not instrumented**).
- Cross-vendor behavior (Metal/M3 only).
- True hardware occupancy / register pressure (wgpu: **unavailable**).
- Any sixth cause outside §1 (e.g. strip path, readback, host packing): **not tested** by this spec.

---

## 3.5 No fixes

This report produces numbers and hypothesis verdicts only. **No fix is proposed.** Experimental shaders and the scratch example were local-only and were deleted after measuring; they were not committed.
