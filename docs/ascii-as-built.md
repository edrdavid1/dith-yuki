# ASCII system — as built (CPU path)

Companion to `.local-doc/ASCII_DITHER_system_spec.md`. This records what shipped
for the CPU / app-integration path (spec §8 steps 2–10). GPU stages (§8.11) and
the performance harness (§8.12) are **not** default yet.

## Product model

- First-class **EffectType / FilterKind::Ascii** (`EffectCategory::Ascii`), not a
  dither mode and not under Dithering in the chooser.
- `LayerKind::Ascii` deferred; ASCII is a filter on a raster layer.
- Preview: `ExecutionScope::FullDocument` → `AsciiJob` → Processed tiles
  (same publish path as Riemersma). Image|ASCII preview toggle skips Ascii in
  the full-doc pass without a separate `CacheStage::Ascii`.
- **Single-flight:** concurrent Processed requests for the same
  `(doc, layer, params_hash, document_gen, include_ascii)` wait on one job;
  cancel only when that key changes (not when a second worker wants the same result).

## Crates

| Path | Role |
|---|---|
| `crates/engine-ascii/` | Fonts (swash), atlas, symbols, descriptors, match (Tone / Shape / ShapeContrast / MaskTwoColor), color targets, cell dither, EdgeOverlay, convert, render, exporters |
| `crates/engine-project/src/filter.rs` | `FilterParams::Ascii` / `AsciiParams` |
| `crates/engine-project/src/algorithms/ascii.rs` | Registry algorithm + param schema |
| `crates/engine-project/src/filters/ascii_job.rs` | Job + export bytes |
| `crates/engine-project/src/filters/full_document.rs` | `compute_ascii_job` / full-document apply |

Bundled fonts (OFL): IBM Plex Mono, Departure Mono (native 11 px → 7×14 cell).
ANSI-16 default palette: VGA.

## Params (`AsciiParams`)

`font`, `size_mode` (`px` \| `columns`), `font_px`, `columns`, `antialias`,
`hinting`, `symbol_set`, `match_mode`, `contrast`, `color_mode`
(`mono` \| `fg` \| `fg_bg`), `color_target` (`truecolor` \| `xterm256` \|
`ansi16_vga` \| `ansi16_xterm` \| `ansi16_win10`), `cell_dither`
(`none` \| `bayer2` \| `bayer4` \| `bayer8` \| `floyd_steinberg`),
`serpentine`, `edge_overlay`, `edge_tau`.

## Export / clipboard

- IPC: `export_ascii`, `ascii_clipboard_text`
- Formats: `txt`, `ansi`, `html`, `svg`, `png`, `json`
- UI: File → **Export ASCII…**; Edit → **Copy ASCII Text** / **Copy ASCII ANSI**
  (in-app MenuBar + native menu)

## Image export composite

`export_image` uses `build_processed_composite_rgba8` (all visible raster layers
with filter stacks), not `find_first_visible_layer`. Same helper as project
thumbnails / Space Quick Look.

## Frontend

- Effect chooser + `AsciiSettings` (full param surface above)
- `frontend/src/shared/ipc/ascii.ts`
- `AsciiExportDialog`

## Preview Image|ASCII

Preview footer toggle (visible when an enabled Ascii filter exists):

- **ASCII** (default): full-document job includes Ascii → Processed shows text-art
- **Image**: Ascii filters skipped in the full-doc pass (Ascii tile apply is identity);
  other filters still run. Export/clipboard always use the Ascii grid.

IPC: `set_ascii_preview` / `get_ascii_preview`. No separate `CacheStage::Ascii` yet —
Processed tiles are invalidated and recomputed on switch.

**Rendering…:** while a full-document ensure is in flight (ASCII / Riemersma),
Preview shows a single status overlay (`full-document-busy` event), not a tile
checkerboard of old/new.

**Param debounce:** tiled filters stay at 100 ms; ASCII and Riemersma use 350 ms
in `useEffectLayer` so slider ticks coalesce before restarting the monolithic job.

## Still open

- Dedicated `CacheStage::Ascii` (optional; preview toggle works without it)
- Dedicated FlexLayout **ASCII** panel (settings live under Effect for now)
- GPU analyze / match / render (`engine-gpu` ascii module) + §6.2 parity gate
- §6.5 perf harness; flip GPU defaults per stage on evidence
- SVG exporter outline `<path>`s (current SVG uses text/rects)
