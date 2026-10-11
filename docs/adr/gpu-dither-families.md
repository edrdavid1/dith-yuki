# ADR: GPU dither families (`blocked_fs`, `blue_noise`)

**Status:** Accepted (approximate path)  
**Date:** 2026-10-10  
**Spec:** `.local-doc/ED-on-GPU.md` (`gpu_dither_families_v1`)

## Context

Exact error diffusion (Floyd–Steinberg and siblings) is sequential. An earlier
`ed_gpu_prototype` showed that naive anti-diagonal parallelism races on the
FS weight `(−1,+1)`. Spec §0.3 required a wavefront spike with `t = x + 2y`,
fixed-point Q8.8, tiled residual carry, before shipping approximate families.

### Spike result (§0.3) — FAILED

Runner: `cargo run -p engine-gpu --example ed_wavefront_spike --release`  
Hardware: Apple M3 (Metal).

| Size | CPU (ms) | GPU (ms, incl. upload/readback) | Speedup | Mismatches |
|---|---|---|---|---|
| 256² | 0.3 | 285.6 | ~0.00× | 24 353 |
| 4096² | 58.5 | 451.2 | 0.13× | 6 757 742 |
| 16384² | 1082.6 | — | — | buffer limit (`max_buffer_size` 256 MiB; `u32` packing needs 1 GiB) |

Acceptance required: 4K bit-identical **and** ≥2× on 16K including I/O.

**Conclusion (spike implementation — not a verdict on wavefront FS):**
Wavefront FS did not show the expected results in the current spike
implementation: 6.7M mismatches at 4K and 0.13× speedup at 4K (16K not
runnable under default wgpu `max_buffer_size` without striping). Possible
causes — a synchronization error (`workgroupBarrier` not on every wave,
and/or a conflicting `atomicAdd`) and the absence of striping for 16K. The
mathematical correctness of wavefront FS (`t = x + 2y`) is **not** disputed.
Returning to exact GPU ED requires a separate spike with corrected
synchronization; for the current PR the approximate family is accepted.

Therefore the approximate `blocked_fs` family proceeds. `blue_noise` proceeds
regardless.

### Follow-up scheme (2026-10-11) — in-workgroup wavefront

Document: `.local-doc/Dither GPU-аналоги ED — схема реализации.md`

Hypothesis for the spike failure: host dispatch per wave (~12k on 4K). New
design moves `t = x + 2y` **inside** the workgroup (`workgroupBarrier`), one
dispatch for all tiles.

| Check | Result (Apple M3 Metal) |
|---|---|
| Geometry | 56×56 core + 8 warm-up = 64×64 WG |
| CPU↔GPU 3×3 tiles (168²) | **PASS** (byte-identical) |
| CPU↔GPU 4K | **PASS** (byte-identical) |
| Speed | **not** an acceptance criterion; measure later |

Shaders: `crates/engine-gpu/src/shaders/blocked_fs.wgsl`, `blue_noise.wgsl`.

### Large images vs `max_storage_buffer_binding_size`

Device creation requests the **adapter’s** `max_storage_buffer_binding_size`
(not the 128 MiB `Limits::default()`). Strip height is derived from that limit.
When the full image exceeds the bind limit, host uses **strip-sized STORAGE
buffers** (shader indexes via `buf_y0`). If even one strip cannot fit, the GPU
entry returns `use CPU path` and the app/CPU reference is used — no panic.
Uncaptured wgpu errors are logged (`on_uncaptured_error`) instead of aborting
the process. Tests can force a tiny bind budget via `force_max_bind` (e.g. 4 MiB
on 4K) to exercise strips without allocating 16K.

### Palette length > 256 (257 colors)

Parity matrix §4 requires a 257-color case. Both families use a **global storage
palette** (`array<u32>` binding) and iterate `0..palette_len` in nearest-color
search — there is **no** validation error and **no** 256-entry shared-memory
cap on the search. Shared memory is not used for the palette in the shipping
shaders; `blue_noise` previously loaded only `min(n, 256)` into workgroup
memory (a bug for n>256) and was corrected to the same global path as
`blocked_fs`. CPU and GPU must still agree byte-for-byte at 257.

### Parity matrix — CI vs release

| Gate | Mode | When |
|---|---|---|
| Per commit / PR CI | `PARITY_MODE=quick` | ~1.8k cases, all critical categories; ~10 s on M3 |
| Pre-release | `PARITY_MODE=full` | Full deterministic matrix; multi-hour (not CI) |
| Optional soak | `PARITY_MODE=random` | Separate from `full`; same `PARITY_SEED` for resume |

Harness: `cargo test -p engine-project --test gpu_parity_matrix --release -- --ignored --nocapture --test-threads=1`.

### Fact: full parity run (2026-10-11)

Recorded result of the deterministic `PARITY_MODE=full` overnight on the agent workstation (uncommitted GPU-families worktree on top of `7a92791`).

| Field | Value |
|---|---|
| Date | 2026-10-11 |
| Base commit | `7a9279148399da1295d74ae06b27969b8b4327fe` (`7a92791`) — branch `chore/gamedev-followups-ipc-bench-report`; GPU parity sources were **uncommitted** at run time |
| Adapter | Apple M3 (Metal) |
| Seed | `0xc0ffee` |
| Cases | 231 622 total → **231 612 passed**, 0 failed, 10 skipped |
| Skips | 10× `params_pad` with pad∈{4,16} (GPU warm-up fixed at 8; not a byte mismatch). Required groups `full_large` / `full_strips`: skipped=0 |
| Wall time | 33 948.56 s (~9.4 h) |
| Test binary | `target/release/deps/gpu_parity_matrix-9eb1fbdb66233a26` |
| SHA-256 | `06d8ac89b7bf1d4f361db87cbb6a192f68623035655b52aff9218aa9fbb8b4e1` |

## Decision

1. Add two families: `blocked_fs` (approximate blocked FS) and `blue_noise`
   (threshold-shift + nearest palette).
2. Fixed-point Q8.8 only; CPU reference defines canonical bytes.
3. `GpuEligibility::Conditional` — GPU when probe + `experimental.gpu_dither_families`
   allow; otherwise identical CPU reference. Never silently substitute exact ED.
4. File format feature ids `blocked-fs` and `blue-noise` at version **1.2**
   (reuse existing feature-id mechanism; no `feature_level` field).
5. Runtime flag `experimental.gpu_dither_families` defaults **false**.
6. Parity policy: **quick** on every CI commit; **full** before release (not in ordinary CI).

## Consequences

**Positive:** Fast path for large images once GPU lands; deterministic
cross-machine output via fixed-point; honest labelling.

**Negative:** Not bit-identical to exact ED; two more algorithms to maintain;
format 1.2 readers required for files that use the new modes.

## Alternatives considered

| Alternative | Outcome |
|---|---|
| Exact GPU ED (wavefront `t=x+2y`) | Deferred — current spike failed parity/speedup; math still valid; retry with fixed sync + striping |
| Stochastic FS | Rejected — same dependency chain, non-deterministic |
| NVIDIA SDK blue-noise texture | Rejected — license / reproducibility |
| 8-bit threshold texture | Rejected — rank collisions (64 px/level) |

## Color space

Display-referred 8-bit per channel. `LinearColor` in this codebase stores
display-referred values in filter I/O after `linear_to_srgb` / `srgb_to_linear`
at the tile boundary. Palette distance is Euclidean in display-referred RGB.
No linear round-trip inside the algorithm (matches exact ED).

## Q8.8 rationale

WGSL has no `i64`. `error × weight` in Q16.16 overflows `i32`. Q8.8 keeps
products inside `i32` for clamped adjusted values (`|e| ≤ 65280`,
`e*7 ≤ 456960`). Remainder-preserving distribution:
`e1 = e - e7 - e3 - e5`.

## Frontend integration follow-up (2026-10-11)

Spec: `.local-doc/Dither интеграция GPU-семейства во фронт, исправления.md`

### Diagnosis A — `pixel_size` inert

| Step | Finding |
|---|---|
| UI → IPC | `DitherSettings` emits `pixel_size`; `update_filter` deserializes `DitherParamsV2`. Debug log at `filter_service.rs` (`target: gpu_dither`) records mode + `pixel_size`. |
| Graph / ED | Exact ED applies `pixel_size` **inside** the filter (`dither_diffusion.rs` block representatives; `docs/tile-pipeline.md` §6). Not a separate graph stage. |
| Node | **Root cause:** `apply_gpu_dither_rgba` discarded the param (`let _ = params.pixel_size` in `gpu_dither.rs`); `blocked_fs_cpu` / GPU kernels have no `pixel_size` argument. Parity group `params_ps` set the field but never passed it into CPU/GPU — false confidence. |
| Cache | `hash_layer_filter_params` includes full filter JSON (so `pixel_size` is in the key). Preview looked stuck because output bytes were identical, not because the cache ignored the slider. |

**Fix:** ED-style block-representative wrap via `with_pixel_size_blocks` around the unchanged Q8.8 / warm-up kernel (CPU + GPU). `ps=1` remains bit-identical to prior goldens. Golden check requires `pixel_size <= 1`.

### Speed bench (Apple M3 Metal)

| Field | Value |
|---|---|
| Date | 2026-10-11 |
| Commit | `feature/gpu-dither-families` tip at measurement (uncommitted follow-up on top of `607c8df`) |
| Adapter | Apple M3 (Metal) |
| Repeats | 5 warm (spec asks 20; 8K skipped this run — `BENCH_SKIP_8K=1`) |
| Harness | `cargo test -p engine-project --test gpu_dither_bench --release -- --ignored` |

Warm median ms (selected rows):

| Algo | Path | Size | ps | pal | median_ms |
|---|---|---|---|---|---|
| blocked_fs | gpu | 1920×1080 | 1 | 2 | 73.3 |
| blocked_fs | cpu | 1920×1080 | 1 | 2 | 128.7 |
| blocked_fs | gpu | 3840×2160 | 1 | 2 | 293.3 |
| blocked_fs | cpu | 3840×2160 | 1 | 2 | 503.4 |
| blocked_fs | gpu | 3840×2160 | 1 | 16 | 421.8 |
| blocked_fs | cpu | 3840×2160 | 1 | 16 | 983.8 |
| blue_noise | gpu | 1920×1080 | 1 | 2 | 54.0 |
| blue_noise | cpu | 1920×1080 | 1 | 2 | 55.0 |
| blue_noise | gpu | 3840×2160 | 1 | 2 | 206.7 |
| blue_noise | cpu | 3840×2160 | 1 | 2 | 217.6 |

**Answers (§5.3):**

1. **Speedup vs CPU reference:** `blocked_fs` ~1.7–2.3× at 1080p/4K (ps=1). `blue_noise` ≈ parity with CPU (I/O-bound). At `ps=4` the lattice shrinks and GPU≈CPU (both cheap).
2. **Dispatch vs rest:** After session pipeline cache, warm `prepare` is near-zero; remaining cost is upload + dispatch + readback + host linear↔sRGB. Pre-cache, every render rebuilt the compute pipeline (`pipeline=new`) — primary cause of the earlier 285–500 ms reports.
3. **Cold vs warm:** First call pays pipeline create (`pipeline=new`); subsequent calls use `pipeline=cached`. Example: `blue_noise` 1080p cold 137 ms → warm 54 ms.

**Optimization applied from table D:** cache `blocked_fs` / `blue_noise` pipelines for the process lifetime; poll GPU before attributing readback time.

**Not in this bench:** full 20-repeat matrix, 8192², exact `floyd_steinberg` wall times (different apply path).

## Reference

- Spec: `.local-doc/ED-on-GPU.md`
- Spike: `crates/engine-gpu/src/ed_wavefront_spike.rs`
- UI family map: `frontend/src/features/effects/dither/algorithmFamilies.ts`
- Integration checklist: `.local-doc/Dither интеграция GPU-семейства во фронт, исправления.md`
