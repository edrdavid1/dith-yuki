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
| `platform/macos/DitherQuickLook/` | Quick Look Preview (+ optional Thumbnail source) |
| `platform/windows/dither-shell/` | COM thumbnail provider and preview handler (`dither_shell.dll`) |
| `platform/windows/dither-shell-diag/` | `dither-shell-diag` (`check`, `render`, `shell-thumb`, `register`, `clear-cache`, `last-errors`) |
| `platform/windows/registry_keys.nsh` | Generated from `registry_keys.rs` (do not edit) |
| `scripts/ci-prepare-*-previews.sh` | CI / `beforeBundleCommand` staging into the Tauri bundle |

Release embedding:

- macOS: Preview + Thumbnail `.appex` → Finder content thumbnails; Space uses Preview.
- Windows: `dither_shell.dll` + NSIS ShellEx → Explorer thumbnails and the preview pane.

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
| `CLSID_PREVIEW` | `{C8BC1EC9-FB9C-4374-9925-D82D2819A965}` |
| ShellEx thumbnail | `{E357FCCD-A995-4576-B01F-234630154E96}` |
| ShellEx preview | `{8895b1c6-b41f-4c1c-a562-0d564250836f}` |
| Preview AppID (64-bit prevhost) | `{6d2b5079-2f0b-48dd-ab7f-97cec514d30b}` |
| ProgIDs (Tauri) | `Dither Project`, `Dither Pattern` |

DLL: `platform/windows/dither-shell` → `dither_shell.dll` (Apartment, process isolation on). Registry source of truth: `registry_keys.rs`.

## If thumbnails or the preview pane do not show

1. `dither-shell-diag clear-cache` (caches show yesterday's image).
2. `dither-shell-diag check` and read which line is `FAIL` (key, DLL path, architecture).
3. ARM64 Windows needs an ARM64 DLL. An x64 DLL does not load into a native ARM64 Explorer.
4. Explorer setting "Always show icons, never thumbnails" turns thumbnails off.
5. Smart App Control or WDAC can block an unsigned DLL (Code Integrity log).
6. `dither-shell-diag render <file> --size 256 --out out.png`. A correct PNG means registration or isolation. A wrong PNG means `gdi` or `dither-thumb`.
7. Thumbnails work and the preview pane does not: preview registration only (`CLSID_PREVIEW`), not the file.
8. `dither-shell-diag last-errors` prints reason codes only (no paths).

## Adding a new file type

1. Add UTI / ProgID / MIME in app declarations.
2. Extend `ThumbKind` + MIME list in `dither-thumb` (still only `thumbnail.png`).
3. Subscribe Quick Look / thumbnail provider to the new UTI only — never generic ZIP.
