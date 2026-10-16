# Palette shared-memory diagnosis

**Date:** 2026-10-16  
**Spec:** `.local-doc/karnel-diadnostic-small-fix.md` (`palette_shared_v1`)  
**Depends on:** `docs/diagnostics/kernel-slowdown-2026-10-16.md`  
**Commit at measure:** `a5749f3`  
**Raw lines:** `profiling/results/2026-10-16/palette_shared_lines.log`

This report produces numbers and gate decisions only. **No production shader change shipped.**

---

## 4.1 Environment

| Item | Value | Cite |
|---|---|---|
| Adapter | Apple M3 (Metal) | `[diag/palette_shared] adapter=Apple M3 (Metal) …` |
| `TIMESTAMP_QUERY` | **true** | same line |
| Palette | **4096** (RGB cube `n=16` → n³) | `palette_len=4096` |
| Size | 1920×1080 | scratch harness |
| Env | `DITHER_PROFILE=1` | command |
| Scratch | local `engine-gpu` example (deleted; not committed) | — |

```bash
DITHER_PROFILE=1 cargo run -p engine-gpu --example palette_shared_diag --release -- <phase1|before|after|phase3>
```

---

## 4.2 Phase 1 — confirmation

| Variant | Kernel ms | Ratio |
|---|---:|---:|
| Production `blue_noise`, 4096 palette | **307.553** | — |
| No palette, 4096 (ignored) | **0.102** | **3017.676×** |

```text
[diag/palette_shared] phase1 baseline_ms=307.553 nopal_ms=0.102 ratio=3017.676
[diag/palette_shared] phase1 GATE PASS
```

**Verdict:** H-B **confirmed** on production-sized palette (ratio ≫20×). Proceeded to Phase 2.

Absolute baseline (~308 ms) matches the prior harness-scale blue_noise 1080p numbers (~338 ms median) within run-to-run variance.

---

## 4.3 Phase 2 — shared palette

Implemented cooperatively loaded `var<workgroup> palette_local: array<u32, 4096>` in both shaders; distance formula / iteration order / tie-break unchanged. Measured, then **reverted** per gate.

| Variant | blue_noise ms | blocked_fs ms | Speedup (bn) | Speedup (bfs) | Parity |
|---|---:|---:|---:|---:|---|
| Before (storage palette) | **306.558** | **489.872** | 1.0× | 1.0× | n/a (baseline) |
| After (shared palette) | **884.052** / **887.618** | **681.610** / **698.234** | **~0.35×** | **~0.72×** | not shipped |

```text
[diag/palette_shared] phase2 tag=before blue_noise_ms=306.558 blocked_fs_ms=489.872
[diag/palette_shared] phase2 tag=after blue_noise_ms=884.052 blocked_fs_ms=681.610
[diag/palette_shared] phase2 tag=after blue_noise_ms=887.618 blocked_fs_ms=698.234
```

**Gate:** speedup **&lt;1.1×** (regression) → **reverted**. Shared palette is not the bottleneck; the issue is loop structure / ALU volume, not storage→shared traffic for this workload on M3.

Parity / `gpu_parity_matrix quick`: **not re-run** after revert; production shaders restored to pre-change content (byte-identical to `a5749f3` tree for these files).

Adapter shared-memory fit: before revert, shaders compiled and ran (19552 B budget under 32768); rejection was **not** observed.

---

## 4.4 Phase 3 — algebraic nearest (scratch)

Run because Phase 2 speedup &lt;2×.

| Variant | Kernel ms | Speedup |
|---|---:|---:|
| Baseline `blue_noise` 4096 | **310.261** | 1.0× |
| Algebraic `‖p‖²−2⟨p,c⟩+‖c‖²` + `c_norm` buffer | **354.076** | **0.876×** |

```text
[diag/palette_shared] phase3 baseline_ms=310.261 algebraic_ms=354.076 speedup=0.876
```

**Gate:** speedup **&lt;2×** → **discard**. Not worth the complexity. Scratch only; not committed.

---

## 4.5 Decision

Phase 1 confirmed that the 4096-entry linear nearest loop dominates kernel time (~3000× vs no-palette). Phase 2 shared-memory caching **regressed** on Apple M3 (~0.35× blue_noise, ~0.72× blocked_fs) and was **reverted**. Phase 3 algebraic rewrite also **regressed** (~0.88×) and was discarded. **Nothing shipped** to production shaders. Next work (out of scope here) must reduce nearest-loop iterations or replace the O(N) search — not relocate the same loop to shared memory.

---

## 4.6 What remains unknown

- Why shared-memory palette hurt occupancy / latency on Metal/M3 (register pressure vs bank conflicts vs barrier cost on previously barrier-free `blue_noise`) — **not instrumented**.
- Cross-vendor behavior of the shared-palette experiment.
- Behavior at `palette_len > 4096` (fallback path implemented then removed with the revert; **untested**).
- Whether a smaller shared cache (e.g. 256 entries) or tiled palette search would help — **not tested**.
- LUT / hierarchical nearest / other algorithmic reductions — **forbidden by this spec**; not tested.
- Exact byte parity of Phase 3 algebraic vs production (timing-only; discard gate did not require parity ship).
