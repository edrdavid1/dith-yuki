Dither Yuki 0.2.0 — где кастыли
Обзор as-built без правок кода. Движок тайлов и дизера — настоящий; продуктовый каркас вокруг него собран так, чтобы demo и один документ жили, а не как Photoshop / Affinity / Aseprite.

1
Документ в процессе
8192²
Жёсткий потолок размера
~250
Тайлов в кэше 256 MB
~5000
Строк в commands.rs
Вердикт
Как студия дизера на одном холсте до ~4K — работает. Как серьёзный imaging-продукт (много документов, печать, ICC, GPU-превью, paint, большие холсты, предсказуемый ED) — нет: слишком много глобального состояния, поколений кэша и «обнулить всё и пересчитать viewport».
Блокеры продукта
Кастыль	Почему ломает продукт	Где
Один документ, doc_id всегда 1
Нет вкладок, нет двух файлов рядом, сериализация ремапит id в 1. Любой multi-doc = переписывать AppState, tile://, undo, dirty.	AppState, serialize/document_dto.rs
Тайл = 260² × 4 × f32 ≈ 1.03 MB
Viewport 40 Composite + Processed + Raw ≈ 120 MB на слой. Бюджет 256 MB ≈ 250 тайлов всех стадий. Print / 8K / много слоёв вытесняют Raw.	architecture.md §13, TileCache
Error diffusion через рекурсию соседей
Тайл (x,y) считает весь префикс на одном воркере (+ lock префикса, anti-diagonal prefill). Нет row-major планировщика. При отсутствии Raw — zero-seed и waiter, wiring «optional». Швы/рассинхрон на краю viewport.	tile_pipeline.rs, diffusion_waiters.rs
GPU opt-in и медленнее CPU
DITHER_GPU=1, submit_lock на все воркеры, нет пула буферов, upload/download 1 MB на тайл. Eligible только Bayer/Halftone/CRT без палитры. На 40 тайлах CPU-пул выигрывает. Нельзя продавать «GPU acceleration».	engine-gpu dispatch.rs
Инвалидация кэша поколениями
document_gen монотонно растёт, CAS insert, frontend documentEpoch + tileRev ?g= на tile://. Replace/undo — снести кэш и перезапросить.visible. Это не модель документа, а борьба со stale tiles.	commands.rs, TileCanvas, engine-tiles generation
По слоям
Модель документа
Live — один ArcSwap Document. Undo — стек до 50 полных Arc<Document>, не команды и не pixel-diff. Dirty = не ptr_eq(live, saved_mark). Paint в модели нет.

Слайдер фильтра клонирует документ, помечает все Processed и Composite dirty, чистит residuals, ставит весь viewport. Debounce 100 ms режет IPC, не работу.

Память и превью
Пирамида — box-filter L0 Composite, фильтры всегда на полном разрешении. Zoom-out рисует 9 display-тайлов, воркеры считают 144 L0. Halo blend'ится в preview.

Потолок 8192² — reject. 16-bit / ICC в типах placeholder (ColorProfileRef::Other(String)). Маски в движке, UI нет.

Backend God-object
commands.rs ~5000 строк: open, tiles, layers, palettes, dirty, install_raster, tests. engine-core — пустой Phase 0 stub в workspace. Dither и DitherV2 живут рядом.

IPC Mutex unwrap на viewport / panel_manager. Кастомный tile:// + Web Worker fetch + ImageBitmap — не shared GPU texture, не SharedArrayBuffer.

Цвет «почти продакшен»
Oklab + LUT 64³ (не KD на hot path) — быстро и с ошибкой на границах ячеек. RGB→LMS под Rec.709. Curve luminance ≠ Oklab L*. CMYK halftone — artistic UCR, не ICC separations.

ASE/GPL/ACO импорт выкидывает не-RGB. Для print-product это не рабочий color pipeline.

Что ещё не продукт, а beta
Тема	Факт
Дистрибуция	Notarization optional; Gatekeeper warning на первом DMG. 0.1.0 не умеет self-update.
Лицензия	Fair Core 1.0 — не Apache/MIT сейчас; для студии/магазина нужна отдельная юридическая история.
Панели	Undock = отдельный OS WebView. Dock affinity / global mouseup — обход ограничений webview, не нативный docking.
Экспорт / batch	Нет video, ICC, batch export, multi-document. SVG export в engine-io — узкий путь.
GPU vs обещание	Документ сам пишет: включить GPU ≠ быстрее. v1 нельзя честно продавать как GPU engine.
Слайдер = полный invalidate	Нет tile-local dirty для ordered; нет stale-discard Composite. Последний кадр важнее, чем не считать лишнее.
Что не кастыль
Тайловый preview, GlobalCoord / rem_euclid, LUT палитры, SIMD blend / levels / f32→u8, WorkerWake Condvar, .dyproj zip, in-app updates с Minisign с 0.2.0 — это реальная инженерия studio-preview, не toy UI. Проблема не в отсутствии кода, а в том, что контракт заточен под один live document и «пересчитать видимое», а не под продукт с сессией, цветом и масштабом.

Если чинить по приоритету
Источник: docs/architecture.md §13–14, tile_pipeline.rs, commands.rs, engine-gpu, frontend TileCanvas / tileWorker. Дата обзора: 20 Aug 2026. Код не менялся.