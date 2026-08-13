# Implementation Plan: Track M — ED kernels + Serpentine

План: [requirements.md](./requirements.md), [design.md](./design.md).
Источник: ROADMAP «Track G» → эта папка. §1 риск — только M2.

**Gate:** A1 closed (already). **M2 separate PR after M1.**

**Locked:** V2 path not legacy buffer; overflow depth 2; serpentine uses `GlobalCoord.y`; default serpentine false.

**Порядок:** M0 → M1 → M2.

---

## 0. Baseline

- [x] 0.1 Inventory `DitherModeV2`, `dither_diffusion.rs` distribute/overflow, `FilterInstance::new` requires_full_row, legacy `DiffusionKernel` fallback in `filter.rs`, DitherSettings mode select
  - _Requirements: 1_

- [x] 0.2 Link docs (`tech-debit` already points here)
  - _Requirements: n/a_

---

## 1. M1 — Kernels (own PR)

- [x] 1.1 Enums serde + `requires_full_row` + TS types
  - _Requirements: 1.1–1.2, 3_

- [x] 1.2 `distribute_*` via offset tables; wire apply match
  - _Requirements: 1.3, 1.5_

- [x] 1.3 Stop JJN/Stucki → FS fallback in V2 `From`
  - Extend PaletteQuantize kernel enum if required
  - _Requirements: 1.4_

- [x] 1.4 Unit offset tests + seam samples; FS/Atkinson still green
  - _Requirements: 1.6_

- [x] 1.5 UI mode entries
  - _Requirements: 1.7_

---

## 2. M2 — Serpentine (own PR)

- [x] 2.1 `serpentine: bool` default false; identity test
  - _Requirements: 2.5–2.6, 3_

- [x] 2.2 Loop direction from `GlobalCoord.y`; mirror kernel in X
  - Parameterize residual consume/produce with `row_dir`
  - _Requirements: 2.2–2.3_

- [x] 2.3 2×2 seam on even **and** odd global rows (new tests, not A1 reuse)
  - _Requirements: 2.4_

- [x] 2.4 UI checkbox for ED modes
  - _Requirements: 2_

---

## Definition of Done

- [ ] M1 merged without serpentine
- [x] Four kernels on residual path; no FS fallback for JJN
- [x] M2: even+odd global row seam tests
- [x] `serpentine=false` bit-identical
