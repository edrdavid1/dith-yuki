# Implementation Plan: Track K — Unified Slider / NumberInput

План: [requirements.md](./requirements.md), [design.md](./design.md).

**Gate:** none. **Do early.** Locked: one IPC debounce in `useEffectLayer`; no second timer in Slider; extract NumberInput; migrate editors.

**Порядок:** K0 → K1 → K2 → K3.

---

## 0. Baseline

- [x] 0.1 Inventory Slider.tsx, Retro CSS, useEffectLayer debounce test, grep `type="number"` / `type="range"` under `features/effects`
  - _Requirements: 3_

- [x] 0.2 Link docs
  - _Requirements: n/a_

**§0.1 grep allowlist (fill):**

```
Date: 2026-08-13
Slider: frontend/src/components/common/Slider.tsx — custom retro track
  (not <input type="range">) + text box; clampAndSnap / formatValue exported.
  CSS: shared/ui/Slider.module.css + RetroSlider.module.css. No local timer.
Debounce: useEffectLayer.updateParams DEBOUNCE_MS=100; test
  hooks/__tests__/useEffectLayer.test.tsx "debounces updateParams calls by 100ms".
editors/ at K0:
  CurvesSettings.tsx — two type="number" per curve point (x/y scalars) → K3
  Dither/Glow/Crt/RGB/Glitch intensity — already Slider
  Glitch seed — already NumberInput (Track J)
  no type="range" in editors (Slider is custom)
outside editors (not K3):
  features/color-lab/PalettePanel.tsx type="number" — Color Lab, out of scope
```

---

## 1. K1 — Contract + debounce ownership

- [x] 1.1 Comment on Slider: immediate onChange; IPC debounce is useEffectLayer
  - Shared `clampAndSnap` / `formatValue` export
  - _Requirements: 1.1–1.4_

- [x] 1.2 Confirm `useEffectLayer` 100ms test green
  - _Requirements: 4.2_

---

## 2. K2 — NumberInput

- [x] 2.1 `NumberInput.tsx` + unit clamp/commit
  - _Requirements: 2_

---

## 3. K3 — Migrate

- [x] 3.1 GlitchSettings seed → NumberInput
  - _Requirements: 3.2, J UI_

- [x] 3.2 CurvesSettings scalar fields → NumberInput (graph unchanged)
  - _Requirements: 3.2_

- [x] 3.3 Grep editors; only allowlisted leftovers
  - _Requirements: 3.1, 3.3_

**§3.3 leftover (fill):**

```
features/effects/editors/: none (no type="number"|type="range")
allowed outside editors:
  features/color-lab/PalettePanel.tsx type="number" — Color Lab, not an effect editor
```

---

## Definition of Done

- [x] No per-panel debounce copies in new editors
- [x] NumberInput exists and is used for seed
- [x] useEffectLayer debounce test green
- [x] Process rule documented on Slider
