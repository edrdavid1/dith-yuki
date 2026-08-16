# MVP: что доделать и на чём держится

Снимок **2026-08-16**. Версия в репо: **0.2.0**. Стадия: фичи треков A–Q в коде, формальный гейт **Beta 0 / Beta 1 не закрыт** (нет записанного ручного QA). Это не 1.0.

Карта треков: [`.cursor-spec/RELEASE_TRACKS.md`](.cursor-spec/RELEASE_TRACKS.md). Бета: [`.cursor-spec/track-p-beta/`](.cursor-spec/track-p-beta/). Ограничения as-built: [`ARCHITECTURE.md`](ARCHITECTURE.md) §14.

---

## 1. Обязательно до «можно отдавать людям» (Beta 1)

Код фич уже есть. Блокер — проверка, подпись, чистый снимок git.

### 1.1 Ручной QA (не начат)

| ID | Что пройти | Где чеклист |
|----|------------|-------------|
| **P4 / A §6.2** | 1:1 FS без шва после полной загрузки; zoom-out (пирамида > 0); sticky-seam при пане (сходит или N/A); `pixel_size` 3/5/7/12 Bayer+FS; Bayer-only smoke vs «до правок» | `.cursor-spec/track-p-beta/tasks.md` §4.1, `.cursor-spec/track-a-correctness/tasks.md` §6.2 |
| **P4 / D §5.3** | `DITHER_GPU=1` pan Halftone без скачка фазы; CRT scanlines через границу тайла; `DITHER_FORCE_CPU=1` совпадает с GPU-сессией; старт без адаптера / FORCE_CPU — приложение живое, один warn | `.cursor-spec/track-p-beta/tasks.md` §4.2 |
| **P4.3** | Скрипт Beta_0 на candidate-сборке, дата/билд | `.cursor-spec/track-p-beta/tasks.md` §4.3 |
| **O5.3** | Release-билд чекает GitHub `latest.json`; Later не качает; Guard Cancel; `tauri dev` не пинает апдейт на старте; future-format fixture → Check for Updates | `.cursor-spec/track-o-updates/tasks.md` §5.3 |
| **Q 4.4** | Портрет Strict vs Guided, один Bayer / `pixel_size` | `.cursor-spec/track-q-palette-dither-modes/tasks.md` §4.4 |
| **TICKET-11** | Замер 202 retry exhausted на release (FS + `pixel_size` 8, большой док, драг слайдера). Код cap **не писать**, пока счётчик не высокий **и** нет cancel-on-rev | `TICKETS_preview_latency.md`, `.kiro/specs/tile-generation-ready-protocol/tasks.md` §5 |

Пока P4 пустой, **Beta 0 в DoD не ставится**. P3 и O уже в дереве → после P4 это сразу **Beta 1**, не отдельный фичеспринт.

### 1.2 Первый настоящий релиз 0.2.0

- Закоммитить текущее дерево (огромный uncommitted diff: движок, UI, Q, иконки, workflow, latency).
- Тег `v*`, секреты `TAURI_SIGNING_PRIVATE_KEY` в GitHub; CI падает без ключа (так и задумано).
- Артефакты: tar.gz / sig / dmg / `latest.json`.
- Живой круг: check → Minisign → install → relaunch (DoD Track O).
- **0.1.0 не умеет self-update** — hop только ручной DMG 0.2.0.
- Apple notarization **не** блокер беты; Gatekeeper warning на первом открытии DMG — известный лимит.

CI сейчас: **только macOS** (`.github/workflows/release.yml`). Windows/Linux-инсталлятор — вне текущего гейта.

### 1.3 Документы, которые врут относительно кода

Поправить после QA или вместе с коммитом релиза:

- `RELEASE_TRACKS.md` строка Q: «не начато» — **ложь**, Q1–Q4.3 в коде, открыт только 4.4.
- `tech-debit.md` шапка «ED seams ~70%» — **устарело**; A1 в tasks закрыт 2026-08-11. Нужен ручной проход §6.2, не повторная реализация wavefront.

---

## 2. Кастыли и сознательные дыры MVP (так и задумано, не «баги на потом втихую»)

Это держит продукт. Менять — отдельное решение, не условие Beta 1.

### Движок / превью

| Кастыль | Как сейчас | Цена |
|---------|------------|------|
| **GPU v1 opt-in** | Default CPU. `DITHER_GPU=1` + глобальный `submit_lock`, нет buffer pool, alloc на каждый тайл, memcpy core скаляром. На viewport Bayer GPU часто **медленнее** CPU-пула (~88 ms vs ~14 ms origin 100%). | Пользователь не видит GPU в UI. v2: пул, batched tiles, без lock — ARCHITECTURE §13.4 / §14.2 |
| **GPU eligibility узкая** | Bayer: только `pixel_size=1`, без палитры, bias=0, angle=0. Halftone: ps=1, без bias. CRT — да. ED / Wave / CustomPng / Glow / Guided / `pixel_size>1` — **всегда CPU**. Шейдеры bias/angle — follow-up Track D, не H. | «Включил GPU» ≠ все фильтры на GPU |
| **Glow CPU-only** | Radius ≤ HALO; GPU отложен. | Нет GPU Glow |
| **ED не на GPU** | Cross-tile residuals + wavefront. Параллельный префикс по антидиагоналям (TICKET-6); kill switch `DITHER_ED_SERIAL_PREFIX=1`. | Far-corner всё ещё дороже origin; pixel-perfect важнее approximate-драга (TICKET-5 won't do) |
| **Нет отдельного ED-scheduler** | Рекурсия left/top/diag на воркере + координатор префикса. | Не row-major pipeline |
| **Processed inline** | `schedule_dirty_viewport_tiles` ставит только Composite; Processed считается внутри, не отдельной очередью. | Сложные зависимости ED спрятаны в compute |
| **Dirty = ptr_eq Arc** | Не `Document.revision`. Снимок структуры, не пиксельный paint. | Paint-aware undo out of scope |
| **Halo в preview composite** | Blend идёт по 260², включая halo. | Лишняя работа; «не blend'ить halo» в §14.2 |
| **Копии тайла на фильтр** | 1.03 MB × (1 + N фильтров + blend). Нет in-place / ping-pong. | Память и alloc на слайдер |
| **Oklab = Rec.709 primaries** | RGB→LMS не из ICC файла. | Нет настоящего ICC |
| **Luminance кривых упрощён** | `CurveChannel::Luminance` ≠ Oklab L*. | «Яркость» не perceptual |
| **LUT 64³ vs KD** | Hot path LUT; KD для сборки LUT и тестов. Расхождение только на границах ячеек. | Редкие отличия nearest |
| **Criterion benches пустые** | `filter_bench` / `compositor_bench` — placeholder `b.iter`. | Нет CI-тренда стоимости фильтра |
| **Мёртвый apply в `filter.rs`** | Старый `apply_filter_to_tile(..., CacheStage)` возвращает **пустой тайл** («Phase 3»). Живой путь — `filters/apply.rs`. | Путаница для агентов; риск вызвать не ту функцию |
| **Color profile** | `ColorProfileRef` — заглушка Phase 5. | Нет профилей в документе |
| **`FilterKind::Placeholder`** | Валидируется, apply пустой. | Наследие прототипа |
| **GPU parity тесты `#[ignore]`** | Без адаптера CI зелёный. | GPU-регресс не ловится на CPU-only CI |
| **Canvas commit timeout 100 ms** | Частичный кадр current-rev, остальные ключи — старый bitmap. | На тяжёлом ED кадр может быть «шахматкой» свежее/старое, пока доедет хвост |
| **Открытые ретраи 202** | Cap не введён (ждём замер TICKET-11). | Теоретический хвост зомби-ретраев без cancel-on-rev |

### Продукт / десктоп

| Кастыль | Как сейчас | Цена |
|---------|------------|------|
| **Один документ** | `doc_id = 1`. | Нет вкладок / нескольких окон документа |
| **Max 8192×8192** | Reject больше. | Не печатные гиганты |
| **Нет mask UI** | `MaskRef` + `apply_mask` в движке есть, панели нет. | Маски не для пользователя |
| **Нет paint** | Слои = импорт / blank raster. Undo = snapshot документа. | Не редактор пикселей |
| **Import layer без scale** | Origin, clip, без ресайза под холст. | Большая картинка обрежется |
| **GPU не в Preferences** | Только env. | Бета-тестер должен знать `DITHER_GPU` |
| **Апдейтер только с 0.2.0** | 0.1.0 мёртвый канал. | Первый hop ручной |
| **Нотаризация optional** | Gatekeeper. | «Неоткрывающееся» приложение на чужом Mac |
| **Релиз-CI = macOS** | Windows `.ico` / NSIS в дереве, пайплайн не гоняет. | Win/Linux не часть Beta 1 |
| **Welcome / Recent** | Blank create не пишется в Recent (спека). | Пустой проект не в списке |
| **Debounce undo = 100 ms** | Слайдер = одно undo на жест, не на каждый тик. | Промежуточные кадры слайдера не в истории |

### Palette / dither (Q)

| Кастыль | Как сейчас |
|---------|------------|
| **Strict — default, старые файлы** | Нет поля → Strict, вид как раньше |
| **Guided — CPU-only** | GPU skip; нет snap-to-palette; общий порог R/G/B |
| **Wave / Halftone / CRT** | Режим палитры их не трогает |
| **Residuals schema** | Не меняли под Guided |

---

## 3. Явно не в MVP (не делать «заодно» до беты)

Из ARCHITECTURE §14.2, TICKETS «Not doing now», ROADMAP:

- Multi-document, video, ICC, batch export
- Mask editing UI
- Paint / pixel undo
- GPU v2 (pool, batched, без `submit_lock`)
- SIMD Bayer / Oklab; LUT для Curves
- In-place `PixelTile` ping-pong
- Proper luminance = Oklab L*
- Progressive per-tile blit (контракт швов)
- Watchdog refetch (костыль поверх честного протокола)
- Prefetch composite вне viewport — только если после TICKET-7–10 снова «вернулся пан — старое»
- Approximate ED на драге (TICKET-5 закрыт: won't do)
- Serpentine×wavefront уже сделан в Track M; не переоткрывать без нового seam-бага
- Полный Windows/Linux updater-канал
- Apple notarization как требование

---

## 4. Порядок, если делать по одному человеку

1. Закоммитить дерево (без этого нет кандидата).
2. Пройти и **записать** P4 + Q 4.4 + O5.3 (дата, машина, билд).
3. По желанию: TICKET-11 замер 202 (числа в тикет, код только по данным).
4. Тег `v0.2.0`, секреты, проверить live updater.
5. Поправить враньё в `RELEASE_TRACKS` / шапке `tech-debit`.
6. Всё из §2 — бэклог после беты, не смешивать с гейтом.

---

## 5. Ссылки

- Гейт беты: `.cursor-spec/track-p-beta/tasks.md` (P4 открыт)
- Апдейтер: `.cursor-spec/track-o-updates/tasks.md` (O5.3 открыт)
- Strict/Guided: `.cursor-spec/track-q-palette-dither-modes/tasks.md` (4.4 открыт)
- Latency / протокол тайлов: `TICKETS_preview_latency.md`, `PREVIEW_LATENCY_REPORT.md`
- Известные лимиты: `ARCHITECTURE.md` §13–14
- Релиз-треки: `.cursor-spec/RELEASE_TRACKS.md`
