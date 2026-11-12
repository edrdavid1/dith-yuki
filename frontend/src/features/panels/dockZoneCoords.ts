/**
 * W6: single coordinate conversion — CSS layout rects → physical screen pixels.
 *
 * Tauri `innerPosition` / `outerPosition` are physical. DOM `getBoundingClientRect`
 * is CSS pixels relative to the webview. Multiply by `devicePixelRatio` (or the
 * window scale factor) and offset by the webview’s physical origin.
 */

export type CssRect = {
  left: number;
  top: number;
  width: number;
  height: number;
};

export type PhysicalOrigin = {
  /** Webview inner origin in physical screen pixels. */
  physicalX: number;
  physicalY: number;
  /** Main window devicePixelRatio / Tauri scaleFactor. */
  dpr: number;
};

export type PhysicalRect = {
  x: number;
  y: number;
  width: number;
  height: number;
};

/**
 * Convert a CSS rect (viewport-relative) into a physical screen rect.
 * Pure — unit-tested for HiDPI and negative monitor origins.
 */
export function cssRectToPhysicalScreen(
  css: CssRect,
  origin: PhysicalOrigin,
): PhysicalRect {
  const dpr = origin.dpr > 0 ? origin.dpr : 1;
  return {
    x: origin.physicalX + css.left * dpr,
    y: origin.physicalY + css.top * dpr,
    width: css.width * dpr,
    height: css.height * dpr,
  };
}

/** Estimate an edge strip in physical pixels from the inner webview size. */
export function physicalEdgeStrip(
  side: 'left' | 'right',
  origin: PhysicalOrigin,
  innerPhysicalWidth: number,
  innerPhysicalHeight: number,
  stripCssWidth: number,
): PhysicalRect {
  const dpr = origin.dpr > 0 ? origin.dpr : 1;
  const width = stripCssWidth * dpr;
  return {
    x:
      side === 'right'
        ? origin.physicalX + innerPhysicalWidth - width
        : origin.physicalX,
    y: origin.physicalY,
    width,
    height: innerPhysicalHeight,
  };
}
