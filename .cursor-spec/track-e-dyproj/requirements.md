# Requirements: Track E — `.dyproj` Project Persistence

## Introduction

Формализация брифа [BRIEF_dyproj_dyuki.md](../BRIEF_dyproj_dyuki.md) §1–2, §4–5 (часть про проект). Цель — **сохранять и открывать документ** как zip-контейнер `.dyproj` с embedded растрами слоёв, палитрами и custom threshold maps.

Трек E также владеет **общей** подсистемой zip + asset embedding (**E0**), которую переиспользует параллельный [track-f-dyuki](../track-f-dyuki/) (`.dyuki`). E0 — единственная жёсткая зависимость F от E; после E0 треки могут идти параллельно.

Источник решений: BRIEF (не переоткрывать без причины). Прецеденты: Color Lab «всегда новая сущность», миграция `DitherMode` → `DitherParamsV2`, `ThresholdMapCache`, `sandbox::resolve_user_path` только для user paths.

## Glossary

- **Dyproj**: zip-архив проекта (`manifest.json` + `document.json` + `layers/*.png` + `assets/threshold_maps/`).
- **E0_Shared_Embed**: общий модуль zip open/write + embedding threshold-map PNG по `content_hash` (используется E и F).
- **Document_Serde**: сериализация `Document` без runtime-полей (`revision`, `generations`, `requires_full_row`) и без сырых пикселей в JSON.
- **Id_Remap**: при `open_project` генерация новых `LayerId` / `PaletteId` / `FilterInstanceId` + таблица old→new для внутренних ссылок.
- **Synthetic_Asset_Path**: детерминированный content-addressed путь в app-data
  (`asset-cache/threshold-maps/{content_hash}.png`) для `ThresholdMapCache`, без
  `resolve_user_path` и **без** project/import uuid в пути (один файл на хэш
  для всех `.dyproj` / `.dyuki`).
- **Project_Path**: путь текущего открытого/сохранённого `.dyproj` в `AppState` (для Save без диалога после Save As).
- **MaskRef_External**: текущая модель маски — `MaskStorage::External(LayerId)` (ссылка на другой слой), не отдельный raster-файл.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Round-trip save → open с визуально идентичным композитом | Multi-document (`doc_id` остаётся 1) |
| Общий E0 zip/embed для `.dyproj` и `.dyuki` | Autosave / crash recovery |
| Id remap при open; runtime-поля пересчитываются | Дублирование panel/viewport state в `.dyproj` |
| Миграции `format_version` + ошибка на future version | OS file-association как блокер MVP (отдельная late task) |
| Threshold maps без зависимости от пути экспортёра | Замена `load_image` (остаётся для импорта картинки) |

---

## Requirements

### Requirement 1: Shared Zip + Asset Embedding (E0)

**User Story:** As a developer, I want one zip/asset-embedding module shared by project and pattern formats, so that threshold maps and archive I/O are not duplicated.

#### Acceptance Criteria

1. THE workspace SHALL add the `zip` crate and expose a reusable module (crate placement locked in [design.md](./design.md)) that can create/open a zip, read/write named entries as bytes, and write/read PNG assets under `assets/threshold_maps/{content_hash}.png`.
2. WHEN embedding a threshold-map PNG, THE module SHALL key the file by a stable content hash of the PNG bytes (BLAKE3, first 16 bytes as hex filename — locked in [design.md](./design.md)), not by original user path, and SHALL NOT store the original user filesystem path in the archive.
3. WHEN unpacking an embedded threshold map for runtime use, THE module SHALL write bytes to a Synthetic_Asset_Path under the shared content-addressed asset cache (`asset-cache/threshold-maps/{content_hash}.png`) and return that path for `DitherModeV2::CustomPng` / `ThresholdMapCache::get_or_load`. Identical PNG bytes from any project or pattern SHALL resolve to the same path (write-if-absent). The path SHALL NOT be scoped by a per-open project or import UUID.
4. Synthetic_Asset_Path resolution SHALL NOT go through `engine_io::sandbox::resolve_user_path` (internal path class). User-chosen Open/Save paths for the `.dyproj` file itself SHALL use sandbox validation as other user paths do today.
5. Track F SHALL consume the same E0 APIs for its `assets/threshold_maps/` entries without a second embedding implementation.

### Requirement 2: Archive Layout and Persist vs Runtime Fields

**User Story:** As a user, I want a `.dyproj` that restores my document structure and pixels, without carrying ephemeral cache counters.

#### Acceptance Criteria

1. A `.dyproj` zip SHALL contain at least: `manifest.json`, `document.json`, `layers/{layer_id}.png` for each raster layer with pixel data, and optional `assets/threshold_maps/{content_hash}.png` for CustomPng maps referenced by filters.
2. `document.json` SHALL reflect `Document` / layer tree / filters / palettes **minus** runtime-only fields: `Document.revision` (reset to 1 on load), `Document.generations` (recreate empty), `FilterInstance.requires_full_row` (recompute from `kind` on load).
3. Panel layout, viewport, and window state SHALL NOT be stored in `.dyproj` (remain in existing `panel_persistence` / app UI state).
4. Adjustment layers (`LayerKind::Adjustment`) SHALL NOT have a `layers/{id}.png` entry.
5. Layer masks: because `MaskRef` uses `MaskStorage::External(LayerId)` (and optional `EmbeddedVector` placeholder), THE format SHALL NOT invent a separate `{id}.mask.png` for External masks; mask pixels live in the referenced layer’s PNG. `MaskStorage::External` LayerIds SHALL participate in Id_Remap (Req 4).

### Requirement 3: Raster Source of Truth on Save

**User Story:** As a user saving a project, I want each raster layer’s source pixels persisted losslessly so reopen does not depend on the original import file.

#### Acceptance Criteria

1. WHEN saving, THE system SHALL assemble each raster layer’s **Raw**-stage pixels from `TileCache` (level 0) into a full RGBA buffer and encode **lossless PNG** into `layers/{file_layer_id}.png`, even if the layer was originally imported as JPEG.
2. `document.json` layer entries SHALL carry `raw_asset: Option<String>` naming the file under `layers/` (null/absent for adjustment layers).
3. IF Raw tiles for a raster layer are incomplete/missing such that a full buffer cannot be assembled, THE save SHALL fail with an explicit error (no silent hole-filled project).
4. Soft size policy (MVP): IF estimated uncompressed raster payload exceeds a documented threshold (design.md; default aligned with existing 8192×8192 mindset, e.g. warn above ~N MB), THE UI/command MAY warn but SHALL still allow save unless a hard limit is later added — hard reject is Non-Goal for MVP unless design sets one.

### Requirement 4: ID Remap on Open

**User Story:** As a developer, I want fresh runtime IDs on every open so id generators and open documents never collide.

#### Acceptance Criteria

1. WHEN `open_project` loads a file, THE system SHALL assign **new** `LayerId`, `PaletteId`, and `FilterInstanceId` values (not reuse file ids as runtime ids).
2. THE loader SHALL build an Id_Remap table and rewrite all internal references: `palette_id` inside filter params, `MaskStorage::External(LayerId)`, and any other LayerId/PaletteId/FilterInstanceId edges discovered in the document model at implementation time.
3. `requires_full_row` SHALL be recomputed from filter `kind` after remap (same contract as legacy Dither migration / `add_filter`), not trusted from file if present.
4. File-side ids MAY be stored in JSON for structure/debug but SHALL only be used as remap keys, not as live ids after load.

### Requirement 5: Open Reuses Document Replacement Path

**User Story:** As a user opening a project, I want the same single-document replacement behavior as opening an image, without a multi-doc model.

#### Acceptance Criteria

1. `open_project(path)` SHALL replace `AppState.document_handle` the same way `load_image` does today (`doc_id = 1`, single document). Multi-document is out of scope.
2. For each raster `raw_asset`, THE loader SHALL decode PNG → float RGBA → `decompose_image_to_tiles` into that layer’s Raw tiles (per remapped LayerId).
3. AFTER unpacking threshold maps to Synthetic_Asset_Path, THE loader SHALL ensure CustomPng filter params point at those paths and that `ThresholdMapCache` can load them.
4. THE loader SHALL clear/invalidate caches appropriately for a full document replace (at least: block representatives, error residuals, palette caches as needed — mirror `load_image` / filter-change invalidation patterns).
5. User path `path` for the `.dyproj` file SHALL be sandbox-validated; internal asset paths SHALL NOT.

### Requirement 6: Versioning and Migrations

**User Story:** As a user, I want old projects to open after schema bumps, and files from newer apps to fail clearly.

#### Acceptance Criteria

1. `manifest.json` SHALL include `format_version: u32` starting at `1`, plus `app_version`, `created_at`, `modified_at`, and document dimensions (or equivalent summary fields locked in design).
2. WHEN `format_version` is less than the current supported version **for that archive `kind`**, THE loader SHALL run the ordered migration chain for **that kind only** (`migrate_dyproj` vs `migrate_dyuki`; each owns its own version counter — never one shared ladder for both formats), same idiom as `From` for Dither legacy → V2.
3. WHEN `format_version` is greater than supported for that kind, THE loader SHALL return a clear user-facing error asking to update the app — no partial load, no crash.
4. Migration functions SHALL be unit-tested with at least one fixture per supported step (v1 identity / future stub ok for MVP with only v1), including that dyproj and dyuki version support are independent.

### Requirement 7: IPC and Project Path State

**User Story:** As a user, I want Save / Save As / Open Project from the app, with Save remembering the last project path.

#### Acceptance Criteria

1. Tauri commands SHALL include `save_project(path)`, `save_project_as(path)`, and `open_project(path)` registered next to existing document commands.
2. `AppState` SHALL hold an optional current project path (design locks field location); `save_project` without a new path uses it; `save_project_as` / first save sets it; `open_project` sets it; closing/replacing via `load_image` SHALL clear or update it per design (default: clear project path when replacing with a raw image import).
3. Frontend SHALL expose Open Project / Save / Save As (MenuBar or existing File menu) with native dialogs filtered to `.dyproj`.
4. Paths chosen by the user SHALL use existing dialog + sandbox patterns.

### Requirement 8: Acceptance / Round-Trip Quality

**User Story:** As a QA engineer, I want automated proof that save/open preserves look and structure.

#### Acceptance Criteria

1. A round-trip test SHALL save a document (multi-layer preferred: ≥1 raster + filters + palette + CustomPng if feasible) to `.dyproj`, open into a fresh state, and assert layer tree structure, filter kinds/params (modulo remapped ids/paths), and palette colors match; composite or full-buffer PNG before/after SHALL match within documented tolerance (prefer bit-identical Raw reassembly; composite may allow ≤1/255 if float round-trip documented).
2. Opening a fixture with `format_version` in the future SHALL error clearly without mutating the live document into a partial state.
3. After open, `requires_full_row` for each filter SHALL equal what `add_filter` would set for the same kind.
4. OS file association + icons MAY be a late non-blocking task (Req 9); not required for Req 8 pass.

### Requirement 9: OS Association (Low Priority)

**User Story:** As a user, I want double-clicking a `.dyproj` to open the app (eventually).

#### Acceptance Criteria

1. Tasks SHALL include a non-blocking item to register `.dyproj` (and optionally `.dyuki`) in `tauri.conf.json` file associations + icons.
2. MVP of E MAY ship without OS association if Open Project from menu works.

---

## Future (explicitly out of MVP)

- Autosave / crash recovery for `.dyproj`
- Multi-document editing
- Hard size caps beyond soft warning
- Embedded raster mask files separate from External layer masks
