# Implementation Plan: Track C4.1 — SVG export follow-up

План: [requirements.md](./requirements.md), [design.md](./design.md).
C4 v1: [track-c-phase1-filters/tasks.md](../track-c-phase1-filters/tasks.md) §5.

**Gate:** none. **Locked:** stay in `engine-io`; extend `export_image`; evenodd holes; no autodetection.

**Порядок:** 0 → 1 → 2 → 3.

---

## 0. Baseline

- [x] 0.1 Inventory `svg_export.rs`, `export_image` SVG branch (hardcoded GreedyMeshing), frontend export format picker
  - _Requirements: 1_

- [x] 0.2 Link from C design + `RELEASE_TRACKS` / `tech-debit`
  - _Requirements: n/a_

---

## 1. Mode plumbing

- [x] 1.1 Request field `svg_algorithm`; pass through to `SvgExportOptions`
  - _Requirements: 1.2_

- [x] 1.2 UI radio Pixel Grid / Contour when format is SVG
  - _Requirements: 1.1, 1.3_

---

## 2. Holes

- [x] 2.1 Inner contour tracing + `fill-rule="evenodd"`
  - Drop “holes out of scope” comments
  - _Requirements: 2.1–2.2_

- [x] 2.2 Donut unit test
  - _Requirements: 2.3_

---

## 3. Tests

- [x] 3.1 Greedy merge-count (not 1 rect/pixel)
  - _Requirements: 3.1_

- [x] 3.2 XML/SVG parse (`roxmltree` or equivalent)
  - _Requirements: 3.2_

- [x] 3.3 v1 greedy tests still green
  - _Requirements: 3.3_

---

## Definition of Done

- [x] User can choose algorithm
- [x] Donut has a hole
- [x] File parses as SVG
- [x] Module not moved
