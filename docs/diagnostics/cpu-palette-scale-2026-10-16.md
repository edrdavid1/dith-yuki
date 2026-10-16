# CPU performance by palette size

**Date:** 2026-10-16  
**Spec:** `.local-doc/cpu-palette-testing.md` (`cpu_palette_scale_v1`)  
**Commit at measure:** `264bd9b`  
**Raw lines:** `profiling/results/2026-10-16/cpu_palette_scale_lines.log`

Measurement only. **No production code changed. No fixes proposed.**

---

## 4.1 Environment

| Item | Value |
|---|---|
| Machine | Apple M3 (darwin aarch64) |
| Path | `DITHER_GPU_PATH=cpu` via `apply_gpu_dither_rgba` |
| Algorithm | `blue_noise` only |
| Size | **748×1024** |
| Profile | `DITHER_PROFILE=1` |
| Method | 1 dropped warm + median of 5 wall-clock applies |
| Fixture | linear RGB gradient (`f32` RGBA) |

Palettes (separate builders; lengths asserted):

- **16:** grayscale ramp 0…255 (16 entries)
- **256:** 8×8×4 RGB cube (3-3-2)
- **4096:** 16×16×16 RGB cube

```bash
DITHER_PROFILE=1 DITHER_GPU_PATH=cpu cargo test -p dither --release \
  cpu_palette_scale -- --ignored --nocapture --test-threads=1
```

Scratch harness deleted after the run; not committed.

---

## 4.2 Results

| Palette | wall_ms (median) | Ratio to 16 | Ratio to 256 |
|---|---:|---:|---:|
| 16 | **18.9** | 1.00× | 0.079× |
| 256 | **240.6** | **12.73×** | 1.00× |
| 4096 | **3694.9** | **195.54×** | **15.36×** |

```text
[diag/cpu_palette] palettes confirmed lens=16 256 4096
[diag/cpu_palette] palette_len=16 size=748x1024 wall_ms=18.9 (median)
[diag/cpu_palette] palette_len=256 size=748x1024 wall_ms=240.6 (median)
[diag/cpu_palette] palette_len=4096 size=748x1024 wall_ms=3694.9 (median)
[diag/cpu_palette_summary] p16=18.9 p256=240.6 p4096=3694.9 ratio_16_4096=195.54
```

Stop gate: p16 = 18.9 ms ≪ 500 ms → continue.

---

## 4.3 Answer

**H-1 confirmed** (`ratio_16_4096 = 195.54 ≥ 10×`): CPU shows the same palette-size sensitivity as GPU (linear nearest search dominates large palettes).

---

## 4.4 Not verified

- `blocked_fs` on CPU (out of scope for this run).
- Other image sizes / pixel_size ≠ 1.
- Whether CPU `unroll4` would match the GPU chain result (not measured; no fixes).
- Document/user palettes vs these constructed ramps/cubes (counts match; color layout differs from GPU harness cube for the 16-entry case).
- Cross-machine behavior.

---

## 4.5 No fixes

These are numbers only. No fixes are proposed.
