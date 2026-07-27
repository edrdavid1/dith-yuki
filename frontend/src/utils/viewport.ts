/**
 * Compute display dimensions that fit the image into the viewport
 * while preserving aspect ratio (fit-to-view).
 *
 * Returns dimensions where:
 * - width <= vpW and height <= vpH
 * - aspect ratio is preserved (within rounding)
 * - at least one dimension fills its axis
 */
export function computeFitToView(
  imgW: number,
  imgH: number,
  vpW: number,
  vpH: number
): { width: number; height: number } {
  if (imgW <= 0 || imgH <= 0 || vpW <= 0 || vpH <= 0) {
    return { width: 0, height: 0 };
  }

  const scaleX = vpW / imgW;
  const scaleY = vpH / imgH;
  const scale = Math.min(scaleX, scaleY);

  return {
    width: Math.round(imgW * scale),
    height: Math.round(imgH * scale),
  };
}
