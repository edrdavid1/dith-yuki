# Design: Track Q — Strict vs Guided palette dither

## Overview

| ID | Deliverable | Notes |
|----|-------------|--------|
| **Q1** | `PaletteDitherMode` + field on `DitherParamsV2` | serde default Strict; migration test first |
| **Q2** | Channel ranges cache + `quantize_channel_guided` | Ordered path |
| **Q3** | ED quantization point | Residuals store unchanged |
| **Q4** | UI + GPU skip + tests | Slider; CPU-only Guided |

Depends on: Track H (bias/angle) and M (ED kernels) already in tree. Source: [SPEC_palette_dither_modes.md](../SPEC_palette_dither_modes.md).

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Default | `Strict` — existing documents and new filters without an explicit choice |
| Type name | Field lives on `DitherParamsV2` (not a separate `DitherV2` struct) |
| Guided vs no-palette | Guided only if `palette_id.is_some()`. `None` = current uniform `levels` |
| Shared threshold v1 | Same Bayer/CustomPng threshold (or same ED compare) for R, G, and B |
| Per-channel threshold phase | **Out of track** — follow-up if visuals need more “life” |
| Snap to palette | **Forbidden** on Guided output |
| GPU | Guided = skip, CPU path. No eligibility-table expansion |
| Who gets Guided | Ordered Bayer/CustomPng + ED modes that already do palette-nearest. Wave / Halftone / CRT: no change |
| UI auto levels | Slider 2–16 always writes `Some(n)` after user move. Initial Guided selection MAY set `channel_levels: None` (engine auto). Optional “Auto” checkbox is not required in v1 |
| Cache | Revision-keyed DashMap next to `PaletteLutCache` (same invalidation) |

Do not reopen: GlobalCoord, rem_euclid, ED corner, LUT vs KD cell boundaries, GPU Bayer exact parity for existing eligible paths.

---

## Current → Target

| Today | Target |
|-------|--------|
| Palette set → always two-nearest Oklab + threshold | Same when Strict |
| No mode field | `palette_dither_mode`, missing JSON → Strict |
| Uniform quantize only if no palette | Unchanged; Guided is a third path (palette present, not nearest) |
| Palette + Bayer → GPU skip | Unchanged; Guided also skip |

---

## Data model

```rust
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum PaletteDitherMode {
    Strict,
    Guided { channel_levels: Option<u8> },
}

impl Default for PaletteDitherMode {
    fn default() -> Self { PaletteDitherMode::Strict }
}
```

Serde: prefer internally tagged or adjacent-to-`DitherColorMode` style already used in `filter.rs`. If an internally tagged enum is awkward for TS, an externally tagged `{ "strict": null }` / `{ "guided": { "channel_levels": 3 } }` is acceptable — **lock at implement time to match existing `DitherModeV2` / color enums**, and keep TS `kind` mapping in one adapter.

Validate Guided `channel_levels`: `None` ok; `Some` in `[2, 16]`.

---

## Guided algorithm

### Channel range

```text
for each channel c in {R,G,B}:
  min = min(palette.colors[*][c])
  max = max(palette.colors[*][c])
  if palette empty or min==max degenerate → [0, 1]
```

Cache key: `(PaletteId, revision)` same as LUT cache.

### Levels

```text
default_channel_levels = ceil(cbrt(max(N,1))).clamp(2, 16)
```

Examples: N=4 → 2; N=16 → 3; N=64 → 4.

### Quantize (ordered)

Reuse the spirit of the no-palette uniform quantize branch, with range and levels substituted:

```rust
fn quantize_channel_guided(
    value: f32,
    range: ChannelRange,
    levels: u8,
    threshold: f32,
) -> f32 {
    let span = (range.max - range.min).max(1e-6);
    let normalized = ((value - range.min) / span).clamp(0.0, 1.0);
    let scaled = normalized * (levels as f32 - 1.0);
    let base = scaled.floor();
    let frac = scaled - base;
    let step = if frac > threshold { base + 1.0 } else { base };
    range.min + (step / (levels as f32 - 1.0)) * span
}
```

`threshold` is the **already** bias/scale-adjusted ordered threshold at that global sample (Track H). Do not bypass `threshold_bias` / `threshold_scale`.

### Quantize (ED)

Replace palette-nearest pick with per-channel `quantize_channel_guided`, using the channel’s accumulated error as the value (existing residual add), and a **shared** decision threshold of `0.5` on the fractional part (same compare for all three channels) **or** the same `frac > 0.5` rule as uniform ED quantize today. Lock at implement: match the no-palette ED quantize compare so Guided ED is “uniform ED inside palette range”, not a second threshold language.

Error buffers: no schema change.

---

## Code touchpoints

- `crates/engine-project/src/filter.rs` — enum, field, `Default`, `validate`
- `crates/engine-color` or `engine-project` — `palette_channel_ranges`, `default_channel_levels`, range cache (prefer next to `PaletteLutCache` in `engine-color` if DashMap already lives there)
- `crates/engine-project/src/filters/dither_ordered.rs` — branch on mode when `palette_id` set
- `crates/engine-project/src/filters/dither_diffusion.rs` — quantize point only
- `crates/engine-gpu/src/prefer.rs` (or current skip helper) — Guided → skip
- `frontend/src/types` / `effects.ts` — TS union
- `frontend/src/features/effects/editors/DitherSettings.tsx` — dropdown + slider, gated on palette

`update_filter` signature unchanged.

---

## Testing

| Test | Assert |
|------|--------|
| `dither_v2_legacy_document_defaults_to_strict_palette_mode` | missing field → Strict |
| `strict_output_always_exact_palette_color` | every pixel ∈ palette |
| `guided_output_not_necessarily_in_palette` | ≥1 pixel ∉ palette on gradient |
| `guided_output_within_palette_channel_range` | channels in range |
| `guided_channel_levels_default_matches_cbrt_formula` | 4/16/64 |
| `guided_gpu_not_eligible` | Bayer+Guided CPU skip |
| Strict identity | Strict + defaults bit-identical to pre-track palette fixture |

Manual: same portrait file, same `pixel_size` / Bayer, Strict vs Guided — Guided must show more shades.

---

## Future

- Per-channel Bayer phase rotate
- GPU Guided (own parity budget, own track)
- Explicit Auto checkbox for `channel_levels: None`
