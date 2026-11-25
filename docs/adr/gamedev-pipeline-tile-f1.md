# ADR: Gamedev Pipeline Tile (F1) — context, route, memory gate, chips, seam metric

**Status:** Accepted (F1 steps 1–4 shipped); F6 strip polish / F2+ open  
**Date:** 2026-11-19  
**Working name:** `gamedev_pipeline_tile_f1`  
**Spec:** [`.local-doc/gamedev-2.md`](../../.local-doc/gamedev-2.md)

**Locked (2026-11-19, step 1 fixtures):** `k = 1.25`, `ε = 0.003` (0.3% of pairs).
Constants live in `engine_project::SEAM_K` / `SEAM_EPSILON`. Fixture suite:
`crates/engine-project/tests/gamedev_seam_fixtures.rs`. Validator:
`validate_seams` / `validate_seams_rgba8` in `gamedev/seams.rs`.

**Locked (2026-11-19, step 3 memory gate; revised 2026-11-22 interactive):** mosaic
Pipeline Tile refuses when either side exceeds `PIPELINE_TILE_MAX_SOURCE_SIDE = 1024`
(spec ~1K interactive target) **or** 9× pixels exceed
`PIPELINE_TILE_MAX_MOSAIC_PIXELS = 9 × 1024²`. Oversize →
`PipelineTileError::InteractiveLimit` / `MemoryGate` (no allocate) + seamless chip
`tile · too large` / Error. Period-aligned ordered/BN that **skips** the mosaic is
not side-gated. Relative seam fail → chip Warn (not Error).

## Context

Gamedev workspace today mostly gates hints (period text, temporal copy, wrap-padding
checkbox) plus a **display-only** FE 3×3 repeat. That conflates inspection with
correctness: a tiled preview of a non-seamless dither can look fine while export
is wrong.

We need a first vertical slice where:

1. **Pipeline Tile** changes pixels (preview == export).
2. **View-mode 3×3** only displays the result.
3. Seam status is a **result-based chip**, not a period essay.
4. ED is honest: approximate, with a relative metric — not a fake binary “seamless ✓”.

Wrap-padding (`ed_wrap_padding_v1`) stays as the lightweight Advanced fallback.

## Decision

### 1. Storage split

| Concern | Store |
|---|---|
| Workspace, `tiling_advanced`, strip visibility | `AppConfig` → `UiVisibility` |
| Job / tile / cell / palette / mask / export / alpha / pixel-perfect / `frame_set` | `Document.gamedev` (`GamedevContext`) in `.dyproj` |
| Chip ok/warn/pending | Runtime hash cache only |

Engine owns pixel-affecting truth; React mirrors. No new document type for frame
sets (`frame_set: Vec<PathBuf>` later in F3).

`GamedevContext` may bootstrap via `Document.extra` then become a typed
`Document` / `DocumentFile` field with migrate.

### 2. Two “3×3”s (do not merge)

| Name | Control | Effect |
|---|---|---|
| **Pipeline Tile** | `GamedevContext.tile.enabled` | Assemble **source** 3×3 → dither mosaic → crop center |
| **View `repeat_3x3`** | Preview view mode | FE (or compositor) repeat of **already dithered** center; no pixel change |

View mode must never drive the seamless chip. Chip validation always runs on the
pipeline/export buffer.

### 3. Tile vs wrap_padding

- Pipeline Tile is the primary seamless path.
- Enabling Tile **forces `wrap_padding = false`** on affected dither filters (Tile wins).
- Wrap-padding remains Advanced-only; chip may offer it when Tile is blocked by the memory gate.

### 4. Full-document routing

When `tile.enabled` and the memory gate passes, the layer uses the full-document
route (same family as wrap-padding / other FD triggers via
`layer_has_full_document_filter_ex`):

1. Build 3×3 mosaic from **source** (not from partially dithered tiles).
2. Run the existing filter stack on the mosaic.
3. Crop the center `W×H`.

**Optimization:** ordered / blue-noise with period dividing `W` and `H` may skip
the mosaic (bit-periodic already). Ring validation still runs.

**Honesty:** ED mosaic reduces seams but does **not** make opposite edges share
error history. Exact “seamless ✓” is reserved for ordered/blue-noise with
tolerance 0. ED UI uses **`seamless ≈`** + metric.

### 5. Memory gate (mandatory F1)

Mosaic ≈ 9× pixels. Before allocate:

- Hard ceiling in the spirit of `wrap_padding_fits_limits` / `MAX_MARGIN_PX`
  (exact constant chosen during impl; document beside wrap-padding limits).
- Oversize → do not run mosaic; chip `error` + actions
  (“Use approximate wrap-padding” / “Disable Tile” / shrink canvas).
- No silent fallback that leaves the chip green.

Measure peak RSS at the max allowed size; record in as-built notes.

### 6. Chip IPC

- Validation is async off the UI thread; UI shows `pending` → `ok` | `warn` | `error`.
- Payload: chip id, state, short label, tooltip (metrics), optional action id.
- Cache key = hash(result buffer identity / generation, algorithm+params, relevant
  `GamedevContext`). Runtime only — not serialized.

Actions in F1: snap/crop to period; disable Tile; enable wrap-padding fallback;
clear pending after revalidate.

### 7. Seam mismatch operator (locked)

Compare **quantized output** byte-by-byte:

- Indexed: palette **index** equality.
- Else: **RGBA** byte equality.

No linear light, no ΔE. Metric = binary fraction of mismatched pairs.

**Alpha (F4 rule):** compare alpha first. Fully transparent pixels: RGB is
ignored; a pair matches only if **both** are fully transparent (or both opaque
with matching RGB under the byte rule when comparing opaque pairs — transparent
vs opaque is a mismatch).

### 8. ED relative metric (per axis + near-zero floor)

Evaluate **horizontal** and **vertical** independently (anisotropic textures
must not dilute a bad axis).

For axis `a ∈ {H, V}`:

- `R_boundary_a` — mismatch fraction on wrap pairs (H: right↔left per row;
  V: bottom↔top per column).
- `R_internal_a` — mismatch fraction on adjacent **internal** columns (H) or
  rows (V), same operator.

Axis ok iff:

```text
R_boundary_a ≤ max(k × R_internal_a, ε)
```

- **`k = 1.25`** (locked).
- **`ε = 0.003`** (0.3% of pairs, locked): near-zero floor so flat regions
  (`R_internal → 0`) are not failed by a vanishingly small seam budget — not a
  palette/content threshold. A single mismatched wrap pair on a 96-row tile
  (`≈1.04%`) still fails.
- Overall ED ok iff **both** axes ok. Chip may show the worse axis.

Ordered / blue-noise: every wrap pair must match (`tol = 0`) via `SeamMode::Exact`.

### 9. Fixtures (locked)

Suite in `tests/gamedev_seam_fixtures.rs`:

1. Smooth gradient (forced wrap-equal edges)  
2. Noisy photo-like  
3. Flat fills + detail block  
4. High-frequency checker  
5. Known **non-seamless** control (must fail)  
6. Anisotropic H-only stitch (H fails, V ok)  
7. Flat ε-floor (clean ok; one-pixel seam fails)

Good fixtures: `R_boundary = 0` → pass exact + relative. Control: blocky
interior (low `R_internal`) with broken wrap edges so axis ratios `≫ k`.
**Do not** use white-noise controls to lock `k` — when `R_internal ≈ 1`, a full
seam yields ratio ≈ 1 and false-passes under `k = 1.25`. Period-not-multiple
remains a separate warn + snap/crop (step 4).

## Consequences

**Positive**

- Preview == export for Tile; view 3×3 cannot fake seamlessness.
- ED messaging stays honest (`≈` + metric).
- Memory failures are explicit.
- Wrap-padding demoted cleanly without deleting the Advanced path.

**Negative**

- Full-doc 9× path is heavy; gate will reject large canvases.
- Relative metric needs fixture tuning before claiming “ok”.
- Document schema gains `GamedevContext` (migrate / `extra` bootstrap).

## Alternatives considered

| Alternative | Outcome |
|---|---|
| Keep FE 3×3 as the seamless feature | Rejected — false `seamless ✓`. |
| Absolute ED mismatch threshold | Rejected — content/palette dependent. |
| Combined H+V single `R` | Rejected — anisotropic false pass/fail. |
| Exact seamless ED | Rejected — impossible for sequential ED. |
| Replace wrap-padding entirely | Rejected — keep as light Advanced fallback. |

## Implementation sketch (non-normative file targets)

| Piece | Likely home |
|---|---|
| `GamedevContext` | `crates/engine-project` + `serialize/document_dto.rs` |
| Mosaic + crop route | `filters/full_document.rs` (+ new helper module) |
| Ring / metric | `crates/engine-project/src/gamedev/seams.rs` (`validate_seams`) |
| Memory gate | next to `wrap_padding_fits_limits` |
| Clear wrap on Tile | set-context / filter update path |
| Chip IPC | `src-tauri/src/commands/` + FE strip |
| View mode | `PreviewWindow` / `TileCanvas` (existing `tileRepeat3x3` → named view mode) |

## Reference

- Spec: `.local-doc/gamedev-2.md` (§2 principles 7–10, §4 storage, §6 F1)
- Related ADR: [`ed-wrap-padding.md`](./ed-wrap-padding.md)
- User guide (update after ship): `docs/help/gamedev/`, `docs/gamedev-toolkit.md`
