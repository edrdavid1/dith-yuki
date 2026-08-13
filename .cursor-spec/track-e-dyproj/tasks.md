# Implementation Plan: Track E — `.dyproj` Project Persistence

План: [requirements.md](./requirements.md), [design.md](./design.md). Бриф: [BRIEF_dyproj_dyuki.md](../BRIEF_dyproj_dyuki.md).

**Gate:** нет зависимости от A/B/C/D. E0 блокирует реализацию I/O в [track-f-dyuki](../track-f-dyuki/).

**Locked:** zip container; IDs remapped on open; no `{id}.mask.png` (External LayerId remap); `AppState.project_path`; soft warn >256 MB uncompressed estimate; E0 in `engine-project::serialize`; content hash = BLAKE3 first 16 bytes hex; CustomPng JSON = `{hash}.png` basename; **separate** `format_version` chains for `dyproj` vs `dyuki`; PNG8 encode with 8-bit-source caveat doc-comment; IncompleteRaw tests force-drop tiles (no eviction); Synthetic_Asset_Path = content-addressed `asset-cache/threshold-maps/{hash}.png` (**no** project/import uuid).

**Порядок:** E0 → E1 → E2 → E3 → E4 → E5. F после E0 (или UI-only параллельно).

---

## 0. Baseline

- [x] 0.1 Inventory
  - `load_image` replace path; `TileCache` Raw assemble needs; `MaskRef` / `CustomPng { path }`; `ThresholdMapCache`; `AppState` fields; MenuBar File actions
  - Confirm MaskRef = External(LayerId) only for raster masks (design locked)
  - _Requirements: 2.5, 3, 5_

- [x] 0.2 Link docs
  - Point from `tech-debit.md` / BRIEF to this folder + track-f
  - _Requirements: n/a (process)_

**§0.1 result (fill in):**

```
Date: 2026-08-12 (updated post E5 + asset-cache fix)
MaskRef: External(LayerId) | EmbeddedVector — no separate mask PNG
Raw source: TileCache level-0 Raw tiles (get_entry / entries.remove for IncompleteRaw tests)
Extension points:
  - load_image / open_project: replace DocumentHandle; insert_fresh Raw; mark Processed+Composite dirty; schedule viewport; clear/set project_path
  - CustomPng: DitherModeV2::CustomPng { path }; materialize → asset-cache/threshold-maps/{hash}.png
  - ThresholdMapCache::get_or_load(path) — keys (path, mtime); sandboxes (cache under home/app-data OK)
  - AppState.project_path: Mutex<Option<PathBuf>>; MenuBar File: Open/Save Project (+ image)
  - engine-project::serialize (archive/assets/migrate/document_dto/id_remap/pixels/project)
  - requires_full_row: FilterInstance::new from kind (FS/Atkinson)
Gate: E0–E5 landed; F may proceed
```

---

## 1. E0 — Shared zip + asset embedding

- [x] 1.1 Add `zip` (+ `image` if not reachable) to `engine-project` (or thin `engine-io` helpers if cleaner)
  - Workspace compiles
  - _Requirements: 1.1_

- [x] 1.2 `serialize::archive`
  - Create zip from entries; open zip; read named entry bytes
  - Unit test round-trip bytes
  - _Requirements: 1.1_

- [x] 1.3 `serialize::assets`
  - `content_hash` = hex(BLAKE3(png_bytes)[0..16]); write/read `assets/threshold_maps/{hash}.png`
  - JSON / callers use basename `{hash}.png` only
  - `materialize_threshold_map(bytes) -> PathBuf` under `asset-cache/threshold-maps/{hash}.png` (content-addressed; **no** project uuid); **no** `resolve_user_path`
  - Smoke: `ThresholdMapCache::get_or_load` on synthetic path
  - _Requirements: 1.2–1.4_

- [x] 1.4 Shared manifest helpers
  - `kind: dyproj | dyuki`; **per-kind** `format_version` check + future-version error (`UnsupportedVersion { kind, … }`)
  - Separate supported-version constants; no shared counter across formats
  - _Requirements: 6.1–6.3_

**E0 exit:** Track F may implement pattern pack/unpack against these APIs.

---

## 2. E1 — Document DTO, remap, migrate

- [x] 2.1 `document_dto` / file shapes
  - Persist tree + palettes + `raw_asset`; strip runtime fields per Req 2
  - CustomPng paths as `{hash}.png` names (no user paths)
  - _Requirements: 2.1–2.4_

- [x] 2.2 `id_remap`
  - New LayerId / PaletteId / FilterInstanceId; rewrite palette_id + MaskStorage::External
  - Recompute `requires_full_row` from kind
  - Unit tests
  - _Requirements: 4.1–4.4, 8.3_

- [x] 2.3 `migrate`
  - `migrate_dyproj` + `migrate_dyuki` stubs (v1 identity each); unsupported version error; unit test that kinds do not share a version ladder
  - _Requirements: 6.2–6.4_

---

## 3. E2 — Pixels save/open

- [x] 3.1 Assemble Raw → PNG
  - Blit level-0 Raw tiles → RGBA8 → PNG; doc-comment on f32→u8 that “lossless” assumes 8-bit import sources
  - Fail `IncompleteRaw` if incomplete; unit test by **manually** removing a Raw tile (eviction is not runtime-exercised)
  - Soft size estimate helper (≥256 MB → warning flag to IPC)
  - _Requirements: 3.1–3.4_

- [x] 3.2 Open PNG → decompose
  - Decode → f32 → `decompose_image_to_tiles` with remapped LayerId
  - Adjustment layers skip PNG
  - _Requirements: 5.2_

- [x] 3.3 `project::save` / `project::open` orchestration
  - Collect CustomPng embeds; write zip; open+migrate+remap+materialize assets
  - Staging-safe open (no half-replaced doc on failure)
  - _Requirements: 5.1–5.5, 1.3_

---

## 4. E3 — Tauri IPC + AppState

- [x] 4.1 `AppState.project_path`
  - Set on save/open; clear on `load_image`
  - _Requirements: 7.2_

- [x] 4.2 Commands `save_project` / `save_project_as` / `open_project`
  - Sandbox user paths; wire invalidate caches like `load_image`
  - Register in `main.rs`
  - _Requirements: 7.1, 7.4, 5.4_

---

## 5. E4 — Frontend

- [x] 5.1 IPC wrappers + MenuBar File entries
  - Open Project / Save / Save As; `.dyproj` filters; surface size warning if returned
  - _Requirements: 7.3, 3.4_

---

## 6. E5 — Acceptance + optional OS assoc

- [x] 6.1 Round-trip integration test
  - Structure + params (modulo ids/paths) + Raw/composite compare; CustomPng without original path
  - Future `format_version` error test
  - _Requirements: 8.1–8.3_

- [x] 6.2 (Optional, non-blocking) `tauri.conf.json` file associations + icons for `.dyproj`
  - _Requirements: 9_

- [x] 6.3 Docs
  - Short note in ARCHITECTURE / BRIEF pointer that formal spec lives here
  - _Requirements: n/a_

---

## Definition of Done

- [x] E0 usable by Track F without copy-paste embedding
- [x] Save → quit/reopen app → Open Project → visually identical (BRIEF §5.1)
- [x] Future format_version → clear error (BRIEF §5.2)
- [x] `requires_full_row` matches `add_filter` semantics (BRIEF §5.5)
- [x] No panel layout in `.dyproj`; no user threshold paths in archive
