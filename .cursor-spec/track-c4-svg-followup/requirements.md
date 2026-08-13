# Requirements: Track C4.1 — SVG export follow-up

## Introduction

C4 v1 **закрыт** (2026-08-12): `crates/engine-io/src/svg_export.rs`, greedy
meshing + external-only contour, sandbox write, Export menu. Holes were
explicitly out of scope; `export_image` **hardcodes** `GreedyMeshing`.

[ADDENDUM_release_plan_L_C4.md](../ADDENDUM_release_plan_L_C4.md) описывает
C4 так, будто его ещё нет. Этот трек — **дельта**, не перенос модуля и не
второй экспорт. Код остаётся в `engine-io`.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md). Не зависит от A–M.

## Glossary

- **Greedy_Meshing**: maximal same-color rects → `<rect>`.
- **Contour_Tracing**: connected components → `<path>`; v1 = external only.
- **Hole**: inner contour of a component (even-odd or subpath with reverse winding).

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| User-visible mode: Pixel Grid vs Contour | Автодетект режима |
| Inner contours (donut) | PDF/EPS, animated SVG |
| Valid SVG parse test | Move crate path |
| Greedy merge quality test | Vector editor |

---

## Requirements

### Requirement 1: Explicit mode in UI / IPC

**User Story:** As a user exporting pixel art vs organic shapes, I want to pick meshing or contours — not a hidden default.

#### Acceptance Criteria

1. Export SVG flow SHALL offer two modes (radio or equivalent): Pixel Grid (GreedyMeshing) and Contour. No autodetection.
2. `export_image` (or a thin wrapper) SHALL pass the chosen `SvgAlgorithm` into `write_svg_file` / `raster_to_svg`. Default MAY remain GreedyMeshing if the user doesn’t change it.
3. Reuse the existing save-dialog path; do not invent a second export dialog stack.

### Requirement 2: Contour holes

**User Story:** As a user exporting a ring / letter O, I want the hole preserved, not filled.

#### Acceptance Criteria

1. `ContourTracing` SHALL emit outer **and** inner contours for a component (even-odd fill rule on the path, or equivalent documented winding).
2. Update the module comment that currently says holes are out of scope for v1.
3. Test: raster of a filled circle/square with a hole of a different (or transparent) interior → SVG path contains both outer and inner; filling with `evenodd` shows the hole. Transparent hole vs different-color hole: lock in design (prefer 4-connected component of equal color; hole = enclosed background/other color).

### Requirement 3: Quality and validity tests

**User Story:** As a maintainer, I want proof meshing merges, and that the file is real SVG.

#### Acceptance Criteria

1. Greedy test: a few large same-color rectangles → `<rect>` count is the merged minimum, **not** one rect per pixel.
2. Output SHALL parse as XML/SVG (not only “file written”). Use a strict XML parse in the test (existing crate or `roxmltree`).
3. v1 tests (solid → 1 rect; checker expectations) SHALL stay green for GreedyMeshing.
