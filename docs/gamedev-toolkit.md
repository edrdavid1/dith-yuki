# Gamedev toolkit — as-built (polish track)

Notes for the Phase 1–3 gamedev toolkit plus the polish items on
`feature/gamedev-toolkit`: **Preset Manager**, **ED approximate wrap-padding**,
and **hex/jasc palette fuzz**.

## Preset Manager

**What a preset stores:** filter stack (all instances, params, order) + palette
**bindings by id** + dither/algorithm settings. It does **not** store export
format, PNG8 vs PNG, output paths, or DPI.

**On disk:** `{app_data_dir}/presets/*.json` (user-scoped, not inside `.dyproj`).
Wire shape: `preset_version: 1`, `id`, `name`, `created_at`, `filters`
(`FilterInstanceFile[]`), `palette_ids`. Schema:
[`schema/filter-preset.schema.json`](./schema/filter-preset.schema.json).

**Tauri commands:** `preset_list`, `preset_save`, `preset_load`, `preset_delete`,
`preset_rename`, `preset_apply_to_doc(doc_id, preset_id, layer_id?)`.

**Validation / migration:**

| Case | Behavior |
|------|----------|
| Unknown `algorithm_id` on **save** | Typed error — refuse to persist |
| Retired `algorithm_id` on **load/apply** | Skip that filter; surface a warning; never panic |
| Missing palette id on apply | Keep `palette_id` in params; warn `Palette N not found — pick another` |

**UI:** Preset Manager dialog (list / save-from-current-stack / rename / delete /
apply). Entry points: Effect panel pattern-action row, Batch Export dialog.

**Engine types:** `engine_project::preset::{FilterPreset, PresetError, …}`.
Storage helper: `src-tauri/src/preset_library.rs`.

## ED approximate wrap-padding

Error diffusion cannot be exactly seamless (sequential wavefront). When
`DitherParamsV2.wrap_padding` is true:

1. Margin = `DiffusionKernel::edge_margin(pixel_size)` (= `pixel_size × max_offset()`).
2. Pad opposite edges (left←right, top←bottom) by that margin.
3. Run ED on the padded buffer, crop back to the original rect.

Labeled **approximate only** in the Effect panel seamless hint, the checkbox
(“Approximate wrap-padding”), and this doc. Ordered modes ignore the flag and
remain bit-identical with the option on or off. Without the flag, ED export
determinism is unchanged (tile-strip path).

**Implementation:** `filters/ed_wrap_padding.rs`; full-document route when any
enabled ED filter has wrap-padding (`layer_has_full_document_filter_ex`).

## hex / jasc palette fuzz

Parsers: `palette/formats/hex.rs`, `jasc.rs`, `pal.rs` (RIFF + JASC autodetect).

| Target | Seeds | CI |
|--------|-------|-----|
| `fuzz/fuzz_targets/fuzz_palette_hex.rs` | `crates/engine-color/tests/fixtures/palettes/*.hex` | `cargo test -p engine-color --test palette_fuzz_mutation` |
| `fuzz/fuzz_targets/fuzz_palette_pal.rs` | `…/*.pal` (JASC + RIFF) | same |

Fuzz targets parse **and** build `LinearColor` entries (same as import). Corpus
under `fuzz/corpus/` stays local (gitignored); copy fixtures before
`cargo fuzz run`. Malformed input → `PaletteError::ParseError`, never panic.

## Out of scope (this track)

- Soft-proof / CMYK print export (separate track).
- Phase 4: Aseprite / CLI / mips / atlases (deferred).
