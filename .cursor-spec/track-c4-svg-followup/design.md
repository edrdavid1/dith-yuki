# Design: Track C4.1 — SVG export follow-up

## Overview

Delta on closed C4 v1. Do not relocate `svg_export.rs`.

| ID | Deliverable |
|----|-------------|
| **C4.1.1** | Export UI + command pass `SvgAlgorithm` |
| **C4.1.2** | Inner contours + evenodd |
| **C4.1.3** | Merge-count + XML parse tests |

Source: [ADDENDUM_release_plan_L_C4.md](../ADDENDUM_release_plan_L_C4.md) vs
[track-c-phase1-filters/design.md](../track-c-phase1-filters/design.md) C4.

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Location | Keep `crates/engine-io/src/svg_export.rs` |
| Mode | User choice only |
| Holes | 4-connected equal-color component; holes = enclosed regions of *other* color or alpha=0; emit as subpaths; `fill-rule="evenodd"` on `<path>` |
| Threshold | Addendum’s `ContourTrace { threshold: f32 }` — v1 already has `tolerance: u8` on `SvgExportOptions`. **Do not add a second threshold.** Wire UI optional tolerance later; MVP = existing tolerance default 0 + algorithm enum |
| IPC | Extend existing `export_image` request body with `svg_algorithm: "greedy_meshing" \| "contour_tracing"` (ignored for PNG/JPEG) rather than a new `export_svg` command unless the body is already too awkward — **prefer extend `export_image`** |
| Parser test | `roxmltree` in `engine-io` dev-dep if not present |

Addendum IPC (`export_svg` + `SvgExportMode`) is **not** followed literally; as-built command is `export_image`.

---

## Contour algorithm upgrade

v1: flood-fill component, walk **outer** Moore boundary, skip holes.

C4.1: after labeling a component, find 8/4-connected holes (background cells with a parent component) and trace each hole. One `<path d="M…Z M…Z">` per color component with evenodd, or one path per contour with a hole compound — **lock: one path element per connected color component**, subpaths for holes.

---

## UI

Export format SVG already exists. When format is SVG, show radio Pixel Grid / Contour next to the save flow (dialog extra, or a small modal before save — **lock: extra controls in the existing export UI**, not a new window type).

---

## Testing

| Test | Assert |
|------|--------|
| Merge | two 4×8 rects of color A separated → 2 rects, not 32 |
| Donut | outer+inner in `d`; evenodd |
| Parse | `roxmltree::Document::parse` Ok |
| Greedy v1 | solid 1 rect |

---

## Future

- Tolerance slider in UI
- Combined meshing then contour
