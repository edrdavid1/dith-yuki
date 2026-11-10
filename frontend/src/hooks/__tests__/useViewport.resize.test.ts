import { describe, it, expect } from 'vitest';
import { resizeCanvasKeepingCenter } from '../useViewport';
import type { ViewportState } from '../../features/preview/TileCanvas';

function vp(partial: Partial<ViewportState> = {}): ViewportState {
  return {
    zoom: 1,
    panX: 0,
    panY: 0,
    canvasWidth: 800,
    canvasHeight: 600,
    ...partial,
  };
}

describe('resizeCanvasKeepingCenter', () => {
  it('keeps the document center fixed when the window grows', () => {
    // 400×300 doc at zoom 1, centered in 800×600 → pan (-200, -150)
    const prev = vp({
      panX: -200,
      panY: -150,
      canvasWidth: 800,
      canvasHeight: 600,
    });
    const next = resizeCanvasKeepingCenter(prev, 1000, 800);
    // Center was at (200, 150) in doc space; still centered.
    expect(next.panX + next.canvasWidth / next.zoom / 2).toBeCloseTo(200);
    expect(next.panY + next.canvasHeight / next.zoom / 2).toBeCloseTo(150);
    expect(next.panX).toBeCloseTo(-300);
    expect(next.panY).toBeCloseTo(-250);
  });

  it('keeps a panned focus point under the viewport center', () => {
    const prev = vp({ panX: 100, panY: 50, zoom: 2, canvasWidth: 400, canvasHeight: 300 });
    const centerX = prev.panX + prev.canvasWidth / prev.zoom / 2;
    const centerY = prev.panY + prev.canvasHeight / prev.zoom / 2;
    const next = resizeCanvasKeepingCenter(prev, 600, 500);
    expect(next.panX + next.canvasWidth / next.zoom / 2).toBeCloseTo(centerX);
    expect(next.panY + next.canvasHeight / next.zoom / 2).toBeCloseTo(centerY);
  });

  it('only updates size when previous canvas was empty', () => {
    const prev = vp({ canvasWidth: 0, canvasHeight: 0, panX: 0, panY: 0 });
    const next = resizeCanvasKeepingCenter(prev, 800, 600);
    expect(next).toEqual({ ...prev, canvasWidth: 800, canvasHeight: 600 });
  });
});
