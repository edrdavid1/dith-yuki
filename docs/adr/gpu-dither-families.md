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

### Fact: quick parity on committed tree (2026-10-11)

Tree is committed on `feature/gpu-dither-families`. Re-run after integration fixes (`pixel_size` wrap, pipeline cache).

| Field | Value |
|---|---|
| Date | 2026-10-11 |
| Commit | `c54cad4b42a0a0723489aba94cc806b9fa77a01d` (`c54cad4`) |
| Adapter | Apple M3 (Metal) |
| Seed | `0xc1` |
| Mode | `PARITY_MODE=quick` `PARITY_REPEAT=1` |
| Cases | **1794 passed**, 0 failed, 0 skipped; goldens checked=9 fail=0 |
| Test binary | `target/release/deps/gpu_parity_matrix-9eb1fbdb66233a26` |
| Binary SHA-256 | `0c3367bf418edde5bb4a67f87b0054f15b6775ae570c50ee7cfab2a907c4016e` |

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

## Next stage (pixel_size matrix, duplicate renders, geometry) — 2026-10-11

Spec: `.local-doc/Dither следующий этап GPU-семейства (pixel_size, повторные рендеры, геометрия тайла).md`

### Duplicate renders (Task 3)

| Finding | Detail |
|---|---|
| Path | Every processed-tile request for a full-document filter enters `ensure_full_document_ex` (`src-tauri/src/tile_pipeline.rs` ~97). Single-flight + ready cache coalesce same-key callers. |
| Zoom / pan | Do **not** change filter params or `document_gen`; they consume published processed tiles / pyramid. No dither recompute when gen is unchanged. |
| Root cause of N× identical GPU logs | `update_filter` always `increment_generation` + `invalidate_layer` even when params were byte-identical (queued flush in `useEffectLayer.ts` after an in-flight IPC, or a second commit of the same settle). |
| Fix | (1) `update_filter` no-op when params/opacity/blend/enabled unchanged — no gen bump, no invalidate. (2) Frontend skips re-flush when queued patch equals the just-sent params. Debounce remains `PREVIEW_SLIDER_DEBOUNCE_MS = 150`. |
| Warmup | `warm_blocked_fs_pipelines()` on app start when `experimental.gpu_dither_families` is on (`src-tauri/src/main.rs`). |
| Placeholder | `PreviewWindow` keeps prior frame after first paint (`hadPreviewFrame`); busy indicator via `full_document_busy` — already shipped in `e47d2d5`. |

### Geometry freeze (Task 4 / Part A §3.8) — **owner chose A** (2026-10-13)

| Variant | Core | Pad | Threads | Declared workgroup bytes | Apple M3 |
|---|---|---|---|---|---|
| **A (frozen)** | 56×56 | 8 | 64 | **3168** (memory C) | ships |
| B | 120×56 | 8 | 64 | ring-scaled | not shipped |
| C | 248×56 | 8 | 64 | ring-scaled | not shipped |
| D | 248×120 | 8 | 128 | ring-scaled | not shipped |

- **`algo_version = 1`** on `DitherParamsV2` (serde default 1; missing JSON → 1). Geometry A **core/pad/threads unchanged**.
- Declared workgroup memory for A (memory C, 2026-10-14): **3168** bytes = `3 ch × 4 ring slots × 66 rows × 4` (`GEOMETRY_A_WORKGROUP_BYTES`). No full tile cache; input read from storage. Safety margin vs M3 `max_compute_workgroup_storage_size` **32768**: ~29 KiB free.
- Prior declared size was 33808 (full tile + one-channel err plane); that figure is historical only.
- Apple M3 `max_compute_workgroup_storage_size` returned: **32768**. Device create still uses a **32768** floor (`BLOCKED_FS_MIN_WORKGROUP_STORAGE`).
- Adapters reporting a **lower** workgroup-storage (or invocations) limit: startup self-test → **`Fail(limits)`**, GPU path stays closed, **CPU reference runs** (same bytes).

### Joint nearest + display I/O (2026-10-14) — `algo_version` stays **1**

**Bug:** per-channel palette substitution (probe RGB with only one channel error-adjusted) made bright solids (`din ≥ 192` on BW) lock to 100% white; average tone was not preserved.

**Fix (blocked_fs only; exact ED untouched):**
1. Display-referred I/O in `apply_gpu_dither_rgba`: `u8 = round(f32*255)`, `f32 = u8/255` (no `linear_to_srgb` / `srgb_to_linear`). Palette stays sRGB u8.
2. Joint nearest: three Q8.8 error planes (GPU: ring of 4 slots/row/channel); one Euclidean display-u8 pick per pixel; lower index wins ties.
3. Ring = 4 (shipped): destination at `t_d` receives writes from `t_d−1..t_d−3` (lifetime 1..=3 waves). With a read/clear | barrier | write schedule, ring **3** is mathematically enough after freeing `t` before writing `t+3`; shipped size stays **4** for margin. Negative test: ring 3 **without** clear mixes stale `t` into `t+3` and drops parity (catches erasures). Full err plane is **not** required for correctness under this schedule.

**Goldens:** recreated **once** under `PARITY_RECORD_GOLDENS=1` for `blocked_fs`. Reason: prior parity only proved GPU≡CPU byte match, **not** average-correctness; I/O + nearest change intentionally retarget the CPU reference. `algo_version` remains 1 (no external format bump; geometry A unchanged).

**Shared memory (print):** `GEOMETRY_A_WORKGROUP_BYTES = 3168` (= `3×4×66×4`). Floor gate remains 32768; margin ≈ 29 KiB.

### Blue noise option A — scale to interval width (2026-10-15)

**Problem with clamp-only:** at `DEFAULT_SPREAD=256`,  
`shift_q8 = ((rank*4 − 32768) * spread) >> 8` is uniform on ±128 u8, then clipped to ±`half_nn`. Clipping **folds the tails**: ~28% of ranks sit on the +`half_nn` edge for `{0,128,255}` at `g=8` (closest=0, `half_nn=64`) → white fraction ≈0.28 → mean ≈ `0.28×128 = 35.8` instead of ~8.

**Fix — scale, then cap:** one scalar shift; gray quantizes to the covering pair, color to full Euclidean nearest.
1. Interval width `A` (display-u8): **gray** palettes (`r=g=b`) → `A = hi−lo` for the brightness neighbors covering the pixel (LUT at palette build); on a solid level, `A = 2·half_nn`. **Color** palettes → `A = 2·half_nn` of the closest entry.
2. `shift_q8 = ((rank*4 − 32768) * (A<<8)) >> 16` at `spread=256` (≡ `((t−32768)·A)>>8`), then ×`spread/256`. Distribution is uniform on ±`A/2`.
3. Cap: gray → `|shift| ≤ (A/2)<<8` (equals `half_nn` on solids); color → `|shift| ≤ half_nn<<8` (α). Solids never leave their cell.
4. Gray must snap to **`{lo,hi}` only** — full-palette nearest leaks into non-adjacent levels on asymmetric gaps (e.g. `{0,50,255}` at g=56 → mean 38 via black).

**Guarantee sketch:**
| Palette | half_nn / A | Solid stays | Average |
|---|---|---|---|
| `{0,255}` | 127 / A=255 | yes | white%≈g/255 |
| `{0,128,255}` | 64,63,63 / A=128 or 127 | yes | mean≈g (g=8→~8) |
| two colors dist 1 | 0 / A=0 | shift≡0 | exact |

GPU packs `half_nn` in palette α; gray `A` is computed in-shader. `algo_version` stays 1; BN goldens recreated once (scale replaces fold).

CPU warm median ms (20 reps; prior spike; B–D GPU not measured on M3):

| Variant | 1920×1080 ps=1 | 3840×2160 ps=1 | 4000×3000 ps=1 | seam_dY (1920 ramp) |
|---|---|---|---|---|
| A | 88.3 | 350.9 | 509.3 | 0.082 |
| B | 79.0 | 318.7 | 458.2 | −0.346 |
| C | 79.7 | 308.9 | 457.1 | −0.054 |
| D | 74.6 | 291.7 | 438.8 | −0.156 |

PNGs: `target/geometry_spike/{A,B,C,D}_*.png`. Sheet: `.local-doc/geometry_comparison_sheet.png`.

**Not verified:** GPU dispatch matrix for B–D; exact FS column on the comparison sheet; overnight `PARITY_MODE=full` (owner).


### Block representative (Task 2.3)

`with_pixel_size_blocks` samples top-left of each `ps×ps` cell — same rule as `docs/tile-pipeline.md` §6. Exact ED representative helpers in `dither_diffusion.rs` were not opened for a private-API compare; property check is block uniformity + GPU=CPU via app path.

### `blue_noise` routing (Q7)

No routing change. Warm median ms (20 reps, Apple M3, `BENCH_SKIP_8K=1`):

| Path | Size | ps | pal | median_ms |
|---|---|---|---|---|
| gpu | 1920×1080 | 1 | 2 | 57.6 |
| cpu | 1920×1080 | 1 | 2 | 57.1 |
| gpu | 1920×1080 | 1 | 16 | 72.4 |
| cpu | 1920×1080 | 1 | 16 | 96.3 |
| gpu | 3840×2160 | 1 | 2 | 221.4 |
| cpu | 3840×2160 | 1 | 2 | 231.2 |
| gpu | 3840×2160 | 1 | 16 | 275.6 |
| cpu | 3840×2160 | 1 | 16 | 390.9 |
| gpu | 4000×3000 | 1 | 2 | 315.1 |
| cpu | 4000×3000 | 1 | 2 | 334.4 |
| gpu | 4000×3000 | 1 | 16 | 395.9 |
| cpu | 4000×3000 | 1 | 16 | 565.2 |

At pal=2, GPU ≈ CPU (I/O-bound). At pal=16, GPU pulls ahead (~1.3–1.4×) because nearest-palette work moves to the GPU. Owner chooses any routing threshold later.

### Fact: exhaustive sRGB transfer identity (2026-10-12)

Production `linear_to_srgb` vs frozen `linear_to_srgb_reference` over every finite
`f32` in `[0, 1]` (≈1.065 × 10⁹ values). Calls go through `#[inline(never)]` +
`black_box` so LLVM cannot fold the identity away; a deliberate one-unit drift
in the reference fails at `bits=0x0`. Reverse transfer (`srgb_to_linear` LUT vs
`srgb_to_linear_reference`) covers all 256 `u8` inputs in the non-ignored suite.

| Field | Value |
|---|---|
| Date | 2026-10-12 |
| Adapter host | Apple M3 (Metal workstation) |
| Test | `palette::tests::linear_to_srgb_matches_reference_all_f32_in_unit_interval` (`#[ignore]`) |
| Checked | **1 065 353 218** values |
| Result | **pass** (0 mismatches) |
| Wall | 4.6 s (`--release`) |
| Nightly | `.github/workflows/nightly.yml` → job `srgb-transfer-exhaustive` |

Harness:

```bash
cargo test -p engine-color --lib \
  palette::tests::linear_to_srgb_matches_reference_all_f32_in_unit_interval \
  --release -- --ignored --nocapture
```

### Fact: log-only `input_hash` / `setup_hash` at Debug (2026-10-12)

Fingerprints in `apply_gpu_dither_rgba` are computed only when
`log::log_enabled!(target: "gpu_dither", Debug)`. Normal Info logging no longer
pays for blake3. Remeasure (fit 4096², 10 release runs): `setup_hash` p50/p95 =
**0.0 / 0.0 ms**; `render_ms_sum` p50 ≈ **128 ms**.

### Fact: full-document publish default = Once (2026-10-12)

Mass / A / B 30-run alternating matrix (5 scenarios) selected **Option A
(`once`)** as the release default: B failed the p50≤Mass rule on 8192².
Report: `.local-doc/publish_ab_experiment.md`. Scale tables:
`.local-doc/scale_a_large.md`.

**Dead branches removed:** `FdPublishMode::{Mass, SliceOnly}` and
`DITHER_FD_PUBLISH` env override were deleted from the release path after the
matrix. The experiment code lived in the working tree on top of panic-RAII
commit `2f4661f` (leader guard); see that commit plus `.local-doc/publish_ab_experiment.md`
for the Mass/A/B numbers. Shipping path is Once-only: claim/swap + background
parallel publish into the pixel pool after `store_ready`.

## Reference

- Spec: `.local-doc/ED-on-GPU.md`
- Spike: `crates/engine-gpu/src/ed_wavefront_spike.rs`
- UI family map: `frontend/src/features/effects/dither/algorithmFamilies.ts`
- Integration checklist: `.local-doc/Dither интеграция GPU-семейства во фронт, исправления.md`
- Next stage: `.local-doc/Dither следующий этап GPU-семейства (pixel_size, повторные рендеры, геометрия тайла).md`
