# Requirements: Track I — Per-filter Opacity / Blend Mode

## Introduction

Формализация [ROADMAP_production_release.md](../ROADMAP_production_release.md) §3 —
самое архитектурно значимое изменение wishlist: фильтр из «чистой трансформации»
становится «трансформация + blend с собственным входом».

**Опасность:** `ErrorResidualsStore` / `IncomingErrorBuffer` (A1, закрыт) считают
остаток по **полной** диффузии. Если блендить *до* записи residual, швы A1
вернутся. Решение ROADMAP: residual всегда от `full_result`; opacity/blend —
визуальный пост-шаг.

Реализовать **одной обёрткой** в диспетчере (`apply.rs` / `apply_single_filter`),
не параметром внутри каждого фильтра.

Frontend DnD стека: IPC `reorder_filter` и drag в `LayersPanel` **уже есть**.
Этот трек не пишет второй reorder; только проверяет полноту и добавляет
opacity/blend UI.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md). Gate: A1 закрыт (уже).

## Glossary

- **Filter_Opacity**: `0.0..=1.0` на `FilterInstance` (не на слое).
- **Filter_Blend**: тот же `BlendMode`, что у слоя (`types.rs`).
- **Full_Then_Blend**: `full = apply(pre)`; residuals from `full`;
  `out = blend(pre, full, opacity, mode)` only for Processed pixels.
- **Fast_Path**: `opacity >= 1.0 && blend == Normal` → return `full` unchanged.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Opacity + blend на каждом `FilterInstance` | Дублировать blend-формулы per-filter |
| Residual ED независимо от opacity | Менять layer opacity/blend |
| Одна обёртка в диспетчере | Новый набор blend modes |
| UI opacity + blend; DnD если дырки | Переписывать LayersPanel DnD с нуля |

---

## Requirements

### Requirement 1: Model

**User Story:** As a user, I want each effect to have its own opacity and blend mode, like a mini-layer, without breaking old projects.

#### Acceptance Criteria

1. `FilterInstance` SHALL include `opacity: f32` (default `1.0`) and `blend_mode: BlendMode` (default `Normal`).
2. Validate: `opacity` in `[0.0, 1.0]`. Reserved blend variants SHALL be rejected or treated as Normal (lock in design — prefer reject on set).
3. Serde defaults SHALL load documents / `.dyuki` filters missing the fields as 100% Normal.
4. Track F `FilterInstanceFile` SHALL persist both fields (always present on new export; default on old import).

### Requirement 2: Dispatcher Wrapper (Full_Then_Blend)

**User Story:** As a user of Floyd–Steinberg at 50% opacity, I want no tile seams, and a correct 50% mix with the pre-filter tile.

#### Acceptance Criteria

1. `apply_single_filter` (or a wrapper immediately around it in `apply_filter_to_tile_with_caches`) SHALL: keep `pre` copy → run existing apply (ED writes residuals from **full** result, opacity not passed into diffusion) → if Fast_Path, return `full` → else `blend_tile`-equivalent of `pre` vs `full`.
2. THE blend implementation SHALL reuse `compositor::blend_tile` / the same per-pixel formulas as layer compositing — not a second formula table.
3. No filter kind SHALL implement its own opacity/blend except via this wrapper.
4. Fast_Path SHALL avoid the extra blend pass (most common case).
5. Test: ED filter, `opacity=0.5`, 2×2 tiles — no seam (same class as A1); visual result is 50% mix with pre-filter. Test: `opacity=1, Normal` bit-identical to today’s apply.

### Requirement 3: UI

**User Story:** As a user, I want opacity and blend on the selected effect, and to reorder effects by dragging.

#### Acceptance Criteria

1. Effect settings (and/or each stack row) SHALL expose opacity (Track K Slider) and blend-mode select (same modes as layer blend UI).
2. Changing opacity/blend SHALL invalidate Processed tiles the same path as `update_filter_params`.
3. Filter stack drag-and-drop: IF `LayersPanel` already reorders via `reorder_filter`, THAT path SHALL remain the primary. EffectSettingsPanel MAY add reorder handles if the stack is also shown there; MUST call the same IPC, not a parallel model.
4. A checklist in tasks.md SHALL record as-built DnD gaps (if any) rather than assuming green.

### Requirement 4: Review Gate

**User Story:** As a reviewer, I want to fail a PR that copies blend into `glow.rs` / `dither_diffusion.rs`.

#### Acceptance Criteria

1. DoD includes code-review check: no per-filter opacity parameters inside kind-specific apply.
2. Diffusion code SHALL NOT read `instance.opacity`.
