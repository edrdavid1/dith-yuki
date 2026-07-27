# Tauri API и модель документа

Продолжение блока архитектуры. Закрывает открытый вопрос §7 из первого документа (группы слоёв, формат передачи тайла на фронт) и весь пункт 6.1/6.2 ТЗ (API команд, структуры данных).

Выбор в пользу этого блока, а не БД проекта — потому что формат проекта (следующий документ) будет просто сериализацией структур `Document`, определённых здесь, плюс дельта-хранилище тайлов. Логичнее сначала зафиксировать модель, потом формат её хранения.

---

## 1. Три канала связи фронт↔бек — и почему не один

Наивно всё сделать через `invoke` (request/response). Проблема: пиксельные данные тайла (256×256×4×f32 ≈ 1 МБ до сжатия) через стандартный Tauri `invoke` в v1 сериализуются в JSON/base64 — это +33% размера и лишний парсинг на каждый тайл, при десятках тайлов в кадре это заметная просадка в 30 fps бюджет. Поэтому команды разнесены по трём разным транспортам, каждый под свою частоту и природу данных:

| Канал | Направление | Частота | Что передаёт |
|---|---|---|---|
| **Custom protocol** (`tile://...`) | Фронт → Бек, pull | Десятки/сек | Бинарные пиксели тайла (сжатые) |
| **`invoke`** | Фронт → Бек, request/response | Редко (клик, drag start/end) | Императивные команды (add_layer, save...) |
| **Events** (`emit`/`listen`) | Бек → Фронт, push | Часто, но малый payload | «Этот тайл теперь другой», прогресс, ошибки |

### 1.1 Custom protocol для пикселей

Регистрируется URI-схема `tile://`, которую фронт дёргает как обычный `fetch()`/`<img src>`:

```
tile://doc/{document_id}/layer/{layer_id}/stage/{stage}/l/{level}/{x}/{y}?gen={generation}
```

Пример: `tile://doc/42/layer/composite/stage/composite/l/2/13/7?gen=118`

Rust-обработчик протокола (регистрируется один раз при старте приложения):

```rust
.register_uri_scheme_protocol("tile", |app, request| {
    let key = parse_tile_key(request.uri())?;
    let cache = app.state::<Arc<TileCache>>();

    match cache.get_ready(key) {
        Some(entry) => http::Response::builder()
            .header("Content-Type", "image/webp")
            .header("Cache-Control", "no-cache") // gen в URL и так делает кеш-инвалидацию явной
            .status(200)
            .body(entry.compressed_bytes.clone())?,
        None => {
            // тайла ещё нет — планируем его немедленно (Immediate) и отдаём 202,
            // фронт узнает о готовности через событие tile-ready ниже
            cache.schedule(key, Priority::Immediate);
            http::Response::builder().status(202).body(vec![])?
        }
    }
})
```

Почему это лучше, чем `invoke` с бинарным ответом (который в Tauri 2 тоже возможен через `tauri::ipc::Response::new(bytes)`): браузерный `fetch`/`<img>` умеет нативно кешировать, приоритизировать, отменять (`AbortController`) и параллелить запросы — это ровно то поведение, которое нужно для тайлового рендеринга, и мы получаем его бесплатно от WebView вместо ручной реализации очереди на фронте.

`gen` в query-параметре — не для кеш-бастинга браузера (это делает `Cache-Control: no-cache`), а чтобы сам обработчик мог провалидировать, что фронт запрашивает актуальную версию, и залогировать/отбросить совсем устаревшие висящие запросы при быстрой перерисовке.

### 1.2 События (push, малый payload)

```rust
#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum EngineEvent {
    TileReady { key: TileKeyDto, generation: u64 },
    TileFailed { key: TileKeyDto, reason: String },
    DocumentStateChanged { revision: u64 }, // для undo/redo, панелей слоёв и т.п.
    ExportProgress { job_id: String, percent: f32 },
    ScratchDiskPressure { used_mb: u64, budget_mb: u64 }, // NFR про память — сигнал в UI
}
```

Фронт подписывается один раз (`listen('engine-event', ...)`), диспетчеризация по `type` внутри JS. `TileReady` — единственное событие в горячем пути; остальные единичны/редки.

### 1.3 `invoke` — только императивные команды

Раз в клик пользователя, не раз в кадр. Полный список — в §4.

---

## 2. Модель документа (Rust)

### 2.1 Дерево слоёв с группами

Открытый вопрос из предыдущего документа: композитинг группы — это под-композитинг с собственным blend/opacity. Решается рекурсивным деревом вместо плоского списка:

```rust
pub struct Document {
    pub id: DocumentId,
    pub width: u32,
    pub height: u32,
    pub color_profile: ColorProfileRef,
    pub root: Vec<LayerNode>,       // верхний уровень — обычный плоский список
    pub palettes: Vec<PaletteId>,
    pub revision: u64,              // растёт при любом структурном изменении (для undo)
    pub generations: GenerationTracker, // из предыдущего документа
}

pub enum LayerNode {
    Leaf(Layer),
    Group(LayerGroup),
}

pub struct LayerGroup {
    pub id: LayerId,           // группа тоже имеет TileKey-адресуемость для кеша Composite
    pub name: String,
    pub blend_mode: BlendMode,
    pub opacity: f32,
    pub visible: bool,
    pub mask: Option<MaskRef>,
    pub children: Vec<LayerNode>, // снизу вверх, как и root
}
```

Композитинг группы — рекурсивный вызов той же функции `composite_tile`, но на под-списке `children`, с результатом, к которому затем (как к обычному `Processed`-слою) применяются `mask`/`opacity`/`blend_mode` самой группы при вкладывании в родительский композит. Кеш `Composite` для группы адресуется тем же `TileKey { layer: group.id, coord, stage: Composite }` — с точки зрения кеша группа неотличима от обычного слоя, что и даёт рекурсию «бесплатно», без специального кода в `TileCache`.

Обход дерева (`layers_bottom_to_top`) — depth-first с плоским итератором, не аллоцирует список каждый раз:

```rust
pub fn walk_bottom_to_top<'a>(nodes: &'a [LayerNode]) -> impl Iterator<Item = LayerRef<'a>> {
    nodes.iter().flat_map(|n| match n {
        LayerNode::Leaf(l) => Either::Left(std::iter::once(LayerRef::Leaf(l))),
        LayerNode::Group(g) => Either::Right(
            std::iter::once(LayerRef::GroupStart(g))
                .chain(walk_bottom_to_top(&g.children))
                .chain(std::iter::once(LayerRef::GroupEnd(g)))
        ),
    })
}
```

### 2.2 Остальные сущности

```rust
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub kind: LayerKind,          // Raster | Adjustment — см. пред. документ
    pub blend_mode: BlendMode,
    pub opacity: f32,
    pub visible: bool,
    pub offset: (i32, i32),
    pub mask: Option<MaskRef>,
    pub filters: Vec<FilterInstance>,
    pub bounds_l0: TileBounds,
}

pub struct MaskRef {
    pub storage: MaskStorage,      // адрес растровых тайлов маски (свой TileKey namespace, stage не нужен — маска не проходит через filter-стадии)
    pub enabled: bool,
    pub inverted: bool,
}

pub struct FilterInstance {
    pub id: FilterInstanceId,     // стабилен для точечной инвалидации/пресетов
    pub kind: FilterKind,         // Curves, Lut3d, Dither, Glitch{..}, ...
    pub params: FilterParams,     // enum с конкретными полями на FilterKind
    pub enabled: bool,
    pub requires_full_row: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
pub enum BlendMode {
    Normal, Multiply, Screen, Overlay, Darken, Lighten,
    ColorDodge, ColorBurn, HardLight, SoftLight, Difference, Exclusion,
    // 10+ по ТЗ — конкретный список согласовать с дизайном/референсом Photoshop
}
```

`FilterInstanceId` отдельно от индекса в `Vec<FilterInstance>` — потому что пресеты и точечная инвалидация должны переживать переупорядочивание фильтров в стеке слоя; адресация по индексу массива сломалась бы при drag-n-drop реордеринге.

---

## 3. Формат данных, уходящих на фронт (snapshot)

Фронту нужна структура документа для панелей (слои, маски, фильтры) — но **не** пиксели (они идут отдельно через `tile://`). Поэтому `Document` не сериализуется целиком «как есть» в JSON: во-первых, там нет `LayerNode`-специфичных deny-list полей (внутренние индексы кеша), во-вторых, нужна плоская для React структура, а не рекурсивное дерево с самодельным enum-tagging.

```rust
#[derive(Serialize)]
pub struct DocumentSnapshotDto {
    pub id: DocumentId,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub layers: Vec<LayerNodeDto>, // плоский список с полем depth/parent_id для дерева на фронте
}

#[derive(Serialize)]
pub struct LayerNodeDto {
    pub id: LayerId,
    pub parent_group: Option<LayerId>,
    pub kind: &'static str, // "raster" | "adjustment" | "group"
    pub name: String,
    pub blend_mode: String,
    pub opacity: f32,
    pub visible: bool,
    pub has_mask: bool,
    pub filters: Vec<FilterInstanceDto>,
    pub thumbnail_url: String, // = tile:// URL на самый грубый уровень пирамиды, переиспользуем протокол
}
```

`thumbnail_url` как `tile://.../l/{max_level}/0/0` — миниатюра слоя в панели это буквально тайл верхнего уровня пирамиды, отдельный код для миниатюр не нужен.

---

## 4. Каталог команд `invoke`

Сгруппированы по областям; сигнатуры даны в терминах Rust-функций (Tauri сам генерирует биндинги под `#[tauri::command]`).

### 4.1 Документ и вьюпорт

```rust
#[tauri::command]
async fn open_document(path: String) -> Result<DocumentSnapshotDto, EngineError>;

#[tauri::command]
async fn get_document_snapshot(doc_id: DocumentId) -> Result<DocumentSnapshotDto, EngineError>;

#[tauri::command]
async fn set_viewport(doc_id: DocumentId, zoom: f32, center_x: f64, center_y: f64, viewport_w: u32, viewport_h: u32) -> Result<(), EngineError>;
// пересчитывает приоритеты в очереди планировщика (§5 пред. документа), не возвращает пиксели —
// они придут через tile:// запросы, которые фронт сам инициирует по новым видимым координатам
```

### 4.2 Слои

```rust
#[tauri::command]
async fn add_layer(doc_id: DocumentId, kind: LayerKindDto, parent_group: Option<LayerId>, index: usize) -> Result<LayerId, EngineError>;

#[tauri::command]
async fn remove_layer(doc_id: DocumentId, layer_id: LayerId) -> Result<(), EngineError>;

#[tauri::command]
async fn duplicate_layer(doc_id: DocumentId, layer_id: LayerId) -> Result<LayerId, EngineError>;

#[tauri::command]
async fn reorder_layer(doc_id: DocumentId, layer_id: LayerId, new_parent: Option<LayerId>, new_index: usize) -> Result<(), EngineError>;

#[tauri::command]
async fn group_layers(doc_id: DocumentId, layer_ids: Vec<LayerId>) -> Result<LayerId, EngineError>;

#[tauri::command]
async fn set_layer_props(doc_id: DocumentId, layer_id: LayerId, patch: LayerPropsPatch) -> Result<(), EngineError>;
// patch: Option<f32> opacity, Option<BlendMode>, Option<bool> visible, Option<(i32,i32)> offset
// -- один универсальный patch-command вместо set_opacity/set_blend_mode/set_visible по отдельности,
//    чтобы drag слайдера не генерировал команду другого типа, чем клик по чекбоксу видимости
```

### 4.3 Фильтры

```rust
#[tauri::command]
async fn add_filter(doc_id: DocumentId, layer_id: LayerId, kind: FilterKindDto, index: usize) -> Result<FilterInstanceId, EngineError>;

#[tauri::command]
async fn update_filter_params(doc_id: DocumentId, layer_id: LayerId, filter_id: FilterInstanceId, params: FilterParamsDto) -> Result<(), EngineError>;
// САМАЯ частая команда в приложении — вызывается на каждый кадр drag слайдера.
// Поэтому именно она не ждёт результата пересчёта: возвращает Ok(()) сразу после
// применения параметра к структуре Document и постановки задач в планировщик.

#[tauri::command]
async fn reorder_filter(doc_id: DocumentId, layer_id: LayerId, filter_id: FilterInstanceId, new_index: usize) -> Result<(), EngineError>;

#[tauri::command]
async fn remove_filter(doc_id: DocumentId, layer_id: LayerId, filter_id: FilterInstanceId) -> Result<(), EngineError>;
```

### 4.4 Маски, палитры, пресеты, undo — заголовки (детали в соответствующих будущих блоках)

```rust
#[tauri::command] async fn set_mask_enabled(doc_id: DocumentId, layer_id: LayerId, enabled: bool) -> Result<(), EngineError>;
#[tauri::command] async fn paint_mask_stroke(doc_id: DocumentId, layer_id: LayerId, stroke: StrokeDto) -> Result<(), EngineError>;

#[tauri::command] async fn import_palette(path: String, format: PaletteFormatDto) -> Result<PaletteId, EngineError>;
#[tauri::command] async fn generate_palette_from_layer(doc_id: DocumentId, layer_id: LayerId, method: PaletteGenMethod, color_count: u8) -> Result<PaletteId, EngineError>;

#[tauri::command] async fn apply_preset(doc_id: DocumentId, target: PresetTarget, preset_id: PresetId) -> Result<(), EngineError>;
#[tauri::command] async fn save_preset(doc_id: DocumentId, source: PresetTarget, name: String) -> Result<PresetId, EngineError>;

#[tauri::command] async fn undo(doc_id: DocumentId) -> Result<DocumentSnapshotDto, EngineError>;
#[tauri::command] async fn redo(doc_id: DocumentId) -> Result<DocumentSnapshotDto, EngineError>;
```

### 4.5 Проект и экспорт

```rust
#[tauri::command] async fn save_project(doc_id: DocumentId, path: Option<String>) -> Result<(), EngineError>; // None = сохранить по текущему пути
#[tauri::command] async fn export_image(doc_id: DocumentId, path: String, format: ExportFormatDto, options: ExportOptionsDto) -> Result<(), EngineError>;
```

---

## 5. Конкурентный доступ к `Document`

`Document` мутируется командами `invoke` (единичные, из main/async runtime потока Tauri) и **читается** worker-пулом планировщика (много потоков, много раз в секунду, для `composite_tile`/применения фильтров). Требования противоречивы: запись редкая, но должна быть быстрой (не блокировать UI-поток на 5+ мс), чтение частое и не должно ждать запись надолго.

Решение — `Document` не изменяется на месте под общим `RwLock`, а хранится через `arc-swap`:

```rust
pub struct DocumentHandle {
    current: ArcSwap<Document>,
}

impl DocumentHandle {
    pub fn snapshot(&self) -> Arc<Document> {
        self.current.load_full() // O(1), lock-free, для worker-потоков планировщика
    }

    pub fn mutate(&self, f: impl FnOnce(&mut Document)) {
        let mut new_doc = (**self.current.load()).clone(); // структурное клонирование, НЕ клонирование пиксельных тайлов (те в отдельном TileCache по Arc)
        f(&mut new_doc);
        new_doc.revision += 1;
        self.current.store(Arc::new(new_doc));
    }
}
```

Это работает дёшево, потому что `Document` — это только **метаданные** (структура слоёв, параметры фильтров, blend-моды), а не пиксели: клонирование дерева из 10–50 слоёв — микросекунды, тогда как пиксельные тайлы живут в `TileCache` по `Arc<PixelTile>` и никогда не клонируются при мутации документа. Worker, начавший `composite_tile` со старым `Arc<Document>`, спокойно доработает на консистентном снапшоте, даже если за это время пришла новая мутация — никаких блокировок и без «порванного» на середине чтения состояния.

---

## 6. Открытые вопросы для следующего блока

- Формат хранения `Document` + `TileCache` дельт на диске (следующий блок — формат проекта/БД) — сериализация `DocumentSnapshotDto`-подобной структуры плюс маппинг `TileKey → offset` в файле пакета.
- `ExportOptionsDto`/`PaletteFormatDto` и полный список форматов палитр — детали кодеков, не архитектуры, можно отложить до реализации.
- История undo/redo здесь не расписана подробно (только команды-заглушки) — нужно решить, храним ли полные снапшоты `Document` (дёшево благодаря `arc-swap`, structural sharing через `im`/persistent-структуры) или diff-патчи; предлагаю поднять отдельно, если нужно углубление.

---

Дальше по логике — **формат проекта / схема хранения на диске**, раз структуры данных теперь зафиксированы.
