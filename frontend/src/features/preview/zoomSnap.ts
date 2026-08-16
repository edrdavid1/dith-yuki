/**
 * Pure zoom/snap helpers for integer zoom mode (Track B2).
 * No DOM — unit-testable.
 */

export type ZoomMode = 'integer' | 'free';

export const ZOOM_MIN = 0.01;
export const ZOOM_MAX = 64;

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/**
 * Snap zoom to nearest integer factor (≥1) or power-of-two reciprocal (<1).
 * Sub-1 policy: 1/2, 1/4, 1/8… so pyramid tiles blit 1:1 (no nearest-neighbour grid).
 */
export function snapIntegerZoom(zoom: number, max = ZOOM_MAX): number {
  if (!Number.isFinite(zoom) || zoom <= 0) return 1;
  if (zoom >= 1) {
    return clamp(Math.round(zoom), 1, max);
  }
  const k = Math.round(Math.log2(1 / zoom));
  return clamp(2 ** -Math.max(k, 0), ZOOM_MIN, 1);
}

/**
 * Floor-to-fit for integer mode: largest integer (or 1/2^k) ≤ fitZoom
 * so the full document still fits.
 */
export function snapIntegerZoomFloor(fitZoom: number, max = ZOOM_MAX): number {
  if (!Number.isFinite(fitZoom) || fitZoom <= 0) return ZOOM_MIN;
  if (fitZoom >= 1) {
    return clamp(Math.floor(fitZoom), 1, max);
  }
  const k = Math.max(0, Math.ceil(Math.log2(1 / fitZoom)));
  return clamp(2 ** -k, ZOOM_MIN, 1);
}

/** Snap a CSS-pixel coordinate to the device-pixel grid. */
export function snapCssPx(v: number, dpr: number): number {
  if (!Number.isFinite(dpr) || dpr <= 0) return Math.round(v);
  return Math.round(v * dpr) / dpr;
}

/**
 * DPR-aware tile rect: snap origin and derive size from snapped end − start
 * so adjacent tiles do not leave 1px gaps.
 */
export function snapTileDrawRect(
  x: number,
  y: number,
  size: number,
  dpr: number,
): { dx: number; dy: number; dw: number; dh: number } {
  const dx = snapCssPx(x, dpr);
  const dy = snapCssPx(y, dpr);
  const dw = snapCssPx(x + size, dpr) - dx;
  const dh = snapCssPx(y + size, dpr) - dy;
  return { dx, dy, dw, dh };
}

/** Next integer / power-of-two zoom step above current. */
export function nextIntegerZoom(zoom: number, max = ZOOM_MAX): number {
  const snapped = snapIntegerZoom(zoom, max);
  if (snapped >= 1) {
    // If already on an integer, step up; if between, snap then step from there
    if (Math.abs(zoom - snapped) < 1e-9) {
      return clamp(snapped + 1, 1, max);
    }
    return snapped > zoom ? snapped : clamp(snapped + 1, 1, max);
  }
  const doubled = snapped * 2;
  if (doubled >= 1) return 1;
  return clamp(doubled, ZOOM_MIN, 1);
}

/** Previous integer / power-of-two zoom step below current. */
export function prevIntegerZoom(zoom: number, max = ZOOM_MAX): number {
  const snapped = snapIntegerZoom(zoom, max);
  if (snapped > 1) {
    if (Math.abs(zoom - snapped) < 1e-9) {
      return clamp(snapped - 1, 1, max);
    }
    return snapped < zoom ? snapped : clamp(snapped - 1, 1, max);
  }
  if (snapped === 1 || zoom >= 1) {
    return 0.5;
  }
  return clamp(snapped / 2, ZOOM_MIN, 1);
}
