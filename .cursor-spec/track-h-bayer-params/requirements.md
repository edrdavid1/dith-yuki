# Requirements: Track H — Bayer Threshold Bias + Pattern Angle

## Introduction

Формализация [ROADMAP_production_release.md](../ROADMAP_production_release.md) Track H.
Два независимых параметра ordered-паттерна в `DitherParamsV2`:

- **H1 Threshold Bias** — сдвиг порога. Нет зависимости от A2. Можно сразу.
- **H2 Pattern Angle** — поворот **сэмплирования паттерна** (Bayer / CustomPng).
  Пересекается с `BlockRepresentativeCache` (A2, уже закрыт). Порядок операций
  зафиксирован в ROADMAP §2 — не переоткрывать.

Не путать с уже существующими углами: `wave_angle` (Wave) и per-channel
halftone angles (C1). Этот трек **не** меняет CMYK Halftone.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md).

## Glossary

- **Threshold_Bias**: аддитивный сдвиг порога перед compare (`T' = clamp(T + bias)`).
- **Pattern_Angle**: поворот координаты **после** block-alignment, **перед**
  `pattern_cell` / `get_threshold_i32`.
- **Block_Then_Rotate**: `global → aligned(pixel_size) → [BRC] → rotate(angle) → threshold`.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Bias на Bayer / CustomPng / Wave / Halftone (общий порог) | Поворот геометрии `pixel_size`-блоков |
| Angle на Bayer и CustomPng | Изменение C1 screen angles / `wave_angle` |
| Швы не возвращаются при `pixel_size>1` и `angle≠0` | GPU-шейдер angle в этом треке (D follow-up) |
| UI через Track K `Slider` | Новый dither mode |

---

## Requirements

### Requirement 1: Threshold Bias

**User Story:** As a user, I want to lighten or darken an ordered dither without changing the matrix, so I can bias the pattern toward more or fewer ink dots.

#### Acceptance Criteria

1. `DitherParamsV2` SHALL gain `threshold_bias: f32` with serde default `0.0`, validated range `[-0.5, 0.5]` (design MAY lock a slightly different closed range; document it).
2. WHEN applying Bayer, CustomPng, Wave, or CmykHalftone, THE compare SHALL use `threshold + bias` (clamped to the same numeric domain as today’s threshold, typically `[0, 1)`).
3. Error-diffusion modes (FS/Atkinson and Track M kernels) SHALL ignore `threshold_bias` (no-op) — bias is an ordered-threshold control.
4. `bias = 0` SHALL be bit-identical to today’s output for the same other params (regression test).
5. UI: Slider on Dither settings, visible for ordered modes. SHALL use Track K `Slider` if K is merged; raw `<input>` is a process regression.

### Requirement 2: Pattern Angle (Bayer / CustomPng)

**User Story:** As a user, I want to rotate the Bayer/PNG threshold lattice without shearing mega-pixels, so large `pixel_size` blocks stay rectangles.

#### Acceptance Criteria

1. `DitherParamsV2` SHALL gain `pattern_angle: f32` (degrees, serde default `0.0`). Range MAY be unbounded or wrapped `rem_euclid(360)`; sampling MUST be periodic.
2. Angle SHALL apply **only** to Bayer2x2/4x4/8x8 and CustomPng. Wave keeps `wave_angle`. CmykHalftone keeps C1 channel angles. ED modes ignore it.
3. THE implementation SHALL follow Block_Then_Rotate (ROADMAP §2):

```text
global coord → aligned(pixel_size) → [BlockRepresentativeCache lookup]
  → rotate(pattern_angle) → pattern_cell / map sample → threshold (+ bias) → compare
```

4. THE implementation SHALL NOT rotate coordinates before block alignment. Blocks MUST remain axis-aligned screen rectangles when `pixel_size > 1`.
5. `pattern_angle = 0` SHALL be bit-identical to today’s Bayer/CustomPng (regression).
6. UI: Slider (degrees) on Dither settings for Bayer/CustomPng. Track K component.

### Requirement 3: Seam and Combined Tests

**User Story:** As a maintainer, I want dedicated tests for the new degrees of freedom, not reuse of A2 tests as sufficient coverage.

#### Acceptance Criteria

1. 2×2 tile seam test: Bayer + `pattern_angle ≠ 0` + `pixel_size = 1` — no phase jump on shared edge (same class as C1/C2 seam tests).
2. Combined: `pixel_size > 1` AND `pattern_angle ≠ 0` — no tile-boundary seam; blocks remain axis-aligned rectangles (visual or structural assert: representative cells form `ps×ps` axis-aligned runs).
3. Bias-only 2×2 seam: `threshold_bias ≠ 0`, `angle = 0` — continuous across tiles.
4. Existing A2 `pixel_size × Bayer` matrix SHALL stay green at `bias=0`, `angle=0`.

### Requirement 4: Persistence

**User Story:** As a user, I want bias/angle saved in `.dyproj` / `.dyuki` without breaking old files.

#### Acceptance Criteria

1. Serde defaults SHALL load old documents missing the fields as `0.0`.
2. New fields SHALL round-trip on `FilterParams::DitherV2` and on Track F file DTOs (they already embed `DitherParamsV2`).
