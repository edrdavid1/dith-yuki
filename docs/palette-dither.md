# Палитровый дизер и цвет

Как Dither Yuki 2 обрабатывает цвет на слое с фильтром Dither, когда к нему привязана палитра из Color Lab.

Палитры, пресеты, IPC Color Lab — в [color-lab.md](./color-lab.md). Тайлы и wavefront ED — в [tile-pipeline.md](./tile-pipeline.md).

---

## 1. Где считается цвет

Пиксели живут в **Rust**, не во фронтенде. Tauri — окно и IPC. React только шлёт параметры фильтра.

| Слой | Пространство | Роль |
|---|---|---|
| Импорт PNG/JPEG | sRGB u8 → linear f32 | `srgb_to_linear` |
| `PixelTile`, палитра в документе | linear RGB f32 `[0,1]` | хранение, Adjust, Curves, blend |
| Strict / Mixed nearest | Oklab | перцептивная близость |
| Simple nearest | sRGB u8 | как старый `findClosestColor` |
| Guided | linear RGB по каналам | квант в min–max палитры |
| Превью / экспорт | linear → sRGB | показ |

Коррекции (яркость, контраст, saturation, blur, sharpness, noise) — **отдельный фильтр Adjust**, до дизера в стеке слоя. Дизер всегда видит уже скорректированный linear RGB.

Без `palette_id` режимы ниже **игнорируются**: каналы режутся на `levels` (2–256).

С палитрой поле `palette_dither_mode` задаёт, *как* пиксель попадает в свотчи. Старые документы без поля грузятся как **Strict**.

---

## 2. Четыре режима Palette dither

В UI (Dithering → Palette dither), только если палитра привязана в Color Lab:

| Режим | Выход всегда цвет палитры? | Идея |
|---|---|---|
| **Strict** (дефолт) | да | два ближайших в Oklab + Bayer / LUT nearest + ED |
| **Guided** | нет | квант каждого канала в диапазоне палитры |
| **Mixed** | да | Guided, потом тот же two-nearest, что Strict |
| **Simple** | да | евклидов nearest в sRGB 8-bit (классический Yuki) |

`Levels per channel` (2–16) есть только у Guided и Mixed.

Алгоритм (Floyd–Steinberg, Bayer 4×4, …) выбирается отдельно. Режим палитры — *метрика и квант*, не замена ядра.

---

## 3. Strict

**Цель:** «честный» constrained dither: на экране только свотчи палитры, выбор по Oklab.

### Ordered (Bayer / PNG / Wave)

`OrderedPalettePicker`:

1. Пиксель (linear RGB) → Oklab.
2. Два ближайших индекса палитры `i1`, `i2`.
3. `mix = d1 / (d1 + d2)` по расстояниям в Oklab.
4. `t = 0.5 + (T − 0.5) × threshold_scale`, `T` — порог матрицы.
5. Если `t < mix` → `i2`, иначе `i1`.

Код: `crates/engine-project/src/filters/dither_ordered.rs` (`OrderedPalettePicker::pick`).

Порог Bayer при `pixel_size > 1` индексируется в **блоках** (`div_euclid(ps)`), иначе при `ps % matrix == 0` вся картинка бьёт в одну ячейку матрицы.

### Error diffusion

Nearest в Oklab через 3D LUT (`PaletteLut3D`). Residual в **линейном** RGB: `q_err = adj − выбранный_цвет`. Ядро FS/Atkinson/… разносит ошибку соседям.

### Когда выглядит «бедно»

На узком тоновом диапазоне (щёки, ровный фон) в Oklab реально участвуют 2–3 свотча. Это не баг: так устроен nearest в перцептивном пространстве. Для более живой картины — Guided или Mixed.

---

## 4. Guided

**Цель:** богаче и шумнее, цвета **не обязаны** совпадать со свотчами.

По палитре считаются `ChannelRange` (min/max каждого linear-канала). Каждый канал:

```text
quantize_channel_guided(value, range, levels, t)
```

`t` — тот же порог Bayer (общий на R, G, B). `levels` — `channel_levels` или auto от размера палитры.

Выход лежит в диапазоне палитры, но может быть «между» свотчами. GPU Bayer для Guided не используется (CPU-only).

ED: тот же квант с `t = 0.5`, residual от guided-цвета.

---

## 5. Mixed

**Цель:** структура Guided + только цвета палитры.

1. `quantize_channel_guided` — как Guided (ступеньки `levels_per_channel`).
2. На **guided RGB** — тот же `OrderedPalettePicker::pick`, что Strict (два ближайших + Bayer).

Не hard snap к одному nearest: внутри большой ячейки Voronoi пара `i1`/`i2` всё равно меняется вдоль тона, Bayer снова получает что дизерить.

Отличие от Strict: на вход pick приходит уже прореженный guided-сигнал → спокойнее, меньше шума, чем Strict на сыром пикселе.

ED: pick от guided с порогом `0.5`; residual **`original − выбранный свотч`**, не от pre-pick guided.

---

## 6. Simple

**Цель:** поведение старого Dither Yuki (`simple-dith-old-version.ts` / `findClosestColor`), палитра — из документа.

Метрика: евклидово расстояние в **sRGB байтах** (не Oklab). Linear пиксель кодируется `linear_to_srgb`, свотчи палитры — тоже.

### Ordered

Как `bayerDither` в TS:

```text
offset = (T − 0.5) × threshold_scale × 64
byte' = clamp(srgb_byte + offset, 0, 255)
nearest Euclidean → linear свотч
```

`Threshold Scale` = старый intensity (`1.0` = 100%).

### Error diffusion

Рабочее пространство ошибки — sRGB байты:

```text
old = clamp(linear_to_srgb(src) + acc_srgb, 0, 255)
pick = nearest(old)
q_err = (old − pal_srgb) × threshold_scale
```

Это не «linear + sRGB в одной куче»: источник переводится в sRGB, аккумулятор уже в sRGB. Так же, как ImageData 0–255 в легаси.

Кожа и света часто прыгают в насыщенные свотчи Apple II (teal `#0E5940`, purple `#E434FE`) — особенность RGB Euclidean, не Oklab.

---

## 7. Pixel size и error diffusion

`pixel_size` — мегапиксель: цвет блока = representative (top-left, global align), остальные пиксели блока копируют его. Как старый `applyPixelScale` (не среднее по блоку).

**Ordered:** порог Bayer на сетке блоков.

**ED:** квантуется только representative. Ядро пишет ошибку на **следующий блок** (`dx × pixel_size`, `dy × pixel_size`), иначе при `ps > 1` FS вырождается в nearest по блокам (плоские пятна). Serpentine считает чётность по ряду блоков (`gy / ps`).

Схема кросс-тайловых residual по-прежнему 2 колонки / 2 ряда: внутри тайла hop полный; на шве широкое ядро (JJN) × большой `ps` может обрезать overflow.

Проверка Simple + FS: `ps = 1` даёт мелкое зерно; `ps > 1` после фикса — зерно на сетке блоков.

---

## 8. Сводка «что на выходе»

```text
источник (linear)
    │
    ├─ нет палитры ── uniform levels + Bayer offset / ED
    │
    └─ есть палитра
           ├─ Strict  → Oklab two-nearest (ordered) / LUT nearest (ED) → свотч
           ├─ Guided  → per-channel quantize в range палитры → любой RGB в range
           ├─ Mixed   → Guided RGB → Strict two-nearest → свотч
           └─ Simple  → sRGB Euclidean (+ Bayer×64 / ED в байтах) → свотч
```

| | Strict | Guided | Mixed | Simple |
|---|---|---|---|---|
| Свотч палитры | да | нет | да | да |
| Пространство match | Oklab | linear channels | Oklab после Guided | sRGB u8 |
| Ordered | i1/i2 + T | квант каналов + T | Guided + i1/i2 + T | offset×64 + 1-nearest |
| ED residual | linear | linear (от guided) | linear (от свотча) | sRGB байты |
| Slider levels/ch | нет | да | да | нет |

---

## 9. Код

| Что | Где |
|---|---|
| Enum + serde default Strict | `crates/engine-project/src/filter.rs` — `PaletteDitherMode` |
| Guided quantize / ranges | `crates/engine-color/src/palette_guided.rs` |
| Ordered Strict / Mixed / Simple | `crates/engine-project/src/filters/dither_ordered.rs` |
| ED, residual, `pixel_size` hop | `crates/engine-project/src/filters/dither_diffusion.rs` |
| sRGB ↔ linear | `crates/engine-color/src/palette/mod.rs` |
| UI | `frontend/src/features/effects/editors/DitherSettings.tsx` |
