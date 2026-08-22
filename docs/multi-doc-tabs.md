# Вкладки и мультипроектность

> As-built документация: несколько открытых проектов в одном процессе, tab bar, shared tile cache.  
> Версия приложения: **0.2.0**. Последнее обновление: 21 августа 2026.
>
> **См. также:**
> - [architecture.md](./architecture.md) — общий стек и IPC
> - [tile-pipeline.md](./tile-pipeline.md) — тайлы, стадии Raw / Processed / Composite
> - Спеки: `.cursor-spec/runtime-document-id/`, `window-chrome-tabs/`, `multi-doc-cache-budget/`, `multi-doc-save-export-raw/`, **`multi-doc-global-fix/`** (P0 data-safety + tab UX)

---

## 1. Зачем это нужно

До runtime `DocumentId` процесс держал **один** live-документ с магическим `id = 1`. `TileKey` не содержал doc → тайлы разных файлов делили ключи; open второго файла перетирал кэш первого.

Сейчас:

- N документов живут параллельно в registry сессий
- UI показывает вкладки (tab bar под title)
- Shared `TileCache` / workers / GPU — process-wide, но ключи namespace по `doc`
- Save / export собирают pixels по правильному `doc_id`; Raw открытых вкладок не вытесняется pressure eviction

Welcome screen: `active_id = None`, `DocumentId(0)` зарезервирован («нет документа»).

---

## 2. Модель данных (backend)

### 2.1 Registry сессий

```
AppState (process-wide)
  sessions: Mutex<HashMap<u32, Arc<DocumentSession>>>
  next_doc_id: AtomicU32              // 1, 2, 3… никогда не reuse после close
  active_id: Mutex<Option<u32>>       // None = welcome
  tile_cache, scheduler, viewport     // один viewport = active session
  palette_cache, palette_lut_cache    // ключ (doc_id, palette_id)
  error_residuals, block_representatives, workers, gpu, …

DocumentSession
  id: DocumentId
  document_handle: DocumentHandle     // ArcSwap snapshot
  undo_manager: Mutex<UndoManager>
  saved_snapshot: Mutex<Option<Arc<Document>>>
  project_path: Mutex<Option<PathBuf>>
  io_inflight: AtomicUsize            // SessionIoGuard (save/export)
```

Файл: `src-tauri/src/document_session.rs`. Поля `AppState` — `src-tauri/src/commands.rs`.

### 2.2 Runtime id ≠ file-local id

| Что | Где | Смысл |
|-----|-----|--------|
| **Runtime `DocumentId`** | `DocumentSession.id`, `TileKey.doc`, IPC `doc_id` | Уникален в процессе, monotonic |
| **File-local id** | `document.json` внутри `.dyproj` | Remap-ключ при open; на disk может быть `1` у каждого файла |
| **LayerId / PaletteId** | Внутри документа | Уникальны **внутри** doc; у двух вкладок оба могут иметь `palette_id = 1` |

При open: `alloc_doc_id()` → `remap_document_file(..., runtime_doc_id)` → сессия с новым runtime id. File-local layer/palette numbering **не** ломаем.

### 2.3 Жизненный цикл сессии

| Операция | Поведение |
|----------|-----------|
| `alloc_doc_id()` | `next_doc_id.fetch_add(1)` → 1, 2, 3… |
| Open image / New / Open project | Alloc → decompose tiles под этот `doc` → `spawn_session` (**старые сессии остаются**) → activate новый → `emit_tabs_changed` |
| `activate(doc)` | `active_id = doc`; pressure только inactive; **без** soft-trim Composite |
| `close_session(doc)` | Отказ, если `io_inflight > 0`; иначе remove + `evict_document` (тайлы, residuals, BRC, waiters, palette KD/LUT); если закрыли active → `active = max(remaining)` |
| Quit / welcome | Нет live doc → `active_id = None` |

Startup **не** создаёт пустой 800×600 с id=1.

---

## 3. UI вкладок (frontend)

### 3.1 Chrome

Title bar (CSD): меню File/Edit/… справа от traffic lights.  
**Tab strip** — на месте прежнего toolbar-слота: вкладки открытых проектов + `+` (New).

Компонент: `frontend/src/features/document/DocumentTabBar.tsx`  
Стили: `DocumentTabBar.module.css`  
Встраивание: `AppLayout.tsx`.

### 3.2 Redux

| Slice | Роль |
|-------|------|
| `tabsSlice` | `tabs: OpenDocumentTab[]`, `activeId`; thunks `refreshTabs`, `activateTab`, `closeTab`; событие `tabsChanged` |
| `documentSlice` | Метаданные **active** doc: `docId`, size, dirty, `documentEpoch`, paths; `parseRuntimeDocId` |

**Источник истины — backend.** Frontend зеркалит через `list_open_documents` / `tabs-changed`.

### 3.3 Действия пользователя

| UI | Backend | Заметки |
|----|---------|---------|
| Клик по вкладке | `set_active_document` → refresh document / layers / filters | `activateTab` ждёт `refreshDocument`, чтобы layers не остались от старого id |
| × на вкладке | `close_document` | Dirty → UnsavedGuard (Save / Don’t Save / Cancel) для **этой** вкладки |
| `+` / New / Open Image / Open Project | Новая сессия + activate | Предыдущие вкладки остаются; dirty других вкладок **не** блокирует open |
| Quit / close window | UnsavedGuard | **Все** dirty вкладки по очереди (VS Code / Photoshop); Cancel прерывает quit |

Welcome / empty preview: `EmptyState` (New / Open Image / Open Project / Recent) когда `docId === null`.

---

## 4. Preview и привязка к `docId`

Один холст = active session.

`PreviewFeature` → `effectiveDocId = document.docId ?? tabs.activeId` (fallback при гонке snapshot).

`TileCanvas`:

1. Props: `docId`, размеры, viewport
2. При смене `docId` / `documentEpoch`: bump `tileRev`, очистка bitmap maps, немедленный `request-tiles` с новым id
3. Сообщения worker / `tile-ready` / `document-changed` с чужим `doc_id` **игнорируются**
4. Доп. delayed refetch (~300 ms) после смены identity

`tile://` URL: `…/doc/{docId}/layer/…/l/{level}/{x}/{y}`.  
`handle_tile_request`: нет сессии с этим id → **404** (не подставлять active).

`documentEpoch` bump’ается на activate / open / create / undo, чтобы холст обновился даже если id не менялся.

---

## 5. Shared TileCache и бюджет памяти

### 5.1 Ключ

```rust
TileKey { doc: u32, layer: u32, coord: TileCoord, stage: CacheStage }
```

Стадии: **Raw** (исходник) → **Processed** (фильтры) → **Composite** (blend).  
Residuals / BlockRepresentativeCache / diffusion waiters — тоже namespace по `doc`.

### 5.2 Бюджет

- Потолок кэша: **512 MiB** (`main.rs`, Decision 0)
- При превышении: `evict_for_pressure(EvictContext { active_doc, open_docs, viewport_coords })`

### 5.3 Политика eviction (критично для вкладок)

```
EvictContext {
  active_doc: Option<u32>,
  open_docs: &HashSet<u32>,   // все live sessions
  viewport_coords: &HashSet<TileCoord>,
}
```

| Правило | Смысл |
|---------|--------|
| **Raw hard-pin** | `stage == Raw && open_docs.contains(doc)` **никогда** не вытесняется pressure |
| Порядок drop | Сначала inactive Composite → Processed → orphan Raw; затем active off-viewport те же стадии |
| Close вкладки | `evict_document(doc)` — полный снос **включая** Raw |
| Soft trim на deactivate | **Явно не делаем** (гарантированный cold return-to-tab). Но тот же эффект уже есть: **pressure** при multi-doc часто сносит inactive Composite/Processed — return на вкладку то warm, то full recompute (в т.ч. far-corner FS — секунды в debug). Это **текущее** поведение, не «отложенная фича» |
| Activate / open | Только `evict_inactive_for_pressure_if_needed` (пустой viewport) |

**Следствие:** N больших изображений ≈ N × ~149 MiB Raw pinned в RAM. 512 MiB — потолок в основном для Processed/Composite preview, не для process RAM и не для произвольного N×Raw. Warning при многих opens + долгосрочно out-of-cache raster source (ADR **D**) — см. `multi-doc-global-fix` M4.

Pressure вызывается на write path (worker, tile_pipeline, open/install) и на activate.

---

## 6. Палитры при нескольких документах

`PaletteKdCache` и `PaletteLutCache` ключуются как **`(runtime_doc_id, PaletteId)`**.

Почему: два файла часто имеют `palette_id = 1`, `revision = 1` с разным числом цветов. Без doc в ключе второй документ брал чужой LUT → index OOB (panic export / дыры при recompute).

При close: `palette_cache.evict_document(doc)` + `palette_lut_cache.evict_document(doc)`.  
Channel ranges (Guided) — тот же scope.

---

## 7. Save / Export

### 7.1 Сборка pixels

`assemble_layer_rgba8` / `assemble_layer_png` / `tile_keys_for_bounds` принимают **`doc_id`** (= `doc.id.0`). Литерал `doc: 1` запрещён.

Save As работает с **active** session и передаёт её `doc.id`.  
Export принимает `ExportImageRequest.doc_id` (может отличаться от active, если клиент так передаст).

### 7.2 Два класса ошибок

| Класс | Условие | Сообщение (смысл) |
|-------|---------|-------------------|
| **SessionGone** | `doc_id` нет в `sessions` | Document was closed; cannot export/save |
| **RawIncomplete** | Сессия есть, нет L0 Raw для bounds | Cannot save/export: image tiles missing from memory — reopen the file (`IncompleteRaw { doc_id, layer_id }`) |

Нельзя склеивать оба в `"Document not found"`.

### 7.3 Mid-save / mid-export close

`session.begin_io()` → `SessionIoGuard` (++`io_inflight`).  
`close_session` при `io_inflight > 0` → отказ: *"Cannot close document while save or export is in progress"*.

---

## 8. IPC и события

### 8.1 Команды вкладок / identity

| Command | Назначение |
|---------|------------|
| `list_open_documents` | `{ tabs, active_id }` |
| `set_active_document(doc_id)` | Activate + schedule |
| `close_document(doc_id)` | Close session |
| `export_image` | `req.doc_id` |
| Open / Load / Create responses | Возвращают `doc_id` |

Почти все мутации (слои, фильтры, палитры, undo, save) идут через **`active_session()`** без аргумента `doc_id` — известный риск гонки, если UI отстаёт от backend.

### 8.2 События

| Event | Payload | Заметки |
|-------|---------|---------|
| `tabs-changed` | `OpenDocumentsPayload` | Список вкладок + active |
| `document-changed` | `{ kind, layer_id?, doc_id? }` | `doc_id` = active на момент emit; kinds: `document_activated`, `document_closed`, `image_loaded`, `project_opened`, filter/*, undo… |
| `tile-ready` | `{ doc_id, layer_id, stage, level, x, y }` | Frontend фильтрует по своему `docId` |
| `dirty-changed` | `{ dirty }` | **Без** `doc_id` — только active |
| `undo-state-changed` | `{ can_undo, can_redo }` | Только active |

---

## 9. Потоки (сценарии)

### 9.1 Открыть второй проект

```
User: Open Project B while A is open
  → alloc_doc_id() = 2
  → remap + install Raw under TileKey.doc=2
  → spawn_session(B); A остаётся в map
  → active = 2; tabs-changed; document-changed(project_opened)
  → pressure: может дропнуть Composite/Processed у A (inactive)
  → Raw A и Raw B pinned
```

### 9.2 Вернуться на вкладку A

```
User: click tab A
  → set_active_document(1)
  → frontend: clear TileCanvas bitmaps, request-tiles(doc=1)
  → workers: recompute Processed/Composite из Raw A (если их съел pressure)
  → tile-ready(doc=1) → paint
```

Если recompute паникует (например, старый bug palette LUT) — дыры остаются. Identity caches должны быть doc-scoped.

### 9.3 Save As первого после открытия второго

```
Activate A → save_project_as
  → begin_io(); assemble_layer_*(doc_id = A.id)
  → Raw A still pinned → PNG ok
  → SessionGone только если вкладку успели закрыть
```

### 9.4 Закрыть вкладку

```
close_document(A)
  → if io_inflight: error
  → remove session; evict_document(A)  // Raw A gone
  → active = max(remaining) или None
  → tabs-changed; frontend refresh
```

---

## 10. Карта файлов

| Путь | Роль |
|------|------|
| `src-tauri/src/document_session.rs` | Registry, alloc/activate/close, pressure helpers, tabs emit, IO guard |
| `src-tauri/src/commands.rs` | AppState, open/save/export, tab IPC |
| `src-tauri/src/main.rs` | Budget 512 MiB; `handle_tile_request` |
| `src-tauri/src/worker.rs` / `tile_pipeline.rs` | Compute + pressure после insert |
| `src-tauri/src/viewport.rs` | Schedule с `doc` active snapshot |
| `src-tauri/src/undo.rs` | Undo per session; dirty active |
| `src-tauri/src/tile_protocol.rs` | Parse `tile://…/doc/{id}/…` |
| `crates/engine-tiles/src/types.rs` | `TileKey.doc` |
| `crates/engine-tiles/src/cache.rs` | `EvictContext`, Raw pin, `evict_for_pressure` |
| `crates/engine-color/src/palette_cache.rs` | KD `(doc, palette)` |
| `crates/engine-color/src/palette_lut.rs` | LUT `(doc, palette)` |
| `crates/engine-project/src/serialize/pixels.rs` | Doc-aware assemble |
| `frontend/src/app/slices/tabsSlice.ts` | Tab state + thunks |
| `frontend/src/app/slices/documentSlice.ts` | Active meta + epoch |
| `frontend/src/features/document/DocumentTabBar.tsx` | Tab UI |
| `frontend/src/shared/ipc/document.ts` | list / setActive / close |
| `frontend/src/features/preview/TileCanvas.tsx` | Doc-bound preview |
| `frontend/src/app/listeners.ts` | Event bridge + epoch |
| `frontend/src/hooks/useWelcomeScreen.ts` | Open/new/recent + quit guard |

---

## 11. Известные ограничения и follow-ups

План M1–M7: [`.cursor-spec/multi-doc-global-fix/`](../.cursor-spec/multi-doc-global-fix/SPEC.md). Статус после `fix/multi-doc-global`:

| Тема | Статус | Spec |
|------|--------|------|
| Мутации IPC с явным `doc_id` | Done — `require_session(doc_id)` на writes | **M1 P0** |
| UnsavedGuard на close вкладки | Done — per-tab Save / Don’t Save / Cancel | **M2 P0** |
| Soft trim Composite на deactivate | Явный trim **не** делаем; **pressure уже** сносит inactive Composite/Processed недетерминированно (warm vs cold return-to-tab) | **M3** (docs) |
| 512 MiB «Budget» | Потолок в основном Processed/Composite; N×Raw ~149 MiB pinned вне него; warning при ≥3 docs | **M4a**; **ADR D** later |
| Raw вне TileCache / reload (ADR **D**) | Follow-up; единственный реальный потолок process RAM | **M4b** ↑ priority |
| Neighbor activation после close | Done — right else left | **M5** |
| `dirty-changed` / undo events с `doc_id` | Done — listeners ignore foreign | **M6** |
| Литерал `doc: 1` | CI: `lint:no-magic-doc1` | **M7** |
| Quit multi-dirty | Done — sequential UnsavedGuard for all dirty tabs | — |
| Per-doc доли бюджета / memory UI | Follow-up | — |
| Split view / два холста | Non-goal | — |

---

## 12. Чеклист ручной приёмки

1. Welcome → Open A → вкладка A active, preview ок.
2. Open B (не закрывая A) → две вкладки; B active; A остаётся в strip.
3. Switch A ↔ B несколько раз → тайлы восстанавливаются (возможна краткая пустота на recompute).
4. На A: Save As / Export → успех после открытия B.
5. На B: Save As / Export → успех (не «doc 1»).
6. Два документа с разными размерами палитр + dither → нет panic на export / дыр от LUT collision.
7. Close A во время idle → A исчезла; B цела; Raw A снят из кэша.
8. Close во время Save/Export → отказ «in progress», документ не пропал.
9. Закрыть все вкладки → welcome / empty state.
10. `tile://` запрос с `doc_id` закрытой сессии → 404.

---

## 13. Связанные ADR (кратко)

1. **Registry + monotonic runtime id** — не второй глобальный handle; id не reuse.
2. **`TileKey.doc`** — изоляция тайлов / residuals / BRC.
3. **Budget 512 MiB + pressure** — inactive first; Raw open sessions hard-pinned.
4. **Doc-aware assemble + split errors** — SessionGone vs RawIncomplete.
5. **Palette caches `(doc, palette_id)`** — нет cross-doc LUT collision.
6. **Tab chrome** — вкладки в title area; registry уже готов до UI.

Спеки-источники лежат в `.cursor-spec/`; этот файл — стабильный as-built обзор для разработчиков и QA.
