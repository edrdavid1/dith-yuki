# Requirements: Track F — `.dyuki` Sharable Patterns

## Introduction

Формализация брифа [BRIEF_dyproj_dyuki.md](../BRIEF_dyproj_dyuki.md) §3, §4–5 (часть про patterns). `.dyuki` — **не документ**, а sharable **recipe**: упорядоченный список `FilterInstance` + embedded палитры и threshold maps, достаточные чтобы воспроизвести обработку на чужой машине без исходных файлов экспортёра.

**Зависимость:** I/O и embedding — только через **E0** из [track-e-dyproj](../track-e-dyproj/) (`engine-project::serialize` archive/assets). После E0 этот трек параллелен E1–E5.

Прецедент поведения: Color Lab Apply/Import всегда создаёт **новые** сущности с новыми id — `.dyuki` import делает то же для палитр и фильтров.

## Glossary

- **Dyuki**: zip pattern archive (`manifest.json`, `filters.json`, `palettes.json`, `assets/threshold_maps/`).
- **Placeholder_Key**: стабильный ключ внутри архива (не runtime `PaletteId`), связывающий `filters.json` ↔ `palettes.json`.
- **Import_Pattern**: unpack → new palettes → remap placeholders → new filter instances → **append** to target layer filter stack.
- **Export_Pattern**: serialize selected (or all) filters on a layer + embed referenced palettes/maps.
- **App_Version_Min**: минимальная версия приложения, способная исполнить все kinds/modes в файле (отдельно от `format_version` схемы).

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Export/import filter recipes with embeds | Exporting layer pixels / opacity / blend / mask / offset |
| Always-new ids on import (no id reuse) | Default “replace entire stack” import (optional UI later) |
| `app_version_min` hard fail if app too old | Silent drop of unknown filter kinds |
| Reuse E0 zip/assets | Separate embedding stack from `.dyproj` |
| Append-on-import | Autosave, multi-doc |

---

## Requirements

### Requirement 1: Archive Layout

**User Story:** As a user sharing a look, I want a small portable file that contains the filter stack and its dependencies.

#### Acceptance Criteria

1. A `.dyuki` zip SHALL contain: `manifest.json`, `filters.json`, `palettes.json` (may be empty object if no palette refs), and optional `assets/threshold_maps/{content_hash}.png`.
2. `filters.json` SHALL be an ordered list of filter instances **without** runtime `FilterInstanceId` and **without** trusting `requires_full_row` from disk (recomputed on import).
3. Palette references inside filter params SHALL use Placeholder_Key strings (not live `PaletteId` integers/uuids from the exporter’s document).
4. `palettes.json` SHALL map Placeholder_Key → palette payload (`name`, `colors`, and any other fields needed to call the same creation path as `add_palette` / Color Lab Apply).
5. Layer-level properties (raster, offset, blend_mode, opacity, mask, visibility, name) SHALL NOT appear in the archive.

### Requirement 2: Manifest Versioning

**User Story:** As a user on an older app, I want a clear error if a pattern needs newer filters — not a silently truncated stack.

#### Acceptance Criteria

1. `manifest.json` SHALL include at least: `format_version` (u32, start at 1), `kind: "dyuki"`, `app_version_min`, `name`, optional `description` / `author`, `created_at`.
2. `format_version` SHALL use the same migration **idiom** as `.dyproj` / E0 (ordered chain + error if file newer than supported), but a **separate** version ladder for `kind: "dyuki"` — not one shared counter with dyproj.
3. WHEN the running app version is older than `app_version_min`, THE import SHALL fail with an explicit user-facing message — **no** silent omission of filters.
4. WHEN exporting, THE exporter SHALL set `app_version_min` to cover the highest requirement among included filter kinds/modes (policy locked in design.md; at minimum: current app version when any non-baseline mode is present, or a per-kind table).
5. WHEN a filter entry uses an unknown enum variant at deserialize time, THE import SHALL fail clearly (serde / format error) even if `app_version_min` was incorrectly low — this is a second line of defense, not a substitute for criterion 3.

### Requirement 3: Export Pattern

**User Story:** As a user, I want to export all or selected filters from a layer as a `.dyuki`.

#### Acceptance Criteria

1. IPC `export_pattern(layer_id, filter_instance_ids: Option<Vec<Id>>, path)` SHALL export the full layer filter stack when `filter_instance_ids` is None/empty-means-all per design lock; when Some(non-empty), only that subset **in stack order**.
2. THE exporter SHALL collect unique palettes and CustomPng maps referenced by the exported filters and embed them via E0 APIs.
3. Exported filters SHALL replace live `palette_id` with Placeholder_Key; SHALL replace CustomPng user/synthetic paths with `{content_hash}.png` embed names; SHALL NOT write exporter machine paths into the archive.
4. User `path` SHALL be sandbox-validated; write via E0 zip helpers.
5. IF a selected filter id is missing on the layer, THE command SHALL error (no partial export unless design explicitly allows skipping — default: fail).

### Requirement 4: Import Pattern (Always New Entities, Append)

**User Story:** As a user importing a shared look onto a layer, I want new palettes/filters that do not clobber existing ones, and that work without the author’s files on disk.

#### Acceptance Criteria

1. IPC `import_pattern(path, target_layer_id)` SHALL: validate manifest → migrate if needed → enforce `app_version_min` → **reject if `target_layer_id` is not a leaf `Layer`** (groups are invalid targets) → materialize threshold maps via E0 content-addressed Synthetic_Asset_Path (`asset-cache/threshold-maps/{content_hash}.png`, shared with `.dyproj`, no per-import uuid) → create a **new** palette per `palettes.json` entry using the same document API as `add_palette` / Color Lab Apply → build Placeholder_Key→new PaletteId map → create new `FilterInstance`s (new ids, remapped palette ids, synthetic CustomPng paths, `enabled` from file, recomputed `requires_full_row`) → **append** them to `target_layer_id.filters` (do not remove existing filters).
2. Import SHALL NEVER reuse existing document palette/filter ids by matching names or old ids.
3. WHEN the same `.dyuki` is imported twice onto the same layer, THE result SHALL be two independent filter subsequences with distinct ids, both functional (BRIEF §5.4). Embedded threshold maps with the same content hash SHALL NOT create duplicate files under the asset cache.
4. AFTER import, THE system SHALL invalidate/recompute tiles for the target layer as with `add_filter`.
5. Replace-entire-stack SHALL NOT be the default; if offered later, it MUST be an explicit UI option.
6. WHEN `filters.json` contains an unknown filter kind/mode enum variant (even if `app_version_min` would otherwise pass), THE import SHALL fail with a clear deserialize/user-facing error and SHALL NOT mutate the target layer stack.

### Requirement 5: Cross-Machine Fidelity

**User Story:** As a recipient with a blank document, I want the imported pattern to look like the author’s layer processing.

#### Acceptance Criteria

1. An acceptance test SHALL: export a stack that includes at least one palette-backed filter and one CustomPng (or skip CustomPng only if harness cannot; prefer include) → import into a **different** empty/minimal document **without** the original palette files / threshold paths on disk → processed output of the target layer SHALL match the source processing within documented tolerance (prefer bit-identical for deterministic ordered modes).
2. `requires_full_row` after import SHALL match `add_filter` for the same kinds (BRIEF §5.5).

### Requirement 6: UI Entry Points

**User Story:** As a user, I want discoverable Export/Import pattern actions without hunting through hidden debug menus.

#### Acceptance Criteria

1. THE UI SHALL provide Export Pattern and Import Pattern actions. Exact placement is design-owned (recommended: EffectSettingsPanel / layer effects context + File or Effects menu); MVP MUST expose both.
2. Export MAY support multi-select of filters when the panel supports selection; if multi-select is not ready, Export entire stack is acceptable for MVP with a follow-up task for subset export UI.
3. Import SHALL require a target layer (selected layer); if none selected, show a clear error/disabled state.
4. File dialogs SHALL filter `*.dyuki`.

### Requirement 7: OS Association (Low Priority)

**User Story:** As a user, I may eventually open `.dyuki` via double-click.

#### Acceptance Criteria

1. A non-blocking task MAY register `.dyuki` beside `.dyproj` in Tauri file associations (coordinate with Track E Req 9).
2. MVP import via dialog is sufficient without OS association.

---

## Future (out of MVP)

- Default or modal “replace stack” import mode
- Pattern browser / library UI
- Signing / marketplace metadata beyond author string
- Partial import when `app_version_min` fails (explicitly rejected)
