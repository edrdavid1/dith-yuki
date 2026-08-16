# Задача агенту: диагностика latency превью на файлах 3k+ при движении слайдера

## Контекст

Симптом: на изображениях 3000+ px превью обновляется с задержкой в секунды при
изменении параметра фильтра ползунком, иногда с визуально некорректным
результатом (швы/устаревший кадр). Три рабочие гипотезы (не подтверждены,
нужно измерить, не чинить вслепую):

1. **ED wavefront** — error diffusion фильтры (FS/Atkinson/JJN/Stucki/Burkes/Sierra)
   рекурсивно тянут left/top/diag соседей на одном воркере, вьюпорт заливается
   с угла, а не с центра.
2. **Полный invalidate на каждый тик слайдера** — `error_residuals.clear()` +
   `invalidate(LayerFilterChanged)` сбрасывают весь документ, а не только
   видимую область, если нет debounce перед вызовом.
3. **GPU-путь мешает, а не помогает** — `submit_lock` сериализует воркеры,
   нет buffer pool, ED всё равно CPU-only — включённый `DITHER_GPU=1` может
   быть контрпродуктивен именно для этого сценария.

Задача — не чинить сходу, а **сначала измерить и локализовать**, потом
предложить план правок с оценкой effort/impact.

---

## Что смотреть (файлы/функции)

### 1. Тип фильтра в реальном сценарии
- `crates/engine-project/src/filters/apply.rs` — какой стек фильтров активен
  в репро-кейсе (Bayer/Ordered vs Error Diffusion vs Palette+ED).
- `crates/engine-project/src/filter.rs` — `requires_full_row` для используемых
  фильтров.

### 2. Путь инвалидации при изменении параметра
- `src-tauri/src/commands.rs` — `update_filter` (или аналог): что вызывается
  при каждом onChange от слайдера — есть ли debounce до `invalidate`, или
  каждый тик доходит до backend.
- Frontend: `frontend/src/features/effects/**` (`useEffectLayer` или где
  реализован debounce 100ms для undo) — применяется ли тот же debounce к
  IPC-вызову `update_filter`, или только к записи в undo-стек.
- `crates/engine-project/src/filters/gpu_bridge.rs` / invalidation logic —
  что именно инвалидируется: весь документ, слой, или только dirty viewport.

### 3. Диспетчеризация тайлов и воркер-пул
- `src-tauri/src/worker.rs` — `tile_worker_loop`: как забирается задача,
  когда происходит рекурсивный вызов `compute_processed_tile` для соседей.
- `src-tauri/src/tile_pipeline.rs` — `compute_processed_tile` /
  `compute_composite_tile`: ветка `requires_full_row` (рекурсия left/top/diag).
- `src-tauri/src/viewport.rs` — `set_viewport`: `scheduler.clear_all()` и
  порядок постановки в очередь (`ViewportCenter` перед `ViewportEdge`).

### 4. GPU-путь (только если включён/используется в репро)
- `crates/engine-gpu/src/dispatch.rs` — `submit_lock`, аллокация 4 буферов
  на тайл, `map_async` timeout.
- Проверить env `DITHER_GPU` в момент репро — включён или дефолтный CPU.

### 5. Кэш и staleness
- `crates/engine-tiles/` — `TileCache`, `evict_preserving_viewport`,
  generation/staleness check — конкретно для `Composite` на `layer 0`,
  где в документации указано исключение из staleness-check
  (`ARCHITECTURE.md` §13.5 / `TILE_PIPELINE.md` §8 Worker Loop, строка про
  "except Composite layer 0"). Это кандидат на баг "иногда некорректно".

---

## Как измерять (следуя методологии из ARCHITECTURE.md §13.8)

1. **Baseline**: один слой, Bayer, `pixel_size=1`, без палитры, zoom 100% —
   замерить wall-clock от onChange слайдера до `tile-ready` для видимых
   тайлов вьюпорта.
2. **Тот же кадр + `DITHER_GPU=1`** — сравнить wall-clock именно по
   вьюпорту (не по одному тайлу).
3. **Переключить на Floyd–Steinberg (или другой ED)** — замерить то же самое.
   Ожидание: скачок latency именно тут подтвердит гипотезу №1.
4. **Замерить количество IPC-вызовов `update_filter`** за один жест
   перетаскивания слайдера (например, за 500ms движения мыши) — подтвердит
   или опровергнет гипотезу №2 про отсутствие debounce.
5. `cargo bench -p engine-project` (`filter_bench`, `compositor_bench`) —
   как referens для чистой стоимости фильтра без IPC/scheduler overhead.
6. Profiler (Instruments на macOS / аналог на др. ОС) на `tile_worker_loop`
   во время воспроизведения — где именно поток проводит время: в вычислении
   пикселя, в ожидании `submit_lock`, в рекурсии на соседа, или в ожидании
   задачи из очереди.

---

## Формат отчёта

Отчёт должен быть коротким и содержать **измерения**, а не только выводы.

```markdown
# Отчёт: диагностика latency превью (дата)

## Репро-кейс
- Разрешение файла, стек фильтров, включён ли DITHER_GPU, платформа/ОС.

## Измерения
| Сценарий                        | Кол-во тайлов вьюпорта | Latency onChange→tile-ready | Кол-во update_filter вызовов за жест |
|----------------------------------|------------------------|------------------------------|----------------------------------------|
| Bayer, CPU                       |                         |                               |                                        |
| Bayer, GPU (DITHER_GPU=1)        |                         |                               |                                        |
| Floyd–Steinberg, CPU             |                         |                               |                                        |

## Где реально уходит время (по профайлеру)
- % времени: вычисление пикселя / submit_lock / рекурсия ED / ожидание очереди / IPC.

## Подтверждённые гипотезы
- [ ] ED wavefront — влияет / не влияет (данные)
- [ ] Отсутствие debounce перед invalidate — влияет / не влияет (данные)
- [ ] GPU submit_lock — влияет / не влияет (данные)

## Баг "иногда некорректно"
- Воспроизведён ли конкретный сценарий (какой стек, какое действие).
- Гипотеза причины (staleness-check exception для Composite layer 0 / race
  в порядке residuals / другое) — с указанием конкретной строки кода.

## Рекомендации (приоритизированные, с effort/impact)
1. ...
2. ...

## НЕ трогать без обсуждения
Инварианты из ARCHITECTURE.md §13.8: GlobalCoord/rem_euclid, ED residuals+corner,
LUT vs KD только на границах ячеек, GPU parity (Bayer exact; Halftone/CRT ≤ 1/255).
```

## Ограничения

- Не менять GPU-контракт (`engine-gpu`) без отдельного отдельного тикета —
  это Track D, v2 требует persistent buffers + снятия lock, это отдельная
  задача, не хотфикс.
- Не трогать фильтры/цвет, пока профиль не показал, что узкое место там.
- Если находка не укладывается в три гипотезы выше — фиксировать отдельно с
  конкретным file:line, не описывать словами "где-то там".
