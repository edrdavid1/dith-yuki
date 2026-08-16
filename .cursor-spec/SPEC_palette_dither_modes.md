# Спека: Strict vs Guided palette dithering + переключатель

Формальный трек (requirements / design / tasks):
[track-q-palette-dither-modes/](./track-q-palette-dither-modes/).
Этот файл — исходный бриф; при расхождении с треком приоритет у locked decisions в design.

## Контекст и цель

Сейчас `OrderedPalettePicker` / palette-constrained ED всегда работают в
режиме **Strict**: для каждого пикселя берутся два ближайших в Oklab цвета
из палитры, Bayer/ED-порог решает, какой поставить. Это физически честный
constrained dither (как настоящее retro-железо), но на изображениях с узким
тональным диапазоном (портрет крупным планом, ровный фон) реально
используется маленькое подмножество палитры, потому что только оно
Oklab-близко к присутствующим тонам.

Раньше приложение (предположительно) давало более "богатую" картинку —
похоже на **per-channel quantize+dither**, не привязанный жёстко к списку
цветов палитры. У нас нет доказательства точного алгоритма старой версии —
не пытаемся байт-в-байт его воспроизвести, а реализуем второй, отдельно
специфицированный режим с тем же духом (богаче, живее), и даём выбор.

**Оба режима — легитимные, разные продуктовые цели, не "старый баг vs новый
фикс".** Strict остаётся дефолтом (текущее поведение, ничего не ломаем для
существующих документов). Guided — новая явная опция.

---

## 1. Модель данных

### 1.1 Новый enum

```rust
// crates/engine-project/src/filter.rs (рядом с DitherColorMode)

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub enum PaletteDitherMode {
    Strict,                              // текущее поведение, default
    Guided { channel_levels: Option<u8> },  // None = auto из размера палитры
}

impl Default for PaletteDitherMode {
    fn default() -> Self { PaletteDitherMode::Strict }
}
```

### 1.2 Поле в DitherV2

```rust
pub struct DitherV2 {
    pub pixel_size: u8,
    pub color_mode: DitherColorMode,
    pub palette_id: Option<PaletteId>,
    pub palette_dither_mode: PaletteDitherMode,  // NEW, default Strict
    // ... остальные поля без изменений
}
```

Поле **релевантно только когда `palette_id.is_some()`** — при отсутствии
палитры игнорируется (uniform quantize к `levels` работает как сейчас,
Guided/Strict тут не применимы).

### 1.3 Миграция старых документов

Документы, сохранённые до этого изменения, не содержат поле —
`#[serde(default)]` на `palette_dither_mode` даёт `Strict`. Это сохраняет
текущее видимое поведение для всех существующих проектов; никто не
проснётся с внезапно другой картинкой. Guided — только по явному выбору
пользователя на новых или существующих слоях.

```rust
#[serde(default)]
pub palette_dither_mode: PaletteDitherMode,
```

Тест миграции: `dither_v2_legacy_document_defaults_to_strict_palette_mode`
— загрузить .json/.dith без поля, убедиться что `palette_dither_mode ==
Strict`.

---

## 2. Режим Strict (существующий, документируем контракт явно)

Ничего не меняется в логике. Формализуем контракт, чтобы Guided не мог
случайно с ним смешаться:

- Вход: пиксель в linear RGB → Oklab.
- Найти два ближайших цвета палитры (`i1`, `i2`) через `PaletteLutCache` /
  `KdTree::nearest` (build path).
- Порог (Bayer threshold, или error-diffusion остаток) решает `i1` vs `i2`.
- **Выход всегда — один из `palette.colors[i]`, дословно**, никогда не
  промежуточное значение.
- GPU eligibility не меняется: `Palette + Bayer → GPU skip, CPU path`
  (как сейчас).

---

## 3. Режим Guided (новый)

### 3.1 Идея

Каждый канал (R, G, B) квантуется и дизерится **независимо**, к числу
уровней, производному от палитры, в диапазоне, который покрывает палитра.
Итоговый пиксель — комбинация трёх независимо выбранных уровней, которая
**не обязана** совпадать ни с одним `palette.colors[i]` дословно. Палитра
здесь работает как ограничитель диапазона и "плотности" тонов, а не как
жёсткий список допустимых RGB-триплетов.

### 3.2 Шаг 1 — диапазон канала из палитры

```rust
pub struct ChannelRange { pub min: f32, pub max: f32 }

pub fn palette_channel_ranges(palette: &Palette) -> [ChannelRange; 3] {
    // min/max по каждому каналу (linear RGB) среди palette.colors
    // Пустая / однокомпонентная палитра → range [0.0, 1.0] (fallback)
}
```

Считается **один раз на revision палитры**, кэшируется рядом с
`PaletteLutCache` (тот же invalidation-паттерн: revision-keyed DashMap).
Не пересчитывать на каждый пиксель.

### 3.3 Шаг 2 — число уровней на канал

```rust
pub fn default_channel_levels(palette: &Palette) -> u8 {
    // cbrt(N), округление вверх, clamp [2, 16]
    let n = palette.colors.len().max(1) as f32;
    (n.cbrt().ceil() as u8).clamp(2, 16)
}
```

Логика: если у палитры 16 цветов, `cbrt(16) ≈ 2.5 → 3` уровня на канал
даёт теоретический потолок `3³ = 27` визуальных сочетаний — заметно
богаче, чем 16 "жёстких" точек, но не безгранично. Пользователь может
переопределить (`channel_levels: Some(n)`) через UI-слайдер 2–16.

### 3.4 Шаг 3 — quantize + dither на канал

Переиспользовать существующий uniform-quantize путь (`dither_ordered.rs`,
ветка "без палитры → uniform quantize к levels"), но:
- диапазон — не `[0,1]`, а `[range.min, range.max]` для этого канала;
- уровни — `channel_levels`, не глобальный `levels: u16` из DitherV2;
- **порог общий на все три канала** (тот же Bayer threshold в данной
  позиции) для базового варианта — сохраняет пространственную когерентность
  паттерна между каналами, не даёт RGB "расползтись" в шум.

```rust
fn quantize_channel_guided(
    value: f32,
    range: ChannelRange,
    levels: u8,
    threshold: f32,       // тот же Bayer threshold, что для Strict-порога
) -> f32 {
    let span = (range.max - range.min).max(1e-6);
    let normalized = ((value - range.min) / span).clamp(0.0, 1.0);
    let scaled = normalized * (levels as f32 - 1.0);
    let base = scaled.floor();
    let frac = scaled - base;
    let step = if frac > threshold { base + 1.0 } else { base };
    range.min + (step / (levels as f32 - 1.0)) * span
}
```

Для **error diffusion**-варианта — тот же принцип, но с накоплением ошибки
по каналу независимо (`ErrorResidualsStore` уже хранит per-channel error,
менять не нужно — только точку квантования подменить на
`quantize_channel_guided` вместо palette-nearest).

### 3.5 Опциональное улучшение — per-channel threshold offset

Классический приём для более "живого" ordered dither: сдвигать Bayer-порог
на разную фазу для R/G/B (например, `threshold_R`, `threshold_G =
rotate(threshold_R)`, `threshold_B = rotate(rotate(threshold_R))`). Даёт
характерный RGB-субпиксельный шум вместо синхронного постеринга по всем
каналам разом. **Не делать в первой версии** — сначала common-threshold
вариант, оценить визуально, добавить как отдельный подпараметр, если
нужно больше "живости".

### 3.6 Что НЕ делать в Guided

- Не снапать финальный цвет к ближайшему `palette.colors[i]` — это вернёт
  Strict через чёрный ход и потеряет весь смысл режима.
- Не менять `error_residuals` схему хранения — она уже per-channel-agnostic.
- Не путать с uniform-quantize-без-палитры (`palette_id: None`) — тот путь
  остаётся как есть, Guided отличается тем, что диапазон/плотность уровней
  выведены из конкретной палитры, а не глобального `levels`.

---

## 4. GPU eligibility — явное решение, не молчаливое расширение

Сейчас инвариант: `Bayer2/4/8: pixel_size==1, no palette, ...`. Guided
концептуально ближе к uniform-quantize (который **eligible** для GPU), чем
к Strict palette-nearest (который **not eligible**).

**Решение по умолчанию: Guided тоже `GPU skip, CPU path`, как Strict, в
первой версии.** Не расширять eligibility-таблицу заодно с этой фичей —
это отдельная оптимизация с отдельными GPU-parity тестами (см. инвариант
"GPU parity: Bayer exact; Halftone/CRT ≤ 1/255" — для Guided потребуется
свой parity-бюджет, не переиспользовать существующий). Если после релиза
Guided понадобится GPU-ускорение — заводить отдельным треком.

---

## 5. UI / фронтенд

### 5.1 Где переключатель

В панели эффекта DitherV2 (`DitherSettings`), **видим только когда
`palette_id` выбран** (аналогично тому, как сейчас скрыты/показаны
palette-специфичные поля). Дропдаун из двух пунктов:

- "Strict — exact palette colors"
- "Guided — palette-derived range (richer)"

При выборе Guided — дополнительный slider "Levels per channel" (2–16),
по умолчанию — авто (`default_channel_levels`), с возможностью override.

### 5.2 Types

```typescript
// frontend/src/types (рядом с DitherColorMode)
export type PaletteDitherMode =
  | { kind: 'strict' }
  | { kind: 'guided'; channelLevels?: number };
```

### 5.3 IPC / commands.rs

`update_filter` — без изменений в сигнатуре команды, поле едет как часть
сериализованного `DitherV2` params, как остальные (`pixel_size`,
`threshold_bias` и т.д.).

---

## 6. Тесты и acceptance

| Тест | Проверяет |
|---|---|
| `dither_v2_legacy_document_defaults_to_strict_palette_mode` | Миграция: старые документы не меняют вид |
| `guided_output_not_necessarily_in_palette` | Guided даёт хотя бы один пиксель, которого нет дословно в `palette.colors` (на тестовом градиенте) — иначе режим ничем не отличается от Strict |
| `guided_output_within_palette_channel_range` | Ни один канал результата не выходит за `[range.min, range.max]` этой палитры |
| `strict_output_always_exact_palette_color` | Регрессия: Strict как был — каждый выходной пиксель точно равен одному из `palette.colors[i]` |
| `guided_channel_levels_default_matches_cbrt_formula` | `default_channel_levels` даёт ожидаемое число для палитр размера 4/16/64 |
| `guided_gpu_not_eligible` | Guided палитра остаётся CPU-only, GPU eligibility-таблица не расширена без явного трека |
| Визуальный acceptance (ручной) | На тестовом портрете (тот же файл, что в скриншоте) Guided даёт заметно больше визуальных оттенков, чем Strict, на том же pixel_size/Bayer |

---

## 7. Порядок реализации

1. Enum + поле в `DitherV2` + serde default + миграционный тест — **сначала**,
   чтобы ничего не сломать для существующих документов независимо от
   остального прогресса.
2. `palette_channel_ranges` + кэш (revision-keyed, рядом с `PaletteLutCache`).
3. `quantize_channel_guided` для Ordered-пути.
4. Error-diffusion вариант (переиспользует существующий per-channel error
   buffer, меняется только точка квантования).
5. UI: дропдаун + levels slider, видимость по `palette_id`.
6. Тесты из §6.
7. Ручная визуальная проверка на портрете из скриншота — сравнить Strict
   vs Guided на одинаковых параметрах, убедиться что Guided даёт то самое
   "живее", ради которого всё затевалось.

## Инварианты, которые нельзя ломать

`GlobalCoord` / `rem_euclid`, ED residuals + `corner`, LUT vs KD только на
границах ячеек, GPU parity (Bayer exact; Halftone/CRT ≤ 1/255) — Guided
не расширяет GPU-eligibility без отдельного трека (см. §4).
