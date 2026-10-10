# Gamedev toolkit — as-built (polish track)

Notes for the Phase 1–3 gamedev toolkit plus the polish items on
`feature/gamedev-toolkit`: **Pattern Manager** (reusable recipes), **ED
approximate wrap-padding**, and **hex/jasc palette fuzz**.

## Pattern Manager (reusable recipes)

Named filter/pattern recipes live in **Pattern Manager** (`.dyuki` library under
`{app_data_dir}/patterns/`). There is no separate Preset Manager dialog.

**UI:** Pattern Manager — list / save / rename / delete / apply / import /
export. Entry points: Effect panel pattern-action row, Batch Export dialog
(“Pattern Manager…”).

**Batch export** still snapshots the live document filter stack + palettes at
run time; use Pattern Manager beforehand to apply a saved recipe to the doc.

## ED approximate wrap-padding

Error diffusion cannot be exactly seamless (sequential wavefront). When
`DitherParamsV2.wrap_padding` is true:

1. Margin `(mx, my)` = `DiffusionKernel::edge_margin_xy(pixel_size)`
   (= `pixel_size × (|dx|max, |dy|max)`).
2. Pad opposite edges (left←right, top←bottom) by that margin.
3. Run ED on the padded buffer, crop back to the original rect.

Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.

Labeled **approximate only** in the Effect panel (checkbox + hint), the ADR, and
this doc. Ordered modes ignore the flag and remain bit-identical with the option
on or off. Without the flag, ED export determinism is unchanged (tile-strip
path). Images above the memory ceiling fall back to tile-strip with a UI warning.

**Visibility:** the wrap-padding checkbox is gated by Workspace → Gamedev with
Tiling → Advanced on (`UiVisibility.show_wrap_padding`). See
`.local-doc/Workspace.md`. Hidden in General; existing `wrap_padding` in
documents still renders and saves unchanged.

**Implementation:** `filters/ed_wrap_padding.rs`; full-document route when any
enabled ED filter has wrap-padding (`layer_has_full_document_filter_ex`).
ADR: `docs/adr/ed-wrap-padding.md`.

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
