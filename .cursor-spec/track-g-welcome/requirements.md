# Requirements: Track G — Welcome Screen

> **Status (2026-08-13):** implemented. As-built: [ARCHITECTURE.md](../../ARCHITECTURE.md) §3.9;
> checklist: [tasks.md](./tasks.md).

## Introduction

Формализация [TASK_welcome_screen.md](../TASK_welcome_screen.md). Цель — при отсутствии открытого документа показывать **стартовый экран** (New Project / Open Image / Open Project / Recent) вместо пустого placeholder, плюс тот же набор действий в File-меню.

Это **обвязка над существующими путями**, не новая модель документа:

| Уже есть | Этот трек |
|----------|-----------|
| `EmptyState` в `PreviewFeature` (слот no-document) | Расширить содержимое; не создавать параллельный экран |
| `panel_persistence.rs` (JSON в app-data-dir) | Тот же паттерн для Recent Files |
| `useDocument.openImage` / `openProject` (Track E) | Welcome и меню вызывают те же функции |
| `load_image` (документ только из файла) | `create_document` — единственный новый backend-путь |

**Зависимость:** Track E (`open_project` / `save_project` / `project_path`) уже в дереве. Трек независим от A–D и F.

## Glossary

- **Welcome_Screen**: содержимое no-document слота (сегодня `EmptyState`) с primary actions и опциональным Recent.
- **Recent_Files**: JSON-список ≤10 последних успешно открытых/сохранённых путей (Image | Project) в app-data-dir.
- **Blank_Document**: документ, созданный `create_document` без чтения с диска: один raster leaf, `project_path = None`.
- **Shared_Open_Path**: один фронтенд-путь открытия (`useDocument` + RTK thunks); Welcome, Recent и MenuBar не дублируют IPC.
- **MAX_DOCUMENT_DIMENSION**: существующий лимит `8192` из `load_image`; переиспользовать, не заводить второй.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Welcome вместо пустого canvas при `!hasDocument` | Параллельный компонент рядом с `EmptyState` |
| Blank document без файла на диске | Color picker фона, пресеты размеров как backend |
| Recent переживает рестарт (как panel layout) | Thumbnails, pin, «remove from recent» в MVP |
| File-меню синхронизировано с Welcome (один источник Recent) | Multi-document, close-document UX redesign |
| `create_document` через тот же decompose, что `load_image` | Новый механизм персистентности (БД, localStorage) |

---

## Requirements

### Requirement 1: Recent Files Persistence

**User Story:** As a user, I want recently opened images and projects to reappear after I quit the app, so I can resume without hunting for the file.

#### Acceptance Criteria

1. THE workspace SHALL add `src-tauri/src/recent_files.rs` following the `panel_persistence.rs` idiom: JSON file in the same app-data-dir root (`recent_files.json` next to `panel_state.json`).
2. A persisted entry SHALL include at least: `path` (string), `kind` (`image` | `project`, serde lowercase), `display_name` (basename, no directory), `opened_at` (ISO-8601). Relative-time strings SHALL NOT be stored.
3. `record_recent_file` SHALL: load the list; if `path` already exists, drop the old entry (no duplicates); insert the new entry at the front; truncate to `MAX_RECENT = 10`; write back. Missing or corrupt file SHALL be treated as an empty list (not an error to the caller).
4. `record_recent_file` SHALL be invoked **on the backend after a successful** `load_image` (`kind: Image`), `open_project` (`kind: Project`), and `save_project` / `save_project_as` (`kind: Project`). Failed commands SHALL NOT record. `create_document` SHALL NOT record (no path on disk yet).
5. Core load/record helpers SHALL be unit-testable with an injected file path (temp dir); `AppHandle` is only for resolving app-data-dir at the command boundary — same split as panel persistence parse-vs-path.

### Requirement 2: Recent Files Read IPC and Dead-Entry Prune

**User Story:** As a user, I want the Recent list to only show files that still exist, so clicking an entry does not fail on a deleted path.

#### Acceptance Criteria

1. Tauri command `get_recent_files` SHALL return the persisted list filtered by `std::path::Path::exists()`.
2. WHEN any entries are dropped by that filter, THE command SHALL rewrite the persisted file without the dead entries (list is ≤10; existence checks are cheap).
3. IF the rewrite fails, THE command SHALL still return the filtered in-memory list and SHALL NOT fail the IPC call (log the write error).
4. Frontend SHALL fetch this list via a dedicated hook `useRecentFiles` (see Req 6); it SHALL NOT keep a second independent copy of Recent data.

### Requirement 3: Create Blank Document

**User Story:** As a user, I want to start a new project with a chosen size and background without opening an image file.

#### Acceptance Criteria

1. Tauri command `create_document(width, height, background)` SHALL validate dimensions with the **same** bounds as `load_image`: both axes in `1..=MAX_DOCUMENT_DIMENSION` (8192). Out of range SHALL return an error string (no panic). Zero and values above the max are invalid.
2. THE command SHALL allocate an in-memory RGBA f32 buffer `width×height` in the **same numeric space as `load_image`’s decoded buffer** (`u8/255.0`, no new color-management path): `Transparent` → all zeros including alpha; `White` → RGB=1.0, alpha=1.0.
3. THE buffer SHALL go through `decompose_image_to_tiles` (same helper as `load_image`), then a `Document` with `doc_id = 1`, one raster leaf layer, no filters, `revision`/generation as `load_image` sets. THE command SHALL replace `document_handle` via the same mutate+invalidate+schedule+`emit_document_changed` path as `load_image`.
4. `AppState.project_path` SHALL be set to `None` (unsaved project). THE command SHALL NOT call `record_recent_file`.
5. Color profile / working space SHALL inherit whatever `load_image` already uses (no extra parameter). Arbitrary background color and size presets are Non-Goals for this command (presets MAY later be frontend-only values sent as `width`/`height`).

### Requirement 4: Welcome Screen in the Existing Empty Slot

**User Story:** As a user with no document open, I want a welcome view with New Project, Open Image, Open Project, and recent files — not an empty canvas or a one-line placeholder.

#### Acceptance Criteria

1. THE no-document UI SHALL extend `frontend/src/components/EmptyState.tsx` (rename of the visual contents is allowed; the component file/slot stays). THE app SHALL NOT introduce a second component competing for the same render slot.
2. THE current render slot is `PreviewFeature` when `!hasDocument` (not `App.tsx` — the task brief was stale). Both the default `EmptyState` branch **and** the `fill` branch that today shows “No document open” SHALL use this Welcome contents so undocked preview is not a third empty UI.
3. Welcome SHALL show: app name/logo; three primary actions — **New Project** (opens NewProjectDialog), **Open Image…** (`useDocument.openImage` dialog path), **Open Project…** (`useDocument.openProject` dialog path).
4. WHEN Recent is empty, THE Recent section SHALL NOT render at all (no empty frame, no “no recent files” copy). WHEN non-empty, each row SHALL show: kind icon (image vs project, visually distinct), `display_name` (emphasized), truncated `path` (secondary), relative time computed **on the frontend** from `opened_at`.
5. Clicking a Recent row SHALL open via Shared_Open_Path: `kind: image` → `openImage(path)` (no file dialog); `kind: project` → `openProject(path)` (no file dialog). These are the same thunks as the File menu, not a third IPC wrapper.

### Requirement 5: New Project Dialog

**User Story:** As a user creating a blank document, I want to set width, height, and background before the canvas appears.

#### Acceptance Criteria

1. A modal `NewProjectDialog` SHALL collect Width (px), Height (px), and Background (radio: Transparent / White), then invoke `create_document`.
2. Frontend validation SHALL reject non-integers, values `< 1`, and values `> MAX_DOCUMENT_DIMENSION` **before** invoke (inline error; Create disabled or no-submit). Backend SHALL still validate (Req 3.1) — frontend is UX, not the only guard.
3. Default values SHALL be width `1920`, height `1080`, background `Transparent` (locked in design; presets as extra buttons are optional frontend sugar, not required).
4. On success, THE document SHALL replace in the UI via the existing `document-changed` / RTK `hasDocument` path used after `load_image` — no separate Welcome-only update pipeline.
5. THE same dialog instance/flow SHALL serve Welcome and File → New Project… (one component, not two implementations).

### Requirement 6: Shared Frontend Wiring

**User Story:** As a user, I want File menu and Welcome to offer the same actions and the same Recent list, without one going stale.

#### Acceptance Criteria

1. `frontend/src/hooks/useRecentFiles.ts` SHALL load via `get_recent_files` on mount and expose `{ entries, refresh }`.
2. Welcome and MenuBar SHALL consume **one** hook instance (lifted to `AppLayout` or equivalent parent) — not two independent fetches that can diverge.
3. `refresh()` SHALL run after successful `openImage` / `openProject` / `createDocument` (and after save that records a project path) so the next visit to Welcome/Open Recent is current without restarting the app.
4. `useDocument` SHALL expose path-parameterized open helpers for Recent (dialog-less) in addition to the existing dialog-based Open Image / Open Project. Create-document SHALL be a thunk in `documentSlice` parallel to `openImage` / `openProject`.

### Requirement 7: MenuBar File Actions

**User Story:** As a user who already has a document (or prefers the menu), I want New Project and Open Recent next to the existing File items.

#### Acceptance Criteria

1. File menu SHALL add **New Project…** (opens the same NewProjectDialog) and **Open Recent** (entries from the shared `useRecentFiles` data).
2. **Open Image** and **Open Project…** already exist (Track E) — this track SHALL reuse them, not add duplicates.
3. WHEN Recent is empty, **Open Recent** SHALL be hidden or disabled-without-submenu (same empty policy as Welcome: no fake “empty” list). WHEN non-empty, choosing an entry SHALL use Shared_Open_Path (Req 4.5).
4. New Project SHALL remain available even when `hasDocument` is true (replaces the current document, same single-doc model as Open Image / Open Project).

### Requirement 8: Acceptance Quality

**User Story:** As a QA engineer, I want automated proof that Recent dedupes, prunes, and that blank documents never pollute Recent.

#### Acceptance Criteria

1. Backend tests SHALL cover: duplicate path → one entry, moved to front, `opened_at` updated; truncation to `MAX_RECENT`; `get_recent_files` drops missing paths and persists the cleaned list; `create_document` size errors; successful `create_document` yields one raster leaf and `project_path = None`; `create_document` does **not** appear in Recent.
2. Frontend tests SHALL cover: Recent section absent when `entries` is empty; Recent click dispatches image vs project open correctly; NewProjectDialog does not invoke backend on invalid size.

---

## Future (explicitly out of MVP)

- Size presets (A4, 4K, …) as frontend chips on the same command
- Arbitrary background color / color-profile picker
- Recent thumbnails, pin, remove-from-list
- Close Document returning to Welcome (if/when close exists)
- OS “new file” / drag-drop onto Welcome beyond whatever Open Image already does
