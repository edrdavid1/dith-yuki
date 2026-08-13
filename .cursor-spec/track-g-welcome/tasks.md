# Implementation Plan: Track G — Welcome Screen

> **Status (2026-08-13):** G0–G5 complete. As-built: [ARCHITECTURE.md](../../ARCHITECTURE.md) §3.9.

План: [requirements.md](./requirements.md), [design.md](./design.md). Бриф: [TASK_welcome_screen.md](../TASK_welcome_screen.md).

**Gate:** Track E open/save project IPC already in tree (`open_project`, `save_project`, `project_path`). No dependency on A–D or F.

**Locked:** extend `EmptyState` (slot = `PreviewFeature`); `recent_files.json` beside `panel_state.json`; record after successful load_image / open_project / save_project*; never record `create_document`; `MAX_RECENT=10`; `MAX_DOCUMENT_DIMENSION=8192` shared with `load_image`; blank pixels in load_image numeric space; one `useRecentFiles` in `AppLayout`; one `NewProjectDialog`; File adds New Project… + Open Recent only.

**Порядок:** G1 → G2 → G3 → G4 → G5. G4 can overlap G3 once the hook and dialog exist.

---

## 0. Baseline

- [x] 0.1 Inventory
  - `EmptyState.tsx` + `PreviewFeature` empty/`fill` branches
  - `panel_persistence.rs` path + JSON idiom
  - `load_image` / `open_project` / `save_project_as` replace + `project_path`
  - `useDocument` / `documentSlice` / `MenuBar` File items
  - Confirm 8192 is still inline in `load_image` (extract in G2)
  - _Requirements: 1, 3, 4, 7_

- [x] 0.2 Link docs
  - Point this folder from `tech-debit.md` and TASK header
  - _Requirements: n/a (process)_

**§0.1 result (fill in):**

```
Date: 2026-08-13
Empty slot: EmptyState.tsx rendered from PreviewFeature when !hasDocument;
  fill branch (PanelWindow floating preview) is a separate “No document open” stub
Persistence precedent: panel_persistence.rs → {app_data_dir}/panel_state.json;
  load missing/corrupt = default; save errors logged, never propagated;
  path helper takes AppHandle; parse/serialize are path-injectable for tests
load_image / open_project / save call sites:
  load_image (commands.rs) — AppHandle, project_path=None, emit image_loaded;
    8192 still inline (`width > 8192 || height > 8192` and zero-reject)
  open_project — AppHandle, sandbox-resolved path, project_path=Some, emit project_opened
  save_project → save_project_as — no AppHandle today; project_path=Some(resolved)
useDocument / documentSlice / MenuBar File:
  useDocument: openImage (dialog), openProject (dialog), saveProject / saveProjectAs
  documentSlice: openImage / openProject / saveProject / saveProjectAs thunks
  MenuBar File: Open Image, Open Project…, Save Project, Save Project As…, Save/Export
  (no New Project, no Open Recent)
Gate: open_project / save_project / save_project_as / project_path already in tree
```

---

## 1. G1 — Recent Files

- [x] 1.1 `src-tauri/src/recent_files.rs`
  - `RecentFileEntry` / `RecentFileKind`; `MAX_RECENT = 10`
  - `load` / `record` / `prune_missing` on injected `&Path`
  - Missing/corrupt → empty vec
  - Unit: dedup+front+opened_at; cap at 10
  - _Requirements: 1.1–1.3, 1.5, 8.1_

- [x] 1.2 Path helper + command
  - `{app_data_dir}/recent_files.json`
  - `get_recent_files`: prune exists(), rewrite if dropped, never fail IPC on rewrite error
  - Register in `main.rs`
  - Unit: prune drops missing path and persists cleaned list
  - _Requirements: 2.1–2.3, 8.1_

- [x] 1.3 Record call sites
  - `load_image` → Image (after success)
  - `open_project` → Project
  - `save_project` / `save_project_as` → Project (`AppHandle` on save if needed)
  - Write failure must not fail the user command
  - _Requirements: 1.4_

---

## 2. G2 — `create_document`

- [x] 2.1 Shared dimension const
  - `MAX_DOCUMENT_DIMENSION = 8192`; `load_image` and `create_document` both use it
  - _Requirements: 3.1_

- [x] 2.2 Command
  - Validate 1..=max; fill Transparent/White f32 buffer; `decompose_image_to_tiles`; one raster leaf; replace handle like `load_image`; `project_path = None`; emit document-changed; **no** `record_recent_file`
  - Response = `LoadImageResponse` shape
  - Register in `main.rs`
  - _Requirements: 3.1–3.5_

- [x] 2.3 Backend tests
  - Invalid size → error, no panic, document unchanged
  - Small success (e.g. 8×8): one leaf, tiles, `project_path` None
  - Explicit: Recent list unchanged after create
  - _Requirements: 8.1_

---

## 3. G3 — Welcome UI

- [x] 3.1 IPC + slice + hook
  - TS wrappers: `getRecentFiles`, `createDocument`
  - `documentSlice.createDocument` thunk (mirror `openImage`)
  - `useRecentFiles`; `useDocument`: `openImageAt` / `openProjectAt` / `createDocument` + existing dialogs
  - _Requirements: 5.4, 6.1, 6.3–6.4_

- [x] 3.2 `NewProjectDialog`
  - Width/Height/Background; defaults 1920×1080 Transparent
  - Client validation vs 1..=8192; no invoke on invalid
  - Mounted once in `AppLayout`
  - RTL: invalid size does not submit
  - _Requirements: 5.1–5.3, 5.5, 8.2_

- [x] 3.3 Extend `EmptyState`
  - Logo/name + three primary actions + Recent list (omit section if empty)
  - Kind icons; truncated path; relative time from ISO
  - Click → Shared_Open_Path by kind
  - `PreviewFeature` `fill` branch uses the same component
  - RTL: empty list hides Recent; kind click maps to the right open helper
  - _Requirements: 4.1–4.5, 8.2_

- [x] 3.4 Lift state in `AppLayout`
  - Single `useRecentFiles()`; pass entries/actions to MenuBar + preview
  - `refresh()` after successful open/create/save
  - _Requirements: 6.2–6.3_

---

## 4. G4 — MenuBar

- [x] 4.1 File items
  - New Project… → same dialog
  - Open Recent from shared entries; hide when empty
  - Do not duplicate Open Image / Open Project
  - New Project enabled even when `hasDocument`
  - Extend `MenuBar.test.tsx`
  - _Requirements: 7.1–7.4, 8.2_

---

## 5. G5 — Docs / polish

- [x] 5.1 ARCHITECTURE note
  - EmptyState = Welcome; `recent_files.rs`; `create_document` next to `load_image`
  - _Requirements: n/a_

---

## Definition of Done

- [x] No-document UI is Welcome with three actions; Recent only if the list is non-empty
- [x] New Project creates a blank raster document in memory, `project_path = None`, not in Recent
- [x] Open Image / Open Project / Recent clicks share `useDocument` thunks
- [x] Recent survives app restart; dead paths disappear on next `get_recent_files`
- [x] File menu has New Project… + Open Recent from the same Recent data as Welcome
- [x] Backend + frontend tests in Req 8 green
