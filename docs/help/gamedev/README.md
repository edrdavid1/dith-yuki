# Gamedev workspace guide

**Gamedev** workspace is for seamless textures, tiles, sprites, and
animation-ready assets. Enable it in Preferences → Workspace.

## Tiling

- Use the preview **3×3** toggle to inspect tile seams.
- Seamless period hints appear under dither settings for ordered modes.
- Animation temporal warnings call out modes that flicker between frames.

### Advanced tiling

Open **Advanced** under the Tiling section to reveal **Approximate wrap-padding**
for error diffusion (pad → ED → crop). Results are approximate, not exact —
see [`docs/adr/ed-wrap-padding.md`](../../adr/ed-wrap-padding.md) and
[`docs/gamedev-toolkit.md`](../../gamedev-toolkit.md).

Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.

## Sprite sheets

Export Image can process sprite-sheet cells independently (ordered dither)
when tile-size controls are visible in Gamedev.

## Patterns and batch

Pattern Manager and Batch Export remain available; apply a Pattern Manager
recipe before a batch run when you want a saved filter stack.
