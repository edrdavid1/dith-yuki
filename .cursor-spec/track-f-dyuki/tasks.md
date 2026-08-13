# Implementation Plan: Track F — `.dyuki` Sharable Patterns

План: [requirements.md](./requirements.md), [design.md](./design.md). Бриф: [BRIEF_dyproj_dyuki.md](../BRIEF_dyproj_dyuki.md) §3.

**Gate:** [track-e-dyproj](../track-e-dyproj/) **E0** merged (archive + assets + version helpers). UI wireframes MAY start earlier.

**Locked:** append-only import; always-new ids; file DTOs with `palette_ref` placeholders + unconditional `enabled`; hard fail on `app_version_min` **and** unknown enum deserialize; leaf-layer-only target; E0 content-addressed `asset-cache/threshold-maps/{hash}.png` (no import uuid); no layer chrome in archive.

**Порядок:** F0 → F1 → F2 → F3 → F4(optional).

---

## 0. Baseline

- [x] 0.1 Confirm E0 APIs
  - `serialize::archive`, `serialize::assets`, manifest version check usable from pattern module
  - Inventory: `add_palette`, `add_filter`, EffectSettingsPanel selection, CustomPng path field
  - _Requirements: 1, 4_

- [x] 0.2 Link docs
  - BRIEF + tech-debit point at track-f
  - _Requirements: n/a_

**§0.1 result (fill in):**

```
Date: 2026-08-12
E0 commit/PR: local serialize module (archive/assets/migrate) — Track E E0–E5 already in tree
add_palette / add_filter entry points:
  - Document::add_palette(name, colors) -> PaletteId  (crates/engine-project/src/document.rs)
  - Tauri add_palette / add_filter in src-tauri/src/commands.rs
  - EffectSettingsPanel: frontend/src/features/effects/EffectSettingsPanel.tsx
  - CustomPng: DitherModeV2::CustomPng { path }
Gate: proceed F0
```

---

## 1. F0 — Pattern serde

- [x] 1.1 File DTOs
  - `FilterInstanceFile`, `palettes.json` map, manifest with `kind: dyuki`
  - Placeholder_Key assignment; CustomPng hash names; strip id / requires_full_row; **`enabled` always**
  - Unknown `FilterKind`/mode → serde error (no other/skip)
  - _Requirements: 1.1–1.5, 2.1, 2.5_

- [x] 1.2 `serialize::pattern` pack/unpack (bytes or path)
  - Uses E0 only for zip/assets (`materialize_threshold_map` content-addressed)
  - Unit: pack→unpack placeholders preserved
  - _Requirements: 1, 3.2–3.3_

- [x] 1.3 `app_version_min` policy helper
  - Kind/mode → min version table; export sets max; import compares to running version
  - Unit: too-old → error
  - _Requirements: 2.3–2.4_

- [x] 1.4 format_version migrate stub for dyuki
  - `migrate_dyuki` only (independent of dyproj version); future version error
  - _Requirements: 2.2_

---

## 2. F1 — IPC

- [x] 2.1 `export_pattern`
  - Subset vs all; embed palettes/maps; sandbox path; fail on missing filter id
  - _Requirements: 3.1–3.5_

- [x] 2.2 `import_pattern`
  - Validate target is leaf `Layer` (not group); new palettes via `add_palette` path; remap; append filters; invalidate tiles
  - Register commands in `main.rs`
  - _Requirements: 4.1–4.6_

---

## 3. F2 — UI

- [x] 3.1 IPC TS wrappers + dialogs (`*.dyuki`)
  - _Requirements: 6.4_

- [x] 3.2 EffectSettingsPanel (and/or menu) Export / Import
  - Target = selected layer; disable/error if none
  - MVP: export entire stack acceptable; subset UI follow-up if multi-select missing
  - _Requirements: 6.1–6.3_

---

## 4. F3 — Acceptance tests

- [x] 4.1 Cross-machine style test
  - Export with palette + CustomPng → remove original files → import on fresh doc → visual/buffer match
  - _Requirements: 5.1_

- [x] 4.2 Double-import test
  - Two independent working subsequences; distinct ids
  - _Requirements: 4.3, BRIEF §5.4_

- [x] 4.3 `requires_full_row` parity vs `add_filter`
  - _Requirements: 5.2_

- [x] 4.4 `app_version_min` / future format_version error tests
  - Stack unchanged on failure
  - _Requirements: 2.2–2.3_

- [x] 4.5 Unknown enum deserialize test (separate from `app_version_min`)
  - Low/valid `app_version_min` + unknown kind/mode in `filters.json` → clear error, stack untouched
  - _Requirements: 2.5, 4.6_

- [x] 4.6 Import onto `LayerGroup` rejected
  - Clear error; document unchanged
  - _Requirements: 4.1_

---

## 5. F4 — Optional OS association

- [x] 5.1 Register `.dyuki` with Track E file-association task
  - _Requirements: 7_

---

## Definition of Done

- [x] Export → import on clean machine/doc matches processing (BRIEF §5.3)
- [x] Double import → two independent stacks (BRIEF §5.4)
- [x] Old app / future format / unknown enum → clear errors, no silent drops
- [x] Import onto group → clear error
- [x] No duplicated zip/embed code outside E0; asset cache is content-hash-only
- [x] Default import appends; does not wipe existing filters
