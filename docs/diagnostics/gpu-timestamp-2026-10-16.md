# GPU timestamp dispatch breakdown

**Date:** 2026-10-16  
**Spec:** `.local-doc/time-test-2.md` (`gpu_timestamp_diag_v1`)  
**Commit:** `d8c1249` (TIMESTAMP_QUERY diag). Measured with that instrumentation on Apple M3.  
**Raw lines:** `profiling/results/2026-10-16/gpu_timestamp_lines.log` (copied from harness stderr)

This report produces numbers only. **No fix is proposed.**

---

## 5.1 Environment

| Item | Value | Source |
|---|---|---|
| Adapter | Apple M3 (Metal) | stderr: `blocked_fs device: Apple M3 (Metal) … timestamp_query=true` |
| `TIMESTAMP_QUERY` advertised/granted | **true** | same line; `set_device_timestamp_available` |
| Host | darwin aarch64 | user machine |
| Build | `cargo test -p dither --release` harness | command below |
| Env | `DITHER_PROFILE=1 DITHER_GPU_PATH=gpu DITHER_PROFILE_QUICK=1` | harness run |

```bash
DITHER_PROFILE=1 DITHER_GPU_PATH=gpu DITHER_PROFILE_QUICK=1 \
  cargo test -p dither --release dither_profile_harness_main_grid \
  -- --ignored --nocapture --test-threads=1
```

Device creation requests `TIMESTAMP_QUERY` only when `DITHER_PROFILE=1` and the adapter advertises it (`crates/engine-gpu/src/blocked_fs.rs` + `timestamp_diag.rs`). Production without the env still uses `Features::empty()` for this feature.

---

## 5.2 Per-scenario breakdown

Medians over **warm** samples (first 3 of each size group dropped when n≥6). Levels=16 grid; L256 rows behave the same (still `dispatches=1`). Values copied from `[diag/gpu_timestamp]` JSON in `gpu_timestamp_lines.log`.

### blocked_fs × 748×1024 (warm n=13)

| Metric | Value | Cite |
|---|---:|---|
| dispatches | **1** | e.g. line with `"size":"748x1024","dispatches":1` |
| strips | 1 | same |
| submit_sum_ms | **0.097** | median |
| poll_ms | **221.988** | median |
| kernel_sum_ms | **220.678** | median |
| dispatch_wall_ms | **222.116** | median |
| kernel_share | **0.994** | 220.678/222.116 |

### blocked_fs × 1920×1080 (warm n=13)

| Metric | Value |
|---|---:|
| dispatches | **1** |
| submit_sum_ms | **0.107** |
| poll_ms | **553.701** |
| kernel_sum_ms | **550.966** |
| dispatch_wall_ms | **553.876** |
| kernel_share | **0.995** |

### blocked_fs × 4000×3000 (n=4; includes harness micro warmups)

| Metric | Value | Cite |
|---|---:|---|
| dispatches | **1** | `"size":"4000x3000","mode":"blocked_fs"` |
| submit_sum_ms | **0.132** | median |
| poll_ms | **2831.330** | median |
| kernel_sum_ms | **2816.148** | median |
| dispatch_wall_ms | **2831.582** | median |
| kernel_share | **0.995** | |

Example raw line:

```text
[diag/gpu_timestamp] {"dispatch_wall_ms":2809.476,"dispatches":1,"kernel_max_ms":2795.564,"kernel_ms":"available","kernel_sum_ms":2795.564,"mode":"blocked_fs","poll_ms":2809.324,"poll_per_dispatch":false,"size":"4000x3000","strips":1,"submit_sum_ms":0.115,"timestamp_query":"available"}
```

### blue_noise × 748×1024 (warm n=13)

| Metric | Value |
|---|---:|
| dispatches | **1** |
| submit_sum_ms | **0.083** |
| poll_ms | **127.722** |
| kernel_sum_ms | **125.767** |
| dispatch_wall_ms | **127.814** |
| kernel_share | **0.984** |

### blue_noise × 1920×1080 (warm n=13)

| Metric | Value |
|---|---:|
| dispatches | **1** |
| submit_sum_ms | **0.093** |
| poll_ms | **341.360** |
| kernel_sum_ms | **338.332** |
| dispatch_wall_ms | **341.617** |
| kernel_share | **0.990** |

### blue_noise × 4000×3000 (n=1 manual)

| Metric | Value | Cite |
|---|---:|---|
| dispatches | **1** | single line in log |
| submit_sum_ms | **0.225** | |
| poll_ms | **2359.196** | |
| kernel_sum_ms | **2338.497** | |
| dispatch_wall_ms | **2359.450** | |
| kernel_share | **0.991** | |

```text
[diag/gpu_timestamp] {"dispatch_wall_ms":2359.45,"dispatches":1,"kernel_max_ms":2338.497,"kernel_ms":"available","kernel_sum_ms":2338.497,"mode":"blue_noise","poll_ms":2359.196,"poll_per_dispatch":false,"size":"4000x3000","strips":1,"submit_sum_ms":0.225,"timestamp_query":"available"}
```

---

## 5.3 Answers

1. **Real kernel time?**  
   Available via GPU timestamp queries (`timestamp_query":"available"`). Warm medians: blocked_fs 748→**221 ms**, 1080→**551 ms**, 4k→**2816 ms**; blue_noise 748→**126 ms**, 1080→**338 ms**, 4k→**2338 ms**. `kernel_max_ms == kernel_sum_ms` because **dispatches=1**.

2. **Dispatches per apply?**  
   **Exactly 1** for every measured full-buffer apply at these sizes (`use_full` path; one strip covering the image). Counted in the submit loop (`DispatchTimestampDiag::dispatches`), not inferred.

3. **Kernel fraction of `dispatch_wall`?**  
   **~98–99.5%** in every scenario (all ≫10%). The bottleneck is **GPU kernel execution**, not host submit overhead.

4. **`poll_ms` / `submit_sum_ms`?**  
   Order **10³–10⁴** (e.g. blocked_fs 1080 ≈ **5175**). `submit_sum_ms` is ~0.1 ms; almost all of `dispatch_wall` is spent in `device.poll(Wait)` waiting for the long kernel. This is **not** “GPU idle with empty work” in the sense of missing dispatches — there is one long compute pass.

---

## 5.4 What this does not determine

- Why the kernel itself is hundreds of ms to seconds (algorithm, memory traffic, occupancy).  
- Per-dispatch kernel distribution when `dispatches>1` (strip-binding path not exercised at these sizes on this adapter).  
- Effect of batching or merging dispatches (**not tested**; forbidden by spec).  
- Whether readback overlaps usefully with compute (readback is timed separately outside this JSON).  
- Cross-vendor behavior (Metal/M3 only here).

---

## 5.5 No fixes

This report produces numbers only. **No fix is proposed.**
