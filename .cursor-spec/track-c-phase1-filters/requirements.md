# Requirements: Track C — Phase 1 Filters (Halftone, Wave, Glow/CRT, SVG)

## Introduction

Трек C из [tech-debit.md](../tech-debit.md) — **новые фильтры Phase 1** и независимый SVG export. Это не correctness-долг и не инфраструктура: deliverable — алгоритмы на CPU, которые пользователь включает в Effect stack / export, и которые станут референсом для трека D (GPU).

**Предусловие:** трек A (A1/A2) закрыт. Иначе Halftone/Wave/CRT наследуют те же классы багов (локальные координаты, halo-clamp, швы). С самого старта использовать:

| Примитив | Источник | Зачем |
|----------|----------|--------|
| `GlobalCoord` / `GlobalCoordSigned` | `engine-tiles` `coords.rs` (готово) | непрерывный паттерн через границу тайлов |
| `BlockRepresentativeCache` | Track A2 | `pixel_size` без ручной block-логики |
| `PaletteLut3D` | Track B1 (желательно) | O(1) nearest при `palette_id` |

C4 (SVG) **не зависит** от C1–C3 и может идти параллельно в любой момент трека.

## Glossary

- **CMYK_Halftone**: ordered-подобный растр по каналам C/M/Y/K с углом экрана на канал и размером точки от тона.
- **Screen_Angle**: аффинный поворот локальных координат ячейки растра (стандарт: C≈15°, M≈75°, Y≈0°, K≈45° — точные default в design.md).
- **Wave_Dither / Line_Modulation**: пороговая функция `T(x,y) = 0.5 + 0.5·sin(...)` на глобальных координатах вместо Bayer-матрицы.
- **Glow**: bloom / soft glow (blur + composite) — отдельный `FilterKind` или params; CPU Gaussian (или box×N) в tile+halo.
- **CRT**: scanlines + опционально phosphor / curvature-lite; яркость линий зависит от `Y_g` через `GlobalCoord`.
- **Pattern_Seam_Test**: непрерывность паттерна на стыке тайлов (образец — тесты в `coords.rs` + 2×2 visual/integration).
- **Greedy_Meshing**: объединение соседних одинаковых цветов в прямоугольники для SVG.
- **Contour_Tracing**: обход контура цветов → path/`<path>` (альтернатива/дополнение meshing).
- **requires_full_row**: флаг диффузии; C1/C2/C3 **не** требуют ED residuals (per-tile параллельны).

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| C1 CMYK Halftone как mode DitherV2 (или эквивалент) | GPU / WGSL (трек D) |
| C2 Wave / Line Modulation dither mode | Новые diffusion kernels (JJN/Stucki) |
| C3 Glow + CRT как CPU filters | Полный физический CRT / lens distortion product |
| C4 SVG export (meshing ± contour) | PDF/EPS, animated SVG |
| Бесшовность через `GlobalCoord` + seam tests | Переписывание tile scheduler |
| UI: выбор mode / params в Effect settings | Color Lab redesign |
| Референс для будущего GPU (Bayer+Halftone+CRT) | Портирование в WGSL в этом треке |

---

## Requirements

### Requirement 1: Gate — Track A Closed

**User Story:** As a maintainer, I want Phase 1 filters started only after ED/`pixel_size` correctness is green, so new modes do not sit on known seam debt.

#### Acceptance Criteria

1. BEFORE merging C1–C3 production paths, Track A acceptance (seam matrix FS/Atkinson + `pixel_size` matrix) SHALL be green as recorded in [track-a-correctness/tasks.md](../track-a-correctness/tasks.md).
2. C4 MAY proceed without waiting on A (export path only).
3. IF A is partially open, C1–C3 implementation PRs SHALL NOT land on main until A closure criteria are met (or an explicit product waiver is recorded in `tech-debit.md`).

### Requirement 2: Coordinate and Block Contracts (All Pattern Filters)

**User Story:** As a developer, I want every new pattern filter to use shared coords and block cache so we never reintroduce `tile_y * 256 + local_y` bugs.

#### Acceptance Criteria

1. CMYK Halftone, Wave, and CRT scanline indexing SHALL obtain document coordinates only via `GlobalCoord` / `GlobalCoordSigned` (no inline `tile.x * TILE_SIZE + …` in production apply).
2. WHEN `pixel_size > 1`, pattern sampling / representative color SHALL use `.aligned(pixel_size)` and `BlockRepresentativeCache` (same contract as Bayer), not halo-clamped local reads for the block origin.
3. Threshold / cell indexing SHALL use `rem_euclid` / helpers equivalent to `pattern_cell` for negative halo coords where halo is in play.
4. A unit or integration test SHALL assert pattern continuity across a shared edge of two adjacent tiles (2×2 canvas minimum) for Halftone, Wave, and CRT.

### Requirement 3: C1 — CMYK Halftone Mode

**User Story:** As a user, I want a print-style CMYK halftone dither so gradients break into angled channel dots instead of Bayer squares.

#### Acceptance Criteria

1. THE engine SHALL expose a new `DitherModeV2` variant (e.g. `CmykHalftone`) wired through serde (`snake_case`), validation, apply dispatcher, and frontend mode select.
2. Per channel (C, M, Y, K) THE filter SHALL apply a screen with configurable **cell size** (or LPI-equivalent) and **angle**; defaults SHALL match design.md (classic offset angles).
3. Dot coverage SHALL be a function of channel tone and distance from the rotated cell center (soft or hard disk — policy in design.md); output MAY be composited back to RGB for display (document conversion path).
4. WHEN `palette_id` is set, final quantization SHALL use `PaletteLut3D` (same as other ordered modes), not a new KD hot path.
5. Mode SHALL set `requires_full_row = false` (no ErrorResidualsStore).
6. Existing Bayer / FS / Atkinson behavior SHALL remain unchanged when those modes are selected.

### Requirement 4: C2 — Wave / Line Modulation Dither

**User Story:** As a user, I want a sinusoidal / line-modulated threshold so dither forms wavy bands instead of a Bayer lattice.

#### Acceptance Criteria

1. THE engine SHALL expose a `DitherModeV2` variant (e.g. `Wave` / `LineModulation`) with params at least: frequency (or wavelength in px), amplitude, phase, and axis/angle (or separate fx/fy).
2. Threshold SHALL follow `T(x,y) = 0.5 + 0.5 * sin(f(X_g, Y_g; params))` (or documented equivalent), with `(X_g,Y_g)` from Req 2; then the same quantize path as ordered dither (levels / palette / `threshold_scale` / `pixel_size` / `color_mode`).
3. Mode SHALL be per-tile parallel (`requires_full_row = false`).
4. Pattern_Seam_Test SHALL pass for Wave on a 2×2 tile canvas.
5. Serde + UI controls SHALL expose the new params with validation ranges documented in design.md.

### Requirement 5: C3 — Glow Filter

**User Story:** As a user, I want a glow/bloom effect on a layer to soft-light bright areas without leaving the CPU pipeline.

#### Acceptance Criteria

1. THE engine SHALL add `FilterKind::Glow` (or equivalent) with params: radius, intensity, threshold (optional), and blend strength; validate ranges; serde round-trip.
2. Apply SHALL run as a per-tile filter using halo (or multi-pass box blur) sufficient for the configured radius; edge behavior at tile borders SHALL not show a darker/lighter seam beyond documented tolerance on a flat field + bright spot straddling the border.
3. Glow SHALL NOT invent manual global coords for pattern; if any spatial noise is added later, it MUST use `GlobalCoord`.
4. Frontend Effect chooser / settings SHALL list Glow with the param editors.
5. Deterministic: same input tile + params → same output (no time-based RNG).

### Requirement 6: C3 — CRT Filter

**User Story:** As a user, I want CRT-style scanlines (and light phosphor look) that stay seamless across tiles when panning.

#### Acceptance Criteria

1. THE engine SHALL add `FilterKind::Crt` (or `FilterParams::Crt { … }` under a dedicated kind) with params at least: scanline strength, scanline period (px), optional RGB triad / mask strength, optional brightness/contrast tweak.
2. Scanline modulation SHALL key off **global** `Y_g` (and `X_g` for mask) via `GlobalCoord` — NEVER `local_y` alone or `tile_y * 256 + local_y` inline.
3. Pattern_Seam_Test for horizontal scanlines across a horizontal tile boundary SHALL pass (no phase jump).
4. `requires_full_row = false`; no diffusion residuals.
5. UI: chooser + settings; serde + validate.

### Requirement 7: C4 — SVG Export

**User Story:** As a user exporting pixel art / flat-color dither results, I want an SVG of merged rectangles or traced paths so the file stays editable and smaller than a PNG of the same flat regions.

#### Acceptance Criteria

1. `crates/engine-io` SHALL expose `svg_export` (module `svg_export.rs`) that accepts a raster (document composite or layer buffer — document chosen path in design.md) and options: algorithm (`GreedyMeshing` | `ContourTracing` | both as stages), color quantization tolerance (optional), and output path/string.
2. Greedy_Meshing SHALL emit non-overlapping axis-aligned rectangles covering equal-color runs (4-connected), each as `<rect>` (or path equivalent) with fill.
3. Contour_Tracing SHALL emit closed paths for color regions (policy: external contours only vs holes — document in design.md).
4. Export SHALL go through existing sandbox / path validation (`engine-io` sandbox) when writing to disk.
5. Unit tests: solid 2-color checker / flat field produce expected rect counts; golden SVG snippet or structural asserts (viewBox size, fill colors).
6. A Tauri command or existing export menu entry SHALL invoke the exporter (minimal UI: format picker “SVG” is enough for this track).

### Requirement 8: Integration, UI, and Preservation

**User Story:** As a user of existing dither modes, I want new filters discoverable without regressing Bayer/FS or unrelated filters.

#### Acceptance Criteria

1. `filters/apply.rs` (and DitherV2 ordered path) SHALL dispatch new modes/kinds; `FilterInstance` / DTO / frontend types SHALL stay in sync.
2. Dither settings UI SHALL list Halftone and Wave alongside Bayer/FS; Glow/CRT in effect chooser.
3. Prop / integration tests for existing dither modes SHALL stay green.
4. Alpha preservation rules consistent with other filters SHALL apply (document if Glow/CRT treat alpha specially).

### Requirement 9: Documentation and Track D Readiness

**User Story:** As a developer starting Track D, I want CPU Halftone and CRT documented and tested so WGSL can match pixel references.

#### Acceptance Criteria

1. `TILE_PIPELINE.md` (or ARCHITECTURE) SHALL list Halftone, Wave, CRT under pattern filters using `GlobalCoord`, and Glow under blur/halo filters.
2. `tech-debit.md` Track C SHALL link this folder; C1–C4 MAY be marked done only when their acceptance criteria above are met.
3. Closing the track for **GPU gate** REQUIRES at least Bayer (already), CMYK Halftone, and CRT CPU paths + seam tests green (Glow optional for the D gate, but listed in tech-debit port order).

### Requirement 10: Testing Matrix (Shared)

**User Story:** As a reviewer, I want a clear minimum test set so “done” is not only a screenshot.

#### Acceptance Criteria

1. Unit: formula helpers (rotated cell distance, wave `T`, scanline gain) with fixed numeric cases.
2. Seam: 2×2 tiles, shared edge continuous for Halftone, Wave, CRT (and Glow flat-field / straddling spot).
3. Serde: new modes/params round-trip.
4. Optional visual: checked-in fixture hashes or manual QA checklist in tasks.md (screenshots at PR) — full visual CI NOT required.
