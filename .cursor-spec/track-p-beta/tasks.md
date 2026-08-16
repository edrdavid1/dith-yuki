# Implementation Plan: Track P — Beta product gate

План: [requirements.md](./requirements.md), [design.md](./design.md).

**Gate:** Track N in tree (`with_document_undo`, replace `clear_history`).
P3 after P2. O3 consumes P1 (do not block P1 on O). C4.1 and Color Lab §6
are sibling folders — not tasks in this file.

**Locked:** Saved_Mark = `Arc::ptr_eq`; one Unsaved_Guard; Apply = replace
selected; Import Layer = Beta 1; title basename + dirty bullet.

**Порядок:** P0 → P1 → P2 → P4. P3 after P2 (Beta 1). P5 docs with P1.
C4.1 / Color Lab §6 parallel anytime.

---

## 0. Baseline

- [x] 0.1 Inventory
  - `AppState.project_path`; `documentSlice.projectPath`; no dirty
  - `main.rs` CloseRequested = panels only; window label `main`
  - Tauri 2 `onCloseRequested` / `destroy` vs `exit` (no re-entry loop)
  - `with_document_undo` / `clear_history` / `save_project_as` hook points
  - Color Lab `handleApply` + local `selectedPaletteId`; `modify_palette`
  - `addRasterLayer` vs `load_image` decode helpers
  - Track A §6.2 / Track D §5.3 empty boxes
  - _Requirements: 1, 2, 4, 6_

- [x] 0.2 Link docs
  - This folder from `RELEASE_TRACKS.md`, `tech-debit.md`
  - Color Lab gap #1 → P2; задача 1 Import Layer → P3
  - Track O design: Saved_Mark is the dirty-flag O3 was waiting for
  - _Requirements: 1.8, 7_

**§0.1 result (fill in):**

```
Date: 2026-08-13
Close API chosen (frontend onCloseRequested vs Rust emit): frontend getCurrentWindow().onCloseRequested on `main`
destroy vs exit: preventDefault → Guard → allowCloseRef + destroy()
save_project_as live Arc: mark_clean snapshots live Arc after successful write
modify_palette IPC gap: replace_palette wraps modify_palette + name
Gate: proceed P1
```

---

## 1. P1 — Dirty_Flag + Unsaved_Guard + title

- [x] 1.1 Backend Saved_Mark
  - `AppState.saved_snapshot: Mutex<Option<Arc<Document>>>`
  - `is_document_dirty` command
  - `dirty-changed` emit from wrapper, undo/redo, clear_history, save
  - Mark clean on save / open_project / load_image / create_document install
  - _Requirements: 1.1–1.7_

- [x] 1.2 Frontend state
  - `documentSlice.dirty` from event + getter on bootstrap
  - Title: `{• }{basename | Untitled} — Dither Engine`
  - _Requirements: 1.7, 3_

- [x] 1.3 `runUnsavedGuard`
  - Modal chrome like NewProjectDialog; three buttons; locked copy
  - Save / Don’t Save / Cancel; Save As if no `project_path`; fail aborts
  - _Requirements: 2.1, 2.4–2.6_

- [x] 1.4 Wire entry points
  - `main` `onCloseRequested`
  - File + Welcome: New / Open Image / Open Project / Open Recent
  - Panels unchanged
  - _Requirements: 2.2–2.3, 2.7–2.8_

- [x] 1.5 Tests
  - Rust: ptr_eq after save; undo to mark = clean; replace install = clean;
    mutation = dirty
  - RTL: Cancel keeps window/doc; Save As cancel aborts; no Guard when clean
  - _Requirements: 1, 2_

---

## 2. P2 — Apply replace

- [x] 2.1 IPC `replace_palette`
  - `modify_palette` + name; undo wrapper; cache invalidate like
    `update_palette_color`
  - _Requirements: 4.1, 4.5_

- [x] 2.2 Color Lab selection on slice
  - Move `selectedPaletteId` onto `colorLabSlice`
  - `extractPalette` / Apply set it; Apply replace vs add
  - Builtin / file import still add
  - _Requirements: 4.2–4.4_

- [x] 2.3 Tests
  - extract → Apply → palette count unchanged
  - Apply with null selection → count + 1
  - filter `palette_id` unchanged on replace
  - _Requirements: 4.6_

---

## 3. P3 — Import Image as Layer (Beta 1)

Do not start until 2.3 is green.

- [x] 3.1 `import_image_layer(path)`
  - Reuse `load_image` decode; add raster; blit origin; clip; no scale
  - _Requirements: 5.1–5.3_

- [x] 3.2 UI + auto-extract
  - File → Import Image as Layer…
  - `maybeAutoExtractPalette(newLayerId)` when pref on
  - Existing filter palette_ids unchanged
  - _Requirements: 5.4–5.6_

- [x] 3.3 Tests
  - Smaller image → transparent remainder
  - Larger → clipped
  - Extract pref off → no extra palette
  - _Requirements: 5_

---

## 4. P4 — Manual QA

- [ ] 4.1 Track A §6.2 walked; boxes ticked in
  [track-a-correctness/tasks.md](../track-a-correctness/tasks.md) and here
  - [ ] 1:1 FS after full load — no seam
  - [ ] Zoom out (pyramid > 0) — no seam
  - [ ] Pan sticky-seam — clears after settle **or** N/A documented
  - [ ] `pixel_size` 3, 5, 7, 12 — Bayer + FS blocks continuous
  - [ ] Bayer-only doc unchanged vs pre-change smoke
  - Date / machine:
  - _Requirements: 6.1_

- [ ] 4.2 Track D §5.3 walked; boxes ticked in
  [track-d-gpu/tasks.md](../track-d-gpu/tasks.md) and here
  - [ ] `DITHER_GPU=1` pan Halftone — no phase jump
  - [ ] `DITHER_GPU=1` pan CRT scanlines across tile boundary
  - [ ] `DITHER_FORCE_CPU=1` same doc matches GPU session
  - [ ] Boot no adapter / FORCE_CPU — app starts, one warn
  - Date / machine:
  - _Requirements: 6.2_

- [ ] 4.3 Beta_0 script (design) on candidate build
  - Date / build:
  - _Requirements: 6.3–6.4_

---

## 5. P5 — Docs

- [x] 5.1 `RELEASE_TRACKS.md` as-built row for P; Beta 0 / Beta 1 lines
  - _Requirements: 7_

- [x] 5.2 Short ARCHITECTURE / beta notes: dirty Arc identity; GPU still
  `DITHER_GPU=1`; 0.1.0 → 0.2.0 is Track O
  - _Requirements: 7.1–7.2_

---

## Definition of Done

### Beta 0 (this track + siblings)

- [x] P1: close / New / Open never drop a dirty doc without Guard
- [x] P2: extract + Apply does not clone the palette
- [ ] P4 checklists recorded
- [x] C4.1 DoD (sibling)
- [x] Color Lab §6 DoD (sibling)
- [x] Release notes: GPU opt-in; no updater in 0.1.0-line DMG

### Beta 1

- [ ] Beta 0
- [x] P3 Import Image as Layer
- [x] Track O DoD (`0.2.0`)
- [x] O3 uses `runUnsavedGuard` (skip if clean)
