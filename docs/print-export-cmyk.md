# Print export (CMYK TIFF) — as built

Pixel CMYK export of the Processed+Composite result for press workflows.
Soft proof remains display-only; RGB export still ignores Soft proof.

Companion product notes may live under `.local-doc/`; this file is the
**as-built** contract for print export on `feature/soft-proof` (and follow-ons).

## What it is (and is not)

| Print export **is** | Print export **is not** |
|---|---|
| CMYK TIFF 8-bit with embedded ICC + PPI | Soft proof / screen simulation |
| Forward `sRGB → CMYK` via **lcms2** (native BPC) | Plate-screen dither / angled halftone |
| Integer nearest-neighbor scale only | Spot colors, overprint, printer marks |
| Optional pure-K for exact RGB black | Certified PDF/X (PDF is a later phase) |

## CMS decision

Bake-off (`crates/engine-color/tests/export_cms_bakeoff.rs`):

- moxcms vs lcms2 **Relative, no BPC**: max channel Δ ≤ 2 on FOGRA51 grid.
- App-owned XYZ “forward BPC” then moxcms: poor match at black vs lcms2 BPC.
- **Ship lcms2 for print export only.** Soft proof stays on moxcms (pure Rust).

`lcms2` is a normal dependency of `engine-color` (MIT). Soft-proof golden tests
still use the same crate as the reference.

## Pipeline

```text
build_processed_composite_rgba8  (ignores Soft proof)
        │
        ▼
optional integer nearest scale (after convert, or scale CMYK)
        │
        ▼
unique RGB ≤ 4096? ──yes──► convert palette once + remap
        │
       no
        ▼
lcms2 pointwise sRGB8 → CMYK8  (+ Relative BPC when requested)
        │
        ▼
pure K / paper white rules
        │
        ▼
TIFF strips (CMYK8 + ICC tag 34675 + X/YResolution) → atomic rename
```

## Pure black / white

- `pure_black_k` (default on): exact RGB(0,0,0) → CMYK(0,0,0,255).
- RGB(255,255,255) → CMYK(0,0,0,0) always.

Near-black dither dots are not remapped in v1.

## Gamut report

Async, non-blocking. Reports out-of-gamut pixel fraction (round-trip ΔE2000 ≥ 2),
shifted palette colors, CMYK collisions, and max ink coverage %. Does not block export.

## UI / IPC

- Menu: **File → Export for Print…**
- Commands: `print_export_estimate`, `print_export_gamut_report`,
  `print_export_run`, `print_export_cancel`
- Does **not** mark the document dirty.

## Code map

| Piece | Location |
|---|---|
| CMS + palette + gamut | `crates/engine-color/src/print_export.rs` |
| TIFF writer | `crates/engine-io/src/cmyk_tiff.rs` |
| Tauri commands | `src-tauri/src/commands/print_export.rs` |
| Dialog | `frontend/src/components/ExportPrintDialog.tsx` |

## Limits / follow-ups

- PDF + OutputIntent is phase 2 (do not claim PDF/X).
- Neutral grays → K-only deferred.
- Confirm ECI embed-in-file license text for bundled PSO profiles.
- Manual open in Photoshop / Affinity / GIMP / `tiffinfo` before calling the feature done.
