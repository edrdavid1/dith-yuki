# Requirements: Tile generation / ready protocol (stale-200)

## Introduction

После TICKET-1 (`insert_fresh_gen` не даёт старому Composite перезаписать новый) превью всё ещё **иногда залипает на прошлых значениях фильтра**. Корень — не батчинг canvas и не длина 202-ретраев, а то, что система может **соврать, что тайл готов** (HTTP 200 на generation < `doc_gen` / dirty) или **потерять факт, что он не готов** (`insert_fresh_gen` отказал в write, ключ остался clean gen N, текущий документ уже N+1, ре-шедула нет → вечный 202 или вечный стейл-200).

Этот багфикс — **закрывающий патч к TICKET-1** плюс клиентский инвариант `rev`, затем страховка батч-коммита. Не perf-трек и не смена визуального контракта (атомарный кадр vs швы).

Источник симптомов: залипание тайлов при слайдере (pixel size / dither) в одном окне и после undock панелей; анализ 2026-08-14.

Связанные тикеты: `TICKETS_preview_latency.md` TICKET-1 (done), TICKET-4 (coalesce), TICKET-5 (won't do — open-ended approximate). Этот спек = **TICKET-7…10** там же.

## Glossary

- **Doc_Gen**: `document_gen` в `GenerationTracker` после `increment_generation` на `update_filter` / undo / load.
- **Entry_Gen**: `CacheEntry.generation`, записанный успешным `insert_fresh_gen`.
- **Ready**: протокол MAY вернуть 200 только если тайл можно безопасно показать как текущий документ.
- **Stale_200**: 200 с пикселями, у которых `Entry_Gen < Doc_Gen` и/или `dirty == true`.
- **Rev**: монотонный счётчик поколения запроса на фронте (`tileRevRef` / query `?g=`). Коммит bitmap только если `decoded.rev >= currentRev` в момент получения; иначе bitmap закрыть и не класть в map.
- **Batch_Commit**: `shouldCommitTileRefresh` — новый кадр рисуется, когда у всех уже отображённых видимых тайлов есть pending-замена.
- **TICKET-1 hole (п.6)**: `insert_fresh_gen` возвращает `false` и не трогает кэш; если ключ при этом **не dirty** относительно *текущего* Doc_Gen, протокол может отдать 200 старого gen или 202 без re-enqueue текущего gen.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Протокол никогда не отдаёт 200, если `Entry_Gen < Doc_Gen` или dirty | Open-ended 202-ретраи без контракта отмены (D) до замера |
| Canvas коммитит bitmap только с неубывающим `rev` | Schedule всех composite за пределами viewport (F) до данных из прода |
| Отказ `insert_fresh_gen` → ключ dirty + re-schedule текущего Doc_Gen | Прогрессивный blit каждого тайла (B) — меняет визуальный контракт |
| Acceptance TICKET-1 дополнен тестом «отказ → dirty + reschedule» | Watchdog-поллинг видимых тайлов (G) |
| Timeout батча — **после** E+C, как страховка, не вместо | Смена pixel-perfect / ED / GPU parity |

**Порядок исполнения (жёсткий):** E → C → regression-фикс п.6 (часть TICKET-1) → A → замерить, нужен ли D → F только по данным.

E и C — одна инвариантная проблема; делать вместе, первым делом. A без C запрещён (тихая порча кадра вместо явного залипания — тот же класс, что TICKET-1).

## Requirements

### Requirement 1 (E): Protocol never serves stale 200

**User Story:** Как canvas, я хочу, чтобы 200 означало «эти пиксели соответствуют текущему документу», а не «в кэше что-то лежит».

#### Acceptance Criteria

1. WHEN `tile://` request hits a cache entry that is `dirty` THEN THE Tile_Protocol SHALL return 202 and SHALL NOT return 200, even if pixel bytes exist
2. WHEN a cache entry exists with `Entry_Gen <` live `Doc_Gen` THEN THE Tile_Protocol SHALL treat it as not Ready: return 202 (and schedule Immediate recompute of current gen if not already queued), SHALL NOT return 200
3. WHEN a cache entry exists, `!dirty`, and `Entry_Gen >= Doc_Gen` THEN THE Tile_Protocol SHALL return 200 with RGBA8 as today
4. THE Tile_Protocol SHALL include the serving generation in a response header `X-Tile-Generation` (u64 decimal) on every 200
5. Cache-bust query `?g=` SHALL continue to be ignored for routing (path identity) and SHALL NOT by itself authorize a 200

### Requirement 2 (C): Client commits only non-decreasing rev

**User Story:** Как пользователь, я не хочу, чтобы поздний ответ на старый жест слайдера перерисовал превью назад.

#### Acceptance Criteria

1. WHEN Tile_Canvas bumps `rev` (document-changed / filter invalidate) THEN subsequent `request-tiles` / `fetch-tile` SHALL pass that `rev` (query `?g=` already present)
2. WHEN the worker posts `tile-decoded` THEN the message SHALL include the `rev` used for that fetch
3. WHEN Tile_Canvas receives `tile-decoded` with `rev < tileRevRef.current` THEN it SHALL drop the bitmap (close it) and SHALL NOT put it in `tileMap` or `refreshPending`
4. WHEN `rev` matches current THEN existing Batch_Commit MAY still wait for a full visible set (Requirement 4 applies only after this)
5. First paint of a key that is not yet on screen SHALL remain progressive (current `shouldCommitTileRefresh` empty-displayed rule)

### Requirement 3 (TICKET-1 closing patch): insert_fresh_gen reject stays dirty and is rescheduled

**User Story:** Как движок, я не хочу «дырку», в которой stale compute отброшен, а актуальный gen никто не считает.

#### Acceptance Criteria

1. WHEN `insert_fresh_gen` returns `false` because cached `generation >` incoming THEN THE cache entry SHALL remain the newer pixels AND if live `Doc_Gen >` cached generation OR the key is required for current viewport THEN the key SHALL be `dirty == true` relative to live Doc_Gen **or** an equivalent: worker SHALL enqueue current-gen recompute for that key (dedup)
2. IF reject happens because the write is older than cache but cache gen **equals** live Doc_Gen THEN the key SHALL stay clean (correct current frame); no extra enqueue required
3. A unit/property test SHALL lock: two inserts gen 2 then gen 1 → cache stays gen 2; then live Doc_Gen becomes 3 without a successful insert → key is dirty and a current-gen task is scheduled (or protocol would 202, not 200)
4. This test is **acceptance addendum to TICKET-1**, typed Bug/regression, not a perf ticket

### Requirement 4 (A): Batch commit timeout — after E+C only

**User Story:** Как пользователь, я не хочу, чтобы один недоехавший тайл держал весь кадр на предыдущем фильтре бесконечно.

#### Acceptance Criteria

1. WORK SHALL NOT start until Requirements 1–3 land and their tests pass
2. WHEN Batch_Commit has been waiting longer than T (default 100ms, constant, documented) AND at least one pending replacement has current `rev` THEN Tile_Canvas SHALL commit all current-rev pending tiles (partial frame allowed)
3. Tiles still missing after timeout SHALL keep the previously displayed bitmap for that key (no hole / #666 flash for that cell)
4. A SHALL NOT commit a decoded bitmap with `rev < current` (Requirement 2 still holds)
5. T MAY be tuned; SHALL NOT become an open-ended wait and SHALL NOT replace protocol correctness

### Requirement 5 (D): Open-ended 202 retries — gated on measurement

**User Story:** Как разработчик, я не расширяю ретраи «по ощущению» после того, как TICKET-5 уже отверг open-ended работу без cancel.

#### Acceptance Criteria

1. Current worker retry (5 attempts, ~1.55s) SHALL remain until a profile shows heavy dither / `pixel_size > 1` **still** hitting the cap **after** Requirements 1–4, in release, with count of 202-exhausted vs stale-200
2. IF D is opened THEN retries MUST abort atomically when `rev` changes (same invalidate that bumps rev); no zombie retry pile on slider drag
3. D is out of scope for the first implementation PR of this spec

### Requirement 6: Explicitly out of scope

1. THE system SHALL NOT switch to per-tile progressive commit as the default (B)
2. THE system SHALL NOT add a periodic watchdog refetch of all visible tiles (G)
3. THE system SHALL NOT schedule all dirty composite tiles outside the viewport/prefetch ring solely to fix “pan back → old tiles” (F) until a post-E+C repro exists in a build
4. Pixel-perfect ED, `GlobalCoord`, GPU parity, TICKET-6 prefix — unchanged

## Invariants (do not break)

Same as `TICKETS_preview_latency.md`: `GlobalCoord` / `rem_euclid`, ED residuals + `corner`, LUT vs KD on cell borders, GPU parity (Bayer exact; Halftone/CRT ≤ 1/255). Atomic-looking filter frames remain the default until Requirement 4 timeout; timeout is a safety valve, not a new product mode.
