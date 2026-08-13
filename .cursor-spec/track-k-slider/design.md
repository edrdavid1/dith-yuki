# Design: Track K — Unified Slider / NumberInput

## Overview

Harden the existing Slider; extract NumberInput; collapse debounce to one layer.

| ID | Deliverable |
|----|-------------|
| **K1** | Debounce ownership locked + implemented once |
| **K2** | `NumberInput` |
| **K3** | Migrate editor raw inputs; grep allowlist |

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Visual | Keep `Slider.module.css` / RetroSlider — no redesign |
| Debounce owner | **Keep 100ms in `useEffectLayer.updateParams`** as the IPC coalescer (already tested). Slider itself stays immediate `onChange` for local/parent state. Do **not** add a second timer in Slider. Document this in the component file so future panels don’t add `setTimeout` |
| Why not move debounce into Slider | Optimistic store updates in `useEffectLayer` need the latest value; a Slider-level debounce would desync optimistic vs IPC or add 200ms. ROADMAP’s “debounce on the component” is satisfied by: no per-panel timers; panels only call `onChange`; the shared hook is the component-adjacent layer |
| NumberInput | Same folder `components/common/NumberInput.tsx`, shared `clampAndSnap` / `formatValue` |
| Curves | Graph stays custom; numeric x/y point fields → NumberInput |
| Glitch seed | NumberInput integer step 1 |

If a panel does not go through `useEffectLayer` (Color Lab, New Project, etc.), that panel MAY use a shared `useDebouncedCallback(100)` from the same helper module — still not a local copy-paste timer.

---

## Adoption rule (process)

New H/I/J/M param UI: Slider or NumberInput only. Raw `<input type="range|number">` in `features/effects/editors/` = process regression (ROADMAP criterion 4).

---

## Testing

- Slider commit on Enter/blur (existing behavior)
- `useEffectLayer` 100ms test remains the IPC debounce proof
- NumberInput clamp

---

## Future

- Keyboard nudging / shift-fine step — not MVP
