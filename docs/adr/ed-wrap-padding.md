# ADR: Approximate wrap-padding for error-diffusion dithering

**Status:** Accepted  
**Date:** 2026-10-10  
**Working name:** `ed_wrap_padding_v1`

## Context

Error diffusion (ED) is sequential: each pixel’s quantization error feeds later
neighbors. That wavefront makes **exact** seamless tiling impossible — opposite
edges never share a consistent accumulated-error state. Users still want ED
textures that tile with reduced visible seams (game assets, repeating patterns).

## Decision

Implement **approximate wrap-padding**:

1. Compute margin `(mx, my) = pixel_size × (|dx|max, |dy|max)` for the kernel.
2. Pad the source with wrapped opposite edges (fresh allocation).
3. Run the existing ED kernel on the padded buffer (unchanged).
4. Crop back to the original `W × H` rect.

Route layers that enable this flag through the full-document path. On memory /
overflow limits, fall back to the tile-strip path and warn in the UI.

Serpentine ED is supported (pad both horizontal sides). Multi-pass ED is not
supported (engine is single-pass).

## Consequences

**Positive**

- Reduced tile seams for ED exports.
- Deterministic when `wrap_padding` is on; bit-identical to prior behavior when off.
- Ordered dither modes ignore the flag (already period-correct).

**Negative**

- Full-document render path: O(W×H) memory and slower large canvases.
- Result is **approximate, not exact** — must stay documented in the UI.

## Alternatives considered

| Alternative | Outcome |
|---|---|
| Do nothing | Rejected — users need reduced ED seams. |
| Exact seamless ED | Rejected — mathematically impossible for sequential ED. |
| Ordered-only seamless | Rejected — does not serve ED users. |

## Canonical hint text

Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.

## Reference

- Spec: `.local-doc/gamedev.md` (`ed_wrap_padding_v1`)
- Implementation: `crates/engine-project/src/filters/ed_wrap_padding.rs`
- User docs: `docs/gamedev-toolkit.md`
