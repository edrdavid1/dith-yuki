# Implementation Plan: Track C — Phase 1 Filters

План закрывает C1–C4 из [tech-debit.md](../tech-debit.md). Спека: [requirements.md](./requirements.md), [design.md](./design.md).

**Gate:** трек A закрыт перед merge C1–C3. C4 можно начинать сразу.

Рекомендуемый порядок внутри трека: C1 → C2 (общий ordered path) → C3 CRT → C3 Glow → docs; C4 параллельно в любой момент.

---

## 0. Baseline

- [x] 0.1 Confirm Track A green
  - `dither_seam_matrix` / A tasks DoD; note date in §0.1 result
  - _Requirements: 1_

- [x] 0.2 Inventory extension points
  - `DitherModeV2`, `get_threshold`, `apply.rs`, frontend `DitherModeV2` + chooser
  - `engine-io` sandbox; export menu entry points
  - _Requirements: 8_
  - Inventory: `filter.rs` (`DitherModeV2`, `FilterKind`/`FilterParams`); `dither_ordered.rs` (`get_threshold`/`get_threshold_i32`, `apply_ordered_with_cache`); `apply.rs` `dispatch_dither_v2`; TS `types/index.ts` + `DitherSettings` / `EffectChooserDialog` / `EFFECT_TO_FILTER_KIND`; `engine-io` `sandbox::resolve_user_path`; Tauri `export_image` + frontend `useDocument` save dialog (PNG/JPEG).

- [x] 0.3 Link docs
  - Point this folder from `tech-debit.md` Track C
  - _Requirements: 9.2_
  - Already linked in `tech-debit.md` Track C header (requirements/design/tasks).

**§0.1 result (fill in):**

```
Date: 2026-08-12
A seam matrix / A2 status: dither_seam_matrix 6/6 ok; all ps∈{1..32}×{Bayer,FS} c0/u0; Atkinson sample clean; Track A tasks DoD closed (2026-08-11).
Gate decision: proceed C1–C3
```

---

## 1. C1 — CMYK Halftone

- [x] 1.1 Types + serde
  - Add `DitherModeV2::CmykHalftone` (Rust + TS); validate; round-trip tests
  - Optional params: cell size / angles (or constants v1)
  - _Requirements: 3.1, 3.2, 8.1_

- [x] 1.2 Screen math helpers
  - Rotated cell + distance + √t radius; unit tests with fixed numbers
  - Coords only via `GlobalCoord` / `GlobalCoordSigned` + `aligned(pixel_size)`
  - _Requirements: 2.1–2.3, 3.3_

- [x] 1.3 Wire ordered apply
  - Branch in ordered engine; CMYK split → screens → RGB reconstruct
  - Palette via `PaletteLut3D` when `palette_id` set; `requires_full_row = false`
  - _Requirements: 3.4, 3.5_

- [x] 1.4 Seam + UI
  - 2×2 pattern continuity test; mode in Dither settings UI
  - Preserve Bayer/FS tests green
  - _Requirements: 2.4, 3.6, 8.2, 8.3, 10_

---

## 2. C2 — Wave / Line Modulation

- [x] 2.1 Types + params
  - `DitherModeV2::Wave { wavelength, amplitude, phase, angle }` (or flat fields on `DitherParamsV2`)
  - Validate ranges per design.md; TS + UI sliders
  - _Requirements: 4.1, 4.5_

- [x] 2.2 Threshold + apply
  - `T = 0.5 + 0.5*sin(...)` on global coords; same quantize path as Bayer
  - `pixel_size` / palette / levels unchanged contract
  - _Requirements: 4.2, 4.3, 2_

- [x] 2.3 Seam test
  - 2×2 Wave continuity; existing props green
  - _Requirements: 4.4, 10_

---

## 3. C3 — CRT

- [x] 3.1 `FilterKind::Crt` + params + apply
  - `filters/crt.rs`; scanlines from `Y_g` via `GlobalCoord`; optional RGB mask from `X_g`
  - Serde, validate, dispatcher
  - _Requirements: 6.1–6.4_

- [x] 3.2 UI + seam
  - Effect chooser + settings; 2×2 horizontal-boundary scanline phase test
  - _Requirements: 6.5, 2.4, 10_

---

## 4. C3 — Glow

- [x] 4.1 `FilterKind::Glow` + blur apply
  - `filters/glow.rs`; radius capped to HALO in v1; threshold + intensity
  - Determinism + alpha policy documented
  - _Requirements: 5.1–5.3, 5.5_

- [x] 4.2 UI + seam sanity
  - Chooser + settings; flat-field / boundary spot check
  - _Requirements: 5.4, 10_

---

## 5. C4 — SVG Export (parallel OK)

- [x] 5.1 `svg_export.rs` greedy meshing
  - `raster_to_svg`; visited mask; `<svg viewBox>` + `<rect>` fills
  - Unit: solid → 1 rect; 2×2 checker expectations
  - _Requirements: 7.1, 7.2, 7.5_

- [x] 5.2 Contour tracing (v1 external contours)
  - Second algorithm enum variant; structural tests
  - _Requirements: 7.3_

- [x] 5.3 Sandbox write + command + minimal UI
  - Path validation; Tauri command; Export → SVG
  - _Requirements: 7.4, 7.6_

---

## 6. Docs and DoD

- [x] 6.1 Update `TILE_PIPELINE.md` / ARCHITECTURE
  - List Halftone, Wave, CRT as GlobalCoord pattern filters; Glow halo/blur; SVG note
  - _Requirements: 9.1_

- [x] 6.2 Mark C1–C4 in `tech-debit.md` when criteria met
  - _Requirements: 9.2_

- [x] 6.3 GPU gate checklist
  - Bayer (existing) + Halftone + CRT seam-green recorded for Track D start
  - _Requirements: 9.3_

---

## Definition of Done (checklist)

- [x] Track A gate satisfied before C1–C3 on main
- [x] `CmykHalftone` + `Wave` in DitherV2 with GlobalCoord + seam tests
- [x] `Glow` + `Crt` filter kinds with UI; CRT seam test; Glow radius≤HALO
- [x] SVG export: meshing (+ contour) + sandbox + menu/command
- [x] Existing dither/filter props green; docs updated
- [x] CPU Halftone + CRT ready as Track D references


## GPU gate checklist result (6.3)

```
Date: 2026-08-12
Bayer: existing ordered_dither_seamless + dither_seam_matrix green
CMYK Halftone: phase1_pattern_seam::cmyk_halftone_2x2_vertical_seam
CRT: filters/crt.rs::crt_seamless_horizontal_boundary
Track D may start against these CPU references.
```
