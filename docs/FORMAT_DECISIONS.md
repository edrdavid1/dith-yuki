# Format decisions log

Journal of findings and choices while implementing
`.local-doc/SPEC_dither_file_format.md`. Newest entries first.

---

## 2026-09-21 — Previews SPEC: Stage A + early ПРОВЕРИТЬ

Source: `.local-doc/SPEC_dither_previews_full.md`.

### §3.4 — Will old loaders reject `thumbnail.png`?

**Checked in current tree (this repo, Stage 4+ loader):**
- `thumbnail.png` is already on the semantic allowlist
  (`secure_zip::is_allowlisted_entry`).
- Unknown *safe* names are ignored with a warning, not a hard error
  (`EntryClass::Unknown` → warning).
- Missing `thumbnail.png` does not block open; hash mismatch on `files`
  is a warning path for integrity, not a refuse-to-open for thumb alone.

**Public / shipped binaries:** no separate released “strict allowlist that
rejects unknown entries” build was found in-repo. Product Q §18.8 remains
open if any older external builds exist; until then writing `thumbnail.png`
is safe for current readers. Result: **proceed with Stage A**.

### Thumbnail encode contract (§3.1)

| Decision | Choice |
|---|---|
| Max long side | 1024 (was 512) |
| Resize filter | `image::Lanczos3` |
| Color / gamma | Resize in the `image` crate working space (approx. gamma); **not** a linear light pipeline. Same path on all platforms via Rust. |
| PNG encoder | `png` crate: `Compression::Fast`, `FilterType::Sub`, `AdaptiveFilterType::NonAdaptive`, RGBA8 only, no ancillary chunks |
| Size budget | ≤ 3 MiB; step-down sides 1024→768→512→384 |
| Failure | Never blocks save; log warning (no paths); write 1×1 transparent placeholder |
| Cache | Process LRU (8) keyed by blake3 of source RGBA |
| `.dyuki` | MUST write `thumbnail.png` = pattern applied to temporary sample 1024×768 (`TODO(design)`) |
| Share Copy | New `include_preview` (default true); false → neutral placeholder |

### Product defaults for open Qs (§18) until owner answers

| Q | Interim |
|---|---|
| Min macOS | `10.15` from `tauri.conf.json`. Space preview API needs 12+ → use data-based `QLPreviewProvider` with deployment target 12.0 for Preview appex; Thumbnail appex can stay 10.15. |
| Min Windows | Windows 10 (align with Tauri when verified). |
| Alpha signing | Existing `scripts/macos-self-sign-cert.sh` (T1 self-signed). |
| Installer mode | TBD — prefer per-machine HKLM; fall back HKCU if Tauri per-user. |
| Phase 2 Peek | After phase 1 + security review. |

### Stage C — icons / UTI (2026-09-21)

- Per-type icons already shipped (`proj-icon` / `pattern-icon` icns+ico) via
  `tauri.*.conf.json` resources + `Info.plist` / NSIS hooks.
- Legacy MIME aliases (`application/x-dither-project|pattern`) added to
  `UTExportedTypeDeclarations` as a string array (Tauri `fileAssociations`
  only carries one `mimeType`; merge plist is the source of truth for tags).
- Tauri ProgIDs remain spaced (`Dither Project` / `Dither Pattern`) — ShellEx
  binds to those names, not `Dither.Project` from the SPEC appendix.

### Stage D — macOS Quick Look wired to `dt_extract`

- Swift `DitherThumb` + bridging header; Preview/Thumbnail call Rust.
- `scripts/build-quicklook.sh` builds universal `libdither_thumb_ffi.a`.
- **Product (2026-09-21):** ship **Preview only**. Finder keeps document-type
  `.icns`; Space / `qlmanage -p` show `thumbnail.png`. Thumbnail appex is not
  embedded (opt-in via `--with-thumbnail` for experiments).

### Stage E — Windows thumbnail provider

- `platform/windows/dither-shell` COM DLL (`IInitializeWithStream` +
  `IThumbnailProvider`), CLSID `{BC7D0A00-220F-46DD-AAA8-C754864EE648}`.
- Cross-checked with `cargo check --target x86_64-pc-windows-gnu`.
- NSIS hooks register ShellEx + versioned DLL folder.
- Release stages `dither_shell.dll` via `tauri.windows.conf.json` resources /
  `scripts/ci-prepare-windows-previews.sh`.
- Autotest via `IShellItemImageFactory` on `windows-latest`: still TODO.

---

Scaffolding under `platform/macos/DitherQuickLook/` (fixed-image spike).
**Project generation:** XcodeGen (`project.yml`), not a committed `.xcodeproj`.
Full T1 quarantine PASS/FAIL on a clean Mac is **manual** — not claimed here.
`xcodegen` generates the project; full **Xcode.app** is required for
`xcodebuild` (Command Line Tools alone are not enough). Agent machine had
CLT only — appex compile is owner-side.

---

## 2026-09-21 — Fix: open panic on dangling dither `palette_id`

User file `te.dyproj` (format 1.0) saved with `palette_id: 1` and
`palettes: []`. Open panicked in `id_remap::remap_filter_params`
(`expect("dither palette_id mapped")`).

Cause: Color Lab persists `lastCreatedId` in localStorage; Effects sync
wrote that id onto a new document that had no palettes.

Fix:
- Remap heals dangling DitherV2 palette refs → `None` (no panic); orphan
  PaletteQuantize → Placeholder with raw params.
- Save sanitizes dangling dither palette ids in `DocumentFile::from_document`.
- Frontend: clear stale `lastCreatedId` when not in `listPalettes`; Effects /
  add-filter only bind ids present in the current document.

---

## 2026-09-21 — Review close-out: fuzz + lint:ipc + Share UI

Product review (SPEC attachment) rejected premature “DoD closed”: fuzz targets
and a first local run are in §17 now; nightly CI can wait. `lint:ipc` is
security-adjacent (§9.8). Share Copy needs UI + a parse-result defaults test.

### Done in this pass
- **Fuzz:** `fuzz/` crate (workspace-excluded) with
  `fuzz_open_dyproj`, `fuzz_open_dyuki`, `fuzz_manifest`, `fuzz_png_limits`,
  `fuzz_migrate`; corpus seeded from goldens. Smoke: ~60s dyproj (~953k exec,
  0 crashes) + ~10s each other target (0 crashes). Full 30 min × 5 local run
  started; recipe in `fuzz/README.md`.
- **CI substitute:** `crates/engine-project/tests/fuzz_mutation.rs` (mutate
  goldens + proptest) — 6/6 green; no panic.
- **lint:ipc:** audit in `frontend/src/shared/ipc/INVOKE_AUDIT.md`; baseline
  allowlist for FlexLayout/LayoutContext/PopoutTestWindow (no archive paths in
  args). `npm run lint:security` (= ipc + html) green; new raw invokes fail CI.
- **Share Copy UI:** `ShareCopyDialog` (SPEC §11 defaults) wired from File →
  Share Copy…; vitest checks default checkbox matrix. Engine
  `share_copy_defaults_have_no_privacy_leaks` parses the ZIP (paths, author,
  originals, PNG ancillary chunks).

### Definition of Done (§17) status

| Criterion | Status |
|---|---|
| v1 goldens open, SHA256SUMS locked | Done |
| Security corpus (zip/json/png) no panic | Done |
| Fuzz targets + local run (no crash) | Done smokes + mutation CI; 30 min×5 running |
| Fuzz nightly CI schedule | **Deferred** |
| Min format on write without new features | Done (`1.0`) |
| Round-trip + preservation | Done |
| Deterministic Share Copy | Done |
| No HTML sinks for file strings | Done (`lint:html`) |
| IPC via `shared/ipc` (new code) | Done (`lint:ipc` + baseline) |
| Share Copy privacy defaults (parse result) | Done |
| Docs §15 | Done |

### Still deferred / product
- Nightly CI job for 30 min × 5 fuzz targets
- Migrate FlexLayout raw invokes off the allowlist (tracker debt)
- OS thumbnails / QuickLook (§19.5)
- Product Qs in SPEC §19

---

## 2026-09-21 — Close-out: Share Copy IPC + DoD report

### Wired after Stage 7
- IPC `share_project_copy` + `DocumentService::share_project_copy` (export only;
  does not change project path / dirty).
- UI: File → **Share Copy…** (MenuBar + native macOS menu).
- Determinism: Share Copy uses fixed timestamp `"0"`; payload maps are
  `BTreeMap`-ordered; tests
  `share_copy_is_byte_identical_for_same_document` and
  `write_with_fixed_timestamp_is_deterministic` green.

### Definition of Done (§17) status (superseded by review close-out above)

| Criterion | Status |
|---|---|
| v1 goldens open, SHA256SUMS locked | Done |
| Security corpus (zip/json/png) no panic | Done (unit/integration) |
| Fuzz 30min nightly | Was deferred too aggressively — see review close-out |
| Min format on write without new features | Done (`1.0`) |
| Round-trip + preservation | Done |
| Deterministic Share Copy | Done |
| No HTML sinks for file strings | Done (`lint:html`) |
| Share Copy privacy defaults | Done (engine test + IPC) |
| Docs §15 | Done (`FORMAT.md`, `SECURITY.md`, schemas, checklist) |

### Still deferred / product (historical — updated above)
- `cargo-fuzz` targets + CI nightly
- `lint:ipc` still fails on pre-existing FlexLayout/raw invoke callers
- OS thumbnails / QuickLook
- Share Copy options UI (defaults only via IPC opts)
- Product Qs in SPEC §19

### Test snapshot (this close-out)
- `engine-project --lib`: 410 tests (incl. new determinism)
- `golden_v1` / `preservation_forward` / `security_*`: previously green
- `cargo check -p dither`: ok
- `npm run lint:html`: ok

---

## 2026-09-21 — Stage 7: docs close-out

- Expanded `docs/FORMAT.md` (layout, MIME, limits table, Share/downgrade).
- Added `docs/SECURITY.md`, `docs/FORMAT_PR_CHECKLIST.md`.
- Hand-written JSON Schema drafts under `docs/schema/` (schemars codegen
  deferred — avoid new deps; schemas document the on-disk shape).
- Fuzz targets / nightly still deferred (noted in SECURITY.md).

---

## 2026-09-21 — Stage 6: UI / IPC hardening

- `tauri.conf.json` CSP: `default-src 'self'`, `script-src 'self'` (no
  `unsafe-inline`/`unsafe-eval`), limited `img-src` (`tile:` + asset),
  `connect-src ipc:`. `style-src` keeps `'unsafe-inline'` for current CSS.
- Frontend `lint:html` / `lint:security` greps for `dangerouslySetInnerHTML` /
  `.innerHTML=` outside tests (alongside existing `lint:ipc`).
- `ipc_guard::catch_loader_panic` wraps `.dyproj` open; JoinError on
  `spawn_blocking` remains a second boundary.
- `sanitize_filename` already in engine (Stage 5); SVG `escape_xml` + export
  asserts no `<script>` / `foreignObject` / href hooks; GPL `Name:` strips
  newlines and formula prefixes.

---

## 2026-09-21 — Stage 5: Share Copy / downgrade / privacy

- `share_project_to_bytes` + `ShareCopyOptions` (strip metadata default on,
  author off, originals off, compact off). Strip = decode→re-encode PNG.
- `downgrade_project_to_bytes` / `plan_downgrade`: explicit only; today target
  must be format `1.x` and losses are empty (no live post-1.0 features).
- `sanitize_filename` for Save As suggestions (§9.8).
- `scan_archive_for_privacy_leaks` greps JSON/text for `/Users/`, `/home/`,
  drive letters, `file://`. Normal save already rewrites CustomPng to basenames.
- Ordinary Save goes through the same `write_project_to_bytes` core as Share.
- **UI:** File → Share Copy… → `share_project_copy` IPC (after Stage 7 close-out).

---

## 2026-09-21 — Stage 4: container / preview / hashes / MIME

- Writers emit `mimetype` first (Stored), then `manifest.json`, then
  lexicographic payload; fixed DOS mtime `1980-01-01`, unix `0o100644`,
  PNG Stored / JSON Deflated, `large_file(true)` for ZIP64 readiness.
- `.dyproj` save always writes `composite.png` + `thumbnail.png` (≤512 long
  side). Composite is a **Raw-layer flat** (Porter-Duff over + opacity); filter
  stacks and non-Normal blend modes are not applied — insurance preview, not
  export parity.
- `.dyuki` thumbnail deferred (no raster in pack); mimetype + `files` hashes
  still written.
- `manifest.files` holds sha256+size for every payload entry except
  `mimetype` / `manifest.json`. Legacy archives without `files` skip the check.
- Threshold map basenames remain 32-hex BLAKE3; `manifest.files` uses full
  SHA-256. Allowlist still accepts 32 or 64 hex names.
- MIME primary: `application/vnd.dither.{project,pattern}+zip` in
  `tauri.conf.json` / `Info.plist`. Reader also accepts legacy
  `application/x-dither-*`.
- `mimetype` remains optional on **read** (v1 goldens); required on **write**.

---

## 2026-09-21 — Stage 3: forward-compat

- File DTOs carry `#[serde(flatten)] extra` maps.
- Unknown `node` tags become invisible Adjustment stubs with
  `extra["__forward_compat_node"]` holding the original JSON; save emits the
  original object again.
- `Document.ext_blobs` holds allowlisted `ext/...` entries across open→save.
- Live compositor walks still only see Leaf/Group (stubs are non-visible).

---


- Open path uses `normalize_manifest_value` + `check_open_gate`
  (`min_reader.major`, `features_required`, then `format_version` migrate ladder).
- Writers emit both legacy fields and `format` / `min_reader` / `generator` /
  `features_*`. Documents without new features still write `format_version: 1`.
- `.dyuki` `app_version_min` is still **written** (old readers) and **accepted**
  with `#[serde(default)]`, but **not enforced** on unpack.
- `check_app_version_min` remains as a unit-tested helper for now; call sites in
  the open path removed.

---


Audited `FilterParams`, `DitherParamsV2`, `DitherModeV2`, palette payloads, and
`algorithm_id` wiring:

| Source field | Type | Executable? | Notes |
|---|---|---|---|
| All numeric filter params | `f32` / `u8` / `u16` / `u64` / bool | No | Validated ranges at apply-time |
| Enum modes (`DitherModeV2`, glitch, …) | closed enums | No | Unknown → deserialize error or Placeholder |
| `palette_id` | id | No | Remapped; missing → error |
| `CustomPng.path` | string | **Path data only** | On save rewritten to `{blake3}.png` basename; open materializes from zip into content-addressed cache — never executed, never shell’d |
| `algorithm_id` | string | **Whitelist** | Resolved via registry; unknown → disabled Placeholder, not dynamic load |
| `Placeholder.raw_params` | JSON | Opaque data | Not evaluated |
| Palette colors | linear RGB floats | No | |

**No** WGSL/GLSL shaders, expressions, scripts, plugin paths, or `dlopen` names
are accepted from files today. If a future filter adds code-like params, it MUST
be blocked at ingest (whitelist id only) and registered as a required feature.

---

## 2026-09-21 — Stage 1: secure_zip allowlist vs SPEC threshold hash length

SPEC §6.2 allowlist shows `assets/threshold_maps/[0-9a-f]{64}.png` (SHA-256).
**Current code** names threshold maps with **32 hex chars** (BLAKE3 truncated, see
`serialize/assets.rs`). Stage 1 allowlist accepts **32 or 64** hex so v1 goldens
and existing projects open; Stage 4 may migrate naming to full SHA-256 in
`manifest.files` without breaking the loader.

`mimetype` is validated when present but **not required** until Stage 4 (v1
archives omit it; loader records a warning).

Sanitize truncates by Unicode scalar count (not grapheme clusters); full
grapheme-aware trim is a follow-up if product requires it.

---

## 2026-09-21 — Stage 0: how released parsers treat the manifest (§5.1)

### Released app versions (public alpha)

- In-repo app version: `0.3.0-alpha.6` (`src-tauri/Cargo.toml`).
- Public releases documented from **0.2.0** (self-update) and **0.3.0-alpha.\*** tags (`docs/RELEASE.md`, README).
- On-disk format for both `.dyproj` and `.dyuki` is still **`format_version = 1`** only (`SUPPORTED_DYPROJ_VERSION` / `SUPPORTED_DYUKI_VERSION`).

### Manifest deserialize behavior (current code)

| Struct | Path | `deny_unknown_fields`? | Unknown JSON keys |
|---|---|---|---|
| `Manifest` | `serialize/migrate.rs` | **No** | Ignored by serde default |
| `PatternManifest` | `serialize/pattern.rs` | **No** | Ignored by serde default |
| `DocumentFile` / layer / filter DTOs | `serialize/document_dto.rs` | **No** | Ignored |

Implications for the Stage 2 manifest redesign:

1. **Safe to add** new optional/nested fields (`format`, `min_reader`, `features_*`, `files`, `generator`, …) to files that still ship `format_version: 1`. Older builds ignore unknown keys and keep opening the file.
2. **Not safe to bump** `format_version` above `1` in files that old builds must open: `check_format_version` rejects `found > supported` with `UnsupportedVersion`. Writers MUST keep writing the minimum major needed (SPEC §5.3).
3. **`format_version` remains required** on both manifests today (no `#[serde(default)]`). Keep writing the legacy alias forever for released parsers.
4. **`.dyuki` `app_version_min` is required** on `PatternManifest` (no default). Stage 2 will stop *using* it for compatibility gates but MUST still **accept** it when reading v1 files. New writers SHOULD omit it only after a soft transition that keeps a defaulted field on the reader (`#[serde(default)]` or `Option`). Decision for Stage 2 write path: omit after reader accepts absence; until then keep writing for byte-compat with current readers.
5. **Duplicate JSON keys** are not rejected (`serde_json` keeps the last value). Stage 1 MUST add an explicit duplicate-key rejector (SPEC §7.1 / T8).
6. **Kind check** is separate from version: `manifest.kind` must match open context (`KindMismatch`).
7. Zip I/O today (`serialize/archive.rs`) uses the `zip` crate with no entry allowlist, no compression-ratio limits, and allocates from declared `uncompressed_size` — Stage 1 `secure_zip` replaces this as the sole entry point.

### Safe Stage 2 migration path (chosen)

- Keep writing `format_version: 1` (legacy alias = `format.major`) for documents that only use v1 features.
- Add new fields alongside; old apps ignore them.
- Treat missing `format` / `min_reader` as derived from `format_version` (SPEC §5.2).
- Do **not** require new fields when reading v1 goldens.

### Open product questions (unchanged from SPEC §19)

Limits, Save-over-migrated behavior, original-image storage, shader/expression filters, OS thumbnails, exact public version matrix — deferred to product owner.

---

## 2026-09-21 — Stage 0: golden fixtures policy

- Location: `crates/engine-project/tests/fixtures/{dyproj,dyuki}/v1/`.
- Files are **immutable** after commit: CI locks SHA-256 via `SHA256SUMS`.
- Regeneration only with explicit `GENERATE_GOLDEN_V1=1` for *new* fixture names; existing hashes must not change.
- Snapshot assertions cover open success + structural fields (size, layer count, filter kinds), not full pixel dumps (pixels covered by existing unit round-trips).
