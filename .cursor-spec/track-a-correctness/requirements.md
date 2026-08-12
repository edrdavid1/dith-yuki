# Requirements: Track A — Correctness Debt (Error Diffusion + pixel_size)

## Introduction

Трек A из [tech-debit.md](../tech-debit.md) — **не новая фича**, а завершение уже начатого correctness-долга в DitherV2. Инфраструктура частично есть (`ErrorResidualsStore`, on-demand left/top recursion, `GlobalCoord`/`GlobalCoordSigned`, `BlockRepresentativeCache`), но остаются открытые баги, из‑за которых швы и неверные mega-pixel блоки видны пользователю.

Закрытие A **блокирует** трек C (CMYK Halftone, Wave Dither, CRT/Glow): новые фильтры должны строиться на стабильных координатах, wavefront-diffusion и block cache, а не наследовать те же классы дефектов.

Треки B (LUT / integer zoom) независимы и могут идти параллельно.

## Glossary

- **Error_Diffusion**: Floyd–Steinberg / Atkinson в `dither_diffusion.rs`; `requires_full_row = true`.
- **ErrorResidualsStore**: side-channel right/bottom overflow residuals между тайлами.
- **Dependency_Enforcement**: on-demand рекурсия left/top в `tile_pipeline.rs` перед Processed.
- **Silent_Skip_Zero_Seed**: ветка, где raw соседа нет в кэше → residual = 0 и **нет** re-invalidation.
- **Pending_Diffusion_Waiters**: реестр «этот Processed ждёт raw соседа» → dirty при load.
- **Diagonal_Error_Loss**: `(dx>0 && dy>0)` overflow за угол тайла отбрасывается в `distribute_fs` / `distribute_atkinson`.
- **Wavefront / IncomingErrorBuffer**: модели полной передачи ошибки на диагональ (в т.ч. к тайлу `(x+1,y+1)`).
- **Pyramid_Level**: `TileCoord.level`; сейчас enforcement только при `level == 0`.
- **BlockRepresentativeCache**: raw (и dithered) цвета top-left блока `pixel_size×pixel_size` без halo-clamp.
- **Seam_Matrix**: `crates/engine-project/tests/dither_seam_matrix.rs` — acceptance matrix.
- **GlobalCoordSigned**: signed document coords с учётом halo (`coords.rs`).

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Бесшовный FS/Atkinson на всех zoom / pyramid levels | Новые фильтры Phase 1 (трек C) |
| Нет залипшего шва после late raw load (или N/A + regression test) | 3D LUT Oklab (B1) |
| Полная передача диагональной ошибки (wavefront / IncomingErrorBuffer) | GPU pipeline (D) |
| Чистая матрица `pixel_size ∈ {1..32} × {Bayer, FS}` | Integer zoom / snap (B2) |
| Block reps из декомпозиции/буфера, не через halo clamp | Переписывание scheduler приоритетов |
| Координаты FS только через `GlobalCoordSigned` | Изменение UI Color Lab / панелей |

---

## Requirements

### Requirement 1: Diagnose Silent-Skip Before Implementing Waiters

**User Story:** As a developer, I want to know whether zero-seed silent-skip is reachable in production before building waiters, so that we do not ship dead code.

#### Acceptance Criteria

1. THE pipeline SHALL expose a diagnostic counter (or equivalent log) counting times Dependency_Enforcement skipped left/top recursion because the neighbor **raw** tile was absent from `tile_cache`.
2. WHEN reproducing with: (a) full 1:1 load then enable DitherV2, and (b) aggressive pan while DitherV2 is on, THE developer SHALL record whether the counter increments and whether a seam remains after the viewport is fully loaded.
3. IF scenario (a) still shows a seam with counter = 0, THEN Silent_Skip SHALL NOT be treated as the sole 1:1 root cause; Diagonal_Error_Loss (Req 4) remains in scope.
4. IF the skip branch is unreachable under realistic cache policy (level-0 raw not evicted), THEN Req 2 MAY be closed as **N/A** with a regression test that encodes the future contract (see Req 2.5), without implementing `pending_diffusion_waiters`.
5. THE diagnosis outcome SHALL be written into the PR / tasks notes before merging waiter code or marking N/A.

### Requirement 2: Pending Diffusion Waiters (Conditional)

**User Story:** As a user panning a large document with error diffusion, I want tiles to recompute when a missing neighbor finally loads, so that a zero-seed seam does not stick forever.

#### Acceptance Criteria

1. IF Req 1 confirms the skip branch is reachable, THEN THE AppState / tile pipeline SHALL maintain `pending_diffusion_waiters: DashMap<TileKey, Vec<TileKey>>` (or equivalent), keyed by the missing **raw** neighbor key, values = Processed keys that computed with zero seed for that neighbor.
2. WHEN Dependency_Enforcement would skip recursion due to missing raw, THE pipeline SHALL register the current Processed key as a waiter under that raw key (and may still proceed with zero seed for this pass).
3. WHEN a raw tile is inserted into `tile_cache`, THE pipeline SHALL remove waiters for that key and mark those Processed tiles dirty / reschedule them via the existing invalidation path (no new ad-hoc event bus).
4. A tile MAY wait on both left and top; THE first ready neighbor MAY trigger recompute (idempotent recompute is acceptable).
5. IF Req 1 closes as N/A, THEN a unit/property test SHALL still document: “if raw neighbor missing → register waiter → on insert → dirty”, implemented against a test double **or** skipped with `#[ignore]` + comment linking to diagnosis — preferred: implement the waiter unit test against an isolated helper so the contract stays locked even if production path is currently dead.
6. AFTER waiters (or N/A), a 2×2 delayed-raw integration scenario SHALL match bit-identical (or within seam tolerance) output versus both raws present from the start.

### Requirement 3: Dependency Enforcement on All Pyramid Levels

**User Story:** As a user zooming out with Floyd–Steinberg, I want the same seamless cross-tile residuals as at 1:1, so that seams do not appear only at distance.

#### Acceptance Criteria

1. THE Dependency_Enforcement gate SHALL NOT require `key.coord.level == 0`; it SHALL run for every pyramid level when the layer has enabled `requires_full_row` filters.
2. `ErrorResidualsStore` keys SHALL continue to include full `TileCoord` (with `level`); left/top lookup SHALL use the same level as the current tile.
3. WHEN left/top neighbors at level N are missing or dirty and their raw exists, THE pipeline SHALL recursively compute those Processed neighbors before the current tile (same as today’s level-0 behavior).
4. Seam / gradient acceptance tests SHALL cover at least one `level > 0` case for FS and Atkinson (in addition to level 0).
5. Existing level-0 behavior and non-diffusion filters SHALL remain unchanged.

### Requirement 4: Diagonal Error Propagation

**User Story:** As a user viewing FS/Atkinson at 1:1 after full load, I want no brightness step on tile boundaries caused by discarded corner error, so that diffusion matches a single-buffer reference.

#### Acceptance Criteria

1. THE comment/behavior “Diagonal overflow (right+bottom) is discarded — negligible” in `distribute_fs` / `distribute_atkinson` SHALL be eliminated as a correctness compromise for production diffusion.
2. THE design SHALL adopt either:
   - **Wavefront scheduling** (process tiles in diagonal / dependency order so `(x,y)` sees completed `(x-1,y)`, `(x,y-1)`, and diagonal contributions), or
   - **IncomingErrorBuffer** (explicit corner/diagonal residual channel from tile `(x,y)` into `(x+1,y+1)` / equivalent seed),  
   as specified in design.md — one primary approach, not both half-finished.
3. WHEN a 2×2 (or larger) tile grid runs FS/Atkinson on a smooth gradient, brightness / luminance along internal boundaries SHALL stay within the Seam_Matrix tolerance (same order as today’s `1e-4` float compare, or a documented tightened bound).
4. A reference comparison (single full-image buffer vs tiled path) for FS on a small canvas SHALL show no systematic boundary darkening attributable to diagonal drop.
5. Bayer / ordered dither SHALL remain unaffected by this change.

### Requirement 5: Seam Matrix Acceptance for A1

**User Story:** As a developer, I want an automated gate that FS/Atkinson stay seamless across levels and gradients, so that Track A cannot regress silently.

#### Acceptance Criteria

1. `dither_seam_matrix.rs` (and/or successor tests) SHALL pass for FS and Atkinson on the gradient fixture at level 0.
2. THE same seam criteria SHALL pass for at least one higher pyramid level used by the viewport when zoomed out.
3. Gradient boundary tests SHALL assert no systematic luminance loss at tile edges beyond tolerance.
4. Closing A1 REQUIRES the above green plus Req 3 complete and Req 4 complete; Req 2 complete or explicitly N/A per Req 1.

### Requirement 6: Block Representative Cache Completeness (A2)

**User Story:** As a user setting `pixel_size` to any value 1–32, I want mega-pixel blocks aligned and colored consistently across tile borders for Bayer and FS.

#### Acceptance Criteria

1. `BlockRepresentativeCache` SHALL provide raw block colors from document-global top-left samples **without** reading via halo-clamped local coordinates.
2. Population SHALL occur at decompose / buffer path (`populate_from_buffer` / `ensure_populated_from_tiles`) and be available before ordered/diffusion apply when `pixel_size > 1`.
3. WHEN filter params change, dithered entries SHALL clear with residuals; WHEN raw image changes, full invalidate SHALL run.
4. Error diffusion with `pixel_size > 1` SHALL use `GlobalCoordSigned` for block alignment and representative tests — no manual `tile_x + HALO` / `coord.x * TILE_SIZE + …` for that purpose in the hot path.
5. Non-representative pixels SHALL copy the representative’s dithered (FS) or quantized (Bayer) color, including when the representative lies in another tile (dithered side-channel / cache).
6. Seam_Matrix for `pixel_size ∈ {1,2,3,4,5,6,7,8,12,16,24,32} × {Bayer8x8, FloydSteinberg}` SHALL be fully clean (tolerance as in existing tests).

### Requirement 7: Coordinate Hygiene for Diffusion

**User Story:** As a developer adding future filters, I want diffusion to be the reference for correct global coords, so that CRT/Halftone do not reintroduce FS-class bugs.

#### Acceptance Criteria

1. THE FS/Atkinson apply path SHALL use `GlobalCoordSigned::from_local_with_halo` (or `GlobalCoord` where unsigned is enough) for global x/y and `.aligned(pixel_size)` for block origin.
2. THE codebase path used for production apply SHALL NOT compute global position as `tile_x + HALO` without going through the coords helpers for block logic.
3. Unit tests in `coords.rs` style (continuity across tile boundary) SHALL remain green; diffusion tests SHALL fail if alignment regresses for non-power-of-two `pixel_size`.

### Requirement 8: Preservation

**User Story:** As a user of non-diffusion filters, I want Track A fixes not to change Bayer-without-pixel_size, Levels, Curves, or compositor output.

#### Acceptance Criteria

1. Ordered dither with `pixel_size == 1` output SHALL remain bit-identical (or within existing float eps) versus pre-A baseline tests.
2. Layers without `requires_full_row` SHALL not enter Dependency_Enforcement or waiter registration.
3. Determinism properties (`dither_determinism_props`) SHALL stay green for FS/Atkinson after wavefront / buffer changes.
4. Alpha preservation and palette-mode properties currently covering diffusion SHALL remain green.

### Requirement 9: Documentation Sync

**User Story:** As a developer starting Track C, I want TILE_PIPELINE / ARCHITECTURE to describe the real enforcement and diagonal model, so that new filters follow the fixed contract.

#### Acceptance Criteria

1. `TILE_PIPELINE.md` section on row-major enforcement SHALL drop “only level 0” if code enforces all levels, and SHALL document waiters or the N/A diagnosis.
2. Diffusion section SHALL document diagonal residual handling (wavefront or IncomingErrorBuffer), not “discarded — negligible”.
3. `tech-debit.md` Track A criteria MAY be marked done only when Req 5 and Req 6 acceptance tests are green.
