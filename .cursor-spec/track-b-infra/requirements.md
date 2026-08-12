# Requirements: Track B — Independent Infrastructure (3D LUT + Integer Zoom)

## Introduction

Трек B из [tech-debit.md](../tech-debit.md) — **независимая инфраструктура**, которую можно вести параллельно с треком A и которая не блокируется correctness-долгом ED/`pixel_size`.

Две независимые подзадачи:

| ID | Тема | Стек |
|----|------|------|
| **B1** | 3D LUT Oklab — O(1) nearest-color вместо `KdTree::nearest` в hot path | Rust (`engine-color` + apply paths) |
| **B2** | Pixel-perfect integer zoom & snap | Frontend (`useViewport`, `TileCanvas`, zoom UI) |

B1 ускоряет квантизацию/диффузию с палитрой; B2 даёт чёткие пиксели при целом масштабе. Треки C/D **не блокируются** B, но C1–C3 и будущий GPU-путь **выигрывают**, если LUT уже есть к моменту Phase 1 filters.

## Glossary

- **PaletteKdCache / KdTree**: текущий кэш и O(log K) nearest в Oklab (`palette_cache.rs`, `kdtree.rs`).
- **PaletteLut3D**: регулярная сетка в Oklab; ячейка хранит индекс ближайшего цвета палитры; lookup O(1).
- **PaletteLutCache**: `DashMap<PaletteId, (revision, Arc<PaletteLut3D>)>` — та же revision-инвалидация, что у KD-cache.
- **Cell_Boundary_Disagreement**: LUT и KD могут расходиться на границах ячеек (почти равные дистанции к двум цветам) — допустимо; системный сдвиг — нет.
- **zoomMode**: `'integer' | 'free'` в состоянии viewport.
- **Integer_Snap**: округление zoom к ближайшему целому множителю (`1×, 2×, …`) только по окончании жеста.
- **DPR_Pixel_Snap**: `CanvasX = round(WorldX * Zoom * dpr) / dpr` (и аналог для Y / размеров отрисовки).
- **Canvas2D path**: текущий рендер в `TileCanvas.tsx` — HTMLCanvas 2D; WebGL не используется.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| O(1) nearest через LUT в hot path quantize/dither с палитрой | Замена или удаление KdTree (он остаётся для build + fallback) |
| Кэш LUT по `(PaletteId, revision)` как у KD | Изменение формата палитр / Color Lab UI |
| Бенчмарк и порог памяти/точности (32³ vs 64³) | GPU LUT / WGSL (трек D) |
| Режим integer zoom + snap по концу жеста | Полный re-enable pyramid levels (отдельный долг) |
| DPR-aware snap координат тайл-отрисовки | Переписывание pan/fit UX |
| UI-переключатель integer / free рядом с zoom controls | Скриншот-CI / visual regression infra |

---

## Requirements

### Requirement 1: PaletteLut3D Structure and Build

**User Story:** As a developer, I want a precomputed Oklab grid that maps any sample to a palette index in O(1), so that per-pixel KD walks disappear from the hot path.

#### Acceptance Criteria

1. THE crate `engine-color` SHALL expose `PaletteLut3D` (new module, e.g. `palette_lut.rs`) with at least: flat `grid: Vec<u16>` of length `size³`, `size: u32`, and axis ranges `(l_range, a_range, b_range)` covering the Oklab domain used by the engine (document defaults: L∈[0,1], a/b∈[-0.4, 0.4] unless bench proves wider ranges needed for real palettes).
2. `PaletteLut3D::build(palette, size, kdtree)` SHALL, for each cell center in the grid, call existing `KdTree::nearest` and store the palette index — build cost O(size³ · log K), once per palette revision.
3. `nearest_index(lab: Oklab) -> u16` SHALL map lab → clamped grid indices → direct array lookup with no tree walk.
4. Empty palette SHALL fail consistently with `PaletteError::Empty` (same contract as `PaletteKdCache::get_or_build`).
5. Default `size` SHALL be chosen by Req 5 (bench); until then implementation MAY use 32 and MUST keep `size` parameterized.

### Requirement 2: PaletteLutCache and Invalidation

**User Story:** As a worker thread applying filters, I want LUT rebuilds only when the palette revision changes, so that concurrent tiles share one `Arc` without contention.

#### Acceptance Criteria

1. THE engine SHALL provide `PaletteLutCache` (or extend the palette cache module) with `get_or_build(palette) -> Result<Arc<PaletteLut3D>, PaletteError>` mirroring `PaletteKdCache` semantics: match on `(id, revision)`, last-writer-wins insert, `evict(id)`.
2. WHEN building a LUT, THE cache SHALL obtain/build a `KdTree` (via existing `PaletteKdCache` or inline build) and SHALL NOT invent a separate invalidation trigger beyond palette `revision`.
3. AppState / tile pipeline SHALL hold a LUT cache instance alongside `PaletteKdCache` (or a combined façade) so apply paths can resolve LUT without global statics.
4. WHEN a palette is removed, BOTH KD and LUT entries for that id SHALL be evictable.

### Requirement 3: Hot-Path Integration

**User Story:** As a user dithering or quantizing to a palette, I want nearest-color lookups to use the LUT so large documents stay responsive.

#### Acceptance Criteria

1. WHEN `palette_id` is set, `palette_quantize.rs`, `dither_diffusion.rs`, and `dither_ordered.rs` hot paths SHALL resolve nearest index via `PaletteLut3D::nearest_index` (or a thin helper), not `KdTree::nearest`, for the production apply path.
2. KdTree SHALL remain used for LUT construction and MAY remain as an explicit fallback when design.md’s policy says so (e.g. tiny palettes where build cost > benefit, or debug flag) — fallback policy SHALL be documented and covered by a unit test if implemented.
3. Public apply signatures MAY gain a `&PaletteLutCache` (or combined cache) parameter; call sites in `apply.rs`, `tile_pipeline`, and tests SHALL be updated so the suite compiles and existing palette props stay green within Cell_Boundary_Disagreement tolerance.
4. Determinism for a fixed palette revision + fixed LUT size SHALL hold: same inputs → same indices from LUT.

### Requirement 4: Correctness vs KdTree

**User Story:** As a developer, I want confidence that the LUT does not systematically pick wrong colors versus the KD oracle.

#### Acceptance Criteria

1. A property/unit test SHALL sample random Oklab points (and/or a grid of cell centers and cell corners) and compare `lut.nearest_index` vs `kdtree.nearest`.
2. Disagreements SHALL be allowed only when the two candidate colors have distances within a documented epsilon of each other **or** the sample lies near a Voronoi boundary relative to cell quantization (Cell_Boundary_Disagreement); a hard failure SHALL trigger if disagreement rate exceeds a documented bound on cell **centers** (centers must match the KD result used at build time — ideally 100% on centers).
3. Existing `dither_palette_props` / palette-mode diffusion props SHALL remain green (bit-identical not required if LUT differs on boundaries; document tolerance if any assertion loosens).

### Requirement 5: Size Tradeoff and Benchmark

**User Story:** As a maintainer, I want a measured choice between 32³ and 64³ so memory and quality are intentional.

#### Acceptance Criteria

1. A criterion bench (Criterion or existing project bench style) SHALL measure: (a) LUT build time for representative palette sizes, (b) quantize/dither throughput LUT vs KD on a large buffer, (c) memory of `grid` for size 32 and 64.
2. THE PR / tasks notes SHALL record the chosen default `size` and rationale (accuracy on a “close colors” palette vs RAM).
3. IF 64³ is chosen only for large K, THEN the policy (threshold on K or always-64) SHALL be stated in design.md and implemented in `get_or_build`.

### Requirement 6: Integer Zoom Mode

**User Story:** As a pixel-art user, I want zoom to land on whole multiples so document pixels map to crisp screen pixels.

#### Acceptance Criteria

1. Viewport state (frontend) SHALL include `zoomMode: 'integer' | 'free'` (default MAY be `'free'` to preserve current behavior, or `'integer'` if product prefers — document choice in tasks).
2. WHEN `zoomMode === 'integer'`, AFTER a zoom gesture ends (wheel idle / pointerup equivalent — NOT on every wheel tick), THE zoom value SHALL snap to the nearest integer factor in `[1, maxIntegerZoom]` clamped to existing zoom min/max; for zoom `< 1`, snapping policy SHALL be documented (e.g. keep free sub-1×, or snap to `1/n` reciprocals — pick one in design.md).
3. DURING an active wheel/pinch gesture in integer mode, zoom MAY stay continuous; snap applies on gesture end so the view does not judder mid-gesture.
4. Preset buttons (`zoomToNextPreset` / `zoomToPrevPreset`) in integer mode SHALL move among integer factors (or existing presets that are integers) without leaving non-integer zoom as the settled value.
5. `fitToView` in integer mode SHALL either snap the fitted zoom to the nearest integer ≤ fit (prefer showing full doc) or temporarily behave as free for that action — policy documented; MUST NOT leave a fractional settled zoom while mode is integer unless policy explicitly allows fit exception.

### Requirement 7: DPR-Aware Draw Snap and Nearest Filtering

**User Story:** As a user on a Retina display at 2×/3× zoom, I want tile edges and pixels aligned without blurry interpolation.

#### Acceptance Criteria

1. `TileCanvas` draw path SHALL apply DPR-aware rounding for screen positions used in `drawImage` when in integer mode (and SHOULD use the same snap helpers whenever it reduces seams; free mode MUST NOT regress pan smoothness). Formula target: `round(world * zoom * dpr) / dpr` for origins; draw sizes SHALL stay consistent so gaps/overlaps between adjacent tiles do not appear at integer zooms.
2. `ctx.imageSmoothingEnabled = false` SHALL remain set for the 2D context (already true — preserve).
3. THE implementation SHALL confirm Canvas2D is the active path (current code); IF WebGL is introduced later, NEAREST sampling is a separate requirement — out of scope unless path changes during this track.
4. CSS `image-rendering: pixelated` (or equivalent on the canvas class) SHALL remain consistent with nearest-neighbor intent where applicable.

### Requirement 8: Zoom Mode UI

**User Story:** As a user, I want to switch between free (trackpad-friendly) and integer (pixel-perfect) zoom without digging into settings files.

#### Acceptance Criteria

1. Preview chrome (near existing zoom in/out / percent controls in `PreviewFeature` or equivalent) SHALL expose a control to toggle `zoomMode`.
2. THE control SHALL be keyboard-focusable and have an accessible name (e.g. “Integer zoom”).
3. Toggling to integer SHALL snap the current zoom immediately (or on next gesture end — document); toggling to free SHALL leave the numeric zoom unchanged.

### Requirement 9: Preservation and Independence

**User Story:** As a user not using palettes / integer zoom, I want Track B to leave non-palette filters and free-zoom behavior intact.

#### Acceptance Criteria

1. Filters without `palette_id` SHALL not require LUT builds on the hot path.
2. Free zoom mode SHALL preserve continuous exponential wheel zoom behavior from today’s `useViewport`.
3. B1 SHALL NOT modify error-diffusion residual geometry or enforcement (Track A ownership).
4. B2 SHALL NOT require Rust crate changes.
5. Closing Track B REQUIRES Req 1–5 for B1 and Req 6–8 for B2 (or explicit deferral of one sub-track recorded in `tech-debit.md`).

### Requirement 10: Documentation Sync

**User Story:** As a developer starting Track C, I want ARCHITECTURE / color docs to mention LUT caching so new filters use it from day one.

#### Acceptance Criteria

1. A short note in `ARCHITECTURE.md` or `COLOR_AND_COLOR_LAB.md` (whichever already describes KD-cache) SHALL document `PaletteLut3D` + revision invalidation + default grid size.
2. `tech-debit.md` Track B SHALL link this folder and MAY mark B1/B2 done only when the corresponding acceptance tests/benches and UI criteria are met.
