# Тикеты: preview latency + correctness (3k+ файлы)

Источник: `AGENT_TASK_preview_latency.md` + отчёт от 2026-08-14.
Порядок ниже — приоритет к исполнению, не порядковые номера тикетов.

Follow-up stale-200 / generation: `.kiro/specs/tile-generation-ready-protocol/`
(TICKET-7…11). Порядок: **E+C → п.6/TICKET-9 → A → замер D → F только по данным**.

---

## TICKET-0: Git blame / контекст по staleness-skip для Composite layer 0

**Статус:** done (2026-08-14)

**Тип:** Investigation (блокер для TICKET-1)
**Effort:** S
**Impact:** —

**Проблема:**
`worker.rs:117` — Composite layer 0 сознательно пропускает staleness-check
(комментарий «always want the latest composite»). Прежде чем убирать этот
skip (TICKET-1), нужно понять, почему он был добавлен — вероятно, чтобы
избежать другого класса багов (например, зависшего preview, если новый
generation-таск ещё не готов, а старый уже discard'нут).

**Что сделать:**
- Git blame на `worker.rs:117`, найти связанный commit/PR/issue.
- Проверить, есть ли тест, который ловит регресс, если skip убрать
  (`schedule dirty tiles`-related тесты).

**Acceptance:**
- Короткая заметка (2-3 абзаца) с причиной skip, приложена к TICKET-1.

**Находка:** skip появился в первом коммите пайплайна `7aff650` (2026-07-28,
«feat: tile-viewport rendering pipeline»), не в последующем багфиксе. Спека
(`.kiro/specs/tile-viewport-rendering`) требовала staleness для *всех* задач
(Property 14 / Req 10.5); исключение для Composite layer 0 написали сразу в
`worker.rs` как оптимизацию: compute читает свежий snapshot, поэтому «stale»
Composite якобы всё равно даёт актуальный кадр при быстром слайдере, и
`ensure_processed_tiles_fresh` подтянет Processed. Теста, который ломается
при снятии skip, нет.

Страх «зависшего preview» (новый gen ещё не в очереди, старый уже discard) —
короткое окно, не hang: `schedule_dirty_viewport_tiles` ставит задачи текущего
gen после increment. Постоянного сценария, ради которого skip надо сохранять,
нет. TICKET-1 снимает skip и добавляет `insert_fresh_gen`.

---

## TICKET-1: Исправить staleness race для Composite layer 0

**Статус:** done (2026-08-14)

**Тип:** Bug (correctness)
**Effort:** S
**Impact:** H — это источник симптома «превью обновляется иногда некорректно»
**Зависит от:** TICKET-0

**Проблема:**
`cache.rs:359–367` — `insert_fresh` всегда пишет `generation: 0` без
сравнения с текущим doc gen. В связке с пропуском staleness-check для
Composite layer 0 (`worker.rs:117`) это даёт гонку: при медленном drag
(паузы >100ms) или compute > debounce два поколения Composite считаются
параллельно, и более медленный **старый** результат может перезаписать
новый → устаревший кадр / швы на экране.

Усиливается пирамидой: L2 может собрать box-filter из смеси старых/новых
L0, пока дети догоняют (`tile_pipeline.rs:310–330`, `380–408`).

**Что сделать:**
- В зависимости от результата TICKET-0: либо добавить сравнение generation
  перед `insert_fresh` для Composite layer 0, либо другой guard, который не
  ломает тот сценарий, ради которого skip был добавлен изначально.

**Acceptance:**
- Тест: два быстрых последовательных `invalidate` с разными generation,
  Composite layer 0 в кэше в итоге соответствует последнему generation, не
  промежуточному.
- Regression: старый сценарий, из-за которого skip был добавлен (по данным
  TICKET-0), не сломан.

**Addendum (regression, 2026-08-14, спека `tile-generation-ready-protocol`):**
`insert_fresh_gen == false` не должен оставлять ключ «тихо готовым» к 200
при `Entry_Gen < live Doc_Gen`. Acceptance дополняется: отказ write → ключ
гарантированно dirty (или эквивалентный re-enqueue текущего gen). Это
закрывающий патч к TICKET-1, не отдельная perf-задача. Реализация — TICKET-9.

---

## TICKET-2: Дедуп pyramid Composite enqueue

**Статус:** done (2026-08-14)

**Тип:** Perf
**Effort:** M
**Impact:** H на zoom-out / fit-to-view

**Проблема:**
`tile_pipeline.rs:291–330`, `enqueue_coarser_parent` — при fit 25% (level 2)
наблюдалось 1224–1634 `composite_ok` при ~189 уникальных L0+L1+L2 тайлах.
Retry-логика энкьюит родительский тайл без проверки, что он уже
queued/fresh. Debug: Bayer fit 7.5s, FS fit 12.3s — это и есть «секунды»
на 3k+ при открытии/fit-to-view.

**Что сделать:**
- Перед enqueue родительского тайла в pyramid проверить, не в очереди ли он
  уже и не fresh ли он в кэше.
- Замерить `composite_ok` до/после — должно приблизиться к числу уникальных
  тайлов пирамиды.

**Acceptance:**
- Fit 25% release: latency origin→tile-ready заметно ниже текущих
  269ms (Bayer) / 418ms (FS); debug — не должен вылезать за секунды.
- `composite_ok` ≈ число уникальных L0+L1+L2 тайлов ±small margin.

---

## TICKET-3: Проверить и по умолчанию выключить `DITHER_GPU` для этого сценария

**Статус:** done (2026-08-14) — дефолт уже CPU, менять не пришлось. Тест
`dither_gpu_default_is_cpu` фиксирует, что unset / `"0"` / `"false"` не включают GPU.

**Тип:** Config / Perf
**Effort:** S (если это просто дефолт/env в текущей сборке)
**Impact:** H, если сейчас включено — 6.1× медленнее CPU на Bayer viewport
(88.5ms vs 14.4ms release, origin 100%)

**Проблема:**
GPU-путь v1: `submit_lock` сериализует все воркеры в одну очередь,
буферы аллоцируются заново на каждый тайл (нет пула). Для viewport с
несколькими десятками тайлов CPU-пул почти всегда выигрывает. ED на GPU
не считается вообще.

**Что сделать:**
- Проверить текущий дефолт `DITHER_GPU` в билдах, которые видит пользователь
  (dev / staging / prod).
- Если где-то стоит `=1` по умолчанию — убрать / вернуть на CPU-default.

**Acceptance:**
- Дефолтная конфигурация — CPU, если явно не задокументировано иное решение.
- GPU v2 (buffer pool, без глобального lock) — отдельный тикет, не трогать
  контракт `engine-gpu` здесь.

---

## TICKET-4: Coalesce `update_filter` при in-flight compute

**Статус:** done (2026-08-14)

**Тип:** Perf
**Effort:** S–M
**Impact:** M — актуально для медленного drag / debug-сборок, где compute
дольше 100ms debounce

**Проблема:**
Debounce (100ms, `useEffectLayer.ts:37,147`) корректно схлопывает continuous
drag в 1 IPC-вызов. Но каждый прошедший `update_filter` вызывает
`error_residuals.clear()` + `invalidate(LayerFilterChanged)` на весь слой
(`commands.rs:1061–1152`), а не на dirty viewport. При паузах >100ms (или
медленном compute) — 4 тика за 500ms дали 4 полных прохода.

**Что сделать:**
- Пока предыдущий invalidate/compute ещё не завершён, копить последние
  params вместо постановки нового полного прохода; применить один раз по
  готовности.

**Acceptance:**
- Сценарий "4 тика с паузой 120ms" даёт меньше 4 полных `error_residuals.clear()`
  прохода без потери итогового корректного результата.

---

## TICKET-6: ED-префикс антидиагональной волной (закрывает TICKET-5)

**Статус:** done — exact, без продуктового компромисса.
**Тип:** Perf
**Effort:** M
**Impact:** H на ED далеко от origin: far-corner FS **245 → 111ms** wall,
первый видимый тайл **176 → 39ms** (A/B в одном процессе, 7 повторов).

**Находка.** `processed_calls=0` во всех сценариях: Processed никогда не идёт
через scheduler. `compute_processed_tile_inner` строил ED-префикс инлайн-
рекурсией в глубину, то есть весь префикс считался на одном воркере, пока
остальные семь простаивали. 144 тайла × 1.9ms ≈ 245ms — измеренный wall почти
равен последовательной сумме. Контроль: Bayer на 16 независимых тайлах получал
от пула ~3x, то есть воркеров хватало, сериализовала структура рекурсии.

**Решение.** Тайлы с одинаковым `x + y` зависят только от тайлов с меньшей
суммой (left / top / diag), поэтому каждая антидиагональ считается параллельно,
а сами диагонали остаются упорядоченными. Множество тайлов и гарантии
топологического порядка те же, что у рекурсии в глубину, поэтому результат
байт-в-байт идентичен — pixel-perfect не затронут.

`ed_prefix_lock` (`AppState`): координатор один. Воркер-поток **ждёт** на
локе — дублировать префикс рекурсией дороже простоя и вытесняет пул rayon,
который нужен координатору. Поток rayon никогда не ждёт (`try_lock`), иначе
координатор упирается в дедлок за собственными диагональными задачами. Эта
разница и дала основной выигрыш: с `try_lock` для всех было 209 → 151ms
(1.39x), с ожиданием воркеров — 245 → 111ms (2.20x).

`DITHER_ED_SERIAL_PREFIX=1` — kill switch на старый порядок и единственный
способ сделать A/B внутри одного процесса.

**Порог** `ED_PARALLEL_PREFIX_MIN = 8`: ниже rayon-барьеры дороже рекурсии.
Origin-100% FS (префикс покрыт видимыми) не меняется — 54.7 → 53.2ms, 1.03x.

**Тесты:**
- `ed_diagonal_prefill_matches_row_major_reference` — байт-в-байт против
  строго последовательного row-major прохода (валидный топологический порядок).
- `ed_diagonal_prefill_is_exact_under_concurrent_coordinators` — 4 потока на
  один префикс, смесь координатора и `try_lock`-проигравших, тот же результат.
- `ed_prefill_skips_small_prefixes` — порог не срабатывает на 3 тайлах.
- `ed_prefix_ab` (`--ignored`) — A/B двух порядков вперемешку, с контрольным
  Bayer-плечом (0.98x = харнес не врёт).

---

## TICKET-5: ED interactive mode (zero-seed на драге) — ЗАКРЫТ, не делаем

**Статус:** closed / won't do — TICKET-6 снял причину.
**Тип:** Perf / UX trade-off

Тикет существовал ради far-corner FS = 256ms. После TICKET-6 первый видимый
тайл там **39ms**, wall 111ms — это уже не тот класс задержки, ради которого
имеет смысл жертвовать pixel-perfect.

Почему закрываем, а не откладываем:

- Пользователь чувствует именно первый видимый тайл: 39ms — ниже порога
  «мгновенно», приближённый кадр там нечего улучшать.
- Approximate завёл бы **третье** временное состояние кадра рядом с
  generation-корректностью (TICKET-1) и coalesce (TICKET-4) — ровно тот класс
  гонки, который уже один раз чинили. Платить этим за ~70ms в неосновном
  сценарии невыгодно.
- TICKET-5b (индикатор approximate на фронте) отпадает вместе с 5a: кадр
  всегда exact, показывать нечего.
- TICKET-6 ускорил **все** ED-проходы, включая commit и экспорт, а не только
  драг.

Возвращаться к этому тикету, только если новый профиль покажет ED-сценарий с
первым видимым тайлом > ~150ms в release.

---

## TICKET-7: Protocol Ready — never stale 200 (E)

**Статус:** done (2026-08-14)
**Тип:** Bug (correctness)
**Effort:** S
**Impact:** H — корень «тайлы залипают на прошлых значениях»
**Спека:** `.kiro/specs/tile-generation-ready-protocol/`
**Порядок:** первый, вместе с TICKET-8

**Проблема:**
`handle_tile_request` отдаёт 200 если entry есть и `!dirty`, без сравнения
`CacheEntry.generation` с live `document_gen`. После TICKET-1 старый compute
может быть отброшен, а в кэше остаться clean кадр прошлого gen → Stale_200.
Canvas считает тайл обновлённым.

**Что сделать:**
Ready = `!dirty && generation >= doc_gen`. Иначе 202 + enqueue текущего gen.
На 200 — заголовок `X-Tile-Generation`.

**Acceptance:**
- Тест: gen 1 в кэше, doc_gen 2, !dirty → не 200.
- Тест: gen == doc_gen, !dirty → 200.
- Dirty при любом gen → 202.

---

## TICKET-8: Client commit only non-decreasing rev (C)

**Статус:** done (2026-08-14)
**Тип:** Bug (correctness)
**Effort:** S
**Impact:** H — та же инвариантная проблема, что TICKET-7
**Зависит от:** делать в одном PR с TICKET-7
**Спека:** `.kiro/specs/tile-generation-ready-protocol/`

**Проблема:**
`?g=` уже есть как cache-bust, но `tile-decoded` не несёт `rev`, и canvas
может закоммитить ответ на предыдущий жест слайдера.

**Что сделать:**
`rev` в `tile-decoded`; drop если `rev < tileRevRef.current`; на bump rev
выбросить stale pending bitmaps.

**Acceptance:**
- Vitest: bitmap со старым rev не попадает в displayed/pending.

---

## TICKET-9: insert_fresh_gen reject → dirty + reschedule (TICKET-1 hole)

**Статус:** done (2026-08-14)
**Тип:** Bug / regression (addendum TICKET-1)
**Effort:** S
**Impact:** H — вечный 202 или вечный stale, если write отброшен и никто не
считает текущий gen
**Зависит от:** TICKET-7 (протокол тогда честно скажет 202; этот тикет
гарантирует, что текущий gen появится)
**Спека:** `.kiro/specs/tile-generation-ready-protocol/` Requirement 3

**Проблема:**
Цель TICKET-1 достигнута (stale не перезаписывает newer). Дыра: `false`
оставляет кэш как был. Если live Doc_Gen уже впереди — нет dirty/enqueue
текущего gen.

**Что сделать:**
При `insert_fresh_gen == false`: если live Doc_Gen > cached generation —
`mark_dirty` + `enqueue_dedup` текущего gen. Если live == cached — не трогать.

**Acceptance:**
- Тест: insert gen 2, reject gen 1, live → 3 ⇒ dirty и задача gen 3.
- Дописать acceptance в TICKET-1 (addendum выше).

---

## TICKET-10: Batch commit timeout (A)

**Статус:** done (2026-08-14)
**Тип:** UX safety valve
**Effort:** S
**Impact:** M — ограничивает, как долго один тайл держит весь кадр
**Спека:** `.kiro/specs/tile-generation-ready-protocol/` Requirement 4

**Проблема:**
`shouldCommitTileRefresh` ждёт полный набор. Один недоехавший тайл = весь
кадр на старом фильтре.

**Почему вторым, не вместо E+C:** без C таймаут закоммитит смесь актуальных и
Stale_200 — тихая порча вместо явного залипания (класс TICKET-1).

**Что сделать:**
После ~100ms коммитить все pending с current rev; остальные ключи оставить
старый bitmap. Stale rev по-прежнему drop.

**Acceptance:**
- Частичный current-rev набор коммитится по таймауту.
- `rev < current` никогда не коммитится.

---

## TICKET-11: Measure 202 retry cap (gate for D) — не код

**Статус:** open after TICKET-10
**Тип:** Investigation
**Effort:** S
**Impact:** —

Open-ended ретраи (D) сознательно откладывались в духе TICKET-5: хвост
зомби без cancel на новый жест. После E+C+A замерить, как часто release
упирается в 1.55s на тяжёлом dither / pixel_size. Код D только если счётчик
exhausted высокий **и** есть атомарный cancel по смене `rev`.

---

## Not doing now (явно отложено)

**F** — schedule composite вне viewport: только если после TICKET-7–10
«вернулся пан — старое» воспроизводится.

**B** — progressive per-tile blit: продуктовый контракт (швы), не встраивать.

**G** — watchdog refetch: костыль, если E–A честные.

SIMD Bayer, in-place `PixelTile` ping-pong, правки Oklab/LUT — single-tile
apply (0.7ms Bayer / 1.4ms FS release) не в бюджете, который объясняет
наблюдаемые «секунды». Не трогать без нового профиля, который покажет
обратное.

## Инварианты (не ломать ни в одном тикете выше)

`GlobalCoord` / `rem_euclid`, ED residuals + `corner`, LUT vs KD только на
границах ячеек, GPU parity (Bayer exact; Halftone/CRT ≤ 1/255).
