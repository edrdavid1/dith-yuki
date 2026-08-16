# Design: Track P — Beta product gate

> **Status:** P1+P2+P3 in tree; P4 eyeball QA pending. Checklist: [tasks.md](./tasks.md).
> Requirements: [requirements.md](./requirements.md).

## Overview

| ID | Deliverable | Gate |
|----|-------------|------|
| **P1** | Dirty_Flag + Unsaved_Guard + title | Beta 0 |
| **P2** | Color Lab Apply = replace selected | Beta 0 |
| **P3** | Import Image as Layer + extract | Beta 1 (after P2) |
| **P4** | Manual QA A §6.2 / D §5.3 + Beta_0 script | Beta 0 |
| **P5** | Docs: `RELEASE_TRACKS`, Color Lab gap #1, O3 hook | with P1 |

**Gate:** Track N closed (`with_document_undo`, replace clears stacks).
No H–M / C4.1 / O gate on P1–P2. P3 after P2. O3 consumes P1; do not
wait for O to land P1.

Sibling work (own folders, parallel with P1–P2):

- [C4.1](../track-c4-svg-followup/)
- [Color Lab §6](../color-lab.md)

---

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| File contract testers will hit on day one | Autosave / crash recovery |
| Stop palette clones from extract→Apply | Palette CRUD redesign |
| Record eyeball QA | Re-open Track A/D code |

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Dirty signal | `Arc::ptr_eq(live, saved_mark)`, not `Document.revision` |
| Saved_Mark storage | `AppState.saved_snapshot: Mutex<Option<Arc<Document>>>` |
| Mark clean | Successful `save_project*`; after `open_project` / `load_image` / `create_document` **install** (live Arc). Mutations via wrapper / undo / redo recompute |
| Mark empty | Welcome / no document → `saved_snapshot = None`, dirty = false |
| Event | `dirty-changed` `{ dirty: bool }` from wrapper, undo/redo, `clear_history`, save. Getter `is_document_dirty` for first paint |
| Guard UI | One modal, NewProjectDialog chrome family. **Not** `window.confirm`. **Not** `tauri-plugin-dialog` |
| Buttons | **Save** / **Don’t Save** / **Cancel** (three, always) |
| Copy | Title `Save changes?`. Body `Save changes to {basename \| Untitled} before closing?` (same string for replace / update; the action is implied by what happens next) |
| Save path | `project_path Some` → `save_project`; else existing Save As. Failure or dialog cancel → abort |
| Close | `main` only. `getCurrentWindow().onCloseRequested` **or** Rust `CloseRequested` on `label == "main"` → prevent → emit `unsaved-close-requested` → frontend Guard → `destroy()` / `exit` on confirm. Pick **frontend `onCloseRequested`** (Tauri 2) so the modal lives in React; Rust panel intercept stays as-is |
| Reusable API | `runUnsavedGuard(): Promise<'save' \| 'discard' \| 'cancel'>` in `frontend/src/shared/unsavedGuard.ts`. MenuBar / Welcome / close / future O3 all call it |
| Title | `{• }{basename \| Untitled} — Dither Engine`. Basename only |
| Apply | `selectedPaletteId` in document → `replace_palette` IPC wrapping `Document::modify_palette` + `rename` if name changed + existing palette-cache invalidate. Else `add_palette` |
| Extract → Apply | After extract, set Color Lab `selectedPaletteId = lastCreatedId` (today extract only `bumpVersion`) |
| Builtin / file import | Still add |
| Import layer | New command `import_image_layer(path)`: decode like `load_image`, `add_layer(raster)`, blit tiles at origin, clip, no scale. File menu item. Auto-extract via existing helper |
| O3 | When dirty exists, skip Guard if `!dirty`. Comment in O design; implement skip in O3 when that track is built — if O3 lands first, treat `hasDocument` as dirty (already locked in O) |

---

## Current → Target

```text
Today
  project_path remembered; no dirty
  main close = silent quit
  Apply = always add_palette
  add raster = empty layer
  A §6.2 / D §5.3 checklists empty

Target (Beta 0)
  Saved_Mark + dirty-changed
  close / New / Open run one Guard
  title • Untitled — Dither Engine
  Apply replaces selected; extract selects it
  QA checklists filled

Target (Beta 1)
  Import Image as Layer + extract
  Track O Restart_Guard uses runUnsavedGuard
```

---

## P1 — Dirty + Guard

### Why ptr_eq, not revision

Track N already restores the **same** `Arc<Document>` on undo. After Save,
`saved_snapshot = live.clone()` (Arc clone). Edit pushes a new Arc.
Undo pops the saved Arc back → `ptr_eq` → clean. `Document.revision`
is omitted on `.dyproj` load and reset to 1 — useless as a save cursor.

`save_project_as` today does `(*snapshot).clone()` for the zipper. Do
**not** store that deep clone as Saved_Mark. Store `document_handle.snapshot()`
after success (the live Arc).

### Wrapper hook

`with_document_undo` already runs after every document mutation.
After `record_mutation` + `emit_undo_state`, also `emit_dirty`.
Same for `apply_undo` / `apply_redo` / `clear_history`.

Save is **not** wrapped (Track N). `save_project_as` sets Saved_Mark
then emits dirty=false.

Replace (`install_raster_document`, `open_project`, `create_document`):
`clear_history` then Saved_Mark = live, dirty=false. The next
`generate_palette` from auto-extract dirties — correct (palette is
unsaved project state).

### Close sequence (frontend)

```text
main onCloseRequested(event)
  event.preventDefault()
  if !hasDocument || !dirty → allow close (destroy)
  else Guard
    save    → save thunk; on ok destroy
    discard → destroy
    cancel  → return (window stays)
```

`destroy` = `getCurrentWindow().destroy()` (or `app.exit(0)` if destroy
re-enters CloseRequested — lock in P1 inventory which one does not loop).

### Replace-doc sequence

File Open / New / Recent / Welcome:

```text
if dirty
  Guard
    cancel → return
    save   → save; on fail return
    discard → fall through
replace document as today
```

Do not record the discarded document as an undo step (already true:
replace clears stacks).

### Tests

| Case | Proof |
|------|--------|
| Save then close | no Guard |
| Edit then undo to save point | dirty false |
| Open Image, extract on, no extra edits | dirty true (palette added) |
| Open Image, extract off, no edits | dirty false |
| New, no edits, close | no Guard |
| Open Project, no edits, close | no Guard |
| Dirty + Close + Cancel | window still open |
| Dirty + Open Image + Cancel | document unchanged |
| Save As cancel during Guard | document unchanged, still dirty |
| Panel close | still docks; no Guard |
| RTL Guard | three buttons; Save invokes save thunk |

---

## P2 — Apply replace

`Document::modify_palette` already replaces the color vec and bumps
palette revision. Missing: IPC that also updates **name**, invalidates
LUT like `update_palette_color`, wrapped in undo.

```text
replace_palette(id, name, srgb_colors)
  with_document_undo
    modify_palette + rename
    increment_generation
    invalidate palette caches for id
    emit document-changed / palette-changed (same as add)
```

Frontend `handleApply`:

```text
if selectedPaletteId != null && palettes.some(p => p.id === selected)
  replace_palette(...)
else
  add_palette(...)
  selectedPaletteId = dto.id
```

`extractPalette`: after success, Color Lab must select `dto.id`.
Today only `palettesSlice.lastCreatedId` moves. Lift `selectedPaletteId`
into `colorLabSlice` **or** set it from `lastCreatedId` in the feature
when `palettesVersion` bumps. Lock: **put `selectedPaletteId` on
`colorLabSlice`** so sidebar + floating window share it (draft already
lives there; local `useState` is the bug).

Do not auto-replace on builtin click.

---

## P3 — Import Image as Layer

Not a new decomposer. Sketch:

1. Sandbox-resolve user PNG/JPEG/WebP (same extensions as Open Image).
2. Decode to RGBA f32 (`load_image` helper — extract if duplicated).
3. `add_layer(raster)` at current size (`doc.width` × `doc.height`).
4. Blit source into Raw tiles at (0,0); clip; leftover tiles stay
   transparent from `add_layer`.
5. Invalidate processed/composite; schedule viewport; emit layer_added.
6. Return new `layer_id`. Frontend: `refreshLayers` +
   `maybeAutoExtractPalette(newId)`.

Mismatch sizes: **no scale**. Document 1920×1080 + 64×64 icon → icon
in the corner, rest empty. Document 64×64 + 1920×1080 → top-left 64×64
crop.

Empty-layer button can stay.

---

## P4 — QA scripts

Copy the boxes; tick with date + machine.

**A §6.2** (from track-a tasks): 1:1 FS; zoom-out; pan sticky-seam;
`pixel_size` 3/5/7/12 Bayer+FS; Bayer-only smoke.

**D §5.3:** `DITHER_GPU=1` pan Halftone; pan CRT; `DITHER_FORCE_CPU=1`
match; no-adapter / FORCE_CPU boot.

**Beta_0 script:**

1. Welcome → New 512×512 Transparent.
2. Open Image (photo) with auto-extract on → one new palette; Apply →
   still one (not two).
3. Add Dither V2 Bayer, FS, Halftone, CRT, Glitch; opacity 50%; Undo
   twice.
4. Save As `qa.dyproj`; quit; open `qa.dyproj` → looks the same.
5. Export PNG; Export SVG Pixel Grid; Export SVG Contour (donut or O).
6. Edit something; ⌘Q → Guard → Cancel → still open; then Don’t Save.

---

## Coupling with Track O

O Req 4.3: *if dirty exists, skip when clean; until then any open
document is dirty.*

P1 is that flag. When implementing O3:

```text
if !hasDocument → install
else if !dirty → install
else runUnsavedGuard()
```

Do not duplicate Save and Restart vs Unsaved_Guard buttons. O3 can
relabel Save → “Save and Restart” **or** keep Unsaved_Guard copy and
restart after `'save' | 'discard'`. Lock: **reuse `runUnsavedGuard`**,
then downloadAndInstall. Do not invent a second three-button modal.

---

## Risks

1. **`onCloseRequested` re-entry** after `destroy`. Inventory in P0;
   use a session flag `allowClose` if needed.
2. **Auto-extract after Open Image** always dirties. Correct. Testers
   who open a JPEG and quit will see Guard — that is the file contract.
   Extract-off + no edits = no Guard.
3. **ptr_eq after save from a deep-cloned doc.** Must store live Arc.
4. **P3 before P2** recreates gap #1 on every layer import. Order is
   a gate, not a suggestion.
