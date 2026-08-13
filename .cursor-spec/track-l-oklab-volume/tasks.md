# Implementation Plan: Track L — 3D Oklab palette volume

План: [requirements.md](./requirements.md), [design.md](./design.md).
Источник: [ADDENDUM_release_plan_L_C4.md](../ADDENDUM_release_plan_L_C4.md).

**Gate:** none. **Locked:** Rust conversion; L up; static gamut mesh; no canvas isolate; `colors_to_oklab` shared.

**Порядок:** L0 → L1 → L2 → L3.

---

## 0. Baseline

- [x] 0.1 Inventory `oklab.rs`, Color Lab panel/slice selection, palette IPC, frontend package.json (no three yet)
  - _Requirements: 1, 3_

- [x] 0.2 Link docs
  - _Requirements: n/a_

**§0.1 inventory (2026-08-13):**

```
oklab.rs: crates/engine-color/src/oklab.rs
  linear_to_oklab(LinRgb) → Oklab { l, a, b }; input is already linear.
  Palettes store LinearColor; hex IPC uses srgb_to_linear then linear_to_oklab.
Color Lab selection (pre-L): no selected index in colorLabSlice.
  Picker used local colorPickerIndex; list had no shared cursor.
  L3 adds selectedColorIndex on the slice (not in draft snapshot).
palette IPC: frontend/src/shared/ipc/palettes.ts — list/import/generate/CRUD.
  No Oklab commands before L1.
package.json: no three (added in L3).
```

---

## 1. L1 — IPC

- [x] 1.1 DTO + `colors_to_oklab` + `get_palette_oklab`
  - Register commands
  - _Requirements: 1.1–1.2_

- [x] 1.2 Unit: Game Boy (or equivalent) vs `linear_to_oklab`
  - _Requirements: 1.3_

---

## 2. L2 — Gamut asset

- [x] 2.1 Dev script using `oklab.rs` → checked-in mesh
  - Header documents regenerate command
  - _Requirements: 2_

---

## 3. L3 — Viewer

- [x] 3.1 Add `three` dependency; `PaletteVolumeViewer` canvas
  - Axes, orbit, gamut mesh, points
  - _Requirements: 3.1–3.3, 3.5_

- [x] 3.2 Click ↔ Color Lab selection (single state)
  - Draft: `colors_to_oklab`
  - _Requirements: 3.4_

- [x] 3.3 Manual QA note in PR: dark+light palette hole
  - _Requirements: 4.2_

**Manual QA:** palette of only dark + only light colors → hole in mid-L (vertical). No canvas isolate.

---

## Definition of Done

- [x] No JS Oklab math
- [x] Game Boy unit test
- [x] Mesh not built at runtime
- [x] No canvas pixel isolate
- [x] Selection synced
