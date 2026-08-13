# Requirements: Track M — ED kernels + Serpentine

## Introduction

В [ROADMAP_production_release.md](../ROADMAP_production_release.md) это было
«Track G». Буква G в репо — Welcome ([track-g-welcome/](../track-g-welcome/)).
Здесь — **Track M**.

Два шага, **разные PR**:

- **M1** — ядра JJN / Stucki / Burkes / Sierra в DitherV2 (тот же
  `IncomingErrorBuffer`, что A1).
- **M2** — Serpentine scanning **после** M1 и только при зелёном A1 (уже
  закрыт). Наивный serpentine ломает wavefront — ROADMAP §1.

As-built: V2 apply только `FloydSteinberg` | `Atkinson`. Legacy
`DiffusionKernel` уже знает JJN/Stucki, но V2 compat **фолбэчит их в FS**.
Burkes/Sierra нет.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md).

## Glossary

- **ED_Kernel**: весовая матрица диффузии; V2 mode variant + `distribute_*`.
- **Overflow_2px**: `ErrorResiduals` right/bottom depth 2, `CORNER_PATCH=2` —
  хватает JJN/Stucki/Burkes/Sierra (reach 2).
- **Serpentine**: нечётные **глобальные** строки R→L; чётность от `GlobalCoord.y`.
- **Row_Direction**: вход в модель IncomingErrorBuffer на уровне тайла.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| V2 modes JJN, Stucki, Burkes, Sierra | Новые ядра сверх wishlist (False Floyd, etc.) |
| Seam tests per kernel (хотя бы sample) | Смешать M2 в тот же PR что M1 |
| Serpentine без швов на 2×2 чёт/нечет | Менять A1 wavefront topology |
| UI mode select (после K) | GPU ED |

---

## Requirements

### Requirement 1: M1 — Kernel matrices in DitherV2

**User Story:** As a user, I want classic print-style diffusion kernels beyond FS/Atkinson, seamless across tiles.

#### Acceptance Criteria

1. `DitherModeV2` SHALL add `JarvisJudiceNinke`, `Stucki`, `Burkes`, `Sierra` (serde `snake_case` names locked in design).
2. `requires_full_row` SHALL be true for all of them (same as FS/Atkinson).
3. Apply SHALL use `dither_diffusion.rs` + existing overflow buffers (depth 2 is enough). Do not reintroduce the legacy in-tile-only `dither.rs` path as production V2.
4. Legacy `DiffusionKernel` JJN/Stucki SHALL stop silently falling back to FS in V2 migration (`DitherParamsV2::from`) — map to the new modes. Add Burkes/Sierra to legacy enum only if needed for PaletteQuantize; **lock: PaletteQuantize `Option<DiffusionKernel>` gains Burkes/Sierra too or stays FS/Atkinson/JJN/Stucki — prefer extend enum**.
5. Weights SHALL match standard published kernels (document fractions in design.md).
6. Tests: each kernel distributes a unit error to the expected neighbor offsets (unit); at least one 2×2 seam/gradient sample per kernel or a matrix subset (FS-equivalent seam helper). Existing FS/Atkinson tests stay green.
7. UI: Dither mode dropdown. Track K controls for any new numeric params (none expected).

### Requirement 2: M2 — Serpentine (separate step)

**User Story:** As a user, I want serpentine scanning without bringing back tile seams on odd rows.

#### Acceptance Criteria

1. M2 SHALL NOT merge until M1 kernels are on the same residual path and A1 seam matrix is still green.
2. Row parity SHALL use `GlobalCoord.y` (or `GlobalCoordSigned.y`), never local `tile_y`.
3. `IncomingErrorBuffer` / neighbor contribution SHALL be parameterized by row direction so a global R→L row still consumes “earlier in wavefront” residuals, not “literally screen-right” blindly (ROADMAP §1). Prefer: direction as an argument computed from `GlobalCoord`, not a rewrite of wavefront scheduling.
4. Test: serpentine ON, 2×2 tiles, seam absent on **both** even and odd **global** rows. This test is **in addition to** A1 tests — A1 green is not sufficient coverage.
5. Param: `serpentine: bool` on `DitherParamsV2`, default `false` (identity with today’s L→R). Applies to all ED V2 modes including M1 kernels.
6. `serpentine=false` SHALL be bit-identical to pre-M2 for FS/Atkinson.

### Requirement 3: Persistence

**User Story:** As a user, I want new modes to round-trip and old files to stay FS/Atkinson.

#### Acceptance Criteria

1. Unknown old files unchanged. New variants serde round-trip.
2. `.dyuki` `app_version_min` table SHALL include the new modes (Track F policy: current app version if table incomplete is acceptable, but prefer explicit entries).
