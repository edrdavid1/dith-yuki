# System previews architecture

Implements `.local-doc/SPEC_dither_previews_full.md`. Decisions:
[`FORMAT_DECISIONS.md`](./FORMAT_DECISIONS.md).

## Components

| Piece | Role |
|---|---|
| `thumbnail.png` in archive | Sole preview payload (write contract in FORMAT.md) |
| `crates/dither-zip-safe` | Shared limits, entry syntax, `ReadAt`, `LimitedReader` |
| `crates/dither-thumb` | `extract()`: EOCD → CD → `mimetype` + `thumbnail.png` → PNG → RGBA |
| `crates/dither-thumb-ffi` | C ABI (`dt_extract` / `dt_free_bitmap`), panic → `DT_INTERNAL` |
| `platform/include/dither_thumb.h` | Committed header (ABI v1) |
| `platform/macos/DitherQuickLook/` | Quick Look Preview + Thumbnail appex (XcodeGen) |
| `platform/windows/dither-shell/` | COM `IThumbnailProvider` (`dither_shell.dll`) |

## ABI

`dt_abi_version()` returns `1`. Platform code must refuse to load on mismatch.

```c
DtStatus dt_extract(const DtIo *io, DtKind kind, uint32_t max_side,
                    int premultiply, DtBitmap *out);
void dt_free_bitmap(DtBitmap *bmp);
```

- `DtIo.read_at` is random-access (not seek+read without mutex).
- No worker threads; cooperative 2 s deadline.
- Memory of `DtBitmap.rgba` owned by Rust until `dt_free_bitmap`.

## Trust boundary

Preview components run in system processes over files the user has not opened.
They read **only** `mimetype` and `thumbnail.png`. See `SECURITY.md`.

## CLSID / GUID (Windows)

Frozen — **never change** (bound to installed registry keys):

| Symbol | Value |
|---|---|
| `CLSID_THUMB` | `{BC7D0A00-220F-46DD-AAA8-C754864EE648}` |
| ShellEx thumbnail | `{E357FCCD-A995-4576-B01F-234630154E96}` |
| ProgIDs (Tauri) | `Dither Project`, `Dither Pattern` |

DLL: `platform/windows/dither-shell` → `dither_shell.dll` (Apartment, process isolation on).

## Adding a new file type

1. Add UTI / ProgID / MIME in app declarations.
2. Extend `ThumbKind` + MIME list in `dither-thumb` (still only `thumbnail.png`).
3. Subscribe Quick Look / thumbnail provider to the new UTI only — never generic ZIP.
