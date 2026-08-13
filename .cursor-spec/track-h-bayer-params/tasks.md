# Implementation Plan: Track H — Bayer Threshold Bias + Pattern Angle

План: [requirements.md](./requirements.md), [design.md](./design.md).
Источник: [ROADMAP_production_release.md](../ROADMAP_production_release.md) §2.

**Gate:** A2 closed (already). H1 MAY land without H2. H2 MUST NOT rotate before BRC.

**Locked:** Block_Then_Rotate; bias `[-0.5, 0.5]`; angle degrees; GPU skip when bias/angle non-default; Track K Slider for UI.

**Порядок:** H0 → H1 → H2 → H3.

---

## 0. Baseline

- [x] 0.1 Inventory `DitherParamsV2`, `get_threshold_i32`, apply loop `aligned(pixel_size)`, `DitherSettings.tsx`, GPU skip hooks
  - _Requirements: 1–2_

- [x] 0.2 Link this folder from `tech-debit.md` / `RELEASE_TRACKS.md`
  - _Requirements: n/a_

---

## 1. H1 — Threshold Bias

- [x] 1.1 Field + validate + serde default 0
  - Rust + TS types; old JSON without field loads
  - _Requirements: 1.1, 4.1–4.2_

- [x] 1.2 Apply: `T' = clamp01(T + bias)` on ordered modes only
  - Unit: bias=0 identity; mid-gray count moves with bias
  - _Requirements: 1.2–1.4_

- [x] 1.3 UI Slider (ordered modes)
  - _Requirements: 1.5_

---

## 2. H2 — Pattern Angle

- [x] 2.1 Field + validate + wrap/period
  - _Requirements: 2.1–2.2, 4_

- [x] 2.2 Rotate helper after align, before `get_threshold_i32`
  - Floor after rotate; `rem_euclid`; not applied to Wave/Halftone/ED
  - GPU skip when `pattern_angle != 0`
  - _Requirements: 2.3–2.5_

- [x] 2.3 UI Slider degrees (Bayer/CustomPng)
  - _Requirements: 2.6_

---

## 3. H3 — Tests

- [x] 3.1 Seam `angle≠0`, `ps=1`
  - _Requirements: 3.1_

- [x] 3.2 Combined `ps>1` + `angle≠0` (rect blocks + no seam)
  - _Requirements: 3.2_

- [x] 3.3 Bias seam; A2 matrix still green at defaults
  - _Requirements: 3.3–3.4_

---

## Definition of Done

- [x] `bias=0, angle=0` bit-identical to pre-track Bayer
- [x] Block_Then_Rotate visible in apply path (review)
- [x] Dedicated seam tests for angle and bias (not only A2 reuse)
- [x] GPU not used for non-default bias/angle
- [x] Old `.dyproj` loads
