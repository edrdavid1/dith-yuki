# Design: Track I — Per-filter Opacity / Blend Mode

## Overview

| ID | Deliverable | Notes |
|----|-------------|--------|
| **I1** | Fields on `FilterInstance` | serde defaults; F file DTO |
| **I2** | Wrapper in `apply.rs` | Full_Then_Blend; reuse `blend_tile` |
| **I3** | UI + DnD audit | Slider + blend select; don't fork reorder |

Source: [ROADMAP_production_release.md](../ROADMAP_production_release.md) §3.
A1 closed — residuals model is stable; wrapper sits **after** `apply_error_diffusion_*` returns.

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Residual | Always from full diffusion / full filter output. Opacity never enters `dither_diffusion.rs` |
| Blend formulas | Reuse `crates/engine-project/src/compositor.rs` `blend_tile` (or extract shared pixel blend used by it) |
| Wrapper site | `apply_filter_to_tile_with_caches` loop: `pre = result.clone(); full = apply_single_filter(pre); result = blend_if_needed(pre, full, instance)` — **not** inside each match arm |
| Fast path | `opacity >= 1.0 && blend_mode == Normal` |
| Defaults | opacity 1, Normal — old files |
| Reserved BlendMode | Reject on validate/set (same as layer if that's the policy; else map to Normal — **lock: reject**) |
| DnD | Existing `reorder_filter` + LayersPanel; no second stack |

ROADMAP sketch (keep):

```rust
fn apply_filter_with_blend(pre: &PixelTile, instance: &FilterInstance, ctx: &FilterCtx) -> PixelTile {
    let full_result = apply_filter(pre, instance, ctx);
    if instance.opacity >= 1.0 && instance.blend_mode == BlendMode::Normal {
        return full_result;
    }
    blend(pre, &full_result, instance.opacity, instance.blend_mode)
}
```

ED writes to `ErrorResidualsStore` **inside** `apply_filter` / diffusion — that happens on `full_result` before the wrapper blends. Neighbors therefore see 100% diffusion. Correct.

---

## Current → Target

| Area | Today | Target |
|------|--------|--------|
| `FilterInstance` | id, kind, params, enabled, requires_full_row | + opacity, blend_mode |
| Layer | already has opacity/blend | unchanged |
| `apply.rs` loop | `result = apply_single_filter(&result, …)` | wrap with Full_Then_Blend |
| Reorder | `reorder_filter` IPC + LayersPanel drag | keep |

---

## Persistence

- `document_dto` / `FilterInstanceFile`: add fields with defaults.
- Invalidation: treat opacity/blend as param-equivalent (same generation bump as `update_filter`).

---

## Testing

| Test | Assert |
|------|--------|
| Fast path identity | opacity 1 Normal == current hash |
| ED 50% 2×2 | no seam; mix with pre |
| Glow 50% | visual mix, no residual involvement |
| Serde missing fields | 1.0 / Normal |
| Review | grep apply paths for `.opacity` outside wrapper |

---

## Future

- Blend as GPU post if filter itself is GPU — out of scope (CPU blend after GPU full_result is fine)
