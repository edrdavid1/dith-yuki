# SPEC: Рефакторинг `src-tauri/src/commands.rs`

**Проект:** Dither Yuki 2
**Статус:** к исполнению
**Исполнитель:** AI coding agent
**Тип задачи:** структурный рефакторинг, без изменения поведения (behavior-preserving)

---

## 0. Контракт с агентом

- Это **чисто структурный** рефакторинг. Никакая бизнес-логика не меняется, ни один баг не фиксится по пути. Если по ходу дела видишь потенциальный баг — фиксируешь отдельным коммитом ПОСЛЕ рефакторинга, с явным комментарием, не смешивая с move.
- Каждая фаза — отдельный коммит (или PR), который должен собираться (`cargo build --all`) и проходить тесты (`cargo test --all`) до перехода к следующей фазе.
- Если после фазы что-то не компилируется — не переходи к следующей фазе, чини текущую.
- Публичные сигнатуры Tauri-команд (имя команды, аргументы, тип возврата) — **не менять**. Фронтенд (`shared/ipc/`) не должен требовать изменений вообще ни в одной фазе.
- Порядок фаз ниже подобран от низкого риска к высокому. Не переставляй местами без причины.

---

## 1. Целевая структура

```
src-tauri/src/
├── commands/
│   ├── mod.rs              # pub use + regestration list для generate_handler!
│   ├── document.rs         # load_image, create_document, new_document, open_project,
│   │                        # save_project(_as), export_pattern/import_pattern, export_image,
│   │                        # import_image_layer, get_document_snapshot, is_document_dirty,
│   │                        # list_open_documents, set_active_document, close_document,
│   │                        # get_recent_files
│   ├── viewport.rs         # set_viewport
│   ├── layers.rs           # get_layer_tree, add_layer, remove_layer, reorder_layer, set_layer_props
│   ├── filters.rs          # add_filter, update_filter, remove_filter, reorder_filter
│   ├── undo.rs              # undo, redo
│   ├── selection.rs        # set_selection, get_selection
│   ├── palette.rs          # replace/create/delete/rename_palette, add/remove/import/export_palette,
│   │                        # add/update/remove/reorder_palette_color, generate_palette,
│   │                        # list_builtin_palettes, import_builtin_palette
│   ├── color_lab.rs        # generate_ramp_palette, generate_harmony_palette, colors_to_oklab,
│   │                        # get_palette_oklab
│   ├── panels.rs           # undock/dock/show/hide_panel, undock_panel_with_size,
│   │                        # move_panel_to_side, move_all_panels_to_side, swap_sidebars,
│   │                        # update_dock_zone, begin/cancel_float_drag, dock_panel_at
│   └── diagnostics.rs      # get_gpu_preview_status, set_gpu_preview_enabled, is_release_build
│
├── services/
│   ├── mod.rs
│   ├── document_service.rs
│   ├── layer_service.rs
│   ├── filter_service.rs
│   ├── undo_service.rs
│   ├── palette_service.rs
│   └── panel_service.rs     # диагностика/gpu — без сервиса, слишком тонкие, остаются в commands/
│
├── state/
│   ├── mod.rs               # pub struct AppState { tiles: TileState, ui: UiState, history: HistoryState, ... }
│   ├── tile_state.rs        # tile_cache, scheduler, palette_cache, palette_lut_cache,
│   │                        # threshold_cache, error_residuals, block_representatives, ed_frontier
│   ├── ui_state.rs          # viewport, panel_manager, selection
│   └── history_state.rs     # undo_manager, saved_snapshot
│
├── commands.rs               # УДАЛЯЕТСЯ в конце (Фаза 8)
└── main.rs                   # правится один раз в Фазе 1 (module wiring) и один раз в Фазе 8 (generate_handler!)
```

**Принцип границы:**
- `commands/*.rs` — ТОЛЬКО `#[tauri::command] fn(...)`: unwrap аргументов, вызов одного метода сервиса, маппинг `Result` в IPC-ошибку, ничего больше. Целевой размер функции — 3-8 строк.
- `services/*.rs` — вся логика: валидация, мутация через `DocumentHandle::mutate`, invalidation, scheduling, запись recent files и т.д. Сервис ничего не знает про Tauri (`tauri::State`, `AppHandle` и т.п. внутрь не пробрасывать дальше, чем нужно — см. §4).
- `state/*.rs` — только структуры данных и их прямые конструкторы/аксессоры, без бизнес-логики.

---

## 2. Инварианты, которые нельзя сломать

Взято из архитектурного документа — агент должен явно сверяться с этим списком после каждой фазы:

1. **Undo/redo порядок операций.** Мутации документа обязаны идти через `with_document_undo` (snapshot `before` → mutate → push стека на успех, ничего не трогать на `Err`). Порядок вызовов внутри не менять.
2. **Dirty flag.** `is_document_dirty` = `!Arc::ptr_eq(live, saved_mark)`. `clear_history` вызывается только из `load_image` / `open_project` / `create_document` / `new_document`. Не путать с undo-шагом.
3. **Invalidation cascade.** После любой мутации документа порядок должен остаться: `invalidate_after_document_replace` → `schedule_dirty_viewport_tiles` → emit `document-changed`. Не менять порядок вызовов при переносе кода в сервис.
4. **Recent files** пишутся только **после успеха** `load_image`, `open_project`, `save_project`, `save_project_as`. Ошибка записи — логируется, не валит команду.
5. **`generation` semantics** (staleness check воркеров) не трогать — `document_gen` / `layer_gen` инкременты должны остаться на тех же местах вызова.
6. **Panel events** — `panel-state-changed` эмитится fan-out ко всем окнам, сохранить точку эмита.

Если в процессе переноса кода в сервис непонятно, в каком порядке должны идти вызовы — **не гадай**, ищи текущий порядок в `commands.rs` и копируй один в один.

---

## 3. Паттерн сервиса (шаблон)

Каждый сервис — тонкая структура, которая держит `Arc<AppState>` (или ссылку на нужный под-стейт) и предоставляет методы 1:1 с командами, но без Tauri-типов в сигнатуре, где это возможно.

```rust
// services/layer_service.rs

pub struct LayerService {
    state: Arc<AppState>,
}

impl LayerService {
    pub fn new(state: Arc<AppState>) -> Self {
        Self { state }
    }

    pub fn add_layer(&self, doc_id: DocId, params: AddLayerParams) -> Result<LayerNodeDto, AppError> {
        // 1. валидация params
        // 2. self.state.document_handle.mutate(|doc| { ... })
        // 3. invalidate + schedule (порядок как было в commands.rs!)
        // 4. serialize DTO
    }
}
```

```rust
// commands/layers.rs

#[tauri::command]
pub fn add_layer(
    state: tauri::State<Arc<AppState>>,
    doc_id: DocId,
    params: AddLayerParams,
) -> Result<LayerNodeDto, String> {
    LayerService::new(state.inner().clone())
        .add_layer(doc_id, params)
        .map_err(|e| e.to_string())
}
```

Если конструировать сервис на каждый вызов накладно (лишний `Arc` клон) — не проблема, `Arc::clone` дешёвый. Не оптимизируй это на этой стадии.

**Ошибки:** если единого `AppError` enum ещё нет — завести его в `services/mod.rs` на Фазе 1, до переноса первой команды. Все сервисы возвращают `Result<T, AppError>`, `commands/*.rs` мапит в `String` на границе (как сейчас, если Tauri-команды уже возвращают `Result<T, String>` — сохранить).

---

## 4. Декомпозиция `AppState`

Выполняется **параллельно** с переносом команд (см. фазы), не отдельным большим шагом — иначе один гигантский breaking-diff.

Целевая форма:

```rust
pub struct AppState {
    pub document_handle: DocumentHandle,
    pub tiles: TileState,
    pub ui: UiState,
    pub history: HistoryState,
    pub gpu: Option<Arc<engine_gpu::GpuContext>>,
    pub worker_wake: WorkerWake,
}

pub struct TileState {
    pub tile_cache: TileCache,
    pub scheduler: Scheduler,
    pub palette_cache: PaletteKdCache,
    pub palette_lut_cache: PaletteLutCache,
    pub threshold_cache: ThresholdMapCache,
    pub error_residuals: ErrorResidualsStore,
    pub block_representatives: BlockRepresentativeCache,
    pub ed_frontier: EdFrontier,
}

pub struct UiState {
    pub viewport: Mutex<ViewportState>,
    pub panel_manager: Mutex<PanelManager>,
    // selection state, если сейчас поле AppState — сюда же
}

pub struct HistoryState {
    pub undo_manager: Mutex<UndoManager>,
    pub saved_snapshot: Mutex<Option<Arc<Document>>>,
}
```

**Правило:** переносить поле в под-стейт можно только в той фазе, где рефакторится домен, который им владеет (например, `tile_cache` переносится в `TileState` в фазе Layers/Filters, потому что именно они его больше всего трогают через invalidation). После переноса поля — везде, где было `state.tile_cache`, становится `state.tiles.tile_cache`. Это будет самая частая причина ошибок компиляции — норм, чинить по месту.

---

## 5. Порядок фаз (от низкого риска к высокому)

Каждая фаза = 1 коммит, обязательный `cargo build --all && cargo test --all` в конце.

### Фаза 0 — подготовка
- Создать `commands/mod.rs`, `services/mod.rs`, `state/mod.rs` (пустые, с `pub mod` заглушками).
- Завести `AppError` в `services/mod.rs`.
- В `commands.rs` пока ничего не трогать. Просто убедиться, что новые пустые модули подключены в `main.rs` и всё собирается.

### Фаза 1 — diagnostics (самый низкий риск, нет state-мутаций)
- Команды: `get_gpu_preview_status`, `set_gpu_preview_enabled`, `is_release_build`.
- Переносятся as-is в `commands/diagnostics.rs`, сервис не нужен (тривиальная логика).
- Критерий готовности: `commands.rs` минус 3 команды, всё остальное на месте, компилируется.

### Фаза 2 — panels
- Команды: `undock_panel`, `undock_panel_with_size`, `dock_panel`, `show_panel`, `hide_panel`, `move_panel_to_side`, `move_all_panels_to_side`, `swap_sidebars`, `update_dock_zone`, `begin_float_drag`, `cancel_float_drag`, `dock_panel_at`.
- Завести `PanelService`, перенести туда логику работы с `PanelManager`.
- `panel_manager: Mutex<PanelManager>` переносится в `UiState` в этой же фазе.
- Проверить: `panel-state-changed` эмитится из того же места по порядку (до/после мутации — как было).

### Фаза 3 — selection
- Команды: `set_selection`, `get_selection`.
- Маленький домен, можно без отдельного сервиса — методы прямо в `commands/selection.rs`, если логика тривиальна (просто geter/сеттер state). Если там есть валидация границ — вынести в `services/selection_service.rs`.

### Фаза 4 — viewport
- Команда: `set_viewport`.
- Это критичный путь (пересчёт pyramid level, visible/prefetch tiles, приоритеты, `scheduler.clear_all()`). Внимательно перенести весь пайплайн `set_viewport` в `ViewportService` **одним куском**, не разбивая логику на "мелкие красивые методы" в этой фазе — сначала move, потом (отдельным будущим тикетом, не сейчас) можно декомпозировать.
- `viewport: Mutex<ViewportState>` → `UiState`.

### Фаза 5 — undo/redo
- Команды: `undo`, `redo`.
- `UndoService`, использует `with_document_undo` как есть.
- `undo_manager`, `saved_snapshot` → `HistoryState`.
- Особое внимание: сохранить точный порядок `DocumentHandle::store` → `increment_document_gen` → `invalidate_after_document_replace` → `schedule_dirty_viewport_tiles` → emit `document-changed`.

### Фаза 6 — layers + filters
- Делать вместе, т.к. filters живут на layers и используют общий invalidation-путь.
- Команды layers: `get_layer_tree`, `add_layer`, `remove_layer`, `reorder_layer`, `set_layer_props`.
- Команды filters: `add_filter`, `update_filter`, `remove_filter`, `reorder_filter`.
- `LayerService` и `FilterService`, оба берут `Arc<AppState>` целиком (им нужен и `document_handle`, и `tiles` для invalidation).
- `tile_cache`, `scheduler`, `ed_frontier` и остальные tile-кэши → `TileState` в этой фазе (это первая фаза, где они реально нужны сервисам).

### Фаза 7 — palette + color_lab
- Самый большой домен (~19 команд), но низкий риск — почти всё это CRUD без сложной invalidation-цепочки, кроме `generate_palette` (async, трогает layer tiles).
- Palette CRUD → `PaletteService`.
- `generate_ramp_palette`, `generate_harmony_palette`, `colors_to_oklab`, `get_palette_oklab` — это чистые色-функции без мутации state, можно оставить прямо в `commands/color_lab.rs` без сервиса, либо вынести в `services/color_lab_service.rs`, если хочется единообразия. Решать по факту — не критично.

### Фаза 8 — document (самый высокий риск, делать последним)
- Команды: `load_image`, `create_document`, `new_document`, `list_open_documents`, `set_active_document`, `close_document`, `get_document_snapshot`, `open_project`, `save_project`, `save_project_as`, `export_pattern`, `import_pattern`, `export_image`, `import_image_layer`, `is_document_dirty`, `get_recent_files`.
- `DocumentService` — самый жирный сервис, трогает почти весь `AppState` (создаёт документ → decompose to tiles → invalidate → schedule → clear_history → record_recent_file).
- Именно здесь `commands.rs` должен опустеть полностью. Удалить файл, обновить `main.rs`: `generate_handler!` собирает список из `commands::document::*`, `commands::layers::*` и т.д.
- Финальная проверка: `grep -r "commands::" src-tauri/src/main.rs` — все ссылки на новые модули, ни одной на старый `commands.rs`.

---

## 6. Definition of Done (на весь рефакторинг)

- [ ] `commands.rs` не существует.
- [ ] Каждый файл в `commands/*.rs` — только тонкие `#[tauri::command]` обёртки, ни один не превышает ~15 строк на функцию.
- [ ] `AppState` — композиция из `TileState` / `UiState` / `HistoryState` + прямые поля (`document_handle`, `gpu`, `worker_wake`).
- [ ] Фронтенд (`frontend/src/shared/ipc/`) не менялся ни разу за весь рефакторинг.
- [ ] `cargo test --all` зелёный после каждой фазы, не только в конце.
- [ ] Ни одна инвариант из §2 не нарушена — агент явно подтверждает это в описании каждого коммита ("Undo order preserved: yes/no + diff line reference").
- [ ] Порядок вызовов internal invalidation pipeline (`invalidate_after_document_replace` → `schedule_dirty_viewport_tiles` → event emit) идентичен для каждой перенесённой команды, где он был.

---

## 7. Что НЕ входит в эту задачу (сознательно откладывается)

- Разбиение `filters/apply.rs`, `compositor.rs` и других крупных файлов внутри `engine-project` — отдельная задача, паттерн там другой (полиморфизм по типу фильтра, не по IPC-домену).
- Оптимизация количества `Arc::clone` при конструировании сервисов.
- Любые изменения поведения, фикс багов, замена логики.
- Тесты нового кода сверх существующего покрытия — рефакторинг не обязан наращивать тесты, только сохранять зелёный `cargo test --all`.
