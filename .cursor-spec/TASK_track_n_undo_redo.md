# Track N — Undo/Redo (Command/Snapshot History)

> Формальная спека: [track-n-undo-redo/](./track-n-undo-redo/)
> ([requirements](./track-n-undo-redo/requirements.md) ·
> [design](./track-n-undo-redo/design.md) ·
> [tasks](./track-n-undo-redo/tasks.md)).
> Этот файл — исходный бриф; при расхождении со спекой побеждает спека.

## Почему snapshot, не command/diff

`Document` уже clone-on-write через `ArcSwap` — мутация клонирует дерево
структурно (persistent data structure: неизменённые поддеревья остаются
тем же `Arc`, клонируется только путь от корня до места мутации). Полный
снапшот `Arc<Document>` в undo-стеке **не** означает полную копию всего
дерева на каждый шаг — это ровно та же экономика, на которой уже держится
`ArcSwap`-модель. Command/diff-подход (хранить не снапшоты, а обратные
операции) дал бы теоретически меньше памяти, но требует отдельной "инверсной"
реализации для каждой мутации (add_layer ↔ remove_layer, add_filter ↔
remove_filter, update_params ↔ update_params(old_value), и т.д.) — двойная
поверхность для багов при том же практическом выигрыше, раз structural
sharing уже почти бесплатен. **Решение: snapshot, не command/diff.**

**Важное ограничение scope:** снапшот `Document` не включает растровые
пиксельные данные (они не часть `Document`, живут в `TileCache` по
`LayerId`). Сейчас в модели нет редактирования сырых пикселей (paint/brush) —
только импорт (один раз при загрузке слоя) и фильтры (детерминированные
функции от structure+params, которые и так пересчитываются). Поэтому
snapshot структуры **достаточен и полон** для текущей модели. Если в
будущем появится рисование по пикселям — эта конструкция потребует
пересмотра (снапшот тогда должен либо включать diff пиксельных правок, либо
raw-редактирование должно жить вне undo-снапшота отдельным механизмом). Не
решать сейчас, просто зафиксировать как явную границу применимости.

---

## 1. Структура

```rust
pub struct UndoManager {
    undo_stack: VecDeque<Arc<Document>>, // bounded
    redo_stack: Vec<Arc<Document>>,
    max_depth: usize, // = 50
}
```

`max_depth = 50` — стартовое значение, зафиксировать явно (не оставлять
"разумное число" неопределённым, как уже обсуждалось в других треках).
Пересмотреть по факту профилирования памяти на реальных документах, не
угадывать заранее.

## 2. Единая точка входа для мутаций — не размазывать по каждому command handler

Каждый Tauri command, который сейчас мутирует `Document` (add_layer,
remove_layer, add_filter, update_filter_params, reorder, palette CRUD и
т.д.), должен идти через одну общую обёртку, а не пушить в undo-стек
самостоятельно в каждом обработчике (иначе кто-то забудет один из ~15
мест мутации — тот же класс риска, что уже обсуждался для per-filter
blend/opacity обёртки в Track I):

```rust
fn mutate_document_with_undo<F>(state: &AppState, mutate: F) -> Result<(), AppError>
where F: FnOnce(&Document) -> Document {
    let current = state.document_handle.load_full();
    let new_doc = mutate(&current);

    let mut undo = state.undo_manager.lock();
    undo.undo_stack.push_back(current.clone()); // Arc clone, дёшево
    if undo.undo_stack.len() > undo.max_depth {
        let dropped = undo.undo_stack.pop_front();
        gc_orphaned_layers(state, dropped, &undo, &new_doc); // см. §4
    }
    undo.redo_stack.clear(); // новая мутация после undo обрывает redo-историю — стандартная семантика

    state.document_handle.store(Arc::new(new_doc));
    Ok(())
}
```

Все существующие command handlers переписать на вызов этой обёртки вместо
прямой мутации `document_handle`.

**Что НЕ идёт через эту обёртку:** viewport/pan/zoom, panel layout, выбор
инструмента, состояние выделения — это не мутации `Document` и уже
персистятся отдельно (`panel_persistence.rs`), не дублировать здесь.

## 3. Undo / Redo команды

```rust
#[tauri::command]
fn undo(state: State<AppState>) -> Result<UndoStateDto, AppError> {
    let mut undo = state.undo_manager.lock();
    let Some(prev) = undo.undo_stack.pop_back() else { return Err(AppError::NothingToUndo) };
    let current = state.document_handle.load_full();
    undo.redo_stack.push(current);
    state.document_handle.store(prev);
    // инвалидация тайлов/кэшей по тому же пути, что использует open_project
    // при замене документа — переиспользовать существующий механизм, не писать новый
    Ok(undo.state_dto())
}
```

`redo` — зеркально. `UndoStateDto { can_undo: bool, can_redo: bool }` —
возвращать после каждой операции, фронт обновляет disabled-состояние
пунктов меню из этого, не опрашивая отдельно.

**Очистка стеков:** при `load_image`/`open_project`/`create_document` (Track
E / Welcome Screen) — оба стека **обязательно** очищаются. Нельзя undo
"через" смену документа — так ведёт себя любой нормальный редактор, и это
также автоматически решает вопрос "что если стек ссылается на LayerId
из уже закрытого документа".

## 4. GC осиротевших тайлов — критическая часть, не опция

Когда `LayerId` перестаёт встречаться **и** в `undo_stack`, **и** в
`redo_stack`, **и** в текущем `Document` — его тайлы (`Raw`, `Processed`,
любые per-layer записи в `ErrorResidualsStore`/`BlockRepresentativeCache`)
можно и нужно удалить из `TileCache`. Без этого шага — единственный сценарий
в текущей кодовой базе, где вытеснение `TileCache` реально понадобится и
станет наблюдаемым, будет молча копить память на каждом цикле
добавить-слой/undo.

```rust
fn gc_orphaned_layers(state: &AppState, dropped: Option<Arc<Document>>, undo: &UndoManager, new_doc: &Document) {
    let Some(dropped) = dropped else { return };
    let dropped_layer_ids: HashSet<LayerId> = collect_layer_ids(&dropped);
    let still_referenced: HashSet<LayerId> = undo.undo_stack.iter()
        .chain(undo.redo_stack.iter())
        .flat_map(|d| collect_layer_ids(d))
        .chain(collect_layer_ids(new_doc))
        .collect();
    for id in dropped_layer_ids.difference(&still_referenced) {
        state.tile_cache.evict_layer(*id); // первый реальный вызов эвикшена в кодовой базе
    }
}
```

Тот же GC нужно прогонять и в `undo()`/`redo()` (не только при переполнении
`max_depth`) — если, скажем, три `undo` подряд отбрасывают ветку `redo`,
которая раньше держала живым какой-то `LayerId` (актуально после нового
действия, обрывающего `redo_stack` в `mutate_document_with_undo`, §2) — тот
же принцип: пересчитать объединение живых `LayerId` после каждого изменения
состава стеков, не только на переполнение по глубине.

**Важное следствие для остальной кодовой базы:** это первый код,
реально вызывающий эвикшен `TileCache`. Это означает, что некоторые ветки,
которые раньше считались "недостижимыми в рантайме" (например, else-ветка
в `tile_pipeline.rs` про отсутствующий raw соседа, диагностированная в
рамках Track A/B) **могут стать реально достижимыми** после того, как этот
трек смёржен. Перепрогнать те диагностические тесты (счётчик на else-ветке)
после того как Track N в проде — не предполагать, что старые выводы
("счётчик всегда 0") останутся верны после появления первого реального
вызова эвикшена.

## 5. Debounce и границы "одного шага истории"

Undo-снапшот **не** должен пушиться на каждое промежуточное значение при
перетаскивании слайдера — иначе один drag даст десятки undo-шагов, каждый
из которых неотличим глазом. Использовать **ту же границу debounce**, что
уже запланирована для IPC-обновлений параметров (Track K, 100ms debounce) —
undo-снапшот пушится в тот же момент, когда debounced-значение реально
уходит на backend и мутирует `Document`, не раньше. Не заводить отдельный,
собственный debounce-таймер для undo — рассинхрон между "когда обновился
превью" и "когда записался undo-шаг" будет путать пользователя (Ctrl+Z
"отменяет" не то визуальное состояние, которое он видел последним).

## 6. Frontend

- `Edit → Undo/Redo` — `disabled` управляется через `UndoStateDto` из
  ответа последней мутации/undo/redo-вызова (не отдельный polling-запрос).
- Tauri accelerator `⌘Z`/`⌘⇧Z` (и `Ctrl+Z`/`Ctrl+Shift+Z` не-Mac) — сейчас
  отсутствует, добавить в конфиг меню.
- **Дополнительно к accelerator — глобальный keydown-хендлер во
  frontend.** На некоторых платформах/webview нативный menu accelerator не
  срабатывает, когда фокус находится в текстовом/числовом input (частый
  баг в реальных приложениях) — проверить это явно как критерий приёмки,
  не полагаться только на нативный accelerator.

## 7. Тесты

1. `add_layer` → `undo` → слоя нет, композит идентичен состоянию до
   добавления → `redo` → слой на месте, композит идентичен состоянию после
   добавления.
2. `undo` → новая мутация → `redo_stack` очищен (обычная mutation после
   undo обрывает возможность redo).
3. `max_depth + 5` мутаций подряд, `max_depth` раз `undo` → дальше `undo`
   даёт понятную ошибку "nothing to undo", не панику, не откат за пределы
   границы истории.
4. **GC-тест:** добавить слой → совершить достаточно мутаций, чтобы он
   вышел за пределы обоих стеков → явно проверить, что его тайлы реально
   удалены из `TileCache` (не просто "тест не упал", а прямая проверка
   отсутствия записей по этому `LayerId`).
5. Debounce: серия быстрых update_filter_params (имитация drag) →
   ровно один undo-шаг после debounce-паузы, не N шагов.
6. `⌘Z` работает, когда фокус на числовом input-поле панели параметров.
7. `open_project`/`load_image`/`create_document` → оба стека пустые,
   `can_undo = can_redo = false` сразу после.

## Критерии приёмки

1. Все существующие мутирующие command handlers переведены на
   `mutate_document_with_undo`, ни один не пушит в undo-стек напрямую.
2. GC осиротевших `LayerId` реализован и покрыт тестом (п.4 выше) — не
   оставлен как "потом", учитывая что это первый реальный эвикшен-вызов в
   кодовой базе и без него это гарантированная утечка при обычном
   использовании (add/undo циклы — частый паттерн реального редактирования).
3. Debounce-граница undo совпадает с debounce-границей Track K, не
   отдельный таймер.
4. Keyboard shortcut работает вне зависимости от фокуса на input-поле.
5. После смёржа — перепрогнать диагностические тесты Track A/B про
   недостижимость эвикшен-веток, обновить выводы если поведение изменилось.
