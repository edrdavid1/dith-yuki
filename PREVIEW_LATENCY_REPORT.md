# Отчёт: диагностика latency превью (2026-08-14)

Перезапуск замера:  
`cargo test -p dither --release preview_latency_diag -- --ignored --nocapture --test-threads=1`  
(и без `--release` для debug / `tauri dev`).

## Перемер после TICKET-0–4 (тот же день, тот же харнес)

Тот же `preview_latency_diag_3k`, тот же 3072×3072 / 1920×1080 / 8 воркеров.
TICKET-4 (coalesce) в этот харнес **не входит** — `simulate_update_filter` один invalidate на сценарий.
TICKET-2 (дедуп пирамиды) входит: `compute_composite_tile` / `enqueue_coarser_parent`.

| Сценарий | Было (wall / first) | Стало | `composite_ok` было → стало |
|---|---|---|---|
| Bayer origin 100% release | 14.4 / 1.2ms | **15.7 / 1.8ms** | 40 → 40 |
| FS origin 100% release | 54.0 / 4.9ms | **52.8 / 5.3ms** | 40 → 40 |
| Bayer far-corner release | 6.5 / 2.1ms | **5.8 / 1.9ms** | 16 → 16 |
| **FS far-corner release** | **250 / 178ms** (16 vis / 144 dirty) | **256.2 / 177.4ms** (dirty=144) | 16 → 16 |
| Bayer fit 25% release | 269 / 204ms | **102.2 / 100.3ms** | 1224–1634 → **189** |
| FS fit 25% release | 418 / 368ms | **162.5 / 154.7ms** | → **203** |
| FS far-corner debug | 2646 / 1958ms | **2940 / 2115ms** | 16 → 16 |
| Bayer fit 25% debug | 7472 / 4374ms | **843 / 781ms** | → **189** |
| FS fit 25% debug | 12315 / 3487ms | **1772 / 1699ms** | → **189** |

Wavefront FS farthest `(7,4)`: 53.1ms release / 563ms debug, 40/40 prefix — как раньше.

**Вывод для TICKET-5:** far-corner FS **не** был завышен дублями enqueue. 256ms ≈ 250ms — чистая цена ED-префикса до (0,0). Fit/zoom-out «секунды» были TICKET-2 (debug 12s → 1.8s; release 418 → 162ms). Origin FS по-прежнему ~53ms, компромисса не требует.

## Репро-кейс

- Файл **3072×3072**, один слой, один DitherV2, `pixel_size=1`, без палитры.
- Вьюпорт **1920×1080**, 8 воркеров (`available_parallelism`).
- macOS darwin 25.5.0 (Apple, 8 потоков).
- `DITHER_GPU`: default CPU; GPU-ветка отдельно с `DITHER_GPU=1` (адаптер есть, Metal).
- Замер: `invalidate(LayerFilterChanged)` + `schedule_dirty_viewport_tiles` + drain воркеров до свежих Composite видимых тайлов (эквивалент onChange→tile-ready **без** IPC/undo/frontend decode).
- Debounce **не** входит в wall-clock таблицы; к пользовательскому latency добавить **+100ms** после последнего mousemove.

`cargo bench -p engine-project` (`filter_bench`, `compositor_bench`) — **пустые placeholder'ы** (~280 ps, пустой `b.iter`). Референс стоимости фильтра — single-tile ниже, не criterion.

## Измерения

Видимые тайлы: origin 100% = **40** (level 0); far-corner 100% = **16** (level 0); fit 25% = **9** (level 2). Сетка L0 = **144**.

| Сценарий | Тайлов вьюпорта | Latency onChange→tile-ready (wall / first) | `update_filter` за жест 500ms |
|---|---|---|---|
| Bayer, CPU, origin 100%, **release** | 40 | **14.4ms / 1.2ms** | 1 (continuous drag) |
| Bayer, GPU (`DITHER_GPU=1`), origin 100%, release | 40 | **88.5ms / 4.9ms** | 1 |
| Floyd–Steinberg, CPU, origin 100%, release | 40 | **54.0ms / 4.9ms** | 1 |
| Bayer, CPU, far-corner 100%, release | 16 | 6.5ms / 2.1ms | 1 |
| FS, CPU, far-corner 100%, release | 16 vis / **144 dirty Processed** | **250ms / 178ms** | 1 |
| Bayer, CPU, fit 25% L2, release | 9 display / 144 L0 | **269ms / 204ms** | 1 |
| FS, CPU, fit 25% L2, release | 9 / 144 L0 | **418ms / 368ms** | 1 |
| Bayer, CPU, origin 100%, **debug** | 40 | **105ms / 16ms** | 1 |
| Bayer, GPU, origin 100%, debug | 40 | **203ms / 20ms** | 1 |
| FS, CPU, origin 100%, debug | 40 | **557ms / 45ms** | 1 |
| FS, CPU, far-corner, debug | 16 / 144 | **2646ms / 1958ms** | 1 |
| Bayer, CPU, fit 25%, debug | 9 / 144 | **7472ms / 4374ms** | 1 |
| FS, CPU, fit 25%, debug | 9 / 144 | **12315ms / 3487ms** | 1 |

Single-tile apply (тот же стек):

| | Bayer8x8 | Floyd–Steinberg | Bayer GPU |
|---|---|---|---|
| release | 0.7ms | 1.4ms | 6.4ms |
| debug | 12.6ms | 18.6ms | 12.2ms |

IPC (vitest, fake timers, `useEffectLayer`):

- 30 тиков × 16ms (500ms continuous) → **1** `update_filter` через 100ms после последнего тика.
- 4 тика с паузой 120ms → **4** вызова (каждый = полный `error_residuals.clear()` + invalidate всего слоя).

Wavefront (один воркер, тайл `(7,4)`):

- Bayer: 1.0ms release / 8ms debug, **1** Processed.
- FS: 52.8ms release / **554ms** debug, **40/40** Processed префикса.

## Где реально уходит время (по таймингам drain ≈ `tile_worker_loop`)

Instruments GUI не гонялся; проценты — из wall-clock тех же путей.

- **Вычисление пикселя (Bayer CPU):** origin 100% release ≈ весь бюджет (~14ms на 40 тайлов, 8 воркеров). Не узкое место на 3k+; debug ×18 (0.7ms → 12.6ms/тайл).
- **Рекурсия ED:** да. FS farthest = 40 тайлов на одном потоке (52.8ms ≈ 40×1.4ms). Far-corner: 16 видимых Composite тянут **все 144** L0 Processed. Debug: **2.6s**. `processed_calls=0` — Processed **не** в scheduler, а inline в `ensure_processed_tiles_fresh` → `compute_processed_tile_inner` (`tile_pipeline.rs:97–210`).
- **`submit_lock`:** да, для Bayer+GPU. Viewport 40 тайлов: GPU **6.1× медленнее** CPU (88.5 vs 14.4ms release). Один тайл GPU 6.4ms vs CPU 0.7ms. ED на GPU не ходит.
- **Ожидание очереди:** нет (воркеры заняты). Condvar не в профиле этого сценария.
- **IPC:** не в compute. +100ms debounce после отпускания / паузы. Continuous drag не флудит.
- **Пирамида L2 (отдельная находка):** fit 25% — `composite_ok` **1224–1634** при ~189 уникальных L0+L1+L2. Retry + `enqueue_coarser_parent` без дедупа. Debug Bayer fit **7.5s** / FS **12.3s** — это «секунды» на 3k+ при fit-to-view в `tauri dev`.

## Подтверждённые гипотезы

- [x] **ED wavefront — влияет.** Origin FS/Bayer ≈ 4× (54 vs 14ms release). Far-corner: Bayer 6.5ms vs FS **250ms** (debug **2.6s**), потому что dirty Processed=144 при 16 видимых. Данные: recurse left/top/diag в `tile_pipeline.rs:97–210`.
- [x] **Отсутствие debounce перед invalidate — не влияет на continuous drag; влияет на медленный жест.** Debounce **есть** (100ms, `useEffectLayer.ts:37,147`). 500ms drag → 1 IPC. Гипотеза «каждый тик доходит до backend» **опровергнута** для непрерывного движения. Но каждый прошедший IPC всё ещё делает `error_residuals.clear()` + `invalidate(LayerFilterChanged)` на **весь** слой (`commands.rs:1061–1152`), не на dirty viewport. Для ED это = пересчёт префикса до (0,0).
- [x] **GPU `submit_lock` — влияет (вредно) для Bayer; для ED нерелевантен.** `DITHER_GPU=1` на origin 100%: 88.5ms vs 14.4ms CPU. `dispatch.rs:107–110`. Default CPU — если репро без env, гипотеза 3 не объясняет FS/fit.

## Баг "иногда некорректно"

Воспроизведение гонки в UI не гонялось (нужен overlap двух `update_filter` пока wall > 100ms: debug FS/fit, или медленный drag). Механизм в коде однозначный:

1. `worker.rs:117` — **Composite layer 0 пропускает staleness-check** (комментарий: «always want the latest composite»).
2. `cache.rs:359–367` — `insert_fresh` всегда пишет `generation: 0`, без сравнения с текущим doc gen.
3. Медленный drag (паузы >100ms) или compute > debounce → два поколения Composite считаются параллельно; более медленный **старый** `insert_fresh` затирает новый → устаревший кадр / швы.

Это не «except Composite layer 0» как защита, а дыра: Processed discard'атся по gen, Composite — нет.

Пирамида усиливает: L2 может собрать box-filter из смеси старых/новых L0, пока дети догоняют (`tile_pipeline.rs:310–330` retry + `380–408` parent wake).

## Рекомендации (приоритизированные, effort/impact)

1. **Не оптимизировать фильтры, пока смотрим `tauri dev`.** Debug ×10–18 vs release. Fit 3k в debug = 7–12s; в release = 0.27–0.42s + 100ms debounce. Effort: S (мерить `--release` / профильный билд). Impact: объясняет симптом «секунды».
2. **Дедуп pyramid Composite** (`tile_pipeline.rs:291–330`, `enqueue_coarser_parent`): не enqueue, если уже queued/fresh; не считать L>0 повторно. Effort: M. Impact: H на zoom-out (1224+ лишних `composite_ok`; debug Bayer 7.5s).
3. **Staleness для Composite layer 0** (или generation на `insert_fresh`): discard/не записывать, если `task.generation != doc_gen`. Effort: S. Impact: H на «иногда некорректно». Сознательный trade-off в комментарии сейчас даёт stale blit.
4. **In-flight coalesce `update_filter`:** пока воркеры считают, копить последний params и один invalidate. Effort: S–M. Impact: M на медленный drag (сейчас 4 полных прохода за ~500ms).
5. **ED interactive:** во время слайдера — zero-seed на краю вьюпорта (швы ок), полный wavefront на pointerup; или row-major scheduler без дублирующей рекурсии. Effort: L (approx) / M (scheduler). Impact: H на FS+pan (16 vis → 144 L0). Корректный ED **нельзя** посчитать без префикса до (0,0).
6. **Не включать `DITHER_GPU=1` для этого сценария.** v1 lock+alloc на тайл медленнее пула CPU. GPU v2 — отдельный тикет (не трогать контракт).

Не делать сейчас: SIMD Bayer, in-place `PixelTile`, правки Oklab/LUT — single-tile 0.7ms не объясняет секунды.

## A/B антидиагональной ED-волны (TICKET-6)

Отдельный харнес `ed_prefix_ab`: оба порядка построения ED-префикса гоняются
**вперемешку в одном процессе**, 7 повторов, медиана. Причина — первый замер
двумя отдельными прогонами показал «ускорение» 256 → 154ms, но при этом
нетронутый Bayer fit 25% тоже упал 102 → 61ms, то есть сдвинулась база машины,
а не код.

| Сценарий | Рекурсия в глубину | Антидиагонали | ratio |
|---|---|---|---|
| **FS far-corner 100%** wall | 244.7ms | **111.0ms** | **2.20x** |
| **FS far-corner 100%** first visible | 175.7ms | **39.3ms** | **4.5x** |
| FS origin 100% wall | 54.7ms | 53.2ms | 1.03x |
| FS fit 25% wall | 127.9ms | 118.7ms | 1.08x |
| Bayer fit 25% wall (контроль) | 52.8ms | 53.7ms | 0.98x |

Контрольное плечо Bayer ≈ 1.00x — эффект действительно локализован в ED-пути.

Одиночный wavefront FS до `(7,4)` (40 тайлов, один воркер, без конкурентов):
**53 → 23ms**. Меньше, чем 2.20x, потому что диагонали у 8×5-префикса узкие
(12 диагоналей, средняя ширина 3.3); у 12×12 far-corner ширина в среднем 6.3.

Промежуточный результат, который стоит помнить: если **все** потоки берут
`try_lock` и проигравшие уходят в рекурсию, выигрыш всего 209 → 151ms (1.39x) —
восемь воркеров дублируют тот же префикс и вытесняют пул rayon. Ожидание
воркера на локе (и `try_lock` только для потоков rayon, чтобы координатор не
встал в дедлок за своими же задачами) даёт 2.20x.

**Вывод:** ED-префикс был не «неустранимой ценой алгоритма», а последовательным
проходом на одном воркере. TICKET-5 (approximate на драге) закрыт как не нужный.

## НЕ трогать без обсуждения

Инварианты ARCHITECTURE.md §13.8: `GlobalCoord` / `rem_euclid`, ED residuals+`corner`, LUT vs KD только на границах ячеек, GPU parity (Bayer exact; Halftone/CRT ≤ 1/255).
