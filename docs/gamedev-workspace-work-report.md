# Work report: Gamedev toolkit polish + Workspace

**Date:** 2026-10-10  
**Branch:** `feature/gamedev-toolkit`  
**Merged to:** `main` via [PR #55](https://github.com/edrdavid1/dith-yuki/pull/55) (`c13daf4`)  
**Specs:** `.local-doc/gamedev.md` (`ed_wrap_padding_v1`), `.local-doc/Workspace.md` (`workspace_profiles_v1`)

---

## 1. Executive summary

This track completed the gamedev polish work and introduced a user-facing **Workspace** concept so tiling / seamless / wrap-padding UI stays out of the way for general users, while remaining fully available for texture/tile workflows.

Shipped together:

| Area | Outcome |
|---|---|
| Pattern Manager | Confirmed as the only recipe UI; separate Preset Manager removed |
| ED wrap-padding | Production path: pad → ED → crop, limits, cancel, tests, ADR |
| Workspace | General (default) / Gamedev with `UiVisibility` as single UI gate |
| UI chrome | Radio buttons styled with Chicago-style round checked dots |
| Palette fuzz | hex / jasc (PAL) fuzz targets + CI mutation tests |

---

## 2. Goals and constraints

### Goals

1. Make approximate seamless ED usable for tiled export (GA-ready, not a stub).
2. Avoid overwhelming non-gamedev users with tiling controls.
3. Keep one binary / one codebase; workspace is a runtime preference only.
4. Never silently mutate document data when switching workspace or hiding UI.

### Constraints honored

- **Non-destructive workspace:** `wrap_padding` and other hidden fields remain in documents and still render/save in General.
- **Visibility ≠ capability:** engine always compiles wrap-padding; UI decides exposure via `UiVisibility`.
- **No filter-logic rewrite for Workspace:** only config + UI gating (plus wrap-padding engine work from its own spec).
- **Canonical hint text** for wrap-padding kept byte-identical across ADR, help docs, Rust constant, and frontend.

---

## 3. Pattern Manager (recipes)

### Decision

A new “Preset Manager” duplicated Pattern Manager. That path was removed so **Pattern Manager** remains the single place for named `.dyuki` recipes.

### As-built

- Library under `{app_data_dir}/patterns/`.
- Entry points: Effect panel pattern actions, Batch Export (“Pattern Manager…”).
- Batch export still snapshots the live document filter stack at run time; apply a recipe first if a saved stack is needed.

---

## 4. ED approximate wrap-padding (`ed_wrap_padding_v1`)

### Problem

Error diffusion is sequential. Exact seamless tiling is impossible. Users still need reduced seams for tiles/textures.

### Decision

Approximate wrap-padding:

1. Margin `(mx, my) = pixel_size × (|dx|max, |dy|max)` via `DiffusionKernel::edge_margin_xy`.
2. Pad source with wrapped opposite edges (fresh allocation).
3. Run existing ED on the padded buffer (unchanged loop semantics).
4. Crop back to original `W × H`.

Documented as **approximate, not exact**. Ordered modes ignore the flag.

### Engine implementation

| Piece | Location |
|---|---|
| Core algorithm, errors, metrics | `crates/engine-project/src/filters/ed_wrap_padding.rs` |
| Asymmetric margins | `DiffusionKernel::max_offset_xy` / `edge_margin_xy` in `filter.rs` |
| Full-document routing | `layer_has_full_document_filter_ex` + wrap path in `full_document.rs` |
| Size / overflow fallback | Tile-strip for that filter; INFO/WARN logs + counters |

**Confirmed before implementation (spec open questions):**

- Serpentine ED: **supported** → pad both horizontal sides.
- Multi-pass ED: **not supported** → single-pass only.
- `edge_margin` for wrap-padding returns `(u32, u32)`, not a scalar.

**Limits (NFR-1):**

- `MAX_PADDED_PIXELS = 200_000_000`
- `MAX_MARGIN_PX = 512`
- Checked arithmetic + `Vec::try_reserve`; cancel polled every 64 rows.

**Canonical hint:**

> Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.

### Tests and fuzz

- Unit tests in `ed_wrap_padding.rs` (margins, wrap, pad/crop, limits, cancel, progress, hint↔ADR).
- Integration: `crates/engine-project/tests/ed_wrap_padding.rs` (routing matrix, seam metric, determinism).
- Property: `tests/prop_ed_wrap_padding.rs`.
- Fuzz: `fuzz/fuzz_targets/fuzz_ed_wrap_padding.rs` + committed seed corpus under `fuzz/corpus/fuzz_ed_wrap_padding/` (gitignore exception).

### Docs

- ADR: `docs/adr/ed-wrap-padding.md`
- As-built: `docs/gamedev-toolkit.md`
- Changelog: `CHANGELOG.md` (Unreleased)

### UI placement (after Workspace)

Checkbox lives under **Gamedev → Tiling → Advanced**, not in General. Frontend constants: `frontend/src/shared/wrapPadding.ts`.

---

## 5. Workspace selection (`workspace_profiles_v1`)

### Concept

| Workspace | Intent |
|---|---|
| **General** (default) | Web / print / general graphics; tiling UI hidden |
| **Gamedev** | Textures, tiles, sprites; tiling tools visible |

### Config

- File: `{app_data_dir}/config.json`
- Types: `Workspace`, `AppConfig`, `AdvancedConfig` (`tiling_advanced`)
- Schema version `1`; v0 → v1 migration adds workspace + advanced defaults
- Atomic writes via `engine_io::atomic_write`; debounced ≤ 1 write / 500 ms
- Unknown JSON fields preserved (`serde` flatten bag)
- Invalid workspace string → fall back to General + WARN

**Rust modules:**

- `src-tauri/src/config/mod.rs`
- `src-tauri/src/config/migrations.rs`
- `src-tauri/src/config/visibility.rs`
- IPC: `src-tauri/src/commands/workspace.rs`

### `UiVisibility` (sole UI authority)

| Flag | General | Gamedev (advanced off) | Gamedev (advanced on) |
|---|---|---|---|
| Tiling section | hidden | visible | visible |
| Seamless 3×3 preview | hidden | visible | visible |
| Tile size (sprite cells) | hidden | visible | visible |
| ED temporal / animation hints | hidden | visible | visible |
| Wrap-padding checkbox | hidden | hidden | visible |
| Advanced toggle | hidden | visible | visible |
| Pattern Manager / Batch Export | visible | visible | visible |

Frontend mirror: `frontend/src/shared/workspace/visibility.ts`  
Context: `frontend/src/features/workspace/WorkspaceContext.tsx`

### UX surfaces

1. **Preferences → Workspace** — two radios (General / Gamedev), immediate effect.
2. **Upgrade prompt** — one-time modal when upgrading from pre-workspace config (not on fresh install).
3. **Hint banner** — non-blocking “working with tiles…” when General users hit tile-like workflows; Switch / Dismiss; auto-dismiss 30s.
4. **Help** — guides under `docs/help/general/` and `docs/help/gamedev/`; Help dialog opens the guide matching the active workspace.

### Bugfix (post-merge of feature work, before PR merge)

**Symptom:** Switching to Gamedev in Preferences did nothing.

**Cause:** Workspace Tauri commands used `State<'_, AppState>` while the app manages `Arc<AppState>`, so IPC state injection failed and `get_app_config` / `set_workspace` never ran.

**Fix:** `6c8490a` — inject `State<'_, Arc<AppState>>`; log IPC failures in `WorkspaceContext`.

**Regression coverage:** `src-tauri/src/commands/workspace_ipc_tests.rs` — mock Tauri app with `.manage(Arc<AppState>)`, asserts commands are registered, and smokes `get_app_config` / `set_workspace` / `set_tiling_advanced` through `get_ipc_response` (would fail on a State type mismatch).

**FR-3 vs “restart” wording:** Workspace switch itself is immediate in-process (FR-3). Any “restart” during verification was only to pick up a **rebuilt Rust binary** under `tauri dev` after the IPC fix — not a product requirement to restart after switching workspace. Shipping builds already load registered handlers at process start; users do not restart for a workspace change.

---

## 6. UI chrome polish

- Native radio inputs restyled to match app checkbox chrome.
- Checked state uses a **round radial inner dot** (not a square).
- Styles in `frontend/src/shared/styles/reset.css`.

---

## 7. Palette hex / jasc fuzz (polish track)

- Fuzz targets: `fuzz_palette_hex`, `fuzz_palette_pal`.
- CI mutation: `cargo test -p engine-color --test palette_fuzz_mutation`.
- Fixtures under `crates/engine-color/tests/fixtures/palettes/`.
- Malformed input must not panic.

---

## 8. Commit history (this merge)

Commits on `feature/gamedev-toolkit` merged in PR #55 (oldest → newest after earlier toolkit base):

| Commit | Summary |
|---|---|
| `c6bf41d` | Gamedev toolkit polish: presets, ED wrap-padding, hex/jasc fuzz |
| `dfd86d7` | Remove separate Preset Manager; reuse Pattern Manager |
| `a85d5bb` | Style native radio buttons to match checkbox chrome |
| `585b265` | Round radial dot for checked radios |
| `6f130f2` | Production ED wrap-padding (limits, cancel, fallback) |
| `e9477de` | Wrap-padding tests + fuzz target/corpus |
| `acd543d` | ADR, toolkit notes, changelog |
| `4bb7ca6` | Shared wrap-padding UI constants |
| `59cb306` | Workspace app config + UiVisibility + IPC |
| `8f091a1` | Workspace frontend context, prompt, hint |
| `3f85a40` | Gate tiling UI + Preferences radios |
| `9171184` | General/Gamedev help guides |
| `6c8490a` | Fix Workspace IPC `Arc<AppState>` injection |

Merge commit on `main`: `c13daf4`.

---

## 9. Key files touched

### Engine

- `crates/engine-project/src/filters/ed_wrap_padding.rs` (new/expanded)
- `crates/engine-project/src/filters/full_document.rs`
- `crates/engine-project/src/filter.rs` (`edge_margin_xy`)
- `crates/engine-project/tests/ed_wrap_padding.rs`
- `crates/engine-project/tests/prop_ed_wrap_padding.rs`

### App / config

- `src-tauri/src/config/*`
- `src-tauri/src/commands/workspace.rs`
- `src-tauri/src/commands/workspace_ipc_tests.rs` (IPC registration + smoke)
- `src-tauri/src/main.rs` (handler registration + `rebind_app_data`)

### Frontend

- `frontend/src/features/workspace/*`
- `frontend/src/shared/workspace/*`
- `frontend/src/shared/wrapPadding.ts`
- `frontend/src/features/preferences/PreferencesPanel.tsx`
- `frontend/src/features/effects/editors/DitherSettings.tsx`
- `frontend/src/components/PreviewWindow.tsx`, `ExportImageDialog.tsx`, `BatchExportDialog.tsx`, `HelpDialog.tsx`

### Docs

- `docs/adr/ed-wrap-padding.md`
- `docs/gamedev-toolkit.md`
- `docs/help/general/README.md`, `docs/help/gamedev/README.md`
- `CHANGELOG.md`

---

## 10. Verification performed

| Check | Result |
|---|---|
| `cargo test config::` (Workspace unit tests) | Passed (11) |
| `cargo test -p engine-project --lib filters::ed_wrap_padding` | Passed (17) |
| `cargo test -p engine-project --test ed_wrap_padding --test prop_ed_wrap_padding` | Passed |
| `cargo clippy -p engine-project --lib --no-deps -D warnings` | Clean for wrap module |
| Vitest `wrapPadding` + `visibility` | Passed |
| Frontend `tsc --noEmit` | Passed |
| Manual: Workspace switch after IPC fix | Fixed (restart only needed in `tauri dev` to load the rebuilt Rust binary; runtime switch is immediate per FR-3) |
| `cargo test workspace_ipc` (IPC registration + smoke) | Added — catches `State<'_, AppState>` vs `Arc<AppState>` |
| `cargo bench -p engine-project --bench wrap_padding_false_bench` (NFR-4 / DoD ≤2%) | **PASS** — path delta ≈ −0.5% (noise); routing abs overhead ≈ 2–5 ns (~0.0004% of one 256² FS tile). No fix ticket. |

### NFR-4 benchmark detail (`wrap_padding = false`)

There was no attached number in the original merge report; the placeholder `filter_bench` does not cover this path. Added `crates/engine-project/benches/wrap_padding_false_bench.rs` and ran on `main` (`c13daf4`).

| Metric | Pre-feature routing | Current (`flag=false`) | Δ |
|---|---|---|---|
| Routing (single ED layer), median | 40.2 ns | 41.2 ns | +2.5% rel / +1.0 ns abs |
| Routing (levels+ED stack), median | 45.2 ns | 47.6 ns | +5.4% rel / +2.4 ns abs |
| Floyd–Steinberg 256² tile | — | 0.61 ms | — |
| Routing overhead vs that tile | — | — | **0.0004%** |
| Tile-strip-shaped path (20× route + 1 ED tile) | 0.605 ms | 0.601 ms | **−0.55%** (DoD gate) |

**Verdict:** DoD gate is the tile-strip-shaped path (not the micro % on a ~40 ns check). Gate **PASS** (< 2% regression). No fix issue opened. Numbers also posted on [PR #55](https://github.com/edrdavid1/dith-yuki/pull/55).

---

## 11. Known limitations and follow-ups

### Known limitations

- Wrap-padding is **approximate**; not bit-periodic seamless ED.
- Wrap-padding skipped above memory / margin ceilings (tile-strip fallback + UI banner).
- Ordered dither ignores wrap-padding (already period-correct).
- Workspace is **app-global** in v1 (not per-project).
- Behavioral hint triggers are best-effort proxies (e.g. Batch Export open ≈ “tile mode” until a dedicated tile export mode exists).

### Suggested follow-ups

1. Run wrap-padding fuzz for an extended soak (`cargo fuzz run fuzz_ed_wrap_padding`).
2. Add UI Vitest coverage for Preferences radios / prompt (spec §7.3 items not all automated).
3. Wire Prometheus-style counters to a real dashboard if ops wants NFR-4 visibility beyond in-process atomics.
4. Consider forcing `spriteCells` off when leaving Gamedev so export options cannot linger in local dialog state.
5. Phase 4 gamedev items remain out of scope (Aseprite / CLI / mips / atlases).
6. Extend the workspace IPC smoke pattern to other high-risk command groups that take `State<'_, Arc<AppState>>` (document / filters / export) so State-type mismatches cannot merge silently again.

---

## 12. How to use (operator notes)

1. **General user:** leave Workspace on General; dither and export as usual — no tiling clutter.
2. **Texture / tile artist:** Preferences → Workspace → **Gamedev**.
3. **Wrap-padding:** Gamedev → Effect → Dither → **Tiling** → enable **Advanced** → **Approximate wrap-padding**.
4. **Seam check:** preview footer **3×3** (Gamedev only).
5. **Recipes:** Pattern Manager (not a separate Preset Manager).

---

## 13. References

- Spec: `.local-doc/gamedev.md`
- Spec: `.local-doc/Workspace.md`
- ADR: `docs/adr/ed-wrap-padding.md`
- As-built: `docs/gamedev-toolkit.md`
- PR: https://github.com/edrdavid1/dith-yuki/pull/55
