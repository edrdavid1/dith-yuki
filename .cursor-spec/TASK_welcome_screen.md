# Стартовый экран: New Project / Open Image / Open Project / Recent

> **Status (2026-08-13):** реализовано — см. [track-g-welcome/tasks.md](./track-g-welcome/tasks.md)
> (все пункты G0–G5 и Definition of Done отмечены). As-built: [ARCHITECTURE.md](../ARCHITECTURE.md) §3.9.

Формальная спека: [track-g-welcome/](./track-g-welcome/)
([requirements](./track-g-welcome/requirements.md) ·
[design](./track-g-welcome/design.md) ·
[tasks](./track-g-welcome/tasks.md)).
Этот файл — исходный бриф; при расхождении со спекой побеждает спека.

## Контекст — не изобретать заново

- Уже есть `frontend/src/components/EmptyState.tsx` — "No-document placeholder".
  Это естественное место для стартового экрана — **расширить/заменить его
  содержимое**, не создавать параллельный компонент, который будет
  конкурировать за один и тот же рендер-слот в `App.tsx`.
- Уже есть паттерн JSON-персистентности на диске — `panel_persistence.rs`.
  Список Recent Files — использовать **тот же паттерн** (новый модуль рядом,
  не новый механизм персистентности).
- Уже есть `useDocument.ts` (открытие/сохранение/экспорт) и (после Track E)
  IPC для `open_project`/`save_project`. Recent-экран — тонкая обвязка над
  уже существующими путями, не новая бизнес-логика загрузки.
- "New Project" — единственная реально **новая** backend-возможность в этой
  задаче: сейчас документ создаётся только через `load_image` (декодирование
  реального файла). Создание пустого документа без изображения — новый путь.

---

## 1. Backend — Recent Files

### 1.1 Персистентность

Новый модуль `src-tauri/src/recent_files.rs`, по образцу `panel_persistence.rs`:

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct RecentFileEntry {
    pub path: String,
    pub kind: RecentFileKind, // Image | Project
    pub display_name: String, // basename, без директории — для UI
    pub opened_at: String,    // ISO-8601
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RecentFileKind { Image, Project }

const MAX_RECENT: usize = 10;

pub fn load_recent_files() -> Vec<RecentFileEntry> {
    // читает JSON из app-data-dir (тот же root, что panel_persistence),
    // при отсутствии файла — пустой список, не ошибка
}

pub fn record_recent_file(path: &str, kind: RecentFileKind) {
    // 1. читает текущий список
    // 2. если path уже есть в списке — удаляет старую запись (не дублирует)
    // 3. вставляет новую запись в начало
    // 4. обрезает до MAX_RECENT
    // 5. записывает обратно на диск
}
```

### 1.2 Где вызывать `record_recent_file`

**На backend, сразу после успешного открытия**, не полагаться на то, что
фронтенд не забудет вызвать отдельную команду:
- В обработчике `load_image` (после успешного decompose) → `record_recent_file(path, Image)`.
- В обработчике `open_project` (после успешной загрузки, Track E) → `record_recent_file(path, Project)`.

Если `load_image`/`open_project` завершаются ошибкой — запись **не**
добавляется (не засорять список путями, которые не открылись).

### 1.3 IPC-команда чтения списка

```rust
#[tauri::command]
fn get_recent_files() -> Vec<RecentFileEntry> {
    let entries = recent_files::load_recent_files();
    // Валидация на чтение: убрать записи, чьи файлы больше не существуют
    // на диске (std::path::Path::exists()) — список маленький (≤10),
    // дешёво проверить на каждый вызов. Если что-то отфильтровано —
    // сразу перезаписать персистентный список без "мёртвых" записей.
    entries.into_iter().filter(|e| Path::new(&e.path).exists()).collect()
}
```

### 1.4 Backend — "New Project"

```rust
#[tauri::command]
fn create_document(width: u32, height: u32, background: BlankBackground) -> Result<DocumentDto, AppError> {
    // 1. Валидация: те же границы, что уже действуют для load_image
    //    (найти существующую константу "Max 8192×8192", переиспользовать,
    //    не заводить новую отдельную).
    // 2. Сгенерировать RGBA-буфер width×height в памяти:
    //    - Transparent: все альфа = 0
    //    - White: RGB=1.0 (linear), alpha=1.0
    //    (Color-picker для произвольного фона — не в MVP, см. Non-goals)
    // 3. Пропустить через ТОТ ЖЕ decompose-путь, что использует load_image
    //    для реального файла (decompose_image_to_tiles) — не писать
    //    отдельный код инициализации тайлов, переиспользовать один код.
    // 4. Создать Document с одним leaf Layer (raster, этот сгенерированный
    //    буфер), без фильтров, revision=1.
    // 5. Заменить document_handle в AppState (тот же путь замены, что
    //    load_image/open_project используют — единый механизм).
    // 6. project_path = None (это новый несохранённый проект).
    // 7. НЕ добавлять в Recent Files (это не файл на диске — нечего
    //    запоминать как путь, пока пользователь не сохранит через
    //    Track E save_project).
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum BlankBackground { Transparent, White }
```

**Non-goals для New Project (MVP):** выбор color profile (наследовать
дефолт, который уже использует `load_image`), произвольный цвет фона
(только Transparent/White), пресеты размеров (можно добавить позже как
чисто фронтенд-фичу поверх этой же команды — задать `width`/`height` из
пресета вместо ручного ввода).

---

## 2. Frontend

### 2.1 Компонент экрана

Расширить `EmptyState.tsx` (не создавать параллельный компонент). Структура:

```
WelcomeScreen (внутри/вместо EmptyState)
├── Логотип/название приложения
├── Primary actions (3 кнопки в ряд или колонкой):
│   ├── "New Project"   → открывает NewProjectDialog
│   ├── "Open Image…"   → существующий useDocument.openImage()
│   └── "Open Project…" → новый useDocument.openProject() (Track E IPC)
└── Recent section
    ├── Если список пуст — не показывать секцию вообще (не пустая
    │   рамка с текстом "no recent files", просто скрыть блок)
    └── Если не пуст — список записей:
        ├── Иконка типа (image vs project — разные иконки)
        ├── display_name (жирным)
        ├── путь усечённый (серым, мелким, для контекста)
        ├── относительное время ("2 hours ago") — форматировать на
        │   фронте из ISO-строки, не хранить готовую строку на бэкенде
        │   (иначе "2 hours ago" протухнет и не обновится, пока список
        │   не перезапросят)
        └── клик → открыть через соответствующий kind:
            Image → openImage(path) (тот же путь, что обычный Open)
            Project → openProject(path)
```

### 2.2 NewProjectDialog (новый модальный компонент)

Поля: Width (число, px), Height (число, px), Background (radio:
Transparent / White). Кнопка "Create" → `invoke('create_document', {...})`
→ после успеха документ заменяется как обычно (тот же механизм, что
срабатывает после `load_image`, скорее всего уже подписан на события
tile-ready/document-changed — переиспользовать существующую подписку,
не городить отдельный путь обновления UI).

Валидация на фронте до отправки: неотрицательные целые, разумный верхний
предел (то же 8192×8192, чтобы не полагаться только на backend-ошибку —
но backend всё равно должен валидировать сам, фронт — только UX, не
единственная линия защиты).

### 2.3 Хук

`frontend/src/hooks/useRecentFiles.ts` (новый, по образцу существующих
хуков в `hooks/`):

```ts
export function useRecentFiles() {
  const [entries, setEntries] = useState<RecentFileEntry[]>([]);
  const refresh = useCallback(async () => {
    setEntries(await invoke('get_recent_files'));
  }, []);
  useEffect(() => { refresh(); }, [refresh]);
  return { entries, refresh };
}
```

Вызывать `refresh()` после успешного открытия чего-либо (после
`openImage`/`openProject`/`createDocument` завершились успехом) — список
должен обновиться сразу при следующем визите на Welcome-экран, не только
при перезапуске приложения.

### 2.4 MenuBar — та же функциональность, не дублирующая логика

В `MenuBar.tsx`, секция File — добавить:
- "New Project…" (тот же `NewProjectDialog`)
- "Open Project…" (тот же `openProject()`)
- "Open Recent" — подменю, список из **того же** `useRecentFiles()` хука,
  не отдельный запрос/копия данных.

("Open Image…" там уже наверняка есть — не трогать, только добавить
недостающие три пункта рядом.)

---

## 3. Тесты

### Backend
- `record_recent_file` дважды с одним и тем же path → список содержит
  одну запись (не дублирует), с обновлённым `opened_at`, в начале списка.
- Список обрезается до `MAX_RECENT` при превышении.
- `get_recent_files` фильтрует несуществующие пути и сохраняет очищенный
  список обратно на диск.
- `create_document`: валидация размеров (превышение лимита → ошибка,
  не паника); успешный вызов даёт документ с одним пустым leaf-слоем,
  `project_path = None`.
- `create_document` **не** попадает в Recent Files (проверить явно, это
  легко забыть реализовать правильно).

### Frontend
- Recent-секция не рендерится при пустом списке (не пустая рамка).
- Клик по recent-записи с `kind: Image` вызывает `openImage`, с
  `kind: Project` — `openProject` (не путать пути).
- NewProjectDialog: невалидный размер (0, отрицательное, > лимита) не
  даёт отправить форму / показывает inline-ошибку до вызова backend.

---

## Критерии приёмки

1. При отсутствии открытого документа показывается Welcome-экран (не
   пустой canvas) с тремя основными действиями и (если есть история) Recent.
2. New Project создаёт документ без обращения к диску, готовый к работе
   (можно сразу добавлять слои/фильтры).
3. Open Image / Open Project с Welcome-экрана используют те же IPC-пути,
   что и соответствующие пункты меню — нет дублирования логики открытия.
4. Recent Files переживает перезапуск приложения (персистентность на диске,
   как panel layout).
5. MenuBar → File содержит те же 4 действия, синхронизирован с Welcome-экраном
   по данным (один хук, не два независимых источника Recent).
