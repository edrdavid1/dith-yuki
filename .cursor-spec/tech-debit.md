# Roadmap: порядок реализации (актуализировано по факту)

Сводка входного статуса (пять «скрытых зависимостей», проверено по коду):

| # | Тема | Статус |
|---|------|--------|
| 1 | Global coords | ✅ инфраструктура готова (`coords.rs`) |
| 2 | ED seams | ⚠️ ~70%, есть незакрытые баги |
| 3 | 3D LUT Oklab | ✅ Track B1 — `PaletteLut3D`/`PaletteLutCache`, default 64³ |
| 4 | GPU pipeline | ✅ Track D — `engine-gpu` Bayer/Halftone/CRT; ED CPU-only |
| 5 | Pixel-perfect zoom/snap | ✅ Track B2 — `zoomMode` integer/free + DPR snap |

Ниже — порядок работы с обоснованием зависимостей, а не по номерам из старого
списка. Треки A и B можно вести параллельно (разные люди/разное время). Трек C
стартует после закрытия трека A. Трек D (GPU) — строго последним.

Production-release хвост (H–P, C4.1, Color Lab §6): [RELEASE_TRACKS.md](./RELEASE_TRACKS.md).
Бета-гейт: [track-p-beta/](./track-p-beta/).

---

## Трек A — закрыть существующий correctness-долг (делать первым)

Спека: [track-a-correctness/](./track-a-correctness/)
([requirements](./track-a-correctness/requirements.md) ·
[design](./track-a-correctness/design.md) ·
[tasks](./track-a-correctness/tasks.md)).

Это не новая работа "с нуля" — это **завершение** уже выданных ранее задач,
которые остались открытыми:

### A1. Error Diffusion — закрыть до конца

> **Status (2026-08-11):** критерии закрытия закрыты в коде — IncomingErrorBuffer
> (`corner`), enforcement на всех levels, waiters helpers + N/A diagnosis,
> seam/Atkinson/level>0 тесты. См. [track-a-correctness/tasks.md](./track-a-correctness/tasks.md).

Уже есть `ErrorResidualsStore` и рекурсия left/top в `tile_pipeline.rs`, но
остались три конкретных незакрытых пункта (все уже были описаны ранее, но
не реализованы, только диагностированы):

1. **Silent-skip zero-seed без реинвалидации** — если raw соседа нет в кэше
   в момент обработки, residual молча становится 0, и ничего не помечает
   тайл dirty когда сосед догрузится. См. `TASK_fix_dither_seam_B.md` (уже
   выдана) — реализовать `pending_diffusion_waiters` + переинвалидацию по
   событию "raw tile loaded", как там описано. **Важно:** перед реализацией
   заново подтвердить триггер (Шаг 1 той задачи), поскольку в диагностике по
   гипотезе B уже выяснилось, что raw-тайлы level-0 в реальности не
   вытесняются из кэша — то есть эта ветка на практике может быть
   недостижима так же, как и в прошлый раз. Если снова недостижима — эту
   часть A1 можно закрыть как "не применимо, зафиксировано тестом-регрессии
   на будущее" и не тратить время на реализацию мёртвой ветки.
2. **Enforcement только на `level == 0`** — раскрыть на все pyramid levels
   (см. `TASK_fix_dither_seam_B.md`, Шаг 3). Это даёт швы на отдалении,
   актуально независимо от исхода пункта 1.
3. **Диагональная потеря ошибки в `distribute_fs`** — подтверждённая
   архитектурная проблема (см. `TASK_diagnose_diffusion_vs_frontend.md`,
   Путь 1). Именно она, по факту диагностики через тест FS/Bayer-переключение,
   похоже, была настоящей причиной шва на 1:1. Реализовать полноценный
   wavefront (диагональный обход) или `IncomingErrorBuffer`-модель, как
   описано в той задаче. Это самая объёмная часть A1 — закладывать на неё
   больше всего времени в оценке.

**Критерий закрытия A1:** тестовая матрица `dither_seam_matrix.rs` чистая
для FS/Atkinson на всех pyramid levels и на градиентном тесте (нет потери
яркости на границе).

### A2. pixel_size / block representative — закрыть до конца

> **Status (2026-08-11):** критерий закрытия зелёный — `dither_seam_matrix`
> all `ps × {Bayer, FS}` clean; Atkinson sample; BRC + dithered side-channel
> в apply path. См. [track-a-correctness/tasks.md](./track-a-correctness/tasks.md).

Уже выдана `TASK_block_representative_cache.md` — реализовать
`BlockRepresentativeCache`, посчитанный при декомпозиции (не через halo).
Плюс точечный фикс координат FS (`GlobalCoordSigned` вместо ручного
`tile_x + HALO`).

**Критерий закрытия A2:** матрица `pixel_size ∈ {1..32} × {Bayer, FS}`
полностью чистая (уже была частично: Bayer на степенях двойки был чист и
раньше, теперь чистым должно быть всё).

**Почему A целиком — первый трек:** каждая новая фича из Phase 1 (CMYK
Halftone, Wave Dither, CRT) будет писаться поверх тех же координатных
примитивов и, вероятно, будет поддерживать свой `pixel_size`. Если
`BlockRepresentativeCache` и wavefront-diffusion не закрыты сейчас, новые
фильтры либо унаследуют те же баги, либо потребуют повторного рефакторинга
сразу после написания. Дешевле закрыть один раз в общем месте.

---

## Трек B — независимая инфраструктура (можно параллельно с A)

Спека: [track-b-infra/](./track-b-infra/)
([requirements](./track-b-infra/requirements.md) ·
[design](./track-b-infra/design.md) ·
[tasks](./track-b-infra/tasks.md)).

### B1. 3D LUT Oklab (Phase 2.1 из исходного плана)

> **Status (2026-08-11):** закрыто — `PaletteLut3D` + `PaletteLutCache` (default
> size=64), hot path quantize/ordered/diffusion, bench + docs. См.
> [track-b-infra/tasks.md](./track-b-infra/tasks.md) §1.5.

Сейчас production hot path использует O(1) `PaletteLut3D::nearest_index`;
`PaletteKdCache` / `KdTree` остаются для build LUT и тестов.

**Файлы:** `crates/engine-color/src/palette_lut.rs`,
интеграция в `palette_quantize.rs`, `dither_ordered.rs`, `dither_diffusion.rs`,
`apply.rs`, `AppState.palette_lut_cache`.

```rust
pub struct PaletteLut3D {
    // 32x32x32 (или 64^3, оценить компромисс память/точность на реальной
    // палитре — крупная палитра с близкими цветами может требовать 64^3,
    // чтобы не терять точность на границах ячеек)
    grid: Vec<u16>, // индекс в palette.colors, плоский массив size^3
    size: u32,
    l_range: (f32, f32), // диапазон Oklab L, обычно [0,1]
    a_range: (f32, f32), // обычно [-0.4, 0.4]
    b_range: (f32, f32),
}

impl PaletteLut3D {
    pub fn build(palette: &Palette, size: u32, kdtree: &KdTree) -> Self {
        // Для каждого узла сетки size^3: перевести индекс узла в точку
        // Oklab (центр ячейки), найти ближайший цвет через уже существующий
        // KdTree::nearest (переиспользовать, не писать поиск заново),
        // записать индекс. Это O(size^3 * log K), но считается один раз
        // при построении/изменении палитры, не на каждый пиксель.
        todo!()
    }

    #[inline]
    pub fn nearest_index(&self, lab: Oklab) -> u16 {
        // Oklab -> индексы сетки (clamp на границы range), прямой lookup,
        // без обхода дерева.
        todo!()
    }
}
```

**Кэширование:** аналогично `PaletteKdCache` — `PaletteLutCache: DashMap<PaletteId, (revision, Arc<PaletteLut3D>)>`,
пересобирается при несовпадении `revision` (та же инвалидация, что уже
работает для KD-tree — переиспользовать существующий триггер, не изобретать
новый).

**Что заменить:** в `palette_quantize.rs` и в diffusion/dither с
`palette_id: Some(...)` — заменить вызов `kdtree.nearest(oklab)` на
`lut.nearest_index(oklab)`. Оставить `KdTree` как есть (используется для
построения LUT и как fallback для точных случаев/маленьких палитр, если
`size^3` для очень больших палитр окажется невыгодным — принять решение по
факту бенчмарка).

**Тесты:** сравнить результат `PaletteLut3D::nearest_index` с
`KdTree::nearest` на случайной выборке точек Oklab — расхождения должны
быть только на границах ячеек сетки (соседний по расстоянию цвет с почти
равной дистанцией), не системная ошибка. Бенчмарк: время квантизации
большого холста до/после — ожидается заметное ускорение на палитрах
среднего/большого размера.

### B2. Pixel-Perfect Integer Zoom & Snap (Phase 3.1)

> **Status (2026-08-11):** закрыто — `zoomMode` free/integer (default free),
> gesture-end snap, DPR tile draw snap, UI toggle «Integer zoom». Screenshots
> attach at PR. См. [track-b-infra/tasks.md](./track-b-infra/tasks.md) §2.

**Файлы:** `frontend/src/hooks/useViewport.ts`,
`frontend/src/features/preview/TileCanvas.tsx`,
`frontend/src/features/preview/zoomSnap.ts`,
`frontend/src/components/PreviewWindow.tsx`.

Реализовано: integer snap на wheel-idle (~120ms), floor-to-fit,
DPR `snapTileDrawRect`, Canvas2D + `imageSmoothingEnabled = false`,
accessible toggle рядом с zoom controls.

---

## Трек C — новые фильтры Phase 1 (после закрытия трека A)

Спека: [track-c-phase1-filters/](./track-c-phase1-filters/)
([requirements](./track-c-phase1-filters/requirements.md) ·
[design](./track-c-phase1-filters/design.md) ·
[tasks](./track-c-phase1-filters/tasks.md)).

Делать **после** A1/A2, не раньше — иначе унаследуют те же классы багов
(halo-clamp, диагональная потеря ошибки для любых будущих diffusion-подобных
эффектов). Использовать `GlobalCoord`/`GlobalCoordSigned` (готово),
`BlockRepresentativeCache` (из A2) и, если применимо, `PaletteLut3D` (из B1)
с самого начала — не писать координаты или block-логику вручную повторно.

### C1. CMYK Halftone
> **Status (2026-08-12):** закрыто — `DitherModeV2::CmykHalftone`, screen math +
> ordered apply, UI, `phase1_pattern_seam`. См. [track-c-phase1-filters/tasks.md](./track-c-phase1-filters/tasks.md).

`crates/engine-project/src/filters/dither_ordered.rs` (расширение файла).
Формулы — как в исходном плане (аффинный поворот под угол растра на канал,
расстояние до центра ячейки). Координаты — через `GlobalCoord::aligned`.

### C2. Wave / Line Modulation Dither
> **Status (2026-08-12):** закрыто — `DitherModeV2::Wave` + wave_* params, seam test.

Тот же файл. Пороговая функция `T(x,y) = 0.5 + 0.5*sin(...)` — координаты
опять через `GlobalCoord`, не считать вручную.

### C3. Glow и CRT
> **Status (2026-08-12):** закрыто — `filters/glow.rs`, `filters/crt.rs`, UI,
> CRT seam unit test; Glow radius ≤ HALO.

`crates/engine-project/src/filters/glow.rs`, `crt.rs` (новые файлы).
CRT scanlines используют `Y_g` — обязательно через `GlobalCoord`, не через
ручной `tile_y * 256 + local_y` (это именно тот паттерн, который дал баг в
FS ранее — не повторять).

**Тесты для всех трёх:** тест на непрерывность паттерна через границу
тайлов (по образцу теста из `coords.rs`), плюс визуальная регрессия на
2×2-тайловом холсте.

### C4. SVG Export (независим от C1-C3, можно в любой момент этого трека)
> **Status (2026-08-12):** закрыто — `engine-io::svg_export` (meshing + contour),
> sandbox `resolve_export_path`, Tauri `export_image` format SVG + save dialog.

`crates/engine-io/src/svg_export.rs` — Greedy Meshing и Contour Tracing как
в исходном плане. Не зависит от диффузии/halftone, можно делать раньше или
параллельно с C1-C3 если есть свободные руки.

---

## Трек D — GPU pipeline (строго последним)

> **Status (2026-08-12):** `engine-gpu` + AppState `gpu`; Bayer/Halftone/CRT WGSL;
> `DITHER_GPU` / `DITHER_FORCE_CPU`; Glow deferred CPU-only. Spec folder above.

Спека: [track-d-gpu/](./track-d-gpu/)
([requirements](./track-d-gpu/requirements.md) ·
[design](./track-d-gpu/design.md) ·
[tasks](./track-d-gpu/tasks.md)).

Не начинать, пока:
- Трек A закрыт (стабильный CPU error diffusion — хотя ED и останется
  CPU-only согласно исходному разделению задач, стабильность нужна как
  референс для всего остального);
- Хотя бы Bayer, CMYK Halftone и CRT (из трека C) существуют на CPU и
  покрыты тестами — без них не с чем сверять вывод WGSL-шейдера.

Дизайн — как в исходном плане: `crates/engine-gpu`, `wgpu::Device/Queue` в
`AppState`, workgroup 16×16 на тайл 256×256. Дополнительно к исходному
плану: **глобальные координаты тайла передавать в шейдер как uniform**
(`tile_offset: vec2<u32>`), чтобы WGSL-версии Bayer/Halftone/CRT
воспроизводили ту же бесшовность, что и CPU-версии на `GlobalCoord` — иначе
GPU-путь тихо вернёт версию с швами, которые уже были закрыты на CPU в
треке A/C, и придётся чинить их второй раз в шейдерном коде.

**Порядок портирования внутри трека D:** начинать с Bayer (самый простой,
уже полностью бессшовный и протестирован) как pilot — проверить всю
инфраструктуру sync/staging/map_async на нём, прежде чем портировать
Halftone/CRT/Glow.

---

## Трек E — `.dyproj` (project persistence) + shared embed

> **Status (2026-08-12):** E0–E5 реализованы в коде — `engine-project::serialize`
> (archive/assets/migrate/document_dto/id_remap/pixels/project), Tauri
> `save_project` / `save_project_as` / `open_project`, File menu, soft size warn,
> `.dyproj` file association stub. Track F может брать E0 API.

Спека: [track-e-dyproj/](./track-e-dyproj/)
([requirements](./track-e-dyproj/requirements.md) ·
[design](./track-e-dyproj/design.md) ·
[tasks](./track-e-dyproj/tasks.md)).

Бриф: [BRIEF_dyproj_dyuki.md](./BRIEF_dyproj_dyuki.md).

Независим от A–D. **E0** (общий zip + threshold-map embedding) — единственный
блокер для трека F. После E0 можно вести E1–E5 и F параллельно.

---

## Трек F — `.dyuki` (sharable patterns)

> **Status (2026-08-12):** F0–F3 реализованы — `serialize::pattern` pack/unpack,
> IPC `export_pattern` / `import_pattern`, File + EffectSettingsPanel UI,
> append-only import с always-new ids. F4 (OS association) — stub в `tauri.conf.json`.

Спека: [track-f-dyuki/](./track-f-dyuki/)
([requirements](./track-f-dyuki/requirements.md) ·
[design](./track-f-dyuki/design.md) ·
[tasks](./track-f-dyuki/tasks.md)).

Бриф: [BRIEF_dyproj_dyuki.md](./BRIEF_dyproj_dyuki.md) §3.

Стартует после **E0**; UI-скелет можно набрасывать раньше. Import всегда
создаёт новые палитры/фильтры и **append** в стек слоя (как Color Lab).

---

## Трек G — Welcome Screen (New / Open / Recent)

> **Status (2026-08-13):** G0–G5 реализованы — `recent_files.rs` + `get_recent_files`,
> `create_document` (shared `MAX_DOCUMENT_DIMENSION=8192`), Welcome в `EmptyState`
> (PreviewFeature empty + fill), `NewProjectDialog`, File → New Project… / Open Recent,
> один `useWelcomeScreen` на окно. Blank create **не** пишется в Recent.

Спека: [track-g-welcome/](./track-g-welcome/)
([requirements](./track-g-welcome/requirements.md) ·
[design](./track-g-welcome/design.md) ·
[tasks](./track-g-welcome/tasks.md)).

Бриф: [TASK_welcome_screen.md](./TASK_welcome_screen.md).

Независим от A–D и F. Нужен Track E (`open_project` / `save_project`) —
уже в дереве. Расширяет существующий `EmptyState` (слот в `PreviewFeature`),
персистентность Recent по образцу `panel_persistence.rs`, blank document
через тот же `decompose_image_to_tiles`, что `load_image`.

---

## Итоговый порядок (коротко)

1. **A1 + A2** (закрыть существующий correctness-долг) — параллельно друг
   с другом, оба блокируют трек C.
2. **B1 + B2** — в любой момент, параллельно с A, ничем не блокируются.
3. **C1-C4** — после A.
4. **D** — после C (минимум Bayer+Halftone+CRT существуют и стабильны).
5. **E + F** — независимо от A–D; F после E0, далее ∥ с остатком E.
6. **G** — ✅ (2026-08-13) Welcome / Recent / `create_document`; после E.
7. **K / J / H§Bias / L / C4.1 / Color Lab §6** — параллельно, без гейта A.
   Карта: [RELEASE_TRACKS.md](./RELEASE_TRACKS.md).
8. **M** (ED-ядра, затем Serpentine) и **I** (per-filter blend) — A1 уже
   закрыт; Serpentine не смешивать с ядрами в одном PR.
9. **H§Angle** — A2 уже закрыт; порядок операций см. ROADMAP §2.
10. **N** — Undo/Redo; после K (debounce уже в дереве); независимо от H–M.
11. **P** — бета-гейт (dirty / Guard / Apply-replace / QA). P1 до O3.
    [track-p-beta/](./track-p-beta/). C4.1 и Color Lab §6 параллельно.
12. **O** — in-app updates; Beta 1. Dirty из P смягчает Restart_Guard.

---

## Трек H — Bayer Threshold Bias + Angle

Спека: [track-h-bayer-params/](./track-h-bayer-params/).
Источник: [ROADMAP_production_release.md](./ROADMAP_production_release.md) §2.

Bias — сразу. Angle — после A2 (закрыт): rotate паттерна **после**
`BlockRepresentativeCache`, не до.

**GPU (v1):** CPU path — source of truth. `try_ordered_bayer_gpu` skips when
`threshold_bias != 0` or `pattern_angle != 0`. Halftone GPU skips non-zero
bias. Shader uniforms — D follow-up, not this track.

## Трек I — Per-filter Opacity / Blend

> **Status (2026-08-13):** закрыт — `opacity`/`blend_mode` на `FilterInstance`
> (serde default 1.0 / Normal); `apply_filter_with_blend` после full apply;
> residual ED не читает opacity; UI Slider + blend select; DnD без второго стека.

Спека: [track-i-filter-blend/](./track-i-filter-blend/).
Источник: ROADMAP §3. Обёртка в `apply.rs`, residual по полной диффузии.
`reorder_filter` + DnD в LayersPanel уже есть — не писать второй стек.

## Трек J — Glitch correctness
> **Status (2026-08-13):** закрыто — `GlobalCoordSigned` + mix(seed, gx, gy, level),
> offset ≤ HALO, dest Block Displace = pre copy, 2×2 seam tests, seed → NumberInput.

Спека: [track-j-glitch/](./track-j-glitch/).
`FilterKind::Glitch` / XorShift64 / seed в params уже есть. Долг: локальные
координаты и clamp 0..259. Сдвиг v1 ≤ HALO (как Glow radius).

## Трек K — Unified Slider / NumberInput

> **Status (2026-08-13):** закрыто — `NumberInput` + compact для curve points;
> debounce 100ms только в `useEffectLayer`; editors без сырых number/range.

Спека: [track-k-slider/](./track-k-slider/). Делать рано: новые панели H/J/M
не должны садиться на сырой `<input>`. `Slider.tsx` и debounce 100ms в
`useEffectLayer` уже есть — довести контракт компонента.

## Трек L — 3D Oklab palette volume

Спека: [track-l-oklab-volume/](./track-l-oklab-volume/).
Источник: [ADDENDUM_release_plan_L_C4.md](./ADDENDUM_release_plan_L_C4.md).
Конверсия только Rust (`oklab.rs`); фронт не дублирует.

## Трек C4.1 — SVG export follow-up

Спека: [track-c4-svg-followup/](./track-c4-svg-followup/).
C4 v1 закрыт 2026-08-12 (`engine-io::svg_export`, holes out of scope, UI
всегда meshing). Аддендум: явный выбор режима, внутренние контуры, валидный SVG.

## Трек M — ED kernels + Serpentine (ROADMAP «G»)

Спека: [track-m-ed-kernels/](./track-m-ed-kernels/).
Буква G занята Welcome. Ядра (JJN/Stucki/Burkes/Sierra) → затем Serpentine
отдельным шагом. Риск Serpentine × wavefront: ROADMAP §1.

## Трек N — Undo / Redo (snapshot history)

Спека: [track-n-undo-redo/](./track-n-undo-redo/)
([requirements](./track-n-undo-redo/requirements.md) ·
[design](./track-n-undo-redo/design.md) ·
[tasks](./track-n-undo-redo/tasks.md)).

Бриф: [TASK_track_n_undo_redo.md](./TASK_track_n_undo_redo.md).

Snapshot `Arc<Document>` (не command/diff), `max_depth = 50`, одна обёртка
на все мутации `Document`, GC осиротевших `LayerId` (`evict_layer` —
первый per-layer эвикшен). Debounce undo = Track K 100ms в
`useEffectLayer`. Replace (`load_image` / `open_project` / `create_document`)
очищает оба стека.

## Трек O — In-app updates

Спека: [track-o-updates/](./track-o-updates/)
([requirements](./track-o-updates/requirements.md) ·
[design](./track-o-updates/design.md) ·
[tasks](./track-o-updates/tasks.md)).

`tauri-plugin-updater` + Minisign; один канал GitHub `latest.json`;
кастомный UI (Help / About); Restart_Guard до download; Too_New_File
(`.dyproj` / `.dyuki`) предлагает Check for Updates. Первый
updater-билд = `0.2.0` (0.1.0 ставится DMG один раз). Apple notarization
не блокер этого трека.

## Трек P — Beta product gate

Спека: [track-p-beta/](./track-p-beta/)
([requirements](./track-p-beta/requirements.md) ·
[design](./track-p-beta/design.md) ·
[tasks](./track-p-beta/tasks.md)).

Не новый движок: Dirty_Flag (`Arc::ptr_eq` Saved_Mark), один Unsaved_Guard
на close / New / Open / O3, Color Lab Apply = replace выбранной палитры,
ручной QA A §6.2 / D §5.3. Import Image as Layer — P3, только Beta 1
(после P2). C4.1 и Color Lab §6 остаются своими папками.

Beta 0 = P1+P2+P4 + C4.1 + §6. Beta 1 = Beta 0 + P3 + O.
