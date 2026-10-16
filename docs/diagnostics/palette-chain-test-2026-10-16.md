# Palette dependency-chain hypothesis test

**Date:** 2026-10-16  
**Spec:** `.local-doc/Dependency-Chain-Hypothesis-Testю.md` (`palette_chain_test_v1`)  
**Depends on:** `docs/diagnostics/palette-shared-2026-10-16.md`  
**Commit at measure:** `264bd9b`  
**Raw lines:** `profiling/results/2026-10-16/palette_chain_lines.log`

Scratch-only. **No production shaders changed. No product decision.**

---

## 1. Environment

| Item | Value | Cite |
|---|---|---|
| Adapter | Apple M3 (Metal) | `[diag/palette_chain] adapter=Apple M3 (Metal) …` |
| `TIMESTAMP_QUERY` | **true** | same line |
| Palette | **4096** (RGB cube `n=16`) | `palette_len=4096` |
| Size | 1920×1080 | same |
| Mode | `blue_noise` only | spec §1.4 |

```bash
DITHER_PROFILE=1 cargo run -p engine-gpu --example palette_chain_diag --release
```

---

## 2. Results

| Variant | Kernel ms | Speedup vs baseline | Parity |
|---|---:|---:|---|
| baseline | **316.483** | 1.00× | — |
| unroll2 | **211.226** | **1.498×** | (same reduction logic) |
| unroll4 | **186.306** | **1.699×** | **pass** (0 mismatches) |
| unroll8 | **189.622** | **1.669×** | (same reduction logic) |

```text
[diag/palette_chain] parity=pass variant=unroll4 mismatches=0
[diag/palette_chain] variant=baseline kernel_ms=316.483
[diag/palette_chain] variant=unroll2 kernel_ms=211.226 speedup=1.498
[diag/palette_chain] variant=unroll4 kernel_ms=186.306 speedup=1.699
[diag/palette_chain] gate1=PASS unroll4_speedup=1.699
[diag/palette_chain] variant=unroll8 kernel_ms=189.622 speedup=1.669 vs_unroll4=0.983
[diag/palette_chain] gate2=SATURATED unroll8 vs unroll4 < 1.2×
```

---

## 3. Gate outcomes

**Gate 1:** unroll4 speedup **1.699× ≥ 1.5×** → **PASS**. Hypothesis confirmed at 4-way unroll.

**Gate 2:** unroll8 vs unroll4 = **0.983×** (&lt; 1.2×) → **SATURATED** at 4-way. Further unroll does not help on this adapter.

`blocked_fs` not measured (blue_noise confirmed; spec §1.4).

---

## 4. One-line verdict

**Chain hypothesis: confirmed** (saturates by 4-way unroll on Apple M3).

---

## 5. No fixes

No fixes proposed. No product recommendations. Measurement only.
