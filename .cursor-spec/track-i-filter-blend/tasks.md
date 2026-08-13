# Implementation Plan: Track I — Per-filter Opacity / Blend Mode

План: [requirements.md](./requirements.md), [design.md](./design.md).
Источник: [ROADMAP_production_release.md](../ROADMAP_production_release.md) §3.

**Gate:** A1 closed (already). Do not pass opacity into diffusion.

**Locked:** wrapper in `apply.rs` loop; reuse `blend_tile`; serde defaults; no per-filter blend copies; DnD = existing IPC.

**Порядок:** I0 → I1 → I2 → I3.

---

## 0. Baseline

- [x] 0.1 Inventory `FilterInstance`, `apply_filter_to_tile_with_caches`, `blend_tile`, `FilterInstanceFile`, layer blend UI, LayersPanel `reorder_filter`
  - Record DnD gaps in §0.1 result
  - _Requirements: 3.3–3.4_

- [x] 0.2 Link docs (`tech-debit`, `RELEASE_TRACKS`)
  - _Requirements: n/a_

**§0.1 result (fill in):**

```
Date: 2026-08-13
DnD as-built: LayersPanel mouse-drag on filter rows → onReorderFilter →
  reorderFilter thunk → IPC reorder_filter. Image Source is not in the
  drag list. EffectSettingsPanel has no stack / no second reorder.
  Gap closed: document-changed listener now refreshes filters on
  filter_reordered (thunk already refreshed; listener was missing).
  No second reorder path.
Layer blend UI to reuse: LayersPanel DropdownMenu + BLEND_MODES
  (12 real modes, no Reserved*). Extracted to shared/blendModes.ts.
  Layer opacity stays the retro popup slider; filter opacity uses
  Track K Slider on EffectSettingsPanel.
```

---

## 1. I1 — Model

- [x] 1.1 Fields + validate + serde defaults on `FilterInstance`
  - _Requirements: 1.1–1.3_

- [x] 1.2 `FilterInstanceFile` + document DTO round-trip
  - _Requirements: 1.4_

- [x] 1.3 TS types / IPC snapshot
  - _Requirements: 1_

---

## 2. I2 — Wrapper

- [x] 2.1 `apply_filter_with_blend` around `apply_single_filter`
  - Fast path; `blend_tile` reuse
  - _Requirements: 2.1–2.4_

- [x] 2.2 Tests: identity fast path; ED opacity 50% 2×2 seam; mix assert
  - _Requirements: 2.5_

- [x] 2.3 Grep/review: no `.opacity` in kind apply modules
  - _Requirements: 4_

---

## 3. I3 — UI

- [x] 3.1 Opacity Slider (Track K) + blend select on selected filter
  - Invalidate via existing update path
  - _Requirements: 3.1–3.2_

- [x] 3.2 Close DnD gaps from §0.1 (if any) via same `reorder_filter`
  - _Requirements: 3.3_

---

## Definition of Done

- [x] Old projects load as 100% Normal
- [x] ED + opacity 50% seamless on 2×2
- [x] Single wrapper; review sign-off
- [x] Layer opacity/blend unchanged
