# Requirements: Track Q — Strict vs Guided palette dither

## Introduction

Формализация [SPEC_palette_dither_modes.md](../SPEC_palette_dither_modes.md).

Сейчас palette-constrained dither (`palette_id` задан) всегда **Strict**:
для пикселя берутся два ближайших в Oklab цвета палитры, Bayer/ED-порог
выбирает один из них. Выход — дословно `palette.colors[i]`. Это честный
constrained dither, но на узком тональном диапазоне реально используется
маленькое подмножество палитры.

**Guided** — второй, отдельно специфицированный режим: per-channel quantize
+ dither в диапазоне, выведенном из палитры. Итоговый RGB **не обязан**
совпадать ни с одним цветом палитры. Палитра задаёт диапазон и плотность
уровней, не жёсткий список допустимых триплетов.

Оба режима — легитимные продуктовые цели, не «баг vs фикс». Strict —
дефолт и текущее поведение. Guided — явная опция.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md). Источник: SPEC.

## Glossary

- **Strict**: текущий palette-nearest dither; выход всегда exact palette color.
- **Guided**: per-channel quantize+dither в `[min, max]` каналов палитры.
- **Channel_Range**: `min`/`max` linear RGB по одному каналу среди `palette.colors`.
- **Channel_Levels**: число квантованных ступеней на канал в Guided (2–16).
- **Palette_Dither_Mode**: поле `DitherParamsV2`; релевантно только при `palette_id.is_some()`.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Enum + поле на `DitherParamsV2`, serde default Strict | Воспроизвести байт-в-байт старую версию приложения |
| Guided ordered + ED с общим порогом на R/G/B в v1 | Per-channel Bayer phase offset (follow-up) |
| UI только при выбранной палитре | GPU eligibility для Guided в этом треке |
| Старые документы без поля остаются Strict | Snap Guided-выхода к nearest palette color |
| | Менять `error_residuals` схему хранения |
| | Менять uniform-quantize при `palette_id: None` |

---

## Requirements

### Requirement 1: Data model and persistence

**User Story:** As a user, I want existing projects to look the same after this update, and I want the new mode saved when I choose it.

#### Acceptance Criteria

1. THE engine SHALL add `PaletteDitherMode` with variants `Strict` (default) and `Guided { channel_levels: Option<u8> }` (`None` = auto from palette size).
2. `DitherParamsV2` SHALL gain `palette_dither_mode: PaletteDitherMode` with `#[serde(default)]` → `Strict`.
3. WHEN a document JSON / `.dyproj` / `.dyuki` omits the field, THE deserializer SHALL yield `Strict`.
4. THE field SHALL be ignored when `palette_id` is `None` (uniform quantize to `levels` unchanged).
5. WHEN `channel_levels` is `Some(n)`, THE validator SHALL require `n` in `[2, 16]`; otherwise return a validation error.
6. Frontend types SHALL mirror the enum (`{ kind: 'strict' } | { kind: 'guided'; channelLevels?: number }`) and round-trip through `update_filter` as part of serialized `DitherParamsV2` (no new IPC command).
7. Test `dither_v2_legacy_document_defaults_to_strict_palette_mode` SHALL load a fixture without the field and assert `palette_dither_mode == Strict`.

### Requirement 2: Strict contract (no behavior change)

**User Story:** As a user, I want Strict to keep exact palette colors so retro-constrained looks do not change.

#### Acceptance Criteria

1. WHEN `palette_id` is set AND mode is Strict, THE engine SHALL map each pixel (linear RGB → Oklab) to two nearest palette colors via `PaletteLutCache` / KD on cell boundaries, then pick `i1` vs `i2` by Bayer threshold or ED residual — **unchanged from today**.
2. EVERY output RGB triple in Strict SHALL equal some `palette.colors[i]` exactly (bit-identical to a palette entry).
3. GPU eligibility for Strict palette + Bayer SHALL remain skip → CPU (no change).
4. Test `strict_output_always_exact_palette_color` SHALL assert (2) on a fixture with a known palette.

### Requirement 3: Guided — range, levels, quantize

**User Story:** As a user, I want a richer dither that uses the palette as a tonal range, so portraits and flat fields use more visual shades than Strict.

#### Acceptance Criteria

1. THE engine SHALL compute `palette_channel_ranges(palette) -> [ChannelRange; 3]` as min/max linear RGB per channel over `palette.colors`. Empty or single-color palette SHALL fall back to `[0.0, 1.0]` per channel.
2. Ranges SHALL be computed once per palette revision and cached with the same invalidation pattern as `PaletteLutCache` (revision-keyed). THE engine SHALL NOT recompute ranges per pixel.
3. `default_channel_levels(palette)` SHALL be `ceil(cbrt(N)).clamp(2, 16)` where `N = palette.colors.len().max(1)`.
4. WHEN `channel_levels` is `None`, Guided SHALL use `default_channel_levels`. WHEN `Some(n)`, it SHALL use `n`.
5. THE engine SHALL quantize each channel independently with `quantize_channel_guided` (SPEC §3.4): normalize into the channel range, dither against **the same** Bayer threshold (or ED residual compare) for R, G, and B, then map the chosen step back into `[range.min, range.max]`.
6. THE final Guided pixel SHALL NOT be snapped to nearest `palette.colors[i]`.
7. Test `guided_output_not_necessarily_in_palette`: on a test gradient, at least one output pixel is not an exact palette color.
8. Test `guided_output_within_palette_channel_range`: no output channel exceeds that palette’s `[min, max]`.
9. Test `guided_channel_levels_default_matches_cbrt_formula`: sizes 4 / 16 / 64 match the formula (2, 3, 4 respectively: `cbrt(4)→2`, `cbrt(16)→3`, `cbrt(64)→4`).

### Requirement 4: Guided on ordered and error-diffusion paths

**User Story:** As an artist, I want Guided on both Bayer/CustomPng and FS/Atkinson-class modes, so the mode is not ordered-only.

#### Acceptance Criteria

1. Ordered path (`dither_ordered.rs`) SHALL use `quantize_channel_guided` with the shared Bayer/CustomPng threshold at that global position (after existing Block_Then_Rotate / bias).
2. Error-diffusion path SHALL keep `ErrorResidualsStore` unchanged (per-channel error already). Only the quantization point SHALL switch from palette-nearest to `quantize_channel_guided`.
3. Uniform quantize with `palette_id: None` SHALL remain the existing `levels` path; Guided MUST NOT be applied there.
4. Wave / CmykHalftone with a palette MAY ignore Guided in v1 if they have no palette-nearest path today; if they do palette-constrain, they SHALL treat Guided as CPU Guided quantize the same way or document skip in design. **Locked default:** apply Guided only where Strict palette-nearest already runs (ordered Bayer/CustomPng + ED). Halftone/Wave/CRT unchanged.

### Requirement 5: GPU policy

**User Story:** As a maintainer, I want Guided to stay CPU-only until a dedicated GPU-parity track exists.

#### Acceptance Criteria

1. WHEN Guided is active (`palette_id` set and mode Guided), THE GPU path SHALL skip (same as Strict palette). Eligibility tables SHALL NOT be extended in this track.
2. Test `guided_gpu_not_eligible` SHALL assert skip for Guided + Bayer.
3. GPU parity budget (Bayer exact; Halftone/CRT ≤ 1/255) SHALL NOT be reused as Guided’s budget. A future GPU track MUST define its own.

### Requirement 6: UI

**User Story:** As an artist, I want to pick Strict vs Guided only when a palette is selected, and optionally override levels per channel.

#### Acceptance Criteria

1. `DitherSettings` SHALL show a two-item control only when `palette_id` is set:
   - “Strict — exact palette colors”
   - “Guided — palette-derived range (richer)”
2. WHEN Guided is selected, THE panel SHALL show a Track K `Slider` “Levels per channel” in `[2, 16]`. Default display SHALL be auto (`default_channel_levels` if known, else 3) with override writing `channel_levels: Some(n)`. Clearing override MAY set `None` (auto); v1 MAY omit an explicit “Auto” toggle and treat slider value as always `Some` after first touch — lock in design.
3. THE control SHALL use existing Slider / select patterns (Track K); raw `<input type="range">` is a process regression.
4. Changing the mode SHALL persist via existing `update_filter` and invalidate preview tiles.

### Requirement 7: Invariants

**User Story:** As a maintainer, I want existing seam and GPU invariants to stay green.

#### Acceptance Criteria

1. `GlobalCoord` / `rem_euclid`, ED residuals + `corner`, LUT vs KD only on cell boundaries SHALL remain unchanged.
2. Strict + `palette_dither_mode` default SHALL be bit-identical to pre-track palette dither on the same fixtures (regression).
3. Existing A2 / H / M seam matrices SHALL stay green at Strict default.
