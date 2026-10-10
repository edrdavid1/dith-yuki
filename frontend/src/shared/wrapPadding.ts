/**
 * ED approximate wrap-padding UI constants.
 * Must stay byte-identical with `WRAP_PADDING_HINT` / limits in
 * `crates/engine-project/src/filters/ed_wrap_padding.rs` (gamedev.md §6.2).
 */
export const WRAP_PADDING_HINT =
  'Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.';

export const WRAP_PADDING_TOOLTIP =
  'Reduces seams when the result is tiled. Slower on large images. Not exact.';

export const WRAP_PADDING_OVERSIZE_TOOLTIP = 'Image too large for wrap-padding.';

export const WRAP_PADDING_OVERSIZE_BANNER =
  'Image too large for wrap-padding. Seams may appear.';

/** Soft memory ceiling (pixels) — mirrors Rust `MAX_PADDED_PIXELS`. */
export const MAX_PADDED_PIXELS = 200_000_000;

/** Hard ceiling on either margin axis — mirrors Rust `MAX_MARGIN_PX`. */
export const MAX_MARGIN_PX = 512;

/** Conservative ED kernel max offset for UI enable checks (covers Stucki/Jarvis). */
const UI_MAX_KERNEL_OFFSET = 2;

/**
 * Whether document dims fit wrap-padding memory / margin ceilings (§6.1).
 * Uses a conservative kernel offset so the checkbox stays disabled when any
 * common ED kernel would exceed limits.
 */
export function wrapPaddingFitsLimits(
  width: number,
  height: number,
  pixelSize: number,
): boolean {
  const ps = Math.max(1, Math.floor(pixelSize) || 1);
  const mx = ps * UI_MAX_KERNEL_OFFSET;
  const my = ps * UI_MAX_KERNEL_OFFSET;
  if (mx > MAX_MARGIN_PX || my > MAX_MARGIN_PX) return false;
  if (width <= 0 || height <= 0) return false;
  const pw = width + 2 * mx;
  const ph = height + 2 * my;
  if (!Number.isFinite(pw) || !Number.isFinite(ph)) return false;
  return pw * ph <= MAX_PADDED_PIXELS;
}
