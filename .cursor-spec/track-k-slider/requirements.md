# Requirements: Track K — Unified Slider / NumberInput

## Introduction

ROADMAP K: один numeric control со слайдером + полем, debounce 100ms **в
компоненте**, чтобы новые панели H/J/M/I не плодили сырые `<input>` и не
копировали debounce.

As-built: `frontend/src/components/common/Slider.tsx` уже совмещает track +
текстовое поле. Debounce 100ms живёт в `useEffectLayer` (`updateParams`), не
в Slider. `GlitchSettings` / `CurvesSettings` ещё имеют `type="number"`.

Этот трек — **дожать контракт и закрыть дырки**, не писать второй слайдер.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md). Делать рано (до UI H/M/I
новых полей, насколько возможно). Нет backend-гейта.

## Glossary

- **Slider**: существующий `components/common/Slider.tsx`.
- **NumberInput**: либо экспорт из того же модуля, либо тонкая обёртка без track — одно значение, те же clamp/step/debounce.
- **Param_Debounce**: 100ms; одна реализация.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Один debounce 100ms на control | Менять визуальный язык / Retro CSS с нуля |
| NumberInput для seed/целых без track | Заменить `useEffectLayer` invalidation semantics |
| Новые панели только через эти компоненты | Редизайн Color Lab swatches |
| Снести сырые number inputs в effect editors | Форсировать K как блокер merge H если H уже стартовал — process regression, чинить |

---

## Requirements

### Requirement 1: Component contract

**User Story:** As a developer adding a dither param, I want Slider to snap, clamp, show a number, and not flood IPC on every pointer move.

#### Acceptance Criteria

1. `Slider` SHALL keep: label, value, min, max, step, decimals, onChange.
2. Debounce 100ms SHALL live in the control (or a shared hook used only by Slider/NumberInput). WHILE dragging, local UI updates immediately; `onChange` to parent/IPC is coalesced.
3. IF `useEffectLayer` already debounces `updateParams` at 100ms, Track K SHALL NOT stack a second 100ms blindly (200ms lag). Lock in design: one layer of debounce on the way to IPC.
4. Enter/blur on the text field SHALL commit immediately (bypass debounce) — already true for Slider text; MUST remain.

### Requirement 2: NumberInput

**User Story:** As a developer, I want an integer seed field with the same clamp/commit rules without a slider track.

#### Acceptance Criteria

1. Export `NumberInput` with min/max/step/decimals/label/onChange, shared clamp+commit+debounce helper with Slider.
2. Glitch seed (Track J) SHALL use it.

### Requirement 3: Adoption

**User Story:** As a reviewer, I want new effect params to fail review if they use raw `<input type="number|range">`.

#### Acceptance Criteria

1. Effect editors under `features/effects/editors/` SHALL not add new raw number/range inputs.
2. Existing raw inputs in `GlitchSettings.tsx` and `CurvesSettings.tsx` SHALL be migrated in this track (curves point fields MAY stay if they are coordinate pairs — lock: migrate if they are simple scalars; leave curve graph as-is).
3. tasks.md DoD: grep editors for `type="number"` / `type="range"` — only allowed exceptions listed.

### Requirement 4: Tests

**User Story:** As a maintainer, I want unit tests that drag/commit behavior does not regress.

#### Acceptance Criteria

1. Keep/extend `formatValue` / Slider tests if present; add debounce test: N rapid onChange from drag → one parent call after 100ms (or one IPC if that’s the chosen layer).
2. Existing `useEffectLayer` debounce test SHALL stay green after the single-layer decision.
