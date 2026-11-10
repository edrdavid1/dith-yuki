/** Visible carriage width (`slider-carrete-icon.svg`). */
export const RETRO_SLIDER_THUMB_WIDTH_PX = 16;

/**
 * How far (CSS px) to inset the carriage **center** from each track end at min/max.
 *
 * Default = half the carriage (8px): matches Pixel Size at value `2` on the
 * 1…32 domain (one step in), and is applied to every slider so mins/maxes
 * look the same regardless of value range. With `translateX(-50%)` the outer
 * edge of the carriage then sits on the track end (slight overlap of the line).
 *
 * Tune this constant to change the start/finish look for all sliders.
 */
export const RETRO_SLIDER_END_INSET_PX = RETRO_SLIDER_THUMB_WIDTH_PX / 2;

/**
 * Travel-range inset as a fraction of track width.
 * Derived from {@link RETRO_SLIDER_END_INSET_PX}.
 */
export function sliderEndPadRatio(trackWidthPx: number): number {
  if (!(trackWidthPx > 0) || !(RETRO_SLIDER_END_INSET_PX > 0)) return 0;
  return Math.min(0.45, RETRO_SLIDER_END_INSET_PX / trackWidthPx);
}

/** Map value ratio 0..1 → visual left ratio with end padding. */
export function sliderVisualRatio(valueRatio01: number, padRatio: number): number {
  const pad = Math.max(0, Math.min(0.45, padRatio));
  const r = Number.isFinite(valueRatio01) ? Math.max(0, Math.min(1, valueRatio01)) : 0;
  return pad + r * (1 - 2 * pad);
}

/** Inverse of sliderVisualRatio for pointer mapping. */
export function sliderValueRatioFromVisual(visualRatio01: number, padRatio: number): number {
  const pad = Math.max(0, Math.min(0.45, padRatio));
  const usable = 1 - 2 * pad;
  if (!(usable > 0)) return 0;
  const v = Number.isFinite(visualRatio01) ? visualRatio01 : 0;
  return Math.max(0, Math.min(1, (v - pad) / usable));
}

/** CSS `left` with end padding; pair with `transform: translateX(-50%)`. */
export function sliderThumbLeftCss(valueRatio01: number, padRatio: number = 0): string {
  return `${sliderVisualRatio(valueRatio01, padRatio) * 100}%`;
}

/** Map pointer X → value ratio 0..1 (accounts for end padding). */
export function sliderRatioFromClientX(
  clientX: number,
  trackLeft: number,
  trackWidth: number,
  padRatio: number = 0,
): number {
  if (!(trackWidth > 0)) return 0;
  const visual = Math.max(0, Math.min(1, (clientX - trackLeft) / trackWidth));
  return sliderValueRatioFromVisual(visual, padRatio);
}
