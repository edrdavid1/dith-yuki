# Requirements: Track P — Beta product gate

## Introduction

Фичевой MVP (A–N) **уже в дереве**. Этот трек — не новый движок, а
**продуктовый контракт беты**: dirty-state, close-guard, Color Lab Apply
без копий, ручной QA. Буквы A–O не переоткрывать.

Два соседних хвоста уже имеют свои папки и **сюда не копируются**:

| Сосед | Папка | Роль в бете |
|-------|--------|-------------|
| C4.1 SVG | [track-c4-svg-followup/](../track-c4-svg-followup/) | Beta 0: выбор Pixel Grid / Contour + дырки |
| Color Lab §6 | [color-lab.md](../color-lab.md) задача 6 | Beta 0: веса chroma/contrast, default 0 |
| Track O | [track-o-updates/](../track-o-updates/) | Beta 1: in-app updates. O3 **потребляет** dirty из этого трека |

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md).

As-built, из-за которого трек нужен:

- `AppState.project_path` есть; `documentSlice.projectPath` есть; **нет**
  `unsaved` / dirty.
- `WindowEvent::CloseRequested` перехватывает только `panel-*` (dock).
  Главное окно `main` закрывается молча.
- Color Lab `handleApply` всегда `addPalette`. `Document::modify_palette`
  уже существует и не используется Apply.
- Auto-extract есть на `openImage`; `addRasterLayer` создаёт **пустой**
  raster, импорта картинки в слой нет.
- Track A §6.2 и Track D §5.3 — чеклисты в tasks, галки пустые.

## Glossary

- **Dirty_Flag**: документ расходится с последней точкой «чисто». Источник
  правды — backend, не догадка фронта.
- **Saved_Mark**: `Arc<Document>`, запомненный после успешного Save /
  Open Project / replace, который считается чистым.
- **Unsaved_Guard**: один трёхкнопочный диалог Save / Don’t Save / Cancel.
- **Replace_Doc**: `load_image` / `open_project` / `create_document` (и
  Future: любой полный replace). Стеки undo уже очищаются (Track N).
- **Apply_Replace**: Color Lab Apply пишет в выбранную палитру документа,
  а не всегда `add_palette`.
- **Beta_0**: внутренний DMG / `tauri build`. Друзья крутят продукт.
- **Beta_1**: тег `v0.2.0` + Track O. Повторяемый канал обновлений.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Dirty + Unsaved_Guard на close / New / Open / O3 | Autosave, crash recovery, multi-document |
| Apply обновляет выбранную палитру | Полный CRUD-редизайн палитр; silent merge builtins |
| Import Image as Layer + extract (**Beta 1**) | Scale/smart-fit импорта; file associations |
| Пройти и записать ручной QA A §6.2 / D §5.3 | Новые seam-фиксы (это регрессия Track A/D, не этот трек) |
| Зафиксировать критерии Beta 0 / Beta 1 | 1.0: mask UI, GPU default, pyramid>0 на фронте, paint-undo |

---

## Requirements

### Requirement 1: Dirty_Flag is Arc identity, not a frontend guess

**User Story:** As a user with undo and `.dyproj`, I want the app to know
whether my document matches the last save, including after Undo back to
that save.

#### Acceptance Criteria

1. THE backend SHALL keep a Saved_Mark: `Option<Arc<Document>>` on
   `AppState` (name locked in design). Dirty SHALL be
   `has_document && !Arc::ptr_eq(live, saved)` when Saved_Mark is `Some`.
   THE design SHALL NOT use `Document.revision` as the dirty signal
   (revision is reset on serialize/open and is the wrong identity).
2. AFTER a successful `save_project` / `save_project_as`, Saved_Mark SHALL
   be the **live** snapshot Arc (not the clone passed into the zipper).
3. AFTER `open_project`, Saved_Mark SHALL be the live snapshot (clean).
4. AFTER `load_image` / `create_document`, Saved_Mark SHALL be the live
   snapshot **immediately after install** (clean until the first
   undo-recorded mutation). Auto-extract that calls `generate_palette` /
   `add_palette` SHALL dirty the document (it already goes through
   `with_document_undo`).
5. AFTER a successful `with_document_undo` mutation, undo, or redo, dirty
   SHALL be recomputed from ptr_eq. Undo/redo back onto the Saved_Mark Arc
   SHALL be clean.
6. Welcome (`!hasDocument`) SHALL be clean. Closing the app from Welcome
   SHALL NOT show Unsaved_Guard.
7. THE backend SHALL expose dirty to the UI without polling: event
   `dirty-changed` `{ dirty: bool }` (plus a getter `is_document_dirty`
   for the first paint / Guard). Mutating command return types SHALL NOT
   all grow a dirty field.
8. Track O Restart_Guard SHALL skip the prompt when dirty is false, once
   this requirement lands. Until O is wired, the flag still exists for
   close / New / Open. This track SHALL add a one-line hook comment in
   [track-o-updates/design.md](../track-o-updates/design.md) pointing at
   Saved_Mark; it SHALL NOT reimplement O3.

### Requirement 2: One Unsaved_Guard, three entry points

**User Story:** As a user, I want the same Save / Don’t Save / Cancel
choice when closing the window, replacing the document, or installing an
update — not three different dialogs.

#### Acceptance Criteria

1. THE app SHALL present **one** Unsaved_Guard component (custom chrome,
   same family as `NewProjectDialog`, **not** `window.confirm`, **not** a
   native Tauri dialog). Buttons: **Save** / **Don’t Save** / **Cancel**.
   Copy locked in design.
2. WHEN dirty is false, all three entry points SHALL proceed with no
   dialog.
3. Entry points SHALL be:
   - Close of window `main` (`CloseRequested`; `api.prevent_close` until
     Guard resolves).
   - File → New Project / Open Image / Open Project / Open Recent (and
     Welcome equivalents) while `hasDocument`.
   - Track O Restart_Guard (consumed later; this track SHALL export a
     reusable `runUnsavedGuard(): Promise<'save' | 'discard' | 'cancel'>`
     that O3 calls).
4. **Save:** if `project_path` is `Some`, `save_project`; else existing
   Save As dialog. IF save fails or the user cancels Save As, abort the
   original action (do not close, do not replace, do not relaunch).
5. **Don’t Save:** proceed (close / replace / relaunch). Undo history is
   in-memory; discard is expected.
6. **Cancel:** no close, no replace, no download/relaunch.
7. Panel windows (`panel-*`) SHALL keep today’s dock-on-close behaviour.
   Unsaved_Guard SHALL NOT run on panel close.
8. Quit via OS (⌘Q / Dock) SHALL hit the same `main` CloseRequested path
   (or an equivalent Guard). Do not add a fourth copy of the dialog.

### Requirement 3: Title shows dirty and project identity

**User Story:** As a user with several windows in Mission Control, I want
to see whether the document is unsaved and which file it is.

#### Acceptance Criteria

1. THE main window title SHALL be
   `{optional "• "}{basename | "Untitled"} — Dither Engine`
   (keep the product prefix already in `tauri.conf.json` title
   `"Dither Engine"`). Dirty prefix is a bullet + space.
2. Basename SHALL come from `project_path` when set; otherwise
   `"Untitled"`. Full filesystem path in the title is **not** required.
3. Title SHALL update on dirty-changed, save, open, create, and replace.

### Requirement 4: Color Lab Apply replaces the selected document palette

**User Story:** As a user who auto-extracts then hits Apply, I do not want
a second identical palette in the list.

#### Acceptance Criteria

1. WHEN Color Lab has a `selectedPaletteId` that exists in
   `Document.palettes`, Apply SHALL **replace** that palette’s name +
   colors and bump **palette** revision (reuse `Document::modify_palette`
   or a thin IPC `replace_palette`). Filters that already reference this
   `PaletteId` SHALL keep the id; LUT/cache invalidation SHALL follow the
   existing `update_palette_color` path (same cascade).
2. WHEN no palette is selected, or the id is not in the document, Apply
   SHALL `add_palette` as today and select the new id.
3. AFTER `extractPalette` / auto-extract, the Color Lab selection SHALL
   become `lastCreatedId` so a subsequent Apply hits criterion 1, not 2.
4. Builtin import, palette-file import, and explicit “New palette”
   SHALL remain **add**. This requirement does not merge builtins into
   the current palette.
5. Apply SHALL go through `with_document_undo` (replace is a document
   mutation). One Apply = one undo step.
6. Tests: extract → Apply → `list_palettes` length unchanged, colors
   match draft; Apply with selection null → length + 1; filters’
   `palette_id` unchanged on replace.

### Requirement 5: Import Image as Layer (Beta 1, after Req 4)

**User Story:** As a user with an open project, I want to add another
image as a layer and get an auto-extract for that layer — without
replacing the document.

#### Acceptance Criteria

1. THE app SHALL add **Import Image as Layer…** (File menu and/or Layers
   panel). It SHALL NOT replace the document (`load_image` stays the
   Open Image path).
2. Decode SHALL reuse the `load_image` numeric path (same RGBA f32
   space, same 8192 cap). New raster layer via existing `add_layer` +
   tile insert (not a second decomposer).
3. Size lock: **place at origin, clip to document bounds, no scale**.
   Smaller image → transparent remainder. Larger → clip. Do not reject
   on mismatch; do not resample.
4. After success, IF the auto-extract pref is on, THE frontend SHALL
   call the existing `maybeAutoExtractPalette` for the **new** layer id.
   Existing filters’ `palette_id` SHALL NOT change (Color Lab задача 1).
5. This requirement is **out of Beta 0**. It SHALL not start until
   Req 4 is green (otherwise each import+Apply doubles palettes).
6. Empty `addRasterLayer` (blank raster) MAY remain for “new empty
   layer”; it is not a substitute for this import.

### Requirement 6: Manual QA is recorded, not implied

**User Story:** As a maintainer shipping a beta DMG, I want the Track A
and Track D eyeball checklists actually ticked on a real window.

#### Acceptance Criteria

1. Track A [tasks.md §6.2](../track-a-correctness/tasks.md) SHALL be
   walked on a **release or `tauri dev` window** (not only
   `dither_seam_matrix`). Results (pass / N/A + note) SHALL be written
   back into that checklist **and** copied into this folder’s tasks.
2. Track D [tasks.md §5.3](../track-d-gpu/tasks.md) SHALL be walked with
   `DITHER_GPU=1` and `DITHER_FORCE_CPU=1` as written. Same recording
   rule.
3. THE Beta_0 script in design SHALL be walked once on the candidate
   DMG: New/Open → filters + Undo → Save `.dyproj` → reopen → Export
   PNG + SVG (mode picker if C4.1 landed) → close-guard → palettes do
   not explode after extract+Apply.
4. Failures that are product bugs SHALL block Beta 0. Environment
   N/A (no GPU adapter) SHALL be documented, not silently skipped as
   pass.

### Requirement 7: Two named gates

**User Story:** As a maintainer, I want a written line between “friends
can download a DMG” and “the updater channel is live.”

#### Acceptance Criteria

1. **Beta 0** MAY ship when ALL of: this track Req 1–4 and 6; C4.1 DoD;
   Color Lab §6 DoD; GPU remains opt-in and is stated in the notes.
   Track O and Req 5 are **not** required.
2. **Beta 1** MAY ship when Beta 0 is done AND Track O DoD AND Req 5.
   First updater build stays `0.2.0` per Track O (do not invent a
   third version scheme here).
3. THE following SHALL stay out of both gates: multi-document, autosave,
   mask editing UI, paint-aware undo, pyramid level > 0 on the frontend,
   GPU Glow / GPU Bayer with non-zero bias/angle, video, ICC, batch
   export, file-association icons, Custom PNG map UI polish, `.dyuki`
   subset UI, GPU-on-by-default.

---

## Future (explicitly out of this track)

- Dirty based on pixel-paint (no paint path yet; Arc identity is enough)
- Persistent “skip this version” (Track O non-goal)
- Apple notarization (Track O distribution note)
- Color Lab live-edit of document swatches from the draft without Apply
