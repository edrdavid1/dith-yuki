# Design: Track M — ED kernels + Serpentine

## Overview

| ID | Deliverable | PR |
|----|-------------|----|
| **M1** | JJN / Stucki / Burkes / Sierra in V2 | first |
| **M2** | Serpentine + direction-aware residuals | second |

Source: [ROADMAP_production_release.md](../ROADMAP_production_release.md) §1.
A1 is closed: `IncomingErrorBuffer` / corner patch exist. Do not regress them.

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Letter | M, not G |
| Overflow | Keep 2px right/bottom/`CORNER_PATCH=2` — all listed kernels fit |
| Sierra | Standard Sierra (5/2/2 row, /32). Sierra Lite / two-row-only are **out** unless named in UI later |
| Burkes | /32 matrix, 2-row |
| Serde names | `jarvis_judice_ninke`, `stucki`, `burkes`, `sierra` |
| Legacy fallback | Remove FS fallback for JJN/Stucki in `DitherParamsV2::from` |
| Serpentine default | false |
| GPU | ED stays CPU |

### Kernel weights (normalize as documented)

**JJN** (/48):  
row0: `7, 5` (x+1, x+2)  
row1: `3, 5, 7, 5, 3` (x-2..x+2)  
row2: `1, 3, 5, 3, 1`

**Stucki** (/42):  
row0: `8, 4`  
row1: `2, 4, 8, 4, 2`  
row2: `1, 2, 4, 2, 1`

**Burkes** (/32):  
row0: `8, 4`  
row1: `2, 4, 8, 4, 2`

**Sierra** (/32):  
row0: `5, 3` at x+1, x+2  
row1: `2, 4, 5, 4, 2` at x-2..x+2  
row2: `2, 3, 2` at x-1, x, x+1 (not a full 5-tap row)

Implement via a shared `distribute_kernel(offsets: &[(i32,i32,f32)])` like legacy `dither.rs`, feeding the same overflow sides as `distribute_fs`.

---

## M2 Serpentine

Today the inner loop is `x = 0..SIZE` increasing. For odd `g.y`:

- Iterate `x` descending in the **interior**.
- FS-style offsets mirror in X: `(+1,0)` becomes `(-1,0)` when going R→L.
- Residuals: right-overflow on an R→L row is produced on the **left** edge in screen space — map to the neighbor that was processed **earlier** on the wavefront. ROADMAP: pass `row_dir` into buffer consume/produce helpers.

**Do not** implement serpentine only inside a tile while keeping L→R between tiles — that re-seams the joint (ROADMAP §1.1).

Wavefront tile order stays A1 diagonal. Only intra-row pixel order and kernel mirror change.

---

## UI

Dither mode `<select>` entries. Checkbox “Serpentine” visible for ED modes. Track K not required for checkbox/select.

---

## Testing

| Test | Step |
|------|------|
| Unit offsets | each kernel |
| Seam sample | 2×2 per kernel (or table) |
| Identity | serpentine false |
| Serpentine seam | even + odd global rows |
| Compat | JJN legacy → V2 JJN not FS |

---

## Future

- Sierra Lite / Stevenson-Arce
- GPU ED (never this track)
