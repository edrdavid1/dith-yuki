# Design: Track H — Bayer Threshold Bias + Pattern Angle

## Overview

| ID | Deliverable | Notes |
|----|-------------|--------|
| **H1** | `threshold_bias` on `DitherParamsV2` | Ordered modes; default 0 |
| **H2** | `pattern_angle` on Bayer / CustomPng | After BRC, never before |
| **H3** | UI + seam tests | Slider; dedicated tests ≠ A2 matrix |

Depends on: Track A2 closed (already). Source: [ROADMAP_production_release.md](../ROADMAP_production_release.md) §2.

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Order of ops | Block_Then_Rotate — ROADMAP §2, do not reopen |
| Angle units | Degrees in params/UI (same as `wave_angle`); radians only inside rotate helper |
| Bias range | `[-0.5, 0.5]`, default 0; clamp threshold after add into `[0, 1)` |
| Who gets bias | Bayer, CustomPng, Wave, CmykHalftone |
| Who gets pattern_angle | Bayer + CustomPng only |
| Rotate formula | Rotate `(gx, gy)` around origin in document space **after** `aligned(ps)`: `x' = x cosθ − y sinθ`, `y' = x sinθ + y cosθ`, then `rem_euclid` into matrix/map. Do not rotate the block grid |
| GPU | Out of this track. CPU path is source of truth; D follow-up if Bayer WGSL must match rotated CPU |
| UI | `DitherSettings.tsx` via Track K `Slider` |

---

## Current → Target

| Today | Target |
|-------|--------|
| `get_threshold_i32(gx, gy)` on aligned global ints | Same, but `gx,gy` may be rotated floats floored/rounded **after** align (lock: floor after rotate, document in code) |
| No bias | `T' = clamp01(T + bias)` |
| Wave/Halftone already have their own angles | Unchanged |

Sampling lock for rotated Bayer: convert aligned `GlobalCoordSigned` to `f32`, rotate, then `floor` to `i32` before `rem_euclid`. Same for CustomPng texel index. Rounding instead of floor is forbidden without a new test proving seam-equivalence — default **floor**.

---

## Code touchpoints

- `crates/engine-project/src/filter.rs` — `DitherParamsV2` fields + `validate`
- `crates/engine-project/src/filters/dither_ordered.rs` — `get_threshold_i32` / apply loop: align → (optional rotate) → sample → bias
- Frontend `DitherSettings.tsx` + `types` / `effects.ts` defaults
- GPU Bayer in `engine-gpu` is **out of scope**; if CPU/GPU mismatch appears for `angle≠0`, document in tech-debit as D follow-up rather than silently GPU-skipping (or skip GPU when `pattern_angle != 0` — acceptable v1: force CPU if angle or bias non-default)

**v1 GPU policy (locked):** if `pattern_angle != 0` or `threshold_bias != 0`, ordered Bayer SHALL take the CPU path (existing `try_ordered_bayer_gpu` returns skip). Prevents silent GPU/CPU divergence.

---

## Testing

| Test | Assert |
|------|--------|
| Default identity | `bias=0, angle=0` matches fixture of current Bayer |
| Bias monotonic | larger bias → more pixels above threshold on a mid-gray field (or equivalent count assert) |
| Angle period | `angle` and `angle+360` bit-identical |
| Seam angle | 2×2 tiles, Bayer4x4, `angle=15`, `ps=1` |
| Seam combo | 2×2, `ps=4`, `angle=30` — blocks still axis-aligned; no edge step |
| Serde | missing fields → 0 |

---

## Future

- GPU rotate/bias uniforms
- Angle for Wave via the same helper (Wave already has `wave_angle` — do not dual-path)
