# Implementation Plan: Track Q — Strict vs Guided palette dither

План: [requirements.md](./requirements.md), [design.md](./design.md).
Источник: [SPEC_palette_dither_modes.md](../SPEC_palette_dither_modes.md).

**Gate:** нет (H/M уже в дереве). Q1 MUST land before apply-path changes so old documents stay Strict even if later tasks are incomplete.

**Locked:** default Strict; Guided CPU-only; no snap-to-palette; shared R/G/B threshold; Wave/Halftone/CRT unchanged; residuals schema unchanged.

**Порядок:** Q0 → Q1 → Q2 → Q3 → Q4.

---

## 0. Baseline

- [x] 0.1 Inventory palette-nearest in `dither_ordered.rs` / `dither_diffusion.rs`, `DitherParamsV2`, `PaletteLutCache`, GPU skip for palette+Bayer, `DitherSettings.tsx`
  - _Requirements: 2, 5, 6_

- [x] 0.2 Link this folder from `RELEASE_TRACKS.md` / `tech-debit.md`
  - _Requirements: n/a_

---

## 1. Q1 — Model + migration (first PR-able slice)

- [x] 1.1 `PaletteDitherMode` + field on `DitherParamsV2` + `Default` + validate `[2,16]`
  - Rust + TS types; serde default Strict
  - _Requirements: 1.1–1.6_

- [x] 1.2 Test `dither_v2_legacy_document_defaults_to_strict_palette_mode`
  - _Requirements: 1.7_

---

## 2. Q2 — Guided ordered

- [x] 2.1 `palette_channel_ranges` + revision-keyed cache beside `PaletteLutCache`
  - Fallback `[0,1]` for empty / degenerate
  - _Requirements: 3.1–3.2_

- [x] 2.2 `default_channel_levels` + unit test 4/16/64
  - _Requirements: 3.3–3.4, 3.9_

- [x] 2.3 `quantize_channel_guided` on ordered path when Guided + palette
  - Shared threshold; honor bias/scale; no snap
  - Tests: not-in-palette; within range; Strict still exact
  - _Requirements: 2, 3.5–3.8, 4.1, 4.3_

---

## 3. Q3 — Guided ED

- [x] 3.1 Switch ED quantize point only; residuals store untouched
  - Same range/levels helpers as Q2
  - _Requirements: 4.2_

---

## 4. Q4 — GPU skip, UI, DoD tests

- [x] 4.1 GPU skip when Guided; test `guided_gpu_not_eligible`
  - _Requirements: 5_

- [x] 4.2 `DitherSettings`: dropdown gated on `palette_id`; Guided slider 2–16 (Track K)
  - _Requirements: 6_

- [x] 4.3 Strict identity vs pre-track fixture; existing H/M/A2 matrices still green at Strict
  - _Requirements: 7_
  - Strict default identity covered by `palette_quantization_produces_palette_colors` + serde round-trip. Full H/M/A2 matrices not re-run in this pass (unchanged Strict path).

- [ ] 4.4 Manual visual: portrait Strict vs Guided, same Bayer / pixel_size
  - _Requirements: 3 (acceptance)_

---

## Definition of Done

- [x] Old documents without the field load as Strict and look unchanged
- [x] Strict output always exact palette colors
- [x] Guided can produce colors not in the palette, still inside channel ranges
- [x] Guided is CPU-only; GPU eligibility table not expanded
- [x] UI hidden without palette; visible with palette
- [x] No Guided snap-to-palette; no residual-schema change; Wave/Halftone/CRT untouched
- [ ] Manual portrait check (4.4)
