# Requirements: Track J — Glitch correctness

## Introduction

ROADMAP Track J описывает Glitch (RGB Shift, Block Displace) как **новый**
независимый фильтр. As-built: `FilterKind::Glitch`, `filters/glitch.rs`,
XorShift64, `seed` в `FilterParams::Glitch`, UI `GlitchSettings.tsx` — уже есть.

Этот трек — **correctness-pass**, не второй фильтр. Текущий apply:

- сидит на локальных `0..259` и `tile.at` с clamp;
- сидит PRNG на `TileCoord`, не на `GlobalCoord`;
- max shift ~20 px при `HALO = 2` → выборка за границей тайла врёт.

Тот же класс бага, что A/C: локальная арифметика на стыке.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md). Не зависит от A/M/I.

## Glossary

- **Shift_Field**: смещение как функция `(GlobalCoord, seed, intensity)`, одинаковая на всех тайлах для одной глобальной точки.
- **Halo_Cap**: v1 max |offset| ≤ `HALO` (как Glow `radius ≤ HALO`).
- **XorShift64**: существующий детерминированный PRNG; seed остаётся в params.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Бесшовный RGB Shift / Block Displace при offset ≤ HALO | Wide-read соседних тайлов / HALO>2 |
| PRNG от global coord + seed | Новый glitch kind (slice, etc.) |
| 2×2 seam test | GPU glitch |
| UI seed + intensity на Track K Slider | Переименовать FilterKind |

---

## Requirements

### Requirement 1: Global coordinates

**User Story:** As a user panning a glitched image, I want the distortion field continuous across tiles.

#### Acceptance Criteria

1. RGB Shift and Block Displace SHALL derive destination/source indexing via `GlobalCoord` / `GlobalCoordSigned` — no `tile_y * 256 + local` and no raw `for y in 0..260` as the *document* position.
2. PRNG stream for a pixel or block SHALL be keyed by **global** position (and `seed`, `level` if needed), not by `TileCoord` alone. Two adjacent tiles SHALL compute the same shift for the same global pixel.
3. Reads outside the current tile buffer SHALL use signed local+halo indexing; they SHALL NOT clamp to `0..259` as if the tile were the whole image.

### Requirement 2: Halo cap (v1)

**User Story:** As a maintainer, I want v1 glitch to stay within HALO rather than reintroduce seams via 20px shifts.

#### Acceptance Criteria

1. Max channel shift (RGB Shift) and max block displacement SHALL be capped so the source sample stays within the halo of the destination pixel (`≤ HALO`). Intensity still scales 0..1 inside that cap.
2. Validate SHALL reject or clamp params that would exceed the cap (lock: clamp at apply + document; prefer validate error only if a numeric param is stored in px — intensity stays 0..1).
3. A follow-up for wide displacement (neighbor fetch) is **out of scope**; mention in Future.

### Requirement 3: Determinism and persistence

**User Story:** As a user, I want the same seed to reproduce the same glitch after zoom/pan/recompute.

#### Acceptance Criteria

1. Same tile + params + seed → bit-identical output (already tested; MUST remain).
2. Seed stays in `FilterParams` (already) so `.dyproj` / `.dyuki` persist it.
3. `intensity = 0` remains a no-op copy (already).

### Requirement 4: Tests and UI

**User Story:** As a reviewer, I want a 2×2 seam test, not only same-tile determinism.

#### Acceptance Criteria

1. 2×2 canvas, RGB Shift, intensity > 0, seed fixed — shared edge matches a single-buffer / no-step criterion (same spirit as CRT/Wave seam tests), with shift ≤ HALO.
2. Same for Block Displace; block grid keyed off global coords so a block straddling the tile edge is consistent.
3. UI: intensity Slider (Track K); seed as NumberInput (Track K). No new raw `<input type="number">`.
