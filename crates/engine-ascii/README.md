# engine-ascii

Text-art / ASCII output stage for Dither Yuki. Design:
[`.local-doc/ASCII_DITHER_system_spec.md`](../../.local-doc/ASCII_DITHER_system_spec.md).
As-built app integration: [`docs/ascii-as-built.md`](../../docs/ascii-as-built.md).

This crate does **not** depend on `engine-project`. It takes an RGBA buffer +
settings and returns an [`AsciiGrid`](crate::AsciiGrid); preview and exporters
render from that grid.

## Bundled fonts (SIL OFL 1.1)

| Font | File | Notes |
|------|------|--------|
| IBM Plex Mono Regular | `assets/fonts/IBMPlexMono-Regular.ttf` | Outline monospace |
| Departure Mono Regular | `assets/fonts/DepartureMono-Regular.otf` | Pixel font; native **11 px → 7×14** cell |

Licenses: `assets/fonts/*-OFL.txt`.

## Current surface

- Font load / monospace validation / cell metrics
- Symbol sets (Bourke ramps, ASCII, blocks, quadrants, sextants, octants, braille, CP437)
- Procedural geometry for block-type glyphs
- Glyph atlas (`swash`) + cache
- Tone / Shape / ShapeContrast / MaskTwoColor matching + EdgeOverlay (DoG/Sobel)
- Color targets: TrueColor, xterm-256, ANSI-16 (VGA / xterm / Win10)
- Cell dither: Bayer 2/4/8 + Floyd–Steinberg (tone + vector shape)
- Colored convert + RGBA / sRGB8 render
- Exporters: TXT, ANSI (+ parse-back), HTML, SVG (text+rects), PNG, JSON

App wiring (outside this crate): `FilterKind::Ascii` / `AsciiParams`, Effect UI,
File → **Export ASCII…**, Edit → **Copy ASCII Text** / **Copy ASCII ANSI**,
Image|ASCII preview toggle. See `docs/ascii-as-built.md`.

## Still open

- SVG outline `<path>` / `<use>` (currently `<text>` / rects)
- JJN/Stucki/Atkinson cell kernels beyond FS
- `CacheStage::Ascii` for Image|ASCII dual view (preview currently publishes as Processed)
- Dedicated FlexLayout ASCII panel (settings live under Effect)
- GPU path + perf harness

## Regenerate Unicode tables

```bash
curl -sSfLO https://www.unicode.org/Public/16.0.0/ucd/UnicodeData.txt
curl -sSfLO https://www.unicode.org/Public/MAPPINGS/VENDORS/MICSFT/PC/CP437.TXT
python3 tools/gen_block_tables.py UnicodeData.txt CP437.TXT > src/symbols/block_tables.rs
```
