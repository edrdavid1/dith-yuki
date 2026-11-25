# Gamedev workspace guide

**Gamedev** is for seamless textures, tiles, and sprite sheets. Enable it in
Preferences → Workspace. That opens **Gamedev Settings** and preview footer
tools for inspecting seams and cell grids.

## Order of actions

1. **Pick a job** in Gamedev Settings → **job** (Tile, Sprites, or Off).
2. **Dither** in the Effect panel (add or adjust a dither effect).
3. **Read status** in Gamedev Settings, then **inspect** in the preview footer
   (Check seams or Cell grid — display only).
4. **Export** via File → Export Image or Batch Export.

You pick the job; the app checks the result. You do not need to know about
wrap-padding or matrix periods unless a status row offers a fix.

## Job

One job at a time:

| Job | Use when |
|---|---|
| **Tile** | The image should repeat without a visible seam. |
| **Sprites** | You are working on a sprite sheet (cell size for Export / Batch). |
| **Off** | Neither — normal dither only. |

**Check seams** in the preview footer only *displays* the current result in a
3×3 grid. It does not change pixels and is not the same as turning Tile on.

**Cell grid** overlays the sprite grid (display only). Enable Sprites first.

## Status

Status rows report what was checked on the current result for the active job:

| Check | Job | Meaning |
|---|---|---|
| **Seamless** | Tile | Opposite edges match (or are close enough for error diffusion). |
| **Cells** | Sprites | Cell size fits the canvas; cells stay separate when that option is on. |
| **Alpha** | both | Soft (semi-transparent) edges, with a one-click threshold if needed. |

Green means verified on this result. Warnings and errors include a fix button
when one is available. Hover a row for details.

## Output

Defaults for this document’s export and preview:

- **Sharp preview** — nearest-neighbor scaling in the preview (view only).
- **Indexed PNG** — prefer PNG8 for Export and Batch.
- **Export scale** — dither at canvas÷N, then scale up with sharp pixels.
  Width and height must divide evenly by N.

## Patterns and batch

Pattern Manager and Batch Export work in every workspace. Apply a Pattern
Manager recipe before a batch run when you want a saved filter stack. In
Gamedev, Batch picks up cell size and indexed PNG from Gamedev Settings.

## Advanced: approximate wrap-padding

Under Effect → Tiling advanced (when enabled), **Approximate wrap-padding** is
a lighter fallback for error diffusion on large canvases. Results are
approximate, not exact — see
[`docs/adr/ed-wrap-padding.md`](../../adr/ed-wrap-padding.md) and
[`docs/gamedev-toolkit.md`](../../gamedev-toolkit.md).

Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.

## Not in this release

Animation / frame-set tools (Apply pattern → folder, temporal badge, stable
chip) are deferred. Engine support may exist; the Gamedev Settings strip does
not expose them yet.
