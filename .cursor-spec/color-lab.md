# Color Lab: auto-extract, встроенные палитры, ramps & harmonization

Основано на as-built описании (`color-and-palette-architecture.md` /
`ARCHITECTURE.md`) — не переизобретаем то, что уже есть (`generate_palette_from_layer`,
`PaletteKdCache`, `colorLabSlice`, `palettesSlice.lastCreatedId`), а достраиваем
поверх.

3D-облако палитры — отдельный [track-l-oklab-volume/](./track-l-oklab-volume/),
не эта страница. Вес chroma/contrast при extract — **задача 6** ниже (ROADMAP,
не отдельный трек). Color Lab **gap #1** (Apply всегда `add_palette`) и
Import Layer extract — [track-p-beta/](./track-p-beta/) P2 / P3, не эта
страница.

**Порядок задач ниже — по зависимостям, не по важности для продукта.** Задачи
1 и 2 полностью независимы от фундамента и друг от друга — можно делать
параллельно/в любом порядке. Задачи 4 и 5 требуют задачи 3 (OkLCH-фундамент) —
делать 3 до них, иначе придётся переписывать конверсии дважды. Задача 6
независима от 1–5 и от Track L.

---

## Задача 1 — Auto-extract палитры при добавлении файла

### Решение по UX (зафиксировано с пользователем)

- Триггер: **оба** случая — открытие/создание документа из изображения, И
  добавление нового слоя из файла (Import Layer). Open Image — эта задача.
  Import Layer as-built отсутствует (`addRasterLayer` = пустой raster);
  дожим — [track-p-beta/](./track-p-beta/) **P3** (Beta 1, после P2).
- Результат: новая палитра создаётся автоматически **и сразу становится
  активной** (auto-apply), без модалки-подтверждения.

### "Активная" — уточнение (нет готового понятия в архитектуре)

В текущей модели нет единого "активного документа-палитры" — палитра
привязывается через `palette_id` к конкретному фильтру (`PaletteQuantize`/
`DitherV2`). Поэтому "auto-apply" реализуется как:

1. Палитра создаётся через существующий путь `generate_palette_from_layer`
   → `add_palette` (тот же путь, что сейчас использует ручной Extract).
2. Её id кладётся в `palettesSlice.lastCreatedId` (поле уже существует).
3. Цвета и имя палитры подставляются в `colorLabSlice` драфт (как уже делает
   Extract сейчас).
4. **Новое:** `PaletteSelector.tsx` при создании/показе для фильтра без явно
   выбранного `palette_id` — дефолтится на `palettesSlice.lastCreatedId`,
   если он есть, вместо пустого/первого в списке. Если у слоя УЖЕ есть
   фильтр с палитрой (`PaletteQuantize`/`DitherV2` с непустым `palette_id`),
   не трогать его автоматически — переключать `palette_id` существующего
   фильтра без явного действия пользователя было бы неожиданным поведением
   (перекрасит уже настроенный дизеринг без спроса). Автовыбор работает только
   для **новых** фильтров, добавляемых после этого момента, и для UI Color Lab.

### Backend

1. Найти обработчик открытия документа из изображения (вероятно
   `open_document`/`create_document_from_image` или аналог в `commands.rs`)
   и обработчик импорта слоя из файла (`import_layer` или аналог).
2. После того как raw-тайлы нового слоя/документа синхронно легли в кэш
   (уже гарантировано декомпозицией, см. `decompose.rs`), вызвать тот же
   код, что стоит за командой `generate_palette` — переиспользовать функцию
   `generate_palette_from_layer` напрямую (не через отдельный IPC roundtrip,
   если уже внутри backend-обработчика импорта), с дефолтными параметрами
   извлечения.
3. **Дефолтные параметры извлечения:** взять те же значения по умолчанию,
   что сейчас стоят в `AutoExtractSection` (метод — median cut или k-means,
   число цветов) — не изобретать новые дефолты, посмотреть текущий UI-код
   компонента и продублировать значения constant'ой на backend (или, если
   проще, оставить извлечение с фронта — см. альтернативу ниже).
4. **Альтернатива проще backend-триггера:** если вызывать extraction из
   Rust-обработчика импорта неудобно (нет доступа к RTK-стору для обновления
   драфта), сделать триггер на фронте: подписаться на событие успешного
   `open_document`/`import_layer` (там где фронт уже получает подтверждение
   от IPC), и оттуда вызвать существующий thunk, который дергает
   `generate_palette` command — то есть просто **программно нажать ту же
   кнопку Extract**, которую пользователь нажимает руками, с дефолтными
   параметрами формы. Это меньше нового кода и переиспользует 100%
   существующей логики (включая заполнение драфта, `bumpVersion` и т.п.).
   **Рекомендация: делать так**, если нет веской причины идти через backend.

### Настройка (toggle)

Добавить переключатель в настройках/preferences: "Автоматически извлекать
палитру при добавлении изображения" (default: **включено**, раз пользователь
явно это запросил как основной сценарий). Хранить в тех же настройках
приложения, где лежат остальные UI-preferences (найти существующий
persisted-settings slice, не создавать новый механизм персистентности).
Без этого тумблера поведение будет навязанным для всех сценариев работы
(например, если человек добавляет референс-слой не для того, чтобы красить
им, а для трейсинга — лишняя палитра в списке каждый раз).

### Риск / известный смежный баг

Каждый auto-extract создаёт **новую** палитру (Apply/Extract сейчас всегда
`add_palette`, не replace — это уже задокументированный gap #1 в as-built).
При частом импорте слоёв список палитр документа будет расти без переиспользования.
Не чинить это в рамках текущей задачи — фикс: [track-p-beta/](./track-p-beta/)
**P2** (Apply replace). Import Layer (P3) не начинать до P2.

### Тесты

- Импорт нового документа из PNG → палитра появилась в `Document.palettes`,
  `lastCreatedId` указывает на неё, драфт Color Lab заполнен.
- Импорт слоя в существующий документ с уже настроенным `PaletteQuantize`
  на другом слое → палитра создана, но `palette_id` существующего фильтра
  **не изменился**.
- Toggle выключен → импорт не создаёт палитру автоматически.

---

## Задача 2 — Встроенные ретро-палитры (Game Boy, Apple, + расширяемо)

Не хардкодить только 2 палитры — сразу закладывать реестр, раз в
изначальном видении заявлены ещё CGA/EGA/C64/Pico-8/NES/Lospec (roadmap
на будущее), чтобы не переделывать структуру при добавлении следующих.

### Уточнение по "Apple" (нужно решить/подтвердить у продукта отдельно)

"Apple" неоднозначно — это может быть Apple II lo-res (6 цветов), Apple II
hi-res/double hi-res (16 цветов), или классический Macintosh System 1-bit/
4-bit палитра. Взять **Apple II 16-цветный (hi-res/lo-res расширенный)** как
наиболее узнаваемый "ретро Apple" вариант по умолчанию — но пометить в
PR/коммите явно, какой именно вариант зашит, чтобы продукт мог поправить
на этапе ревью, если имелся в виду другой набор.

### Backend

1. Новый модуль `crates/engine-color/src/palette/presets.rs`:

```rust
pub struct PalettePreset {
    pub id: &'static str,       // "gameboy", "apple2", ...
    pub name: &'static str,     // отображаемое имя
    pub colors_srgb: &'static [(u8, u8, u8)], // цвета в sRGB, как обычно
                                               // хранятся палитры на границе IPC
}

pub const BUILTIN_PRESETS: &[PalettePreset] = &[
    PalettePreset {
        id: "gameboy",
        name: "Game Boy",
        colors_srgb: &[(15, 56, 15), (48, 98, 48), (139, 172, 15), (155, 188, 15)],
    },
    PalettePreset {
        id: "apple2",
        name: "Apple II",
        colors_srgb: &[/* 16 цветов Apple II, свериться с эталонным
                          источником (напр. Lospec или официальная таблица
                          NTSC-артефактных цветов Apple II) перед тем как
                          зашивать — цвета Apple II специфичны из-за NTSC
                          color artifacting, недостаточно взять "на глаз" */],
    },
    // легко добавлять следующие пресеты сюда же
];

pub fn find_preset(id: &str) -> Option<&'static PalettePreset> {
    BUILTIN_PRESETS.iter().find(|p| p.id == id)
}
```

2. Tauri-команды в `commands.rs`:
   - `list_builtin_palettes()` → список `{id, name, preview_colors}` (не полные
     цвета, только для превью грида в UI, если нужно облегчить payload —
     или сразу все цвета, они маленькие, можно не оптимизировать).
   - `import_builtin_palette(preset_id: String)` → находит пресет, конвертирует
     sRGB→linear (переиспользовать существующую конверсию, которой уже
     пользуется `add_palette`), вызывает `add_palette` с именем пресета →
     новая палитра в документе, тем же путём что обычный import.

### Frontend

1. В `ImportExportSection` (или рядом) добавить секцию "Built-in retro
   palettes" — грид превью-свотчей (маленькие полоски цветов пресета) с
   названием, по клику — `import_builtin_palette(id)`, дальше то же самое
   поведение, что у обычного Import: новая палитра в документе + цвета в
   драфт (консистентно с уже описанным поведением Import в as-built).
2. Список пресетов получать через `list_builtin_palettes` (не хардкодить
   на фронте — единственный источник истины бэкенд, чтобы фронт и бэкенд не
   расходились при добавлении новых пресетов в будущем).

### Тесты

- `import_builtin_palette("gameboy")` → 4 цвета, корректная sRGB→linear
  конверсия (проверить конкретные ожидаемые linear-значения).
- Неизвестный `preset_id` → корректная ошибка, не паника.

---

## Задача 3 — OkLCH-фундамент (делать до задач 4 и 5)

### Зачем

Сейчас в `engine-color/src/oklab.rs` есть только Oklab (`L, a, b`). Ramps
(интерполяция без "проседания в грязный серый") и harmonization (вращение
по тону) обе оперируют в терминах **Lightness / Chroma / Hue**, то есть
цилиндрическим представлением Oklab — OkLCH. Без него обе фичи 4 и 5 будут
либо реализовывать одну и ту же конверсию `a,b → C,H` дважды в разных
местах (и рано или поздно рассинхронизируются), либо тащить работу с
`(a, b)` напрямую, что для harmonization (нужно "повернуть тон на X
градусов") менее естественно и легче наделать ошибок в тригонометрии.

### Что делать

В `crates/engine-color/src/oklab.rs` (или новый `oklch.rs` рядом) добавить:

```rust
pub struct OkLch {
    pub l: f32, // lightness, как в Oklab
    pub c: f32, // chroma = sqrt(a^2 + b^2)
    pub h: f32, // hue в радианах или градусах — определиться с конвенцией
                // и явно задокументировать в doc-комментарии, чтобы не
                // путать вызывающий код (рекомендация: радианы внутри
                // движка, конверсия в градусы только на границе UI/IPC)
}

impl From<Oklab> for OkLch {
    fn from(lab: Oklab) -> Self {
        let c = (lab.a * lab.a + lab.b * lab.b).sqrt();
        let h = lab.b.atan2(lab.a);
        OkLch { l: lab.l, c, h }
    }
}

impl From<OkLch> for Oklab {
    fn from(lch: OkLch) -> Self {
        Oklab {
            l: lch.l,
            a: lch.c * lch.h.cos(),
            b: lch.c * lch.h.sin(),
        }
    }
}
```

Плюс **gamut clipping** (уже упомянут в изначальном видении, нужен и для
ramps, и для harmony, т.к. оба генерируют цвета алгоритмически, а не берут
их из реального изображения — сгенерированный цвет может не влезать в sRGB):

```rust
/// Возвращает true, если LinRgb выходит за пределы [0,1] по любому каналу
/// (т.е. не представим в sRGB gamut).
pub fn is_out_of_srgb_gamut(rgb: LinRgb) -> bool { /* ... */ }

/// Простой clip: уменьшает Chroma при фиксированных L и H, пока цвет не
/// попадёт в gamut (бинарный поиск по C — более качественные методы типа
/// gamut mapping через CSS Color 4 алгоритм можно отложить, для первой
/// версии достаточно бинарного поиска по chroma).
pub fn clip_to_srgb_gamut(lch: OkLch) -> OkLch { /* ... */ }
```

### Тесты

- Round-trip: `Oklab → OkLch → Oklab` даёт исходные `a, b` в пределах
  погрешности float для набора тестовых цветов (включая ахроматичные, где
  `C ≈ 0` и `H` не определён — проверить что не паникует и не даёт NaN на
  `atan2(0, 0)`).
- `clip_to_srgb_gamut` для заведомо вне-гамутного OkLCH даёт цвет внутри
  `[0,1]` по всем каналам RGB, и не меняет `L`/`H` (только `C` уменьшается).

---

## Задача 4 — Ramps Generator (градиенты в Oklab)

Зависит от Задачи 3.

### Backend

Новая функция в `engine-color`, например `crates/engine-color/src/ramps.rs`:

```rust
/// Генерирует `steps` цветов, равномерно интерполированных между `from` и
/// `to` в пространстве Oklab (лёгкость и цветность идут по прямой в Lab,
/// не в LCH — интерполяция по кратчайшей дуге в LCH актуальна отдельно,
/// см. ниже про harmony, для ramps обычно линейная Lab-интерполяция даёт
/// более предсказуемый результат без "перекруток" тона).
pub fn generate_ramp(from: LinRgb, to: LinRgb, steps: usize) -> Vec<LinRgb> {
    let lab_from: Oklab = from.into();
    let lab_to: Oklab = to.into();
    (0..steps)
        .map(|i| {
            let t = i as f32 / (steps - 1).max(1) as f32;
            let interpolated = Oklab {
                l: lerp(lab_from.l, lab_to.l, t),
                a: lerp(lab_from.a, lab_to.a, t),
                b: lerp(lab_from.b, lab_to.b, t),
            };
            let lch: OkLch = interpolated.into();
            let clipped = clip_to_srgb_gamut(lch); // используем фундамент из Задачи 3
            Oklab::from(clipped).into() // обратно в LinRgb
        })
        .collect()
}
```

Команда `generate_ramp_palette(from_hex, to_hex, steps) -> Vec<ColorDto>` —
**не** создаёт палитру в документе автоматически (в отличие от Задачи 1) —
это исследовательский инструмент, пользователь крутит параметры и смотрит
превью, поэтому результат идёт **только в драфт Color Lab**, как ручной
ввод — тот же паттерн, что и обычное редактирование драфта. Палитра
создаётся, только когда пользователь жмёт Apply, как обычно.

### Frontend

Новая секция в Color Lab (например `RampGeneratorSection`): два цветовых
пикера (from/to — переиспользовать существующий `ColorPicker`), слайдер
количества шагов, живое превью полоски, кнопка "Insert into draft"
(добавляет/заменяет цвета в текущем `colorLabSlice` драфте).

### Тесты

- `generate_ramp(black, white, 5)` → 5 цветов, монотонно возрастающая
  lightness, без "провала" в середине (характерная проблема наивной sRGB
  интерполяции, которую и решает Oklab — тест должен явно проверять
  монотонность L, не только конечные точки).
- Ramp между двумя out-of-gamut-граничными цветами не даёт NaN/некорректных
  RGB значений на выходе (gamut clipping отрабатывает на каждом шаге).

---

## Задача 5 — Harmonization (цветовая гармония)

Зависит от Задачи 3.

### Backend

`crates/engine-color/src/harmony.rs`:

```rust
pub enum HarmonyRule {
    Monochromatic,
    Analogous,
    Complementary,
    Triadic,
    SplitComplementary,
}

/// Генерирует палитру на основе базового цвета и правила гармонии.
/// Вращения — по Hue в OkLCH, Lightness/Chroma базового цвета сохраняются
/// (кроме Monochromatic, где наоборот варьируется L при фиксированных C/H).
pub fn generate_harmony(base: LinRgb, rule: HarmonyRule, count: usize) -> Vec<LinRgb> {
    let base_lch: OkLch = Oklab::from(base).into();
    let hue_offsets: Vec<f32> = match rule {
        HarmonyRule::Complementary => vec![0.0, std::f32::consts::PI], // 180°
        HarmonyRule::Triadic => vec![0.0, 2.0 * std::f32::consts::PI / 3.0, 4.0 * std::f32::consts::PI / 3.0],
        HarmonyRule::Analogous => {
            // count равномерно распределённых узких смещений, напр. ±30°
            // сгенерировать динамически по count, не хардкодить фикс. набор
            todo!()
        }
        HarmonyRule::SplitComplementary => vec![0.0, std::f32::consts::PI - 0.5, std::f32::consts::PI + 0.5], // ~150°/210°
        HarmonyRule::Monochromatic => {
            // здесь варьируем L, а не H — отдельная ветка ниже
            todo!()
        }
    };

    match rule {
        HarmonyRule::Monochromatic => {
            (0..count)
                .map(|i| {
                    let t = i as f32 / (count - 1).max(1) as f32;
                    let l = lerp(0.15, 0.9, t); // разумный рабочий диапазон L, не 0..1 впритык к чёрному/белому
                    let lch = OkLch { l, c: base_lch.c, h: base_lch.h };
                    Oklab::from(clip_to_srgb_gamut(lch)).into()
                })
                .collect()
        }
        _ => hue_offsets
            .into_iter()
            .map(|offset| {
                let lch = OkLch { l: base_lch.l, c: base_lch.c, h: base_lch.h + offset };
                Oklab::from(clip_to_srgb_gamut(lch)).into()
            })
            .collect(),
    }
}
```

Команда `generate_harmony_palette(base_hex, rule, count) -> Vec<ColorDto>` —
тот же паттерн, что Ramps (Задача 4): результат в драфт, не auto-apply,
Apply создаёт палитру как обычно.

### Frontend

Секция в Color Lab: пикер базового цвета, выбор правила (radio/select:
Monochromatic / Analogous / Complementary / Triadic / Split-Complementary),
для Analogous — слайдер количества и/или ширины разброса, превью свотчей,
кнопка "Insert into draft" как у Ramps.

### Тесты

- Complementary от произвольного базового цвета → второй цвет имеет
  `H = base.H + π` (по модулю `2π`), тот же `L` и `C` (до gamut-клипа).
- Monochromatic → все цвета имеют одинаковые `C`/`H` (до клипа), различный `L`.
- Triadic → три цвета равномерно по кругу (120° друг от друга).

---

## Задача 6 — Auto-extract вес по Chroma / Contrast

Не отдельный трек: [ROADMAP_production_release.md](./ROADMAP_production_release.md)
и [RELEASE_TRACKS.md](./RELEASE_TRACKS.md). Минорное расширение
`crates/engine-color/src/palette/generate.rs`. Не зависит от задач 3–5 и от
Track L. Можно в любой момент.

Сейчас `generate_palette` / MedianCut / K-Means трактуют пиксели равномерно
(после subsample). На фото это даёт серые «средние» кластеры и пропускает
насыщенные акценты и кромки.

### Параметры (locked)

Добавить в generate API (и в IPC `generate_palette`, если он прокидывает
опции) два веса `0.0..=1.0`, default **0** = текущее поведение:

- `chroma_weight` — насколько чаще брать пиксели с высокой OkLCH/Oklab chroma
  `sqrt(a²+b²)` (через существующий `oklab.rs`; OkLCH модуль желателен, но
  не обязателен — chroma считается из Oklab).
- `contrast_weight` — насколько чаще брать пиксели с высоким локальным
  контрастом. v1: luminance vs mean of the **sample batch** (cheap, no extra
  neighbor pass). Neighbor-based Sobel is a follow-up, not MVP.

При обоих 0 результат SHALL быть bit-identical / set-identical к сегодняшнему
на том же subsample (regression test).

### Как применять вес

Не менять геометрию MedianCut-сплитов «тихо». v1: **weighted resample**
перед кластеризацией — построить дискретное распределение
`w = 1 + chroma_weight * chroma_norm + contrast_weight * contrast_norm`
и набрать `MAX_GENERATION_SAMPLES` (или текущий размер списка) с
replacement, детерминированно (фиксированный RNG seed **или** systematic
weighted stride — lock: **детерминированный systematic**: отсортировать по
кумулятивному весу и взять равномерные квантили, без RNG).

### UI

Слайдеры в `AutoExtractSection` (Track K `Slider`). Подписи Chroma / Contrast.
Default 0 so existing users don’t change extract until they opt in.

### Тесты

- `chroma_weight=0, contrast_weight=0` → same palette as current fixture.
- High chroma_weight on an image of gray field + one saturated patch →
  saturated color appears in a small K (e.g. K=4) more reliably than weight 0
  (assert presence of high-chroma swatch).
- Weights out of range → validate error.

---

## Итоговый чеклист приёмки всей фичи

1. Задача 1: импорт документа/слоя → палитра автоматически создана и
   выбрана как дефолт для новых фильтров; toggle отключает поведение;
   существующие фильтры не переключаются без спроса.
2. Задача 2: минимум Game Boy и Apple (с явной пометкой какой именно Apple)
   доступны через UI одним кликом, архитектура расширяема под будущие
   пресеты без изменения формата.
3. Задача 3: OkLCH конверсии и gamut clipping есть, покрыты тестами,
   задокументирована конвенция единиц Hue (радианы/градусы).
4. Задача 4: Ramps Generator даёт монотонные по L градиенты без провалов
   в серый, доступен в UI, не создаёт палитру документа сам по себе.
5. Задача 5: минимум 5 правил гармонии реализованы и покрыты тестами на
   корректность углов, доступны в UI, не создают палитру документа сами
   по себе.
6. Задача 6: веса chroma/contrast default 0 не меняют extract; ненулевой
   chroma-вес поднимает насыщенный акцент в маленькой палитре.