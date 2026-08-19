# Как сейчас работают цвет и Color Lab

As-built описание Color Lab. Общая карта системы — [architecture.md](./architecture.md). Режимы палитрового дизера (Strict / Guided / Mixed / Simple) — [palette-dither.md](./palette-dither.md).

---

## 1. Цветовые пространства

| Пространство | Где | Зачем |
|---|---|---|
| **Linear RGB (f32)** | `PixelTile`, `Palette.colors` | Постоянное хранилище пикселей и палитр. Curves, Levels, blend работают здесь. |
| **Oklab (f32)** | Внутри палитровой квантизации / error diffusion; генераторы (ramps, harmony) | Перцептивно равномерный nearest-color и диффузия ошибки. Не сохраняется в документе. |
| **OkLCH (f32)** | `engine-color/oklch.rs`, ramps/harmony | Цилиндрическая форма Oklab (L, C, hue в **радианах**): гамут-клип в sRGB, правила гармонии. |

Вход в Oklab — уже линейный RGB (без повторной sRGB→linear). Матрица LMS — sRGB/Rec.709 primaries.

На UI и в IPC цвета палитры обычно идут как **sRGB u8** (и hex); при записи в документ — linear f32.

---

## 2. Backend: `engine-color`

Крейт: `crates/engine-color/`.

| Модуль | Файл | Роль |
|---|---|---|
| Oklab | `src/oklab.rs` | `LinRgb` ↔ `Oklab`, `oklab_to_linear_unclamped`, `oklab_dist_sq` |
| OkLCH | `src/oklch.rs` | `OkLch`, gamut check / `clip_to_srgb_gamut` |
| Ramps | `src/ramps.rs` | `generate_ramp` — lerp в Oklab + clip |
| Harmony | `src/harmony.rs` | Mono / Analogous / Complementary / Triadic / SplitComplementary |
| KD-tree | `src/kdtree.rs` | `KdTree::build`, `nearest` → индекс цвета |
| Палитра | `src/palette/mod.rs` | `Palette`, `LinearColor`, import/export API |
| Пресеты | `src/palette/presets.rs` | `BUILTIN_PRESETS`, `find_preset` (`gameboy`, `apple2`, …) |
| Форматы | `src/palette/formats/` | ASE, ACO, GPL, PAL, CSV, JSON |
| Генерация | `src/palette/generate.rs` | Median cut, K-means; subsample ≤ `MAX_GENERATION_SAMPLES` (200k); HashSet-дедуп |
| Кэш | `src/palette_cache.rs` | `PaletteKdCache` (DashMap по id + revision) |
| Threshold map | `src/threshold_map.rs` | PNG grayscale для ordered dither |

### Сущность `Palette`

```text
Palette {
  id: u32,
  name: String,
  colors: Vec<LinearColor>,  // linear RGB f32
  revision: u64              // стартует с 1; ++ при изменении цветов
}
```

Палитра — **сущность документа**, не параметр фильтра и не upstream-нода.

### Built-in presets

Источник правды — только `palette/presets.rs`. Frontend **не** хардкодит RGB-таблицы; список приходит через `list_builtin_palettes`.

| id | Содержимое |
|---|---|
| `gameboy` | 4 классических DMG greens |
| `apple2` | 16 цветов lo-res NTSC / AppleWin |

Добавление пресета = запись в `BUILTIN_PRESETS` без смены IPC-формы.

---

## 3. Документ и CRUD палитр

Файл: `crates/engine-project/src/document.rs`.

- `Document.palettes: Vec<Palette>`
- `add_palette` / `modify_palette` / `remove_palette` / `get_palette`
- `remove_palette` отказывает, если фильтры ссылаются на id (`PaletteInUse`)

Ссылки:

- `FilterParams::PaletteQuantize { palette_id }`
- `FilterParams::DitherV2 { palette_id: Some(...) }`

`DocumentSnapshotDto` отдаёт **только ids**; полные цвета — через `list_palettes`.

Генерация из слоя: `crates/engine-project/src/palette_gen.rs` → `generate_palette_from_layer` (вызывает `engine_color::palette::generate`).

---

## 4. Фильтры, которые используют палитру

### PaletteQuantize

`crates/engine-project/src/filters/palette_quantize.rs`

1. `palette_id` из параметров.
2. `PaletteLutCache::get_or_build(palette, kd_cache, DEFAULT_LUT_SIZE)` → `Arc<PaletteLut3D>`
   (KD-tree строится внутри кэша для cell centers; hot path не ходит по дереву).
3. Пиксель: linear → Oklab → `lut.nearest_index` → `palette.colors[index]`.
4. Alpha не трогается; опционально error diffusion в Oklab.

Default grid: **64³** (~512 KiB `u16`). Ranges: L∈[0,1], a/b∈[-0.4,0.4].
(32³ was ~same throughput but higher Cell_Boundary_Disagreement on dense palettes.)

### Dither V2

Параметры: `DitherParamsV2 { …, palette_id: Option<PaletteId> }`.

- `Some(id)` — nearest в Oklab через `PaletteLut3D` (тот же LUT-кэш).
- `None` — квантизация по `levels` на канал.

Legacy `FilterParams::Dither` мигрирует в V2 с `palette_id: None`.

### Инвалидация

Изменение цветов → `revision++` → dirty слои с этим id → воркеры → при mismatch revision
пересобираются и `PaletteKdCache`, и `PaletteLutCache`. `delete_palette` вызывает `evict` на обоих.

### UI эффектов ↔ Color Lab

- Новый filter с пустым/`None` `palette_id` получает `palettesSlice.lastCreatedId` при создании (`layersSlice.addLayerWithEffect`).
- `PaletteSelector`: при обязательном выборе и пустом id показывает `lastCreatedId`.
- Редактор **Dither** (`features/effects`): локального селектора палитры нет — `palette_id` синхронизируется с `lastCreatedId` (`EffectsFeature` + подсказка в `DitherSettings`).

---

## 5. Tauri-команды палитр и генераторов

Реализация: `src-tauri/src/commands.rs`. Обёртки: `frontend/src/shared/ipc/palettes.ts`.

| Команда | Поведение |
|---|---|
| `list_palettes` | Все палитры → DTO (sRGB + hex) |
| `add_palette` | Имя + sRGB u8 → linear → новая палитра |
| `create_palette` | Пустая палитра по имени |
| `import_palette` | Файл → parse → **всегда новая** палитра |
| `export_palette` | id + путь + формат → файл |
| `generate_palette` | **async** + `spawn_blocking`: stride-sample тайлов (~200k) → MedianCut/KMeans → новая палитра |
| `list_builtin_palettes` | id, name, colors (sRGB) из registry |
| `import_builtin_palette` | preset → sRGB→linear → новая Document-палитра (как import) |
| `generate_ramp_palette` | from/to hex + steps → `Vec` цветов; **не** пишет в Document |
| `generate_harmony_palette` | base hex + rule + count → `Vec` цветов; **не** пишет в Document |
| `rename_palette` | Только имя |
| `remove_palette` | Строгое удаление (ошибка, если in use) |
| `delete_palette` | Снимает ссылки / удаляет PaletteQuantize, затем палитру + `evict` кэша |
| `add_color_to_palette` / `update_palette_color` / `remove_palette_color` / `reorder_palette_color` | Мутация цветов, `revision++` |

### Производительность `generate_palette`

Раньше синхронный проход по всем пикселям + O(n²) дедуп вешал UI на «пёстрых» фото. Сейчас:

1. Команда асинхронная, тяжёлая работа в blocking pool.
2. Stride при чтении raw-тайлов (цель ~`MAX_GENERATION_SAMPLES`).
3. В `generate.rs` — повторный subsample + HashSet-дедуп по квантованному ключу.

---

## 6. Color Lab (frontend)

### Где живёт UI

Вход: `frontend/src/features/color-lab/ColorLabFeature.tsx` (панель `colorlab`: docked / floating).

| Компонент | Роль |
|---|---|
| `ColorLabBody` | Layout секций |
| `AutoExtractSection` | Метод + count + кнопки Extract raw/actual |
| `ImportExportSection` | Диалоги файла |
| `BuiltinPresetsSection` | Dropdown (`lp-dropdown-wrap`): свотчи + имя; выбор → `import_builtin_palette` |
| `RampGeneratorSection` | From/to + steps → preview → Insert **заменяет** цвета драфта |
| `HarmonySection` | Base + rule → Insert в драфт |
| `PaletteManualEditor` | Список hex + picker |
| `ColorLabFooter` | Apply и т.п. |
| `useColorLabDraftSync` | Синхронизация драфта между окнами + `localStorage` |
| `ColorPicker` | Оверлей выбора цвета |

`PalettePanel` / `SwatchGrid` умеют живой CRUD через IPC, но основной Color Lab Feature их **не** монтирует.

### Redux / prefs

- **`colorLabSlice`** — драфт: `name`, `colors`, extract settings, ошибки/успех, multi-window epoch.
- **`palettesSlice`** — `version` + **`lastCreatedId`** (последняя созданная палитра: Apply / Import / Extract / builtin).
- Shell pref **`autoExtractPalettes`** (default **true**) в `ShellContext` / `dither.shellPrefs` — Preferences UI.

Драфт: `localStorage` (`dither.colorLab.draft`) + событие `color-lab-draft-changed`.

### Поведение Color Lab

Редактируется **черновик**, не «текущая палитра документа» на лету.

| Действие | Документ | Драфт |
|---|---|---|
| Правка hex / picker / sort | — | да |
| Выбор существующей палитры | — | загрузка цветов |
| **Apply** | всегда `add_palette` (новая) | — |
| Import файла | новая палитра | цвета из файла |
| Extract | `generate_palette` → новая | цвета из результата |
| Builtin preset | `import_builtin_palette` → новая | как после import |
| Ramp / Harmony Insert | — | **замена** цветов драфта |
| Export | побочный `add_palette` + файл | — |

Правки в драфте **не** обновляют палитру, уже привязанную к фильтру, и сами по себе **не** перерисовывают тайлы.

### Auto-extract при открытии изображения

`frontend/src/app/autoExtract.ts`:

1. После успешного Open Image (`useDocument`) вызывается `maybeAutoExtractPalette(dispatch, layerId)`.
2. Если pref выключен — no-op.
3. Иначе тот же путь, что ручной Extract: `generate_palette` → драфт + `bumpVersion({ lastCreatedId })`.
4. Ошибка экстракта не откатывает открытие файла (ошибка уходит в Color Lab UI).

Import Layer from file как отдельный триггер пока может отсутствовать; когда появится — тот же helper.

---

## 7. Связь Color Lab ↔ фильтры ↔ превью

```text
Color Lab (драфт RTK)
        │ Apply / Import / Extract / Builtin
        ▼
Document.palettes  ←── palette_id ──  PaletteQuantize / DitherV2
        │                              (Dither UI: sync с lastCreatedId)
        │  (IPC color-* мутации)
        ▼
invalidate → tile workers → PaletteKdCache → Oklab nearest → TileCanvas
```

Ramp/Harmony пишут только в драфт; в Document попадают после Apply (или после Extract/Import/Builtin, если пользователь так сохранил).

---

## 8. Сквозные сценарии

### A. Редактирование без Apply

UI → `colorLabSlice` → localStorage / другие окна → Document не меняется → превью не меняется.

### B. Apply / Extract / Builtin

Новая палитра в `Document.palettes` → `lastCreatedId` → селекторы / Dither подхватывают id. Тайлы грязнятся, когда фильтр ссылается на этот id.

### C. Auto-extract при Open

Open image → тайлы в кэше → (pref on) `generate_palette` в фоне → драфт заполнен, `lastCreatedId` выставлен → новый Dither/Quantize может сразу использовать палитру.

### D. Ramp / Harmony → Apply

Insert в драфт → пользователь жмёт Apply → новая Document-палитра.

### E. Живое изменение уже используемой палитры

Через `update_palette_color` / … (SwatchGrid / IPC, не Apply Color Lab) → `revision++` → перерисовка.

---

## 9. Ключевые пути

```text
crates/engine-color/                 # Oklab, OkLCH, ramps, harmony, palettes, presets
crates/engine-project/src/document.rs
crates/engine-project/src/palette_gen.rs
crates/engine-project/src/filters/palette_quantize.rs
crates/engine-project/src/filters/dither_*.rs
src-tauri/src/commands.rs            # palette + generate_* IPC
frontend/src/features/color-lab/     # UI Color Lab
frontend/src/app/autoExtract.ts
frontend/src/app/shell/ShellContext.tsx   # autoExtractPalettes pref
frontend/src/app/slices/colorLabSlice.ts
frontend/src/app/slices/palettesSlice.ts
frontend/src/shared/ipc/palettes.ts
frontend/src/components/PaletteSelector.tsx
frontend/src/features/effects/EffectsFeature.tsx
```

---

## 10. Известные расхождения «как задумано» vs «как сейчас»

1. Color Lab Apply создаёт **новую** палитру, а не обновляет выбранную (gap #1 — список палитр растёт от auto-extract / Apply / builtin).
2. Живой CRUD цветов документа в UI есть (`PalettePanel` / `SwatchGrid` + IPC), но основной Color Lab Feature этим путём не пользуется.
3. Import / builtin всегда добавляют палитру, без replace.
4. В DTO снимка документа — только ids; цвета только через `list_palettes`.
5. Auto-extract на Import Layer зависит от наличия фронтового хука импорта слоя (Open Image уже подключён).
